//! Downloading, verifying, placing and launching the apps (docs/launcher-plan.md 5 and 6).
//!
//! Two kinds are installed here: `Files` (executables placed with `selfupdate::swap_in`, the
//! retired ones kept in the roll back folder) and `Setup` (the app's own installer, run silently,
//! elevated through `runas` for a machine scope). `Zip` is refused.
//!
//! Rules this file keeps: nothing downloaded is placed or run before its size and SHA-256 are
//! checked; every failure leaves the old version where it was and deletes the bad download; a
//! running app is asked to close (`WM_CLOSE`) and waited for, never killed; nothing parsed from a
//! manifest or a roll back note can panic (the release build aborts on a panic).
use crate::catalog::CatalogApp;
use crate::detect;
use crate::model::{FileSpec, Installed, Kind, PlatformEntry, Scope, Source};
use crate::state::{Record, Store};
use crate::transport::Transport;
use mdshared_app::selfupdate;
use mdshared_app::update::{Version, sha256_hex, verify_sha256_file};
use std::fmt;
use std::io::Write as _;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// The platform entry Hub installs from (the same string as `manifest::PLATFORM`).
const PLATFORM: &str = "windows-x86_64";
/// The note the roll back folder holds beside the retired files.
const ROLLBACK_NOTE: &str = "rollback.txt";
/// What a file of unknown size may be at most (the transport holds the body in memory today).
const UNSIZED_CAP: usize = 512 * 1024 * 1024;

/// What an install reports to the window, in order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Progress {
    Downloading {
        file: String,
        done: u64,
        total: u64,
    },
    Verifying(String),
    /// Waiting for the named program to close.
    Waiting(String),
    Placing,
    Finished,
}

/// Why an install did not happen. The old version is in place in every case.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InstallError {
    /// The manifest has nothing for this platform, or a kind Hub does not install ("zip").
    Unsupported(String),
    /// The manifest names something unsafe or incomplete (a file name with a separator, ...).
    Invalid(String),
    Cancelled,
    /// The transport failed.
    Download {
        file: String,
        reason: String,
    },
    /// `what` is "size" or "sha256".
    Mismatch {
        file: String,
        what: &'static str,
        expected: String,
        received: String,
    },
    /// A program of the app is still open after the wait.
    Running(String),
    /// A file operation failed (the reason says which).
    Io(String),
    /// The installer ran and exited with this code (or could not be run: `None`).
    Setup {
        code: Option<u32>,
        reason: String,
    },
    /// Nothing to roll back to.
    NoRollback,
}

impl fmt::Display for InstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unsupported(s) => write!(f, "not supported: {s}"),
            Self::Invalid(s) => write!(f, "the release is not usable: {s}"),
            Self::Cancelled => write!(f, "cancelled"),
            Self::Download { file, reason } => write!(f, "could not download {file}: {reason}"),
            Self::Mismatch {
                file,
                what,
                expected,
                received,
            } => write!(
                f,
                "{file} did not match its {what}: expected {expected}, received {received}"
            ),
            Self::Running(s) => write!(f, "{s} is still open"),
            Self::Io(s) => write!(f, "{s}"),
            Self::Setup {
                code: Some(c),
                reason,
            } => write!(f, "the installer exited with code {c} ({reason})"),
            Self::Setup { code: None, reason } => write!(f, "the installer did not run: {reason}"),
            Self::NoRollback => write!(f, "there is no previous version to go back to"),
        }
    }
}

impl std::error::Error for InstallError {}

/// Runs a downloaded installer and waits for it. `Ok(exit code)`; `Err` is a short reason when it
/// could not be started at all (the user declined the elevation prompt, the file is gone).
pub trait SetupRunner: Send + Sync {
    fn run(&self, exe: &Path, args: &str, dir: &Path, scope: Scope) -> Result<u32, String>;
}

/// Who runs a setup: Windows (`ShellExecuteExW` `runas` for a machine scope, a plain process for
/// a user scope), or a runner a test supplies.
#[derive(Clone)]
pub enum Elevation {
    Real,
    Custom(Arc<dyn SetupRunner>),
}

/// Sees and closes the running programs of an app.
pub trait Closer: Send + Sync {
    /// True while any of these executables (absolute paths) runs.
    fn is_running(&self, exes: &[PathBuf]) -> bool;
    /// Asks their windows to close (`WM_CLOSE`). Never kills.
    fn ask_close(&self, exes: &[PathBuf]);
}

/// The Windows closer: `detect::any_running` and `EnumWindows` + `PostMessageW(WM_CLOSE)`.
pub struct RealCloser;

impl Closer for RealCloser {
    fn is_running(&self, exes: &[PathBuf]) -> bool {
        detect::any_running(exes)
    }
    fn ask_close(&self, exes: &[PathBuf]) {
        sys::post_close(exes);
    }
}

type Redetect = Arc<dyn Fn(&CatalogApp, &Store) -> Option<Installed> + Send + Sync>;

pub struct Opts {
    /// Set from the window's Cancel; read between files and while waiting for an app to close.
    pub cancel: Arc<AtomicBool>,
    pub elevate: Elevation,
    pub closer: Arc<dyn Closer>,
    /// How long to wait for the app to close (30 s).
    pub close_timeout: Duration,
    /// How often that wait looks (250 ms).
    pub poll: Duration,
    /// Time allowed for one file's download (10 min).
    pub download_timeout: Duration,
    /// `%LOCALAPPDATA%\Programs` for a first install; None asks `detect::Env::system()`.
    pub programs: Option<PathBuf>,
    /// How a Setup install finds what the installer made; None is `detect::detect` on this PC.
    pub redetect: Option<Redetect>,
}

impl Default for Opts {
    fn default() -> Self {
        Opts {
            cancel: Arc::new(AtomicBool::new(false)),
            elevate: Elevation::Real,
            closer: Arc::new(RealCloser),
            close_timeout: Duration::from_secs(30),
            poll: Duration::from_millis(250),
            download_timeout: Duration::from_secs(600),
            programs: None,
            redetect: None,
        }
    }
}

impl Opts {
    pub fn with_cancel(cancel: Arc<AtomicBool>) -> Opts {
        Opts {
            cancel,
            ..Opts::default()
        }
    }
    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
    fn programs_dir(&self) -> PathBuf {
        self.programs
            .clone()
            .unwrap_or_else(|| detect::Env::system().programs)
    }
}

fn io_err(what: &str, e: std::io::Error) -> InstallError {
    InstallError::Io(format!("{what}: {e}"))
}

/// A plain file name: letters, digits, `.`, `_`, `-`; not `.` or `..`.
fn plain_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name.len() <= 128
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// A relative path with only normal components (no `..`, no drive, no root).
fn relative_ok(rel: &str) -> bool {
    !rel.is_empty()
        && Path::new(rel)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}

fn entry_of(m: &crate::model::HubManifest) -> Option<&PlatformEntry> {
    m.platforms.iter().find(|p| p.platform == PLATFORM)
}

fn cache_dir(store: &Store, app: &CatalogApp, version: &str) -> PathBuf {
    store.cache_dir(app.id, version)
}

fn make_record(folder: &Path, exe: &Path, version: &str, kind: &str, date: &str) -> Record {
    Record {
        folder: folder.to_path_buf(),
        exe: exe.to_path_buf(),
        version: version.to_string(),
        kind: kind.to_string(),
        date: date.to_string(),
    }
}

