//! Hub updates itself, on top of the shared updater (`mdshared_app::updater`), unchanged in its
//! format and its swap: Hub's own release carries `hub-update.txt` (format 1), one executable, no
//! engine, no uninstall entry.
//!
//! What this adds is the order Hub needs (plan section 7). The shared `install` downloads and
//! swaps in one go; Hub must swap itself LAST, when no install of an app is running, so this
//! splits it in two: [`stage`] downloads and verifies and leaves `hub.exe.new` beside the
//! running exe (nothing replaced), and [`apply_when_idle`] swaps it in only when `busy` is false.
//! Then [`restart`] starts the new Hub with `relaunch`'s flag. If the swap fails `swap_in` puts
//! the old exe back and Hub keeps running as it was.
//!
//! Nothing here runs at start or at idle: the engine calls [`check`] on its schedule, after the
//! apps (Hub checks itself first, so its own update shows at the top), on a worker thread.
use crate::transport::Transport;
use mdshared_app::relaunch;
use mdshared_app::selfupdate as swap;
use mdshared_app::update::{self, Download, Version};
use mdshared_app::updater::{self, App, Files};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Hub's own manifest: the one place that names Hub's repository (a working name until the
/// owner names it). The pin on every download URL ([`releases_prefix`]) is cut from it.
pub const MANIFEST_URL: &str =
    "https://github.com/Kaan88/hub-rs/releases/latest/download/hub-update.txt";
/// Names another manifest, to try a release before it is the latest (the shared rule).
pub const MANIFEST_ENV: &str = "HUB_UPDATE_MANIFEST";

const FILE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Hub is a few MiB; a file larger than this is not Hub.
const MAX_EXE_BYTES: usize = 64 * 1024 * 1024;

/// This build as the shared updater knows it.
pub fn app() -> App {
    App {
        name: "hub",
        version: env!("CARGO_PKG_VERSION"),
        manifest_url: MANIFEST_URL,
        manifest_env: MANIFEST_ENV,
        engine: None,
        extras: &[],
        uninstall_key: None,
    }
}

/// `https://github.com/<owner>/<repo>/releases/`: where every download of a manifest read from
/// [`MANIFEST_URL`] must lie.
pub fn releases_prefix() -> &'static str {
    const MARK: &str = "/releases/";
    match MANIFEST_URL.find(MARK) {
        Some(i) => &MANIFEST_URL[..i + MARK.len()],
        None => MANIFEST_URL,
    }
}

/// A newer Hub that can be installed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    pub version: Version,
    pub notes: Option<String>,
    /// Known once it is downloaded (the format has no size).
    pub size: Option<u64>,
    pub download: Download,
}

/// A newer Hub, downloaded and verified, waiting beside the running exe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Staged {
    pub version: Version,
    /// The exe that will be replaced; its `.new` neighbour holds the new one.
    pub exe: PathBuf,
    pub sha256: String,
    pub size: u64,
}

/// What the swap came to.
#[derive(Debug, PartialEq, Eq)]
pub enum Apply {
    /// An install is running (or the caller said so): nothing was touched, ask again later.
    Deferred,
    /// The new Hub is in place; the running process is still the old one. Call [`restart`].
    Swapped,
    /// Nothing changed (the old exe is in place); why.
    Failed(String),
}

/// Where Hub's own update stands, for the window and the tray. The engine holds one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Status {
    #[default]
    None,
    Available(Offer),
    Downloading(Version),
    Ready(Staged),
    Failed(String),
}

impl Status {
    pub fn label(&self) -> String {
        match self {
            Status::None => String::new(),
            Status::Available(o) => format!("Hub {} is available", o.version),
            Status::Downloading(v) => format!("Downloading Hub {v}..."),
            Status::Ready(s) => format!("Hub {} is ready: restart to finish", s.version),
            Status::Failed(e) => format!("Hub could not update itself: {e}"),
        }
    }
}

/// Looks at Hub's manifest. A manifest named by `app.manifest_env` is read as it is (the shared
/// trial rule); otherwise every download must lie under [`releases_prefix`].
pub fn check(app: &App, t: &dyn Transport) -> Result<Option<Offer>, String> {
    let pin = std::env::var_os(app.manifest_env)
        .is_none()
        .then(releases_prefix);
    check_pinned(app, t, pin)
}