/// Downloads one file to `<dir>\<name>.part`, checks size and SHA-256, and renames it to
/// `<dir>\<name>`. A bad download is deleted before the error is returned.
fn fetch_verified(
    t: &dyn Transport,
    spec: &FileSpec,
    dir: &Path,
    opts: &Opts,
    progress: &mut dyn FnMut(Progress),
) -> Result<PathBuf, InstallError> {
    if !plain_name(&spec.name) {
        return Err(InstallError::Invalid(format!(
            "file name {:?} is not a plain name",
            spec.name
        )));
    }
    if opts.cancelled() {
        return Err(InstallError::Cancelled);
    }
    std::fs::create_dir_all(dir).map_err(|e| io_err("cache folder", e))?;
    let part = dir.join(format!("{}.part", spec.name));
    let done = dir.join(&spec.name);
    _ = selfupdate::remove_any(&part);
    _ = selfupdate::remove_any(&done);
    progress(Progress::Downloading {
        file: spec.name.clone(),
        done: 0,
        total: spec.size,
    });
    let cap = if spec.size > 0 {
        usize::try_from(spec.size)
            .unwrap_or(UNSIZED_CAP)
            .saturating_add(1)
    } else {
        UNSIZED_CAP
    };
    let body = t
        .get(&spec.url, cap, opts.download_timeout)
        .map_err(|reason| InstallError::Download {
            file: spec.name.clone(),
            reason,
        })?;
    if opts.cancelled() {
        return Err(InstallError::Cancelled);
    }
    let len = body.len() as u64;
    let written = std::fs::File::create(&part).and_then(|mut f| {
        f.write_all(&body)?;
        f.sync_all()
    });
    if let Err(e) = written {
        _ = selfupdate::remove_any(&part);
        return Err(io_err("writing the download", e));
    }
    let received_hash = sha256_hex(&body);
    drop(body);
    progress(Progress::Downloading {
        file: spec.name.clone(),
        done: len,
        total: len,
    });
    if spec.size > 0 && len != spec.size {
        _ = selfupdate::remove_any(&part);
        return Err(InstallError::Mismatch {
            file: spec.name.clone(),
            what: "size",
            expected: spec.size.to_string(),
            received: len.to_string(),
        });
    }
    progress(Progress::Verifying(spec.name.clone()));
    let good = verify_sha256_file(&part, &spec.sha256).unwrap_or(false);
    if !good {
        _ = selfupdate::remove_any(&part);
        return Err(InstallError::Mismatch {
            file: spec.name.clone(),
            what: "sha256",
            expected: spec.sha256.clone(),
            received: received_hash,
        });
    }
    if let Err(e) = std::fs::rename(&part, &done) {
        _ = selfupdate::remove_any(&part);
        return Err(io_err("keeping the download", e));
    }
    Ok(done)
}

/// Absolute paths of the executables that must be closed before `folder` is touched.
fn app_exes(app: &CatalogApp, entry: &PlatformEntry, folder: &Path) -> Vec<PathBuf> {
    let mut rels: Vec<&str> = vec![entry.exe.as_deref().unwrap_or(app.exe)];
    rels.extend(app.wait.iter().copied());
    rels.extend(entry.wait.iter().map(String::as_str));
    let mut out: Vec<PathBuf> = Vec::new();
    for rel in rels {
        if !relative_ok(rel) {
            continue;
        }
        let p = folder.join(rel);
        if !out.contains(&p) {
            out.push(p);
        }
    }
    out
}

/// Asks the running programs among `exes` to close and waits for them: up to
/// `opts.close_timeout`, then `Running`. Never kills.
fn ensure_closed(
    exes: &[PathBuf],
    opts: &Opts,
    progress: &mut dyn FnMut(Progress),
) -> Result<(), InstallError> {
    if exes.is_empty() || !opts.closer.is_running(exes) {
        return Ok(());
    }
    let label = exes
        .first()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    progress(Progress::Waiting(label.clone()));
    let start = Instant::now();
    let mut last_ask: Option<Instant> = None;
    loop {
        if last_ask.is_none_or(|t| t.elapsed() >= Duration::from_secs(1)) {
            opts.closer.ask_close(exes);
            last_ask = Some(Instant::now());
        }
        std::thread::sleep(opts.poll);
        if !opts.closer.is_running(exes) {
            return Ok(());
        }
        if opts.cancelled() {
            return Err(InstallError::Cancelled);
        }
        if start.elapsed() >= opts.close_timeout {
            return Err(InstallError::Running(label));
        }
    }
}