/// [`check`] with the prefix given (`None` = no pin).
pub fn check_pinned(
    app: &App,
    t: &dyn Transport,
    prefix: Option<&str>,
) -> Result<Option<Offer>, String> {
    let fetch = |url: &str, max: usize, timeout: Duration| t.get(url, max, timeout);
    let Some(o) = updater::check(app, &fetch)? else {
        return Ok(None);
    };
    if let Some(prefix) = prefix
        && !o.app.url.starts_with(prefix)
    {
        return Err("the update is not from Hub's own releases".to_string());
    }
    Ok(Some(Offer {
        version: o.version,
        notes: o.notes,
        size: None,
        download: o.app,
    }))
}

/// Downloads `offer`, checks its SHA-256, and writes it beside `exe` as `exe.new`, synced to the
/// disk. `exe` is not touched. On any failure nothing is left behind.
pub fn stage(offer: &Offer, exe: &Path, t: &dyn Transport) -> Result<Staged, String> {
    let bytes = t.get(&offer.download.url, MAX_EXE_BYTES, FILE_TIMEOUT)?;
    if !update::verify_sha256(&bytes, &offer.download.sha256) {
        return Err("the download does not match its SHA-256".to_string());
    }
    let new = swap::staged(exe);
    if let Err(e) = write_synced(&new, &bytes) {
        let _ = swap::remove_any(&new);
        return Err(format!("could not write the download: {e}"));
    }
    Ok(Staged {
        version: offer.version.clone(),
        exe: exe.to_path_buf(),
        sha256: offer.download.sha256.clone(),
        size: bytes.len() as u64,
    })
}

fn write_synced(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut f = std::fs::File::create(path)?;
    f.write_all(bytes)?;
    f.sync_all()
}

/// Swaps the staged Hub in, but only when `busy` is false (no install of an app is running: an
/// install must not lose its Hub halfway, and Hub swaps itself last). The staged file is checked
/// against its hash once more first. A failure leaves the old exe in place and discards the
/// staged copy.
pub fn apply_when_idle(staged: &Staged, busy: bool) -> Apply {
    if busy {
        return Apply::Deferred;
    }
    let new = swap::staged(&staged.exe);
    match update::verify_sha256_file(&new, &staged.sha256) {
        Ok(true) => {}
        Ok(false) => {
            let _ = swap::remove_any(&new);
            return Apply::Failed("the staged update changed on the disk".to_string());
        }
        Err(e) => return Apply::Failed(format!("the staged update is gone: {e}")),
    }
    match swap::swap_in(std::slice::from_ref(&staged.exe)) {
        Ok(()) => Apply::Swapped,
        Err(e) => {
            // `swap_in` has put the old exe back; the staged copy would fail the same way.
            let _ = swap::remove_any(&new);
            Apply::Failed(e.to_string())
        }
    }
}

/// What to start for the restart: the exe, if it is there and is not a `.new` or `.old` copy.
pub fn restart_target(exe: &Path) -> Option<PathBuf> {
    let files = Files {
        app: exe.to_path_buf(),
        engine: None,
        extras: Vec::new(),
    };
    relaunch::target(Some(&files))
}

/// Starts the new Hub with `--restarted`. Call it last: the window closed, the tray icon removed.
/// The new process waits for this one to go (`relaunch::wait_for_predecessor`).
pub fn restart(exe: &Path) -> std::io::Result<()> {
    match restart_target(exe) {
        Some(t) => relaunch::spawn(&t),
        None => Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "there is no Hub to start",
        )),
    }
}

/// This process's exe when this copy may update itself (not a cargo build's output folder).
pub fn installed_exe() -> Option<PathBuf> {
    updater::installed_files(&app()).map(|f| f.app)
}