/// Installs or updates `app` from `m`. `cur` is what is there now, if anything.
pub fn install(
    app: &CatalogApp,
    m: &crate::model::HubManifest,
    cur: Option<&Installed>,
    store: &Store,
    t: &dyn Transport,
    opts: &Opts,
    progress: &mut dyn FnMut(Progress),
) -> Result<Installed, InstallError> {
    let entry = entry_of(m).ok_or_else(|| {
        InstallError::Unsupported(format!("this release has no {PLATFORM} build"))
    })?;
    let version = m.version.to_string();
    let cache = cache_dir(store, app, &version);
    let result = match entry.kind {
        Kind::Zip => Err(InstallError::Unsupported("zip installs".to_string())),
        Kind::Files => install_files(app, m, entry, cur, store, t, opts, &cache, progress),
        Kind::Setup => install_setup(app, m, entry, cur, store, t, opts, &cache, progress),
    };
    match &result {
        Ok(_) => {
            _ = selfupdate::remove_any(&cache);
            progress(Progress::Finished);
        }
        // A verified download is worth keeping when the user can simply try again.
        Err(InstallError::Cancelled | InstallError::Running(_)) => {}
        Err(_) => {
            _ = selfupdate::remove_any(&cache);
        }
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn install_files(
    app: &CatalogApp,
    m: &crate::model::HubManifest,
    entry: &PlatformEntry,
    cur: Option<&Installed>,
    store: &Store,
    t: &dyn Transport,
    opts: &Opts,
    cache: &Path,
    progress: &mut dyn FnMut(Progress),
) -> Result<Installed, InstallError> {
    if entry.files.is_empty() {
        return Err(InstallError::Invalid("the release lists no files".into()));
    }
    let exe_rel = entry.exe.as_deref().unwrap_or(app.exe);
    if !relative_ok(exe_rel) {
        return Err(InstallError::Invalid(format!(
            "exe {exe_rel:?} is not relative"
        )));
    }
    for (i, f) in entry.files.iter().enumerate() {
        if !plain_name(&f.name) {
            return Err(InstallError::Invalid(format!(
                "file name {:?} is not a plain name",
                f.name
            )));
        }
        if entry.files[..i]
            .iter()
            .any(|g| g.name.eq_ignore_ascii_case(&f.name))
        {
            return Err(InstallError::Invalid(format!("{} is listed twice", f.name)));
        }
    }
    let folder = match cur {
        Some(c) => c.folder.clone(),
        None => opts.programs_dir().join(app.folder),
    };
    let exe = folder.join(exe_rel);
    // The program Hub starts must be one of the files (or already be there).
    let exe_listed = entry.files.iter().any(|f| folder.join(&f.name) == exe);
    if !exe_listed && !exe.is_file() {
        return Err(InstallError::Invalid(format!(
            "{exe_rel} is neither in the release nor in the folder"
        )));
    }

    // 1. Download and verify everything before anything is touched.
    let mut cached = Vec::new();
    for spec in &entry.files {
        cached.push(fetch_verified(t, spec, cache, opts, progress)?);
    }
    if opts.cancelled() {
        return Err(InstallError::Cancelled);
    }

    // 2. Stage beside the targets and check the staged copies.
    std::fs::create_dir_all(&folder).map_err(|e| io_err("install folder", e))?;
    let targets: Vec<PathBuf> = entry.files.iter().map(|f| folder.join(&f.name)).collect();
    let unstage = |targets: &[PathBuf]| {
        for tg in targets {
            _ = selfupdate::remove_any(&selfupdate::staged(tg));
        }
    };
    for ((spec, src), target) in entry.files.iter().zip(&cached).zip(&targets) {
        let staged = selfupdate::staged(target);
        let ok = std::fs::copy(src, &staged)
            .map(|_| ())
            .map_err(|e| io_err(&format!("staging {}", spec.name), e))
            .and_then(|()| {
                if verify_sha256_file(&staged, &spec.sha256).unwrap_or(false) {
                    Ok(())
                } else {
                    Err(InstallError::Mismatch {
                        file: spec.name.clone(),
                        what: "sha256",
                        expected: spec.sha256.clone(),
                        received: "a different file after staging".to_string(),
                    })
                }
            });
        if let Err(e) = ok {
            unstage(&targets);
            return Err(e);
        }
    }

    // 3. The app must be closed.
    if let Err(e) = ensure_closed(&app_exes(app, entry, &folder), opts, progress) {
        unstage(&targets);
        return Err(e);
    }
    if opts.cancelled() {
        unstage(&targets);
        return Err(InstallError::Cancelled);
    }

    // 4. Place, all or none.
    progress(Progress::Placing);
    let had: Vec<bool> = targets.iter().map(|p| p.is_file()).collect();
    if let Err(e) = selfupdate::swap_in(&targets) {
        unstage(&targets);
        return Err(io_err("placing the files", e));
    }

    // 5. Keep what was retired for a roll back (best effort: the install itself has succeeded).
    keep_for_rollback(app, store, cur, &folder, &exe, entry, &targets, &had);

    let rec = make_record(&folder, &exe, &m.version.to_string(), "files", &m.date);
    store
        .save_installed(app.id, &rec)
        .map_err(|e| io_err("recording the install", e))?;
    Ok(Installed {
        folder,
        exe,
        version: Some(m.version.clone()),
        source: Source::Hub,
        kind: "files".to_string(),
    })
}

fn move_file(from: &Path, to: &Path) -> std::io::Result<()> {
    if std::fs::rename(from, to).is_ok() {
        return Ok(());
    }
    std::fs::copy(from, to)?;
    selfupdate::remove_any(from)
}

#[allow(clippy::too_many_arguments)]
fn keep_for_rollback(
    app: &CatalogApp,
    store: &Store,
    cur: Option<&Installed>,
    folder: &Path,
    exe: &Path,
    entry: &PlatformEntry,
    targets: &[PathBuf],
    had: &[bool],
) {
    let rb = store.rollback_dir(app.id);
    _ = selfupdate::remove_any(&rb);
    if !had.iter().any(|h| *h) {
        return;
    }
    if std::fs::create_dir_all(&rb).is_err() {
        return;
    }
    let mut note = String::new();
    note.push_str("hub-rollback: 1\n");
    note.push_str(&format!("folder: {}\n", folder.display()));
    note.push_str(&format!("exe: {}\n", exe.display()));
    if let Some(v) = cur.and_then(|c| c.version.as_ref()) {
        note.push_str(&format!("version: {v}\n"));
    }
    for ((spec, target), had) in entry.files.iter().zip(targets).zip(had) {
        if *had {
            if move_file(&selfupdate::retired(target), &rb.join(&spec.name)).is_ok() {
                note.push_str(&format!("restore: {}\n", spec.name));
            }
        } else {
            note.push_str(&format!("remove: {}\n", spec.name));
        }
    }
    if std::fs::write(rb.join(ROLLBACK_NOTE), note).is_err() {
        _ = selfupdate::remove_any(&rb);
    }
}

#[allow(clippy::too_many_arguments)]
fn install_setup(
    app: &CatalogApp,
    m: &crate::model::HubManifest,
    entry: &PlatformEntry,
    cur: Option<&Installed>,
    store: &Store,
    t: &dyn Transport,
    opts: &Opts,
    cache: &Path,
    progress: &mut dyn FnMut(Progress),
) -> Result<Installed, InstallError> {
    let setup = entry
        .setup
        .as_ref()
        .ok_or_else(|| InstallError::Invalid("the release lists no installer".into()))?;
    let installer = fetch_verified(t, &setup.file, cache, opts, progress)?;
    if opts.cancelled() {
        return Err(InstallError::Cancelled);
    }
    if let Some(c) = cur {
        ensure_closed(&app_exes(app, entry, &c.folder), opts, progress)?;
        if opts.cancelled() {
            return Err(InstallError::Cancelled);
        }
    }
    progress(Progress::Placing);
    let dir = installer.parent().unwrap_or(cache).to_path_buf();
    let code = match &opts.elevate {
        Elevation::Real => sys::run_setup(&installer, &setup.silent, &dir, setup.scope),
        Elevation::Custom(r) => r.run(&installer, &setup.silent, &dir, setup.scope),
    }
    .map_err(|reason| InstallError::Setup { code: None, reason })?;
    if code != 0 {
        return Err(InstallError::Setup {
            code: Some(code),
            reason: setup.file.name.clone(),
        });
    }
    let found = match &opts.redetect {
        Some(f) => f(app, store),
        None => detect::detect(app, store, &detect::Env::system()),
    }
    .ok_or_else(|| InstallError::Setup {
        code: Some(0),
        reason: format!(
            "the installer finished but {} was not found afterwards",
            app.name
        ),
    })?;
    // An installer replaced the program: an older roll back note no longer describes it.
    _ = selfupdate::remove_any(&store.rollback_dir(app.id));
    let rec = make_record(
        &found.folder,
        &found.exe,
        &m.version.to_string(),
        "setup",
        &m.date,
    );
    store
        .save_installed(app.id, &rec)
        .map_err(|e| io_err("recording the install", e))?;
    Ok(Installed {
        version: Some(m.version.clone()),
        source: Source::Hub,
        kind: "setup".to_string(),
        ..found
    })
}

struct Note {
    folder: PathBuf,
    exe: PathBuf,
    version: Option<Version>,
    restore: Vec<String>,
    remove: Vec<String>,
}

/// Reads the roll back note. Anything odd is `None`, never a panic.
fn read_note(rb: &Path) -> Option<Note> {
    let bytes = std::fs::read(rb.join(ROLLBACK_NOTE)).ok()?;
    if bytes.len() > 16 * 1024 {
        return None;
    }
    let text = String::from_utf8(bytes).ok()?;
    let (mut folder, mut exe, mut version) = (None, None, None);
    let (mut restore, mut remove) = (Vec::new(), Vec::new());
    let mut first = true;
    for line in text.lines().take(256) {
        let Some((k, v)) = line.split_once(": ") else {
            continue;
        };
        if first {
            if k != "hub-rollback" || v != "1" {
                return None;
            }
            first = false;
            continue;
        }
        match k {
            "folder" => folder = Some(PathBuf::from(v)),
            "exe" => exe = Some(PathBuf::from(v)),
            "version" => version = Version::parse(v).ok(),
            "restore" if plain_name(v) => restore.push(v.to_string()),
            "remove" if plain_name(v) => remove.push(v.to_string()),
            _ => {}
        }
    }
    if first || restore.is_empty() {
        return None;
    }
    Some(Note {
        folder: folder?,
        exe: exe?,
        version,
        restore,
        remove,
    })
}

/// Puts the retired files of the last `Files` install back (the program, not the user's data).
pub fn rollback(app: &CatalogApp, store: &Store) -> Result<Installed, InstallError> {
    rollback_with(app, store, &Opts::default(), &mut |_| {})
}

/// `rollback` with the options (closing the app) and progress of an install.
pub fn rollback_with(
    app: &CatalogApp,
    store: &Store,
    opts: &Opts,
    progress: &mut dyn FnMut(Progress),
) -> Result<Installed, InstallError> {
    let rb = store.rollback_dir(app.id);
    let note = read_note(&rb).ok_or(InstallError::NoRollback)?;
    let restore_targets: Vec<PathBuf> = note.restore.iter().map(|n| note.folder.join(n)).collect();
    for n in &note.restore {
        if !rb.join(n).is_file() {
            return Err(InstallError::NoRollback);
        }
    }
    let mut exes: Vec<PathBuf> = vec![note.exe.clone()];
    for rel in app.wait {
        if relative_ok(rel) {
            exes.push(note.folder.join(rel));
        }
    }
    ensure_closed(&exes, opts, progress)?;
    let unstage = |targets: &[PathBuf]| {
        for tg in targets {
            _ = selfupdate::remove_any(&selfupdate::staged(tg));
        }
    };
    for (n, target) in note.restore.iter().zip(&restore_targets) {
        if let Err(e) = std::fs::copy(rb.join(n), selfupdate::staged(target)) {
            unstage(&restore_targets);
            return Err(io_err(&format!("staging {n}"), e));
        }
    }
    progress(Progress::Placing);
    if let Err(e) = selfupdate::swap_in(&restore_targets) {
        unstage(&restore_targets);
        return Err(io_err("putting the previous files back", e));
    }
    selfupdate::clean_up(&restore_targets);
    for n in &note.remove {
        _ = selfupdate::remove_any(&note.folder.join(n));
    }
    let version_text = note
        .version
        .as_ref()
        .map(|v| v.to_string())
        .unwrap_or_default();
    let rec = make_record(&note.folder, &note.exe, &version_text, "files", "");
    store
        .save_installed(app.id, &rec)
        .map_err(|e| io_err("recording the roll back", e))?;
    _ = selfupdate::remove_any(&rb);
    progress(Progress::Finished);
    Ok(Installed {
        folder: note.folder,
        exe: note.exe,
        version: note.version,
        source: Source::Hub,
        kind: "files".to_string(),
    })
}

/// The command `launch` runs, before it is started (a function of its own so it can be looked at).
fn launch_command(app: &CatalogApp, inst: &Installed) -> std::process::Command {
    let mut cmd = std::process::Command::new(&inst.exe);
    cmd.args(app.launch_args)
        .current_dir(&inst.folder)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    sys::detach(&mut cmd);
    cmd
}

/// Starts the app detached, in its install folder, with the catalog's arguments. Nothing waits.
pub fn launch(app: &CatalogApp, inst: &Installed) -> std::io::Result<()> {
    launch_command(app, inst).spawn().map(drop)
}

#[cfg(windows)]
mod sys {
    use super::Scope;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::process::CommandExt;
    use std::path::{Path, PathBuf};
    use windows_sys::Win32::Foundation::{CloseHandle, HWND, LPARAM};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, INFINITE, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        QueryFullProcessImageNameW, WaitForSingleObject,
    };
    use windows_sys::Win32::UI::Shell::{
        SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW, ShellExecuteExW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible, PostMessageW, WM_CLOSE,
    };

    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;

    pub fn detach(cmd: &mut std::process::Command) {
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
    }

    fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
        s.encode_wide().chain(std::iter::once(0)).collect()
    }

    fn norm(p: &Path) -> String {
        p.to_string_lossy().replace('/', "\\").to_lowercase()
    }

    struct Ctx {
        exes: Vec<String>,
    }

    unsafe extern "system" fn each_window(hwnd: HWND, lparam: LPARAM) -> i32 {
        // SAFETY: `lparam` is the address of the `Ctx` that `post_close` keeps alive for the call.
        let ctx = unsafe { &*(lparam as *const Ctx) };
        // SAFETY: plain Win32 calls on a window handle EnumWindows gave us; results are checked.
        unsafe {
            if IsWindowVisible(hwnd) == 0 {
                return 1;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, &mut pid);
            if pid == 0 {
                return 1;
            }
            let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if h.is_null() {
                return 1;
            }
            let mut buf = [0u16; 1024];
            let mut len = buf.len() as u32;
            let ok = QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len);
            CloseHandle(h);
            if ok == 0 || len as usize > buf.len() {
                return 1;
            }
            let path = String::from_utf16_lossy(&buf[..len as usize])
                .replace('/', "\\")
                .to_lowercase();
            if ctx.exes.contains(&path) {
                PostMessageW(hwnd, WM_CLOSE, 0, 0);
            }
        }
        1
    }

    /// `WM_CLOSE` to every visible top-level window of a process whose image is one of `exes`.
    pub fn post_close(exes: &[PathBuf]) {
        let ctx = Ctx {
            exes: exes.iter().map(|p| norm(p)).collect(),
        };
        // SAFETY: the callback only reads `ctx`, which outlives this synchronous call.
        unsafe {
            EnumWindows(Some(each_window), &ctx as *const Ctx as LPARAM);
        }
    }

    pub fn run_setup(exe: &Path, args: &str, dir: &Path, scope: Scope) -> Result<u32, String> {
        match scope {
            Scope::User => {
                let mut cmd = std::process::Command::new(exe);
                cmd.raw_arg(args).current_dir(dir);
                let status = cmd.status().map_err(|e| e.to_string())?;
                // No code means the process was ended by the system: not a success.
                Ok(status.code().map(|c| c as u32).unwrap_or(u32::MAX))
            }
            Scope::Machine => {
                let verb = wide("runas".as_ref());
                let file = wide(exe.as_os_str());
                let params = wide(args.as_ref());
                let cwd = wide(dir.as_os_str());
                // SAFETY: the struct is zeroed (all pointers null) and then given pointers into the
                // vectors above, which live until after the call; the process handle it returns
                // is waited on and closed here.
                unsafe {
                    let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
                    info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
                    info.fMask = SEE_MASK_NOCLOSEPROCESS;
                    info.lpVerb = verb.as_ptr();
                    info.lpFile = file.as_ptr();
                    info.lpParameters = params.as_ptr();
                    info.lpDirectory = cwd.as_ptr();
                    info.nShow = 1;
                    if ShellExecuteExW(&mut info) == 0 {
                        return Err(
                            "the elevation prompt was declined or the installer could not start"
                                .into(),
                        );
                    }
                    if info.hProcess.is_null() {
                        return Err("no process handle to wait on".into());
                    }
                    WaitForSingleObject(info.hProcess, INFINITE);
                    let mut code = 0u32;
                    let got = GetExitCodeProcess(info.hProcess, &mut code);
                    CloseHandle(info.hProcess);
                    if got == 0 {
                        return Err("the installer's exit code could not be read".into());
                    }
                    Ok(code)
                }
            }
        }
    }
}

#[cfg(not(windows))]
mod sys {
    use super::Scope;
    use std::path::{Path, PathBuf};

    pub fn detach(_cmd: &mut std::process::Command) {}
    pub fn post_close(_exes: &[PathBuf]) {}
    pub fn run_setup(exe: &Path, args: &str, dir: &Path, scope: Scope) -> Result<u32, String> {
        if scope == Scope::Machine {
            return Err("machine installs are Windows only".into());
        }
        let status = std::process::Command::new(exe)
            .args(args.split_whitespace())
            .current_dir(dir)
            .status()
            .map_err(|e| e.to_string())?;
        Ok(status.code().map(|c| c as u32).unwrap_or(u32::MAX))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{HubManifest, SetupSpec};
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicUsize;

    struct FakeTransport {
        files: HashMap<String, Vec<u8>>,
        calls: Mutex<Vec<String>>,
    }
    impl Transport for FakeTransport {
        fn get(&self, url: &str, _max: usize, _t: Duration) -> Result<Vec<u8>, String> {
            self.calls.lock().unwrap().push(url.to_string());
            self.files
                .get(url)
                .cloned()
                .ok_or_else(|| "404".to_string())
        }
    }
    fn transport(files: &[(&str, &[u8])]) -> FakeTransport {
        FakeTransport {
            files: files
                .iter()
                .map(|(k, v)| (format!("https://x/{k}"), v.to_vec()))
                .collect(),
            calls: Mutex::new(Vec::new()),
        }
    }

    const APP: CatalogApp = CatalogApp {
        id: "fake",
        name: "Fake",
        blurb: "",
        repo: "fake",
        exe: "fake.exe",
        launch_args: &[],
        folder: "FakeApp",
        machine_folder: None,
        uninstall_keys: &[],
        wait: &["fake-engine.exe"],
        legacy_manifest: None,
        tile: [0, 0, 0],
        glyph: "",
    };

    fn scratch(name: &str) -> PathBuf {
        let base = std::env::var_os("CARGO_TARGET_TMPDIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let d = base.join(format!("hub-install-{name}-{}", std::process::id()));
        _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    struct Rig {
        root: PathBuf,
        store: Store,
    }
    impl Rig {
        fn new(name: &str) -> Rig {
            let root = scratch(name);
            let store = Store {
                dir: root.join("hub"),
            };
            Rig { root, store }
        }
        fn programs(&self) -> PathBuf {
            self.root.join("Programs")
        }
        fn folder(&self) -> PathBuf {
            self.programs().join(APP.folder)
        }
        fn opts(&self) -> Opts {
            Opts {
                close_timeout: Duration::from_millis(200),
                poll: Duration::from_millis(10),
                programs: Some(self.programs()),
                ..Opts::default()
            }
        }
        fn cache(&self, v: &str) -> PathBuf {
            self.store.cache_dir(APP.id, v)
        }
        fn current(&self) -> Installed {
            Installed {
                folder: self.folder(),
                exe: self.folder().join("fake.exe"),
                version: Some(Version::parse("1.0.0").unwrap()),
                source: Source::Hub,
                kind: "files".into(),
            }
        }
    }
    impl Drop for Rig {
        fn drop(&mut self) {
            _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn spec(name: &str, body: &[u8]) -> FileSpec {
        FileSpec {
            name: name.into(),
            url: format!("https://x/{name}"),
            sha256: sha256_hex(body),
            size: body.len() as u64,
        }
    }
    fn entry(kind: Kind) -> PlatformEntry {
        PlatformEntry {
            platform: PLATFORM.into(),
            kind,
            exe: Some("fake.exe".into()),
            files: vec![],
            zip: None,
            setup: None,
            uninstall: None,
            min: None,
            wait: vec![],
            updates_self: false,
        }
    }
    fn manifest(version: &str, e: PlatformEntry) -> HubManifest {
        HubManifest {
            app: "fake".into(),
            version: Version::parse(version).unwrap(),
            date: "2026-11-22".into(),
            notes: String::new(),
            notes_file: None,
            platforms: vec![e],
        }
    }
    fn files_manifest(version: &str, files: &[(&str, &[u8])]) -> HubManifest {
        let mut e = entry(Kind::Files);
        e.files = files.iter().map(|(n, b)| spec(n, b)).collect();
        manifest(version, e)
    }
    fn read(p: &Path) -> Vec<u8> {
        std::fs::read(p).unwrap()
    }
    fn names(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir)
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v
    }
    fn run(
        rig: &Rig,
        m: &HubManifest,
        cur: Option<&Installed>,
        t: &FakeTransport,
        opts: &Opts,
    ) -> (Result<Installed, InstallError>, Vec<Progress>) {
        let mut seen = Vec::new();
        let r = install(&APP, m, cur, &rig.store, t, opts, &mut |p| seen.push(p));
        (r, seen)
    }

    #[test]
    fn fresh_install_places_the_files_and_records_it() {
        let rig = Rig::new("fresh");
        let t = transport(&[("fake.exe", b"exe v2"), ("fake-engine.exe", b"engine v2")]);
        let m = files_manifest(
            "2.0.0",
            &[("fake.exe", b"exe v2"), ("fake-engine.exe", b"engine v2")],
        );
        let (r, seen) = run(&rig, &m, None, &t, &rig.opts());
        let inst = r.unwrap();
        assert_eq!(inst.folder, rig.folder());
        assert_eq!(inst.exe, rig.folder().join("fake.exe"));
        assert_eq!(inst.version, Some(Version::parse("2.0.0").unwrap()));
        assert_eq!(read(&inst.exe), b"exe v2");
        assert_eq!(read(&rig.folder().join("fake-engine.exe")), b"engine v2");
        assert_eq!(names(&rig.folder()), ["fake-engine.exe", "fake.exe"]);
        assert!(rig.store.load_installed("fake").is_some());
        assert!(
            !rig.cache("2.0.0").exists(),
            "the cache is removed after success"
        );
        assert!(
            !rig.store.rollback_dir("fake").exists(),
            "nothing to roll back to"
        );
        assert!(matches!(
            seen.first(),
            Some(Progress::Downloading { done: 0, .. })
        ));
        assert!(seen.contains(&Progress::Verifying("fake.exe".into())));
        assert!(seen.contains(&Progress::Placing));
        assert_eq!(seen.last(), Some(&Progress::Finished));
        let placing = seen.iter().position(|p| *p == Progress::Placing).unwrap();
        let verifying = seen
            .iter()
            .rposition(|p| matches!(p, Progress::Verifying(_)))
            .unwrap();
        assert!(verifying < placing, "verified before placed");
    }

    #[test]
    fn update_over_an_existing_install_keeps_the_old_files_for_roll_back() {
        let rig = Rig::new("update");
        std::fs::create_dir_all(rig.folder()).unwrap();
        std::fs::write(rig.folder().join("fake.exe"), b"exe v1").unwrap();
        std::fs::write(rig.folder().join("settings.ini"), b"mine").unwrap();
        let t = transport(&[("fake.exe", b"exe v2")]);
        let m = files_manifest("2.0.0", &[("fake.exe", b"exe v2")]);
        let cur = rig.current();
        let (r, _) = run(&rig, &m, Some(&cur), &t, &rig.opts());
        r.unwrap();
        assert_eq!(read(&rig.folder().join("fake.exe")), b"exe v2");
        assert_eq!(read(&rig.folder().join("settings.ini")), b"mine");
        assert_eq!(
            names(&rig.folder()),
            ["fake.exe", "settings.ini"],
            "no .old or .new left"
        );
        let rb = rig.store.rollback_dir("fake");
        assert_eq!(read(&rb.join("fake.exe")), b"exe v1");
        assert!(rb.join(ROLLBACK_NOTE).is_file());
    }

    #[test]
    fn a_hash_mismatch_places_nothing_and_deletes_the_download() {
        let rig = Rig::new("hash");
        std::fs::create_dir_all(rig.folder()).unwrap();
        std::fs::write(rig.folder().join("fake.exe"), b"exe v1").unwrap();
        // Same length as promised, different bytes.
        let t = transport(&[("fake.exe", b"exe vX")]);
        let m = files_manifest("2.0.0", &[("fake.exe", b"exe v2")]);
        let cur = rig.current();
        let (r, seen) = run(&rig, &m, Some(&cur), &t, &rig.opts());
        match r {
            Err(InstallError::Mismatch {
                file,
                what,
                expected,
                received,
            }) => {
                assert_eq!(file, "fake.exe");
                assert_eq!(what, "sha256");
                assert_eq!(expected, sha256_hex(b"exe v2"));
                assert_eq!(received, sha256_hex(b"exe vX"));
            }
            other => panic!("expected a mismatch, got {other:?}"),
        }
        assert_eq!(
            read(&rig.folder().join("fake.exe")),
            b"exe v1",
            "old version intact"
        );
        assert_eq!(names(&rig.folder()), ["fake.exe"]);
        assert!(
            names(&rig.cache("2.0.0")).is_empty(),
            "the bad download is deleted"
        );
        assert!(rig.store.load_installed("fake").is_none());
        assert!(!seen.contains(&Progress::Placing));
    }

    #[test]
    fn a_size_mismatch_is_refused_before_hashing_and_deleted() {
        let rig = Rig::new("size");
        let t = transport(&[("fake.exe", b"exe v2 but longer")]);
        let m = files_manifest("2.0.0", &[("fake.exe", b"exe v2")]);
        let (r, seen) = run(&rig, &m, None, &t, &rig.opts());
        match r {
            Err(InstallError::Mismatch {
                what,
                expected,
                received,
                ..
            }) => {
                assert_eq!(what, "size");
                assert_eq!(expected, "6");
                assert_eq!(received, "17");
            }
            other => panic!("expected a size mismatch, got {other:?}"),
        }
        assert!(!rig.folder().exists());
        assert!(names(&rig.cache("2.0.0")).is_empty());
        assert!(!seen.iter().any(|p| matches!(p, Progress::Verifying(_))));
    }

    #[test]
    fn a_later_file_that_fails_leaves_the_first_unplaced() {
        let rig = Rig::new("second-bad");
        std::fs::create_dir_all(rig.folder()).unwrap();
        std::fs::write(rig.folder().join("fake.exe"), b"exe v1").unwrap();
        let t = transport(&[("fake.exe", b"exe v2"), ("fake-engine.exe", b"nope")]);
        let m = files_manifest(
            "2.0.0",
            &[("fake.exe", b"exe v2"), ("fake-engine.exe", b"engine v2")],
        );
        let cur = rig.current();
        let (r, _) = run(&rig, &m, Some(&cur), &t, &rig.opts());
        assert!(matches!(r, Err(InstallError::Mismatch { .. })));
        assert_eq!(read(&rig.folder().join("fake.exe")), b"exe v1");
        assert_eq!(names(&rig.folder()), ["fake.exe"]);
    }

    #[test]
    fn cancel_before_the_start_does_nothing() {
        let rig = Rig::new("cancel0");
        let t = transport(&[("fake.exe", b"exe v2")]);
        let m = files_manifest("2.0.0", &[("fake.exe", b"exe v2")]);
        let opts = rig.opts();
        opts.cancel.store(true, Ordering::Relaxed);
        let (r, _) = run(&rig, &m, None, &t, &opts);
        assert_eq!(r, Err(InstallError::Cancelled));
        assert!(t.calls.lock().unwrap().is_empty(), "nothing was fetched");
        assert!(!rig.folder().exists());
    }

    #[test]
    fn cancel_between_files_stops_before_the_next_and_places_nothing() {
        let rig = Rig::new("cancel1");
        std::fs::create_dir_all(rig.folder()).unwrap();
        std::fs::write(rig.folder().join("fake.exe"), b"exe v1").unwrap();
        let t = transport(&[("fake.exe", b"exe v2"), ("fake-engine.exe", b"engine v2")]);
        let m = files_manifest(
            "2.0.0",
            &[("fake.exe", b"exe v2"), ("fake-engine.exe", b"engine v2")],
        );
        let opts = rig.opts();
        let cur = rig.current();
        let flag = opts.cancel.clone();
        let mut seen = Vec::new();
        let r = install(&APP, &m, Some(&cur), &rig.store, &t, &opts, &mut |p| {
            if matches!(&p, Progress::Verifying(f) if f == "fake.exe") {
                flag.store(true, Ordering::Relaxed);
            }
            seen.push(p);
        });
        assert_eq!(r, Err(InstallError::Cancelled));
        assert_eq!(
            t.calls.lock().unwrap().len(),
            1,
            "the second file was not fetched"
        );
        assert_eq!(read(&rig.folder().join("fake.exe")), b"exe v1");
        assert_eq!(names(&rig.folder()), ["fake.exe"]);
        assert!(
            !names(&rig.cache("2.0.0"))
                .iter()
                .any(|n| n.ends_with(".part"))
        );
        assert!(rig.store.load_installed("fake").is_none());
    }

    struct FakeRunner {
        code: u32,
        seen: Mutex<Vec<(String, String, Scope)>>,
        make: Option<PathBuf>,
    }
    impl SetupRunner for FakeRunner {
        fn run(&self, exe: &Path, args: &str, _dir: &Path, scope: Scope) -> Result<u32, String> {
            self.seen.lock().unwrap().push((
                exe.file_name().unwrap().to_string_lossy().into_owned(),
                args.to_string(),
                scope,
            ));
            if self.code == 0
                && let Some(folder) = &self.make
            {
                std::fs::create_dir_all(folder).unwrap();
                std::fs::write(folder.join("fake.exe"), b"installed").unwrap();
            }
            Ok(self.code)
        }
    }
    fn setup_manifest(scope: Scope) -> (HubManifest, FakeTransport) {
        let mut e = entry(Kind::Setup);
        e.setup = Some(SetupSpec {
            file: spec("fake-setup.exe", b"setup bytes"),
            silent: "/VERYSILENT /NORESTART".into(),
            scope,
        });
        (
            manifest("3.0.0", e),
            transport(&[("fake-setup.exe", b"setup bytes")]),
        )
    }

    #[test]
    fn a_failing_setup_leaves_everything_unchanged() {
        let rig = Rig::new("setup-fail");
        let existing = Record {
            folder: rig.folder(),
            exe: rig.folder().join("fake.exe"),
            version: "1.0.0".into(),
            kind: "setup".into(),
            date: "x".into(),
        };
        rig.store.save_installed("fake", &existing).unwrap();
        std::fs::create_dir_all(rig.folder()).unwrap();
        std::fs::write(rig.folder().join("fake.exe"), b"old").unwrap();
        let runner = Arc::new(FakeRunner {
            code: 1,
            seen: Mutex::new(vec![]),
            make: Some(rig.folder()),
        });
        let mut opts = rig.opts();
        opts.elevate = Elevation::Custom(runner.clone());
        opts.redetect = Some(Arc::new(|_, _| panic!("must not look: setup failed")));
        let (m, t) = setup_manifest(Scope::User);
        let (r, _) = run(&rig, &m, Some(&rig.current()), &t, &opts);
        assert!(
            matches!(r, Err(InstallError::Setup { code: Some(1), .. })),
            "{r:?}"
        );
        assert_eq!(rig.store.load_installed("fake"), Some(existing));
        assert_eq!(read(&rig.folder().join("fake.exe")), b"old");
        assert!(
            !rig.cache("3.0.0").exists(),
            "the installer download is deleted"
        );
        assert_eq!(runner.seen.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_tampered_setup_is_never_run() {
        let rig = Rig::new("setup-hash");
        let runner = Arc::new(FakeRunner {
            code: 0,
            seen: Mutex::new(vec![]),
            make: None,
        });
        let mut opts = rig.opts();
        opts.elevate = Elevation::Custom(runner.clone());
        let (m, _) = setup_manifest(Scope::Machine);
        let t = transport(&[("fake-setup.exe", b"setup byteZ")]);
        let (r, _) = run(&rig, &m, None, &t, &opts);
        assert!(matches!(
            r,
            Err(InstallError::Mismatch { what: "sha256", .. })
        ));
        assert!(runner.seen.lock().unwrap().is_empty());
    }

    #[test]
    fn a_setup_that_succeeds_is_redetected_and_recorded() {
        let rig = Rig::new("setup-ok");
        let runner = Arc::new(FakeRunner {
            code: 0,
            seen: Mutex::new(vec![]),
            make: Some(rig.folder()),
        });
        let mut opts = rig.opts();
        opts.elevate = Elevation::Custom(runner.clone());
        let folder = rig.folder();
        opts.redetect = Some(Arc::new(move |_, _| {
            let exe = folder.join("fake.exe");
            exe.is_file().then(|| Installed {
                folder: folder.clone(),
                exe,
                version: None,
                source: Source::Folder,
                kind: String::new(),
            })
        }));
        let (m, t) = setup_manifest(Scope::Machine);
        let (r, seen) = run(&rig, &m, None, &t, &opts);
        let inst = r.unwrap();
        assert_eq!(inst.version, Some(Version::parse("3.0.0").unwrap()));
        assert_eq!(inst.kind, "setup");
        assert_eq!(inst.source, Source::Hub);
        assert_eq!(inst.exe, rig.folder().join("fake.exe"));
        let saved = rig.store.load_installed("fake").unwrap();
        assert_eq!(saved.exe, inst.exe);
        assert_eq!(
            *runner.seen.lock().unwrap(),
            [(
                "fake-setup.exe".to_string(),
                "/VERYSILENT /NORESTART".to_string(),
                Scope::Machine
            )]
        );
        assert_eq!(seen.last(), Some(&Progress::Finished));
        assert!(!rig.cache("3.0.0").exists());
    }

    #[test]
    fn a_setup_that_leaves_nothing_to_find_is_an_error() {
        let rig = Rig::new("setup-nofind");
        let runner = Arc::new(FakeRunner {
            code: 0,
            seen: Mutex::new(vec![]),
            make: None,
        });
        let mut opts = rig.opts();
        opts.elevate = Elevation::Custom(runner);
        opts.redetect = Some(Arc::new(|_, _| None));
        let (m, t) = setup_manifest(Scope::User);
        let (r, _) = run(&rig, &m, None, &t, &opts);
        assert!(matches!(r, Err(InstallError::Setup { code: Some(0), .. })));
        assert!(rig.store.load_installed("fake").is_none());
    }

    #[test]
    fn when_the_second_rename_fails_the_first_file_is_put_back_and_nothing_is_recorded() {
        let rig = Rig::new("swapfail");
        std::fs::create_dir_all(rig.folder()).unwrap();
        std::fs::write(rig.folder().join("fake.exe"), b"exe v1").unwrap();
        std::fs::write(rig.folder().join("fake-engine.exe"), b"engine v1").unwrap();
        // A folder where the engine's retired copy goes makes its swap fail after the exe's.
        std::fs::create_dir_all(rig.folder().join("fake-engine.exe.old").join("in-the-way"))
            .unwrap();
        let t = transport(&[("fake.exe", b"exe v2"), ("fake-engine.exe", b"engine v2")]);
        let m = files_manifest(
            "2.0.0",
            &[("fake.exe", b"exe v2"), ("fake-engine.exe", b"engine v2")],
        );
        let cur = rig.current();
        let (r, seen) = run(&rig, &m, Some(&cur), &t, &rig.opts());
        assert!(matches!(r, Err(InstallError::Io(_))), "{r:?}");
        assert!(seen.contains(&Progress::Placing));
        assert_eq!(read(&rig.folder().join("fake.exe")), b"exe v1");
        assert_eq!(read(&rig.folder().join("fake-engine.exe")), b"engine v1");
        assert_eq!(
            names(&rig.folder()),
            ["fake-engine.exe", "fake-engine.exe.old", "fake.exe"],
            "no .new or .old of ours is left"
        );
        assert!(rig.store.load_installed("fake").is_none());
        assert!(!rig.store.rollback_dir("fake").exists());
        assert!(!rig.cache("2.0.0").exists());
    }

    #[test]
    fn rollback_restores_the_previous_exe_bytes() {
        let rig = Rig::new("rollback");
        std::fs::create_dir_all(rig.folder()).unwrap();
        std::fs::write(rig.folder().join("fake.exe"), b"exe v1").unwrap();
        let t = transport(&[("fake.exe", b"exe v2"), ("fake-new.exe", b"added in v2")]);
        let m = files_manifest(
            "2.0.0",
            &[("fake.exe", b"exe v2"), ("fake-new.exe", b"added in v2")],
        );
        let cur = rig.current();
        run(&rig, &m, Some(&cur), &t, &rig.opts()).0.unwrap();
        assert_eq!(read(&rig.folder().join("fake.exe")), b"exe v2");

        let back = rollback_with(&APP, &rig.store, &rig.opts(), &mut |_| {}).unwrap();
        assert_eq!(read(&rig.folder().join("fake.exe")), b"exe v1");
        assert_eq!(back.version, Some(Version::parse("1.0.0").unwrap()));
        assert_eq!(
            names(&rig.folder()),
            ["fake.exe"],
            "the file v2 added is gone, nothing left over"
        );
        assert_eq!(rig.store.load_installed("fake").unwrap().version, "1.0.0");
        assert_eq!(
            rollback(&APP, &rig.store),
            Err(InstallError::NoRollback),
            "one roll back, then nothing"
        );
    }

    #[test]
    fn rollback_with_nothing_kept_or_a_garbled_note_is_an_error_not_a_panic() {
        let rig = Rig::new("rollback-none");
        assert_eq!(rollback(&APP, &rig.store), Err(InstallError::NoRollback));
        let rb = rig.store.rollback_dir("fake");
        std::fs::create_dir_all(&rb).unwrap();
        for junk in [
            &b"\xff\xfe\x00garbage"[..],
            b"",
            b"hub-rollback: 1\nrestore: ..\\evil.exe\nfolder: C:\\x\nexe: C:\\x\\a.exe\n",
            b"hub-rollback: 1\nrestore: a.exe\n",
            b"hub-rollback: 2\nrestore: a.exe\nfolder: x\nexe: y\n",
        ] {
            std::fs::write(rb.join(ROLLBACK_NOTE), junk).unwrap();
            assert_eq!(rollback(&APP, &rig.store), Err(InstallError::NoRollback));
        }
        // A note naming a file that is not there.
        std::fs::write(
            rb.join(ROLLBACK_NOTE),
            format!(
                "hub-rollback: 1\nfolder: {0}\nexe: {0}\\a.exe\nrestore: a.exe\n",
                rig.root.display()
            ),
        )
        .unwrap();
        assert_eq!(rollback(&APP, &rig.store), Err(InstallError::NoRollback));
    }

    struct FakeCloser {
        running: AtomicBool,
        closes_after_ask: bool,
        asks: AtomicUsize,
    }
    impl Closer for FakeCloser {
        fn is_running(&self, _: &[PathBuf]) -> bool {
            self.running.load(Ordering::Relaxed)
        }
        fn ask_close(&self, _: &[PathBuf]) {
            self.asks.fetch_add(1, Ordering::Relaxed);
            if self.closes_after_ask {
                self.running.store(false, Ordering::Relaxed);
            }
        }
    }

    #[test]
    fn an_app_that_will_not_close_stops_the_install_with_the_old_version_intact() {
        let rig = Rig::new("stuck");
        std::fs::create_dir_all(rig.folder()).unwrap();
        std::fs::write(rig.folder().join("fake.exe"), b"exe v1").unwrap();
        let closer = Arc::new(FakeCloser {
            running: AtomicBool::new(true),
            closes_after_ask: false,
            asks: AtomicUsize::new(0),
        });
        let mut opts = rig.opts();
        opts.closer = closer.clone();
        let t = transport(&[("fake.exe", b"exe v2")]);
        let m = files_manifest("2.0.0", &[("fake.exe", b"exe v2")]);
        let cur = rig.current();
        let (r, seen) = run(&rig, &m, Some(&cur), &t, &opts);
        assert_eq!(r, Err(InstallError::Running("fake.exe".into())));
        assert!(closer.asks.load(Ordering::Relaxed) >= 1);
        assert!(seen.contains(&Progress::Waiting("fake.exe".into())));
        assert_eq!(read(&rig.folder().join("fake.exe")), b"exe v1");
        assert_eq!(names(&rig.folder()), ["fake.exe"], "staged copies removed");
        assert!(
            rig.cache("2.0.0").join("fake.exe").is_file(),
            "the verified download is kept for a retry"
        );
    }

    #[test]
    fn an_app_that_closes_when_asked_is_updated() {
        let rig = Rig::new("closes");
        std::fs::create_dir_all(rig.folder()).unwrap();
        std::fs::write(rig.folder().join("fake.exe"), b"exe v1").unwrap();
        let closer = Arc::new(FakeCloser {
            running: AtomicBool::new(true),
            closes_after_ask: true,
            asks: AtomicUsize::new(0),
        });
        let mut opts = rig.opts();
        opts.closer = closer.clone();
        let t = transport(&[("fake.exe", b"exe v2")]);
        let m = files_manifest("2.0.0", &[("fake.exe", b"exe v2")]);
        let cur = rig.current();
        let (r, seen) = run(&rig, &m, Some(&cur), &t, &opts);
        r.unwrap();
        assert_eq!(closer.asks.load(Ordering::Relaxed), 1);
        assert!(seen.contains(&Progress::Waiting("fake.exe".into())));
        assert_eq!(read(&rig.folder().join("fake.exe")), b"exe v2");
    }

    #[test]
    fn zip_and_missing_platform_are_refused() {
        let rig = Rig::new("zip");
        let t = transport(&[]);
        let (r, _) = run(
            &rig,
            &manifest("2.0.0", entry(Kind::Zip)),
            None,
            &t,
            &rig.opts(),
        );
        assert!(matches!(r, Err(InstallError::Unsupported(s)) if s.contains("zip")));
        let mut e = entry(Kind::Files);
        e.platform = "linux-x86_64".into();
        let (r, _) = run(&rig, &manifest("2.0.0", e), None, &t, &rig.opts());
        assert!(matches!(r, Err(InstallError::Unsupported(_))));
        assert!(t.calls.lock().unwrap().is_empty());
    }

    #[test]
    fn names_and_paths_that_could_escape_the_folder_are_refused() {
        let rig = Rig::new("names");
        let t = transport(&[("fake.exe", b"x")]);
        for bad in ["..\\evil.exe", "a/b.exe", "C:evil.exe", "", "..", "a b.exe"] {
            let mut f = spec("fake.exe", b"x");
            f.name = bad.into();
            let mut e = entry(Kind::Files);
            e.files = vec![f];
            let (r, _) = run(&rig, &manifest("2.0.0", e), None, &t, &rig.opts());
            assert!(matches!(r, Err(InstallError::Invalid(_))), "{bad:?}: {r:?}");
        }
        let mut e = entry(Kind::Files);
        e.files = vec![spec("fake.exe", b"x")];
        e.exe = Some("..\\..\\x.exe".into());
        let (r, _) = run(&rig, &manifest("2.0.0", e), None, &t, &rig.opts());
        assert!(matches!(r, Err(InstallError::Invalid(_))));
        // The exe must be among the files or already there.
        let mut e = entry(Kind::Files);
        e.files = vec![spec("fake.exe", b"x")];
        e.exe = Some("other.exe".into());
        let (r, _) = run(&rig, &manifest("2.0.0", e), None, &t, &rig.opts());
        assert!(matches!(r, Err(InstallError::Invalid(_))));
        assert!(!rig.folder().exists());
    }

    #[test]
    fn a_transport_failure_is_reported_with_the_file_and_changes_nothing() {
        let rig = Rig::new("net");
        let t = transport(&[]);
        let m = files_manifest("2.0.0", &[("fake.exe", b"exe v2")]);
        let (r, _) = run(&rig, &m, None, &t, &rig.opts());
        assert_eq!(
            r,
            Err(InstallError::Download {
                file: "fake.exe".into(),
                reason: "404".into()
            })
        );
        assert!(!rig.folder().exists());
    }

    #[test]
    fn launch_runs_the_exe_in_its_folder_with_the_catalogs_arguments() {
        const LAUNCH: CatalogApp = CatalogApp {
            launch_args: &[
                "--exact",
                "install::tests::launch_helper",
                "--ignored",
                "--nocapture",
            ],
            ..APP
        };
        let rig = Rig::new("launch");
        let exe = rig.root.join("fakeapp.exe");
        std::fs::copy(std::env::current_exe().unwrap(), &exe).unwrap();
        let inst = Installed {
            folder: rig.root.clone(),
            exe,
            version: None,
            source: Source::Hub,
            kind: String::new(),
        };
        let cmd = launch_command(&LAUNCH, &inst);
        let args: Vec<_> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args, LAUNCH.launch_args);
        assert_eq!(cmd.get_current_dir(), Some(rig.root.as_path()));

        let marker = rig.root.join("marker.txt");
        // SAFETY: only this test sets this variable, before the child (which reads it) starts.
        unsafe { std::env::set_var("HUB_LAUNCH_MARKER", &marker) };
        launch(&LAUNCH, &inst).unwrap();
        let start = Instant::now();
        while !marker.is_file() && start.elapsed() < Duration::from_secs(20) {
            std::thread::sleep(Duration::from_millis(50));
        }
        let text = std::fs::read_to_string(&marker).expect("the launched program ran");
        let mut lines = text.lines();
        assert_eq!(
            PathBuf::from(lines.next().unwrap()).canonicalize().unwrap(),
            rig.root.canonicalize().unwrap(),
            "working directory is the install folder"
        );
        assert!(text.contains("launch_helper"), "arguments arrived: {text}");
    }

    /// Run by `launch_runs_the_exe...` as the launched program; does nothing in a normal run.
    #[test]
    #[ignore = "helper process for the launch test"]
    fn launch_helper() {
        if let Some(path) = std::env::var_os("HUB_LAUNCH_MARKER") {
            let cwd = std::env::current_dir().unwrap_or_default();
            let args: Vec<String> = std::env::args().collect();
            _ = std::fs::write(path, format!("{}\n{}\n", cwd.display(), args.join(" ")));
        }
    }

    #[test]
    fn closing_windows_of_a_program_that_is_not_running_is_harmless() {
        RealCloser.ask_close(&[PathBuf::from("C:\\definitely\\not\\here\\nothing.exe")]);
    }
}