/// Removes what an earlier update left beside `exe` (`.old`, `.new`). At start, before anything
/// is staged: it would remove a pending staged copy too.
pub fn clean_up(exe: &Path) {
    swap::clean_up(std::slice::from_ref(&exe.to_path_buf()));
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicU32, Ordering};

    const BASE: &str = "https://github.com/Kaan88/hub-rs/releases/";
    const MURL: &str = "https://github.com/Kaan88/hub-rs/releases/latest/download/hub-update.txt";

    struct Fake {
        files: Mutex<HashMap<String, Vec<u8>>>,
        log: Mutex<Vec<String>>,
    }
    impl Fake {
        fn new() -> Fake {
            Fake {
                files: Mutex::new(HashMap::new()),
                log: Mutex::new(Vec::new()),
            }
        }
        fn put(&self, url: &str, bytes: &[u8]) {
            self.files
                .lock()
                .unwrap()
                .insert(url.to_string(), bytes.to_vec());
        }
        fn asked(&self) -> Vec<String> {
            self.log.lock().unwrap().clone()
        }
    }
    impl Transport for Fake {
        fn get(&self, url: &str, max: usize, _t: Duration) -> Result<Vec<u8>, String> {
            self.log.lock().unwrap().push(url.to_string());
            let b = self
                .files
                .lock()
                .unwrap()
                .get(url)
                .cloned()
                .ok_or_else(|| "Status(404)".to_string())?;
            if b.len() > max {
                return Err("TooLarge".into());
            }
            Ok(b)
        }
    }

    fn manifest(version: &str, url: &str, sha: &str) -> String {
        format!(
            "manifest: 1\nversion: {version}\nnotes: Faster tray.\nwindows-x86_64.asset: hub.exe\nwindows-x86_64.url: {url}\nwindows-x86_64.sha256: {sha}\n"
        )
    }

    fn test_app(version: &'static str) -> App {
        App {
            name: "hub",
            version,
            manifest_url: MURL,
            // A name nothing sets: the default URL is read.
            manifest_env: "HUB_TEST_SELFUPDATE_NEVER_SET",
            engine: None,
            extras: &[],
            uninstall_key: None,
        }
    }

    fn scratch(name: &str) -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let d = std::env::temp_dir().join(format!(
            "hubtest-selfupdate-{}-{}-{name}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A fake release 0.2.0 on the fake github, and a scratch folder with the "installed" exe.
    fn world(name: &str, new_bytes: &[u8]) -> (Fake, PathBuf, PathBuf, Offer) {
        let dir = scratch(name);
        let exe = dir.join("hub.exe");
        std::fs::write(&exe, b"old hub").unwrap();
        let url = format!("{BASE}download/v0.2.0/hub.exe");
        let fake = Fake::new();
        fake.put(&url, new_bytes);
        fake.put(
            MURL,
            manifest("0.2.0", &url, &update::sha256_hex(new_bytes)).as_bytes(),
        );
        let offer = check_pinned(&test_app("0.1.0"), &fake, Some(BASE))
            .unwrap()
            .unwrap();
        (fake, dir, exe, offer)
    }

    fn read(p: &Path) -> Vec<u8> {
        std::fs::read(p).unwrap()
    }

    #[test]
    fn the_app_is_hub_with_one_exe_and_the_one_manifest_constant() {
        let a = app();
        assert_eq!(a.name, "hub");
        assert_eq!(a.manifest_url, MANIFEST_URL);
        assert_eq!(a.manifest_env, "HUB_UPDATE_MANIFEST");
        assert!(a.engine.is_none() && a.extras.is_empty() && a.uninstall_key.is_none());
        assert_eq!(
            releases_prefix(),
            "https://github.com/Kaan88/hub-rs/releases/"
        );
        assert!(a.current_version() >= Version::parse("0.1.0").unwrap());
    }

    #[test]
    fn a_newer_version_is_offered_with_its_notes() {
        let (fake, dir, _exe, offer) = world("newer", b"new hub");
        assert_eq!(offer.version, Version::parse("0.2.0").unwrap());
        assert_eq!(offer.notes.as_deref(), Some("Faster tray."));
        assert_eq!(offer.size, None);
        assert_eq!(fake.asked(), vec![MURL.to_string()]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_same_or_an_older_version_is_not_offered() {
        let fake = Fake::new();
        let url = format!("{BASE}download/v0.2.0/hub.exe");
        fake.put(MURL, manifest("0.2.0", &url, &"a".repeat(64)).as_bytes());
        assert_eq!(
            check_pinned(&test_app("0.2.0"), &fake, Some(BASE)),
            Ok(None)
        );
        assert_eq!(
            check_pinned(&test_app("0.3.0"), &fake, Some(BASE)),
            Ok(None)
        );
    }

    #[test]
    fn pre_releases_order_as_the_shared_version_does() {
        let fake = Fake::new();
        let url = format!("{BASE}download/v0.2.0-rc.1/hub.exe");
        let sha = "a".repeat(64);
        // A release candidate is newer than the release before it ...
        fake.put(MURL, manifest("0.2.0-rc.1", &url, &sha).as_bytes());
        let o = check_pinned(&test_app("0.1.0"), &fake, Some(BASE))
            .unwrap()
            .unwrap();
        assert_eq!(o.version.to_string(), "0.2.0-rc.1");
        // ... older than its own release, and newer than an older candidate.
        assert_eq!(
            check_pinned(&test_app("0.2.0"), &fake, Some(BASE)),
            Ok(None)
        );
        assert!(
            check_pinned(&test_app("0.2.0-beta.1"), &fake, Some(BASE))
                .unwrap()
                .is_some()
        );
        // The release is offered over its candidate.
        fake.put(MURL, manifest("0.2.0", &url, &sha).as_bytes());
        assert!(
            check_pinned(&test_app("0.2.0-rc.1"), &fake, Some(BASE))
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn a_download_outside_hubs_releases_is_refused_unless_the_trial_variable_names_the_manifest() {
        let fake = Fake::new();
        let foreign = "https://example.com/hub.exe";
        fake.put(MURL, manifest("0.2.0", foreign, &"a".repeat(64)).as_bytes());
        let e = check_pinned(&test_app("0.1.0"), &fake, Some(BASE)).unwrap_err();
        assert!(e.contains("own releases"), "{e}");
        // No pin (what `check` does when the variable is set): read as it is.
        assert!(
            check_pinned(&test_app("0.1.0"), &fake, None)
                .unwrap()
                .is_some()
        );
    }

    #[test]
    fn the_manifest_variable_is_honoured_and_the_default_is_then_not_asked() {
        static APP: App = App {
            name: "hub",
            version: "0.1.0",
            manifest_url: MURL,
            manifest_env: "HUB_TEST_SELFUPDATE_MANIFEST_X7",
            engine: None,
            extras: &[],
            uninstall_key: None,
        };
        let trial =
            "https://github.com/Kaan88/hub-rs/releases/download/v0.2.0-trial/hub-update.txt";
        let url = format!("{BASE}download/v0.2.0-trial/hub.exe");
        let fake = Fake::new();
        fake.put(trial, manifest("0.2.0", &url, &"b".repeat(64)).as_bytes());
        // SAFETY: a variable only this test reads or writes.
        unsafe { std::env::set_var("HUB_TEST_SELFUPDATE_MANIFEST_X7", trial) };
        let got = check(&APP, &fake);
        // SAFETY: as above.
        unsafe { std::env::remove_var("HUB_TEST_SELFUPDATE_MANIFEST_X7") };
        assert_eq!(got.unwrap().unwrap().version.to_string(), "0.2.0");
        assert_eq!(fake.asked(), vec![trial.to_string()]);
    }

    #[test]
    fn a_manifest_that_is_not_there_or_not_a_manifest_is_an_error_not_a_panic() {
        let fake = Fake::new();
        assert!(check_pinned(&test_app("0.1.0"), &fake, Some(BASE)).is_err());
        fake.put(MURL, b"\xff\xfe not a manifest \0");
        assert!(check_pinned(&test_app("0.1.0"), &fake, Some(BASE)).is_err());
        fake.put(MURL, b"manifest: 1\nversion: 0.2.0\n");
        assert!(check_pinned(&test_app("0.1.0"), &fake, Some(BASE)).is_err());
    }

    #[test]
    fn stage_writes_the_verified_download_beside_the_exe_and_replaces_nothing() {
        let (fake, dir, exe, offer) = world("stage", b"new hub");
        let st = stage(&offer, &exe, &fake).unwrap();
        assert_eq!(st.version, offer.version);
        assert_eq!(st.size, 7);
        assert_eq!(read(&exe), b"old hub");
        assert_eq!(read(&swap::staged(&exe)), b"new hub");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_hash_mismatch_is_refused_and_nothing_is_swapped_or_left() {
        let (fake, dir, exe, mut offer) = world("mismatch", b"new hub");
        offer.download.sha256 = update::sha256_hex(b"something else");
        let e = stage(&offer, &exe, &fake).unwrap_err();
        assert!(e.contains("SHA-256"), "{e}");
        assert_eq!(read(&exe), b"old hub");
        assert!(!swap::staged(&exe).exists());
        assert!(!swap::retired(&exe).exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_failed_download_leaves_nothing() {
        let (fake, dir, exe, mut offer) = world("nodl", b"new hub");
        offer.download.url = format!("{BASE}download/v9/missing.exe");
        assert!(stage(&offer, &exe, &fake).is_err());
        assert!(!swap::staged(&exe).exists());
        assert_eq!(read(&exe), b"old hub");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_busy_install_defers_the_swap_and_an_idle_hub_swaps() {
        let (fake, dir, exe, offer) = world("busy", b"new hub");
        let st = stage(&offer, &exe, &fake).unwrap();
        assert_eq!(apply_when_idle(&st, true), Apply::Deferred);
        assert_eq!(apply_when_idle(&st, true), Apply::Deferred);
        // Nothing moved while the install ran, and the staged copy waits.
        assert_eq!(read(&exe), b"old hub");
        assert_eq!(read(&swap::staged(&exe)), b"new hub");
        assert!(!swap::retired(&exe).exists());
        assert_eq!(apply_when_idle(&st, false), Apply::Swapped);
        assert_eq!(read(&exe), b"new hub");
        assert_eq!(read(&swap::retired(&exe)), b"old hub");
        assert!(!swap::staged(&exe).exists());
        // The next start tidies the old copy away.
        clean_up(&exe);
        assert!(!swap::retired(&exe).exists());
        assert_eq!(read(&exe), b"new hub");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_swap_that_fails_puts_the_old_exe_back() {
        use std::os::windows::fs::OpenOptionsExt;
        let (fake, dir, exe, offer) = world("swapfail", b"new hub");
        let st = stage(&offer, &exe, &fake).unwrap();
        // Hold the staged file open without sharing delete: renaming it into place fails after
        // the old exe was moved aside, which is the case `swap_in` must undo.
        let hold = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(swap::staged(&exe))
            .unwrap();
        let r = apply_when_idle(&st, false);
        assert!(matches!(r, Apply::Failed(_)), "{r:?}");
        drop(hold);
        assert_eq!(read(&exe), b"old hub", "the old exe is back");
        assert!(!swap::retired(&exe).exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_staged_file_that_changed_on_the_disk_is_not_swapped_in() {
        let (fake, dir, exe, offer) = world("tamper", b"new hub");
        let st = stage(&offer, &exe, &fake).unwrap();
        std::fs::write(swap::staged(&exe), b"evil hub").unwrap();
        let r = apply_when_idle(&st, false);
        assert!(matches!(r, Apply::Failed(_)), "{r:?}");
        assert_eq!(read(&exe), b"old hub");
        assert!(!swap::staged(&exe).exists());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_staged_file_that_vanished_is_a_failure_not_a_panic() {
        let (fake, dir, exe, offer) = world("vanish", b"new hub");
        let st = stage(&offer, &exe, &fake).unwrap();
        std::fs::remove_file(swap::staged(&exe)).unwrap();
        assert!(matches!(apply_when_idle(&st, false), Apply::Failed(_)));
        assert_eq!(read(&exe), b"old hub");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_restart_target_is_the_installed_exe_never_a_leftover() {
        let dir = scratch("target");
        let exe = dir.join("hub.exe");
        assert_eq!(restart_target(&exe), None, "no exe, nothing to start");
        std::fs::write(&exe, b"x").unwrap();
        assert_eq!(restart_target(&exe), Some(exe.clone()));
        std::fs::write(swap::staged(&exe), b"x").unwrap();
        assert_eq!(restart_target(&swap::staged(&exe)), None);
        std::fs::write(swap::retired(&exe), b"x").unwrap();
        assert_eq!(restart_target(&swap::retired(&exe)), None);
        assert!(restart(&dir.join("absent.exe")).is_err());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_status_says_what_the_window_shows() {
        let (_f, dir, exe, offer) = world("status", b"new hub");
        assert_eq!(Status::None.label(), "");
        assert_eq!(
            Status::Available(offer.clone()).label(),
            "Hub 0.2.0 is available"
        );
        assert_eq!(
            Status::Downloading(offer.version.clone()).label(),
            "Downloading Hub 0.2.0..."
        );
        let st = Staged {
            version: offer.version.clone(),
            exe,
            sha256: offer.download.sha256.clone(),
            size: 7,
        };
        assert_eq!(
            Status::Ready(st).label(),
            "Hub 0.2.0 is ready: restart to finish"
        );
        assert!(
            Status::Failed("x".into())
                .label()
                .contains("could not update")
        );
        let _ = std::fs::remove_dir_all(dir);
    }
}
