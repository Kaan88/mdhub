//! The glue between hub-core and the window: the headless `Engine` that owns what Hub knows (what
//! is installed, what is latest, what an install is doing), turns a `Hit` or a key into work, runs
//! the work on worker threads one install at a time, and builds the `Vm` the drawing code reads.
//! Nothing here touches a window, so every behaviour is tested without one. Nothing here asks the
//! network anything until a check is asked for (the first one is scheduled, never run at start).
use crate::vm::{Act, AppVm, Banner, Hit, NotesState, NotesVm, View, Vm};
use crate::work;
use hub_core::catalog::{CATALOG, CatalogApp};
use hub_core::check;
use hub_core::detect::{self, Env};
use hub_core::install::{Closer, Elevation, InstallError, Opts, Progress, RealCloser};
use hub_core::manifest;
use hub_core::model::{HubManifest, Installed, Kind, Latest, Row, Scope, Source};
use hub_core::notify::{Memory, Message, Pending};
use hub_core::selfupdate::{self, Apply, Offer, Staged, Status};
use hub_core::state::{Store, ViewMode};
use hub_core::transport::Transport;
use mdshared_ui::settings::{Row as SettingRow, Value};
use std::cell::Cell;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// What the worker threads and the shell tell the engine.
#[derive(Clone, Debug)]
pub enum Event {
    Progress {
        id: &'static str,
        p: Progress,
    },
    /// An install or a roll back ended.
    Finished {
        id: &'static str,
        result: Result<Installed, InstallError>,
    },
    /// A check ended: what each app's release says now.
    Checked(Vec<(&'static str, Latest)>),
    /// Which installed apps run, looked up on a worker (never on the window's thread).
    Running(Vec<(&'static str, bool)>),
    /// A second launch asked for this window (the shell raises it, or makes it).
    Raise,
    /// The tray icon's menu, its icon or its balloon was used (`tray.rs`). Acted on by the shell.
    Tray(crate::tray::TrayCmd),
    /// Hub's own update, looked at and downloaded on the check's worker.
    Hub(HubEvent),
    /// The full release notes of `version` of `id` were fetched (cleaned text) or could not be
    /// (the reason). Asked for only when the notes view is opened.
    Notes {
        id: &'static str,
        version: String,
        result: Result<String, String>,
    },
}

/// What the check worker found out about Hub's own update.
#[derive(Clone, Debug)]
pub enum HubEvent {
    /// Hub's manifest was read: a newer Hub, none, or why not.
    Checked(Result<Option<Offer>, String>),
    /// The newer Hub was downloaded and verified (written beside the running exe), or why not.
    Staged(Result<Staged, String>),
}

/// Where a worker posts its events (the window's event loop, or a channel in a test).
pub trait Post: Send + Sync + 'static {
    fn post(&self, ev: Event);
}

/// What the engine does to the outside world besides installing: start an app, show a folder,
/// open a page, ask whether an app runs. A test supplies one that only records.
pub trait Shell: Send + Sync {
    fn launch(&self, app: &CatalogApp, inst: &Installed) -> std::io::Result<()>;
    fn reveal(&self, path: &Path) -> bool;
    fn open_url(&self, url: &str) -> bool;
    fn running(&self, exe: &Path) -> bool;
}

pub struct RealShell;

impl Shell for RealShell {
    fn launch(&self, app: &CatalogApp, inst: &Installed) -> std::io::Result<()> {
        hub_core::install::launch(app, inst)
    }
    fn reveal(&self, path: &Path) -> bool {
        mdshared_ui::platform::reveal_in_file_manager(path)
    }
    fn open_url(&self, url: &str) -> bool {
        mdshared_ui::platform::open_url(url)
    }
    fn running(&self, exe: &Path) -> bool {
        detect::running(exe)
    }
}

/// What an install is run with besides the cancel flag.
#[derive(Clone)]
pub struct Tmpl {
    pub elevate: Elevation,
    pub closer: Arc<dyn Closer>,
    pub close_timeout: Duration,
    pub poll: Duration,
}

impl Tmpl {
    pub fn real() -> Tmpl {
        let d = Opts::default();
        Tmpl {
            elevate: Elevation::Real,
            closer: Arc::new(RealCloser),
            close_timeout: d.close_timeout,
            poll: d.poll,
        }
    }
}

/// What a worker (or a test) is given to work from.
pub struct Deps {
    pub store: Store,
    pub env: Env,
    pub transport: Arc<dyn Transport>,
    pub post: Arc<dyn Post>,
    pub shell: Arc<dyn Shell>,
    pub tmpl: Tmpl,
    /// The first check: after this long since start, and whether it runs even when not due.
    pub first_check: Option<(Duration, bool)>,
    /// The exe Hub would replace to update itself; None when this copy cannot (a build folder).
    pub hub_exe: Option<PathBuf>,
    /// Where "Start with Windows" is written.
    pub run_key: RunKey,
}

/// Where "Start with Windows" lives: a value under `HKCU\<key>` that starts `exe --tray`. The
/// real one is the Run key and the installed exe; a test names a key of its own.
#[derive(Clone, Debug)]
pub struct RunKey {
    pub key: String,
    pub value: String,
    /// What the value starts; None: this copy cannot be started at login (a build folder).
    pub exe: Option<PathBuf>,
}

impl RunKey {
    /// The real Run key, for the installed Hub. A copy that is not installed (a cargo build
    /// folder) has no exe to put there, and no business with the key at all: it gets no key, so
    /// it neither writes a build output's path into the owner's startup list nor removes (or
    /// shows) the value the installed Hub wrote.
    pub fn real() -> RunKey {
        RunKey::real_for(selfupdate::installed_exe())
    }

    fn real_for(installed: Option<PathBuf>) -> RunKey {
        match installed {
            Some(exe) => RunKey {
                key: hub_core::autostart::RUN_KEY.to_string(),
                value: hub_core::autostart::RUN_VALUE.to_string(),
                exe: Some(exe),
            },
            None => RunKey::none(),
        }
    }

    /// No key at all: every attempt is refused (the picture of `--demo`, tests that do not care).
    pub fn none() -> RunKey {
        RunKey {
            key: String::new(),
            value: String::new(),
            exe: None,
        }
    }
}

/// The Run key this run uses: the real one, except in a build with `test-hooks`, where
/// `HUB_RUN_KEY_PATH` names a key under `HKCU` (a test's own, never the real Run key) and the
/// value is started from this very exe.
pub fn system_run_key() -> RunKey {
    run_key_for(
        cfg!(feature = "test-hooks"),
        std::env::var("HUB_RUN_KEY_PATH").ok(),
        std::env::current_exe().ok(),
        selfupdate::installed_exe(),
    )
}

/// [`system_run_key`] with its inputs given. A `test_hooks` build never reaches the real Run key:
/// without `HUB_RUN_KEY_PATH` it has no key at all (a scratch copy of the exe counts as installed,
/// so it would otherwise write the owner's startup list), and a path outside `HKCU\Software\HubTest`
/// is refused the same way.
pub(crate) fn run_key_for(
    test_hooks: bool,
    env: Option<String>,
    current: Option<PathBuf>,
    installed: Option<PathBuf>,
) -> RunKey {
    if !test_hooks {
        return RunKey::real_for(installed);
    }
    match env {
        Some(k)
            if k.strip_prefix(hub_core::autostart::TEST_ROOT)
                .is_some_and(|t| t.starts_with('\\') && t.len() > 1) =>
        {
            RunKey {
                key: k,
                value: hub_core::autostart::RUN_VALUE.to_string(),
                exe: current,
            }
        }
        _ => RunKey::none(),
    }
}

/// The detection environment: the PC's own, except in a build with `test-hooks`, where
/// `HUB_PROGRAMS` (and `HUB_PROGRAM_FILES`, `;` separated) name folders standing in for
/// `%LOCALAPPDATA%\Programs` and Program Files, and the registry is not read at all.
pub fn system_env() -> Env {
    #[cfg(feature = "test-hooks")]
    if let Some(p) = std::env::var_os("HUB_PROGRAMS").filter(|p| !p.is_empty()) {
        let files = std::env::var_os("HUB_PROGRAM_FILES")
            .map(|v| std::env::split_paths(&v).collect())
            .unwrap_or_default();
        return Env::custom(p, files, Vec::new());
    }
    Env::system()
}

// ---- the row of one app -------------------------------------------------------------------------

/// What Hub is doing with one app right now.
#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    Idle,
    /// Waits for the install in front of it.
    Queued,
    Downloading {
        frac: f32,
        text: String,
    },
    Verifying,
    Placing,
    /// Waiting for the app to close.
    Waiting(String),
    Failed(String),
    /// Finished at this time (Unix seconds).
    Done(u64),
}

pub struct Slot {
    pub app: &'static CatalogApp,
    pub installed: Option<Installed>,
    pub latest: Latest,
    pub skipped: Option<String>,
    pub running: bool,
    pub phase: Phase,
}

fn load_slot(app: &'static CatalogApp, store: &Store, env: &Env) -> Slot {
    let latest = store
        .cached_manifest(app.id)
        .and_then(|b| manifest::parse(&b, app).ok())
        .map_or(Latest::Unknown, |m| Latest::Found(Box::new(m)));
    Slot {
        app,
        installed: detect::detect(app, store, env),
        latest,
        skipped: store.skipped(app.id),
        running: false,
        phase: Phase::Idle,
    }
}

/// "35.2 MB" (decimal), or empty for 0.
pub fn fmt_size(bytes: u64) -> String {
    match bytes {
        0 => String::new(),
        1..=999_999 => format!("{} KB", bytes.div_ceil(1000)),
        b => format!("{:.1} MB", b as f64 / 1e6),
    }
}

/// "21 Nov" from `YYYY-MM-DD`; anything else is shown as it is, or empty.
pub fn fmt_date(d: &str) -> String {
    const M: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    let mut it = d.split('-');
    let (_, m, day) = (it.next(), it.next(), it.next());
    match (m.and_then(|m| m.parse::<usize>().ok()), day) {
        (Some(m @ 1..=12), Some(day)) if it.next().is_none() => {
            format!("{} {}", day.trim_start_matches('0'), M[m - 1])
        }
        _ => d.chars().take(12).collect(),
    }
}

fn short_hash(h: &str) -> String {
    if h.chars().count() > 12 {
        format!("{}\u{2026}", h.chars().take(6).collect::<String>())
    } else {
        h.to_string()
    }
}

/// The words a failed install is shown with.
pub fn fail_text(e: &InstallError) -> String {
    let t = match e {
        InstallError::Mismatch {
            what,
            expected,
            received,
            ..
        } => format!(
            "The download did not match its {} (expected {}, got {}); nothing was installed",
            if *what == "sha256" { "SHA-256" } else { what },
            short_hash(expected),
            short_hash(received)
        ),
        InstallError::Running(n) => format!("{n} is still open: close it and try again"),
        InstallError::Download { file, reason } => format!("Could not download {file}: {reason}"),
        other => {
            let s = other.to_string();
            let mut c = s.chars();
            c.next()
                .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        }
    };
    t.chars().take(300).collect()
}

/// What the status line says when a roll back is asked for while the app is open.
fn roll_back_open(name: &str) -> String {
    format!("Close {name} first, then roll back")
}

fn age_text(done: u64, now: u64) -> String {
    let s = now.saturating_sub(done);
    match s {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{} minutes ago", s / 60).replace("1 minutes", "1 minute"),
        _ => format!("{} hours ago", s / 3600).replace("1 hours", "1 hour"),
    }
}

/// The one-line status text of the byte counts: "12.4 of 35.2 MB".
fn progress_text(done: u64, total: u64) -> String {
    if total == 0 {
        return fmt_size(done);
    }
    let mb = |b: u64| format!("{:.1}", b as f64 / 1e6);
    format!("{} of {} MB", mb(done), mb(total))
}

pub fn row_of(slot: &Slot, now: u64) -> Row {
    match &slot.phase {
        Phase::Queued => Row::Waiting("Waiting for another install to finish".into()),
        Phase::Downloading { frac, text } => Row::Downloading(*frac, text.clone()),
        Phase::Verifying => Row::Verifying,
        Phase::Placing => Row::Downloading(1.0, "Putting the files in place".into()),
        Phase::Waiting(t) => Row::Waiting(t.clone()),
        Phase::Failed(t) => Row::Failed(t.clone()),
        Phase::Done(t) => Row::Done(age_text(*t, now)),
        Phase::Idle => match (&slot.installed, &slot.latest) {
            // Nothing to install from: say why instead of offering a button that cannot work.
            (None, Latest::Unknown) => Row::NoInfo("Not checked yet".into()),
            (None, Latest::NoManifest(r)) => Row::NoInfo(r.clone()),
            (i, l) => check::row_state(i.as_ref(), l, slot.skipped.as_deref()),
        },
    }
}

fn entry_of(m: &HubManifest) -> Option<&hub_core::model::PlatformEntry> {
    m.entry()
}

/// What an install or update downloads, in bytes (0 when the release does not say).
pub fn download_bytes(m: &HubManifest) -> u64 {
    match entry_of(m) {
        Some(e) => match (&e.setup, e.kind) {
            (Some(s), Kind::Setup) => s.file.size,
            _ => e.files.iter().map(|f| f.size).sum(),
        },
        None => 0,
    }
}

fn details_of(slot: &Slot) -> Vec<(String, String)> {
    let app = slot.app;
    let mut d: Vec<(String, String)> = Vec::new();
    let mut machine = false;
    if let Latest::Found(m) = &slot.latest
        && let Some(e) = entry_of(m)
    {
        let names: Vec<&str> = match (&e.setup, e.kind) {
            (Some(s), Kind::Setup) => vec![s.file.name.as_str()],
            _ => e.files.iter().map(|f| f.name.as_str()).collect(),
        };
        machine = matches!(&e.setup, Some(s) if s.scope == Scope::Machine);
        if !names.is_empty() {
            d.push(("Installs".into(), names.join(", ")));
        }
    }
    d.push(("From".into(), format!("github.com/Kaan88/{}", app.repo)));
    if matches!(slot.latest, Latest::Found(_)) {
        d.push(("Checked with".into(), "SHA-256 listed in hub.txt".into()));
    }
    let into = match &slot.installed {
        Some(i) => i.folder.display().to_string(),
        None if machine => format!(
            "Program Files\\{}",
            app.machine_folder.unwrap_or(app.folder)
        ),
        None => format!("Programs\\{}", app.folder),
    };
    d.push(("Into".into(), into));
    let beside: Vec<&str> = app
        .wait
        .iter()
        .filter(|w| **w != app.exe)
        .map(|w| w.rsplit(['\\', '/']).next().unwrap_or(w))
        .collect();
    if !beside.is_empty() {
        d.push(("Beside it".into(), beside.join(", ")));
    }
    d.push(("Left alone".into(), "your settings and data".into()));
    d
}

pub fn app_vm(slot: &Slot, now: u64) -> AppVm {
    let app = slot.app;
    let (latest, date, size, notes) = match &slot.latest {
        Latest::Found(m) => (
            Some(m.version.to_string()),
            fmt_date(&m.date),
            fmt_size(download_bytes(m)),
            m.notes.clone(),
        ),
        _ => (None, String::new(), String::new(), String::new()),
    };
    AppVm {
        id: app.id,
        name: app.name,
        blurb: app.blurb,
        glyph: app.glyph,
        tile: app.tile,
        row: row_of(slot, now),
        installed: slot.installed.as_ref().map(|i| {
            i.version
                .as_ref()
                .map_or_else(|| "unknown".to_string(), |v| v.to_string())
        }),
        latest,
        date,
        size,
        running: slot.running,
        notes,
        details: details_of(slot),
    }
}

// ---- input mapped to commands ---------------------------------------------------------------------

/// What the user asked for, by app id (or by position for the keyboard).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Cmd {
    Open(&'static str),
    Install(&'static str),
    Update(&'static str),
    Adopt(&'static str),
    Cancel(&'static str),
    TryAgain(&'static str),
    UpdateAll,
    CheckNow,
    CancelAll,
    SetView(View),
    /// Row `i` of the list as shown.
    Select(usize),
    /// The "..." button of row `i`.
    OpenMenu(usize),
    MenuPick(&'static str, MenuItem),
    BannerYes,
    BannerNo,
    /// "Restart now" on Hub's own bar (or the About button).
    HubRestart,
    /// "Later" on Hub's own bar.
    HubLater,
    /// The release notes view of this app (the pane's link, the menu's item).
    OpenNotes(&'static str),
    /// The key for the release notes of the selected app.
    NotesOfSelected,
    /// Back from the notes view.
    NotesClose,
    /// "Try again" in the notes view.
    NotesRetry,
    /// "Release page" in the notes view: the release on GitHub, in the browser.
    NotesPage,
    FocusFilter,
    FilterChar(char),
    FilterBack,
    FilterBlur,
    SelectRel(i32),
    /// The selected row's main button.
    Primary,
    Esc,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MenuItem {
    Open,
    ShowInFolder,
    ReleaseNotes,
    SkipVersion,
    RollBack,
    GitHub,
}

pub fn act_cmd(id: &'static str, act: Act) -> Cmd {
    match act {
        Act::Open => Cmd::Open(id),
        Act::Install => Cmd::Install(id),
        Act::Update => Cmd::Update(id),
        Act::Adopt => Cmd::Adopt(id),
        Act::Cancel => Cmd::Cancel(id),
        Act::TryAgain => Cmd::TryAgain(id),
    }
}

/// What a click on `hit` asks for. Window hits (drag, min, max, close) are the shell's own.
pub fn command(hit: Hit, vm: &Vm) -> Option<Cmd> {
    let id = |i: usize| vm.apps.get(i).map(|a| a.id);
    match hit {
        Hit::UpdateAll => Some(Cmd::UpdateAll),
        Hit::CheckNow => Some(Cmd::CheckNow),
        Hit::CancelAll => Some(Cmd::CancelAll),
        Hit::ViewSwitch(v) => Some(Cmd::SetView(v)),
        Hit::Filter => Some(Cmd::FocusFilter),
        Hit::Row(i) => id(i).map(|_| Cmd::Select(i)),
        Hit::Action(i, act) => id(i).map(|id| act_cmd(id, act)),
        Hit::Menu(i) => id(i).map(|_| Cmd::OpenMenu(i)),
        Hit::Notes(i) => id(i).map(Cmd::OpenNotes),
        Hit::NotesBack => Some(Cmd::NotesClose),
        Hit::NotesRetry => Some(Cmd::NotesRetry),
        Hit::NotesPage => Some(Cmd::NotesPage),
        // The bar over the list is an app's question or Hub's own "Restart now"; which one it is
        // decides the command, so a stale or doubled click never answers the other bar.
        Hit::BannerYes => Some(match &vm.banner {
            Some(b) if b.hub => Cmd::HubRestart,
            _ => Cmd::BannerYes,
        }),
        Hit::BannerNo => Some(match &vm.banner {
            Some(b) if b.hub => Cmd::HubLater,
            _ => Cmd::BannerNo,
        }),
        Hit::Drag | Hit::Min | Hit::Max | Hit::Close | Hit::Settings | Hit::None => None,
    }
}

/// The keys the window reacts to, reduced from the system's.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Enter,
    Esc,
    Tab,
    Backspace,
    F5,
    PageUp,
    PageDown,
    Home,
    End,
    Char(char),
}

/// What a key asks for. `cols` is how many tiles a row of the grid holds (1 in the list).
pub fn key_command(
    filter_focus: bool,
    key: Key,
    ctrl: bool,
    shift: bool,
    cols: usize,
) -> Option<Cmd> {
    if filter_focus {
        return match key {
            Key::Char(c) if !ctrl && !c.is_control() => Some(Cmd::FilterChar(c)),
            Key::Char('f') => Some(Cmd::FocusFilter),
            Key::Backspace => Some(Cmd::FilterBack),
            Key::Esc => Some(Cmd::Esc),
            Key::Enter | Key::Down | Key::Tab => Some(Cmd::FilterBlur),
            Key::F5 => Some(Cmd::CheckNow),
            _ => None,
        };
    }
    let cols = cols.max(1) as i32;
    match key {
        Key::Up => Some(Cmd::SelectRel(-cols)),
        Key::Down => Some(Cmd::SelectRel(cols)),
        Key::Left if cols > 1 => Some(Cmd::SelectRel(-1)),
        Key::Right if cols > 1 => Some(Cmd::SelectRel(1)),
        Key::Tab => Some(Cmd::SelectRel(if shift { -1 } else { 1 })),
        Key::Enter => Some(Cmd::Primary),
        Key::Esc => Some(Cmd::Esc),
        Key::F5 => Some(Cmd::CheckNow),
        Key::Char('f') if ctrl => Some(Cmd::FocusFilter),
        Key::Char('r') if ctrl => Some(Cmd::CheckNow),
        Key::Char('/') if !ctrl => Some(Cmd::FocusFilter),
        Key::Char('n') if !ctrl => Some(Cmd::NotesOfSelected),
        _ => None,
    }
}

/// What a key does to the notes view's text: the arrows, pages, Home and End move it.
pub fn notes_nav(key: Key) -> Option<crate::changelog::Nav> {
    use crate::changelog::Nav;
    match key {
        Key::Up => Some(Nav::LineUp),
        Key::Down => Some(Nav::LineDown),
        Key::PageUp => Some(Nav::PageUp),
        Key::PageDown | Key::Char(' ') => Some(Nav::PageDown),
        Key::Home => Some(Nav::Home),
        Key::End => Some(Nav::End),
        _ => None,
    }
}

/// May a held key repeat this command? Moving the selection and editing the filter may; starting
/// an install, launching an app or a check may not (holding Enter would install, then cancel).
pub fn repeatable(cmd: &Cmd) -> bool {
    matches!(
        cmd,
        Cmd::SelectRel(_) | Cmd::FilterChar(_) | Cmd::FilterBack
    )
}

// ---- scheduling -----------------------------------------------------------------------------------

/// How long until the next check should start: `Some(ZERO)` means now, `None` means never (the
/// interval is 0 and no first check is wanted). `first` is the start-up check: the delay since
/// start and whether it runs even when none is due; it is `None` once a check has begun.
pub fn check_wait(
    first: Option<(Duration, bool)>,
    since_start: Duration,
    last: Option<u64>,
    now: u64,
    interval_hours: u32,
    failures: u32,
) -> Option<Duration> {
    let first_left = first.map(|(d, _)| d.saturating_sub(since_start));
    if let Some((_, true)) = first {
        return first_left;
    }
    if interval_hours == 0 {
        return None;
    }
    let wait = if last.is_some_and(|l| l > now) {
        Duration::ZERO
    } else {
        let due = check::next_due(last, interval_hours, failures);
        Duration::from_secs(due.saturating_sub(now))
    };
    Some(wait.max(first_left.unwrap_or(Duration::ZERO)))
}

// ---- the engine -----------------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum JobKind {
    Install,
    Rollback,
}

struct Active {
    id: &'static str,
    cancel: Arc<AtomicBool>,
}

pub struct Engine {
    pub store: Store,
    pub env: Env,
    transport: Arc<dyn Transport>,
    post: Arc<dyn Post>,
    shell: Arc<dyn Shell>,
    tmpl: Tmpl,
    pub slots: Vec<Slot>,
    /// A fixed picture (`--demo`): nothing is read, nothing is run.
    fixed: Option<Vm>,
    view: View,
    filter: String,
    filter_focus: bool,
    sel: usize,
    /// The selection was moved or clicked by the user: only then does Enter act on it.
    sel_touched: bool,
    hover: Option<usize>,
    running_busy: bool,
    /// Apps the user said yes to closing (the offer bar), so the install may close them.
    consented: Vec<&'static str>,
    banner: Option<&'static str>,
    pending_close: Vec<&'static str>,
    queue: VecDeque<(&'static str, JobKind)>,
    active: Option<Active>,
    checking: bool,
    notice: Option<String>,
    last_check: Option<u64>,
    failures: u32,
    interval: u32,
    first: Option<(Duration, bool)>,
    started: Instant,
    own: Own,
    tell: Tell,
    /// The release notes view, when it is open (never at start).
    notes: Option<NotesOpen>,
    run_key: RunKey,
    /// "Start with Windows" as the registry says, read when first wanted (never at start).
    autostart: Cell<Option<bool>>,
    tray_on: bool,
}

/// The release notes view that is open: which version's notes, and how far they have come.
struct NotesOpen {
    id: &'static str,
    version: String,
    date: String,
    one_liner: String,
    /// The manifest's `notes.file`, None when the release has none.
    file: Option<String>,
    state: NotesState,
}

/// Hub's own update: where it stands, and what is waiting to be done with it.
struct Own {
    exe: Option<PathBuf>,
    status: Status,
    /// "Later" was pressed on the banner for the version that is ready.
    dismissed: bool,
    /// "Restart now" was pressed while an install ran: swap when the installs are done.
    when_idle: bool,
    /// The swap is done: the shell ends the process and starts the new Hub from this exe.
    restart: Option<PathBuf>,
    /// The version the check that is out was told is downloaded already: that one it does not
    /// fetch again, whatever has become of it since.
    told_ready: Option<mdshared_app::update::Version>,
}

impl Own {
    fn new(exe: Option<PathBuf>) -> Own {
        Own {
            exe,
            status: Status::None,
            dismissed: false,
            when_idle: false,
            restart: None,
            told_ready: None,
        }
    }
}

/// What Hub tells the user after a check: the rule's memory, the message waiting to be shown, and
/// what decides whether to tell at all.
struct Tell {
    mem: Memory,
    msg: Option<Message>,
    on: bool,
    /// The tray icon is up: only then can anything be told.
    tray_up: bool,
    /// The window is open and has the focus: the user is looking at the answer.
    focused: bool,
}

fn view_of(m: ViewMode) -> View {
    match m {
        ViewMode::List => View::List,
        ViewMode::Large => View::Large,
        ViewMode::Small => View::Small,
    }
}

fn mode_of(v: View) -> ViewMode {
    match v {
        View::List => ViewMode::List,
        View::Large => ViewMode::Large,
        View::Small => ViewMode::Small,
    }
}

fn matches_filter(filter: &str, a: &AppVm) -> bool {
    let f = filter.trim().to_lowercase();
    f.is_empty() || a.name.to_lowercase().contains(&f) || a.id.contains(&f)
}

impl Engine {
    /// Reads what Hub knows without asking the network: cached manifests and detection.
    pub fn new(d: Deps) -> Engine {
        let slots = CATALOG
            .iter()
            .map(|a| load_slot(a, &d.store, &d.env))
            .collect();
        let view = view_of(d.store.view());
        let (last_check, failures, interval) = (
            d.store.last_check(),
            d.store.failures(),
            d.store.interval_hours(),
        );
        let (notified, notify_on, tray_on) = (
            d.store.notified(),
            d.store.notify_enabled(),
            d.store.tray_enabled(),
        );
        Engine {
            store: d.store,
            env: d.env,
            transport: d.transport,
            post: d.post,
            shell: d.shell,
            tmpl: d.tmpl,
            slots,
            fixed: None,
            view,
            filter: String::new(),
            filter_focus: false,
            sel: 0,
            sel_touched: false,
            running_busy: false,
            consented: Vec::new(),
            hover: None,
            banner: None,
            pending_close: Vec::new(),
            queue: VecDeque::new(),
            active: None,
            checking: false,
            notice: None,
            last_check,
            failures,
            interval,
            first: d.first_check,
            started: Instant::now(),
            own: Own::new(d.hub_exe),
            notes: None,
            tell: Tell {
                mem: Memory::from_line(&notified),
                msg: None,
                on: notify_on,
                tray_up: false,
                focused: false,
            },
            run_key: d.run_key,
            autostart: Cell::new(None),
            tray_on,
        }
    }

    /// An engine that only shows `vm` (the picture of `--demo`): it reads nothing from this PC.
    pub fn demo(vm: Vm, post: Arc<dyn Post>) -> Engine {
        struct Nothing;
        impl Transport for Nothing {
            fn get(&self, _: &str, _: usize, _: Duration) -> Result<Vec<u8>, String> {
                Err("demo".into())
            }
        }
        Engine {
            store: Store::at(std::path::PathBuf::new()),
            env: Env::custom(std::path::PathBuf::new(), Vec::new(), Vec::new()),
            transport: Arc::new(Nothing),
            post,
            shell: Arc::new(RealShell),
            tmpl: Tmpl::real(),
            slots: Vec::new(),
            view: vm.view,
            sel: vm.sel,
            fixed: Some(vm),
            filter: String::new(),
            filter_focus: false,
            sel_touched: false,
            running_busy: false,
            consented: Vec::new(),
            hover: None,
            banner: None,
            pending_close: Vec::new(),
            queue: VecDeque::new(),
            active: None,
            checking: false,
            notice: None,
            last_check: None,
            failures: 0,
            interval: 0,
            first: None,
            started: Instant::now(),
            own: Own::new(None),
            notes: None,
            tell: Tell {
                mem: Memory::new(),
                msg: None,
                on: false,
                tray_up: false,
                focused: false,
            },
            run_key: RunKey::none(),
            autostart: Cell::new(Some(false)),
            tray_on: false,
        }
    }

    pub fn is_demo(&self) -> bool {
        self.fixed.is_some()
    }

    /// A test points the engine at the exe it may replace and the registry key it may write.
    #[cfg(test)]
    pub fn set_hub_exe(&mut self, exe: Option<PathBuf>) {
        self.own.exe = exe;
    }

    #[cfg(test)]
    pub fn set_run_key(&mut self, k: RunKey) {
        self.run_key = k;
        self.autostart.set(None);
    }

    /// Is any work in flight (a check, an install, a queued install)?
    pub fn busy(&self) -> bool {
        self.active.is_some()
            || self.checking
            || !self.queue.is_empty()
            || matches!(self.own.status, Status::Downloading(_))
    }

    /// Is an install or a roll back running or waiting? (A check may be dropped at exit; this may
    /// not be, without being told to stop first.)
    pub fn installing(&self) -> bool {
        self.active.is_some() || !self.queue.is_empty()
    }

    /// The window is closing: nothing more is started, the running job is asked to stop (it
    /// stops before it places anything, or finishes what it is placing). True while it still runs.
    pub fn begin_close(&mut self) -> bool {
        if self.fixed.is_some() {
            return false;
        }
        self.apply(Cmd::CancelAll);
        self.active.is_some()
    }

    pub fn checking(&self) -> bool {
        self.checking
    }

    pub fn filter_focus(&self) -> bool {
        self.filter_focus
    }

    pub fn view(&self) -> View {
        self.view
    }

    // ---- what is shown ----

    fn all_apps(&self, now: u64) -> Vec<AppVm> {
        match &self.fixed {
            Some(v) => v.apps.clone(),
            None => self.slots.iter().map(|s| app_vm(s, now)).collect(),
        }
    }

    fn visible_ids(&self, now: u64) -> Vec<&'static str> {
        self.all_apps(now)
            .iter()
            .filter(|a| matches_filter(&self.filter, a))
            .map(|a| a.id)
            .collect()
    }

    /// The window's picture at `now` (Unix seconds).
    pub fn vm(&self, now: u64) -> Vm {
        let apps: Vec<AppVm> = self
            .all_apps(now)
            .into_iter()
            .filter(|a| matches_filter(&self.filter, a))
            .collect();
        let updates = apps.iter().filter(|a| a.row == Row::Update).count();
        let sel = self.sel.min(apps.len().saturating_sub(1));
        let busy_n = self.queue.len() + usize::from(self.active.is_some());
        let busy = (busy_n > 0).then(|| match (busy_n, &self.active) {
            (1, Some(a)) => format!(
                "Installing {}\u{2026}",
                CATALOG
                    .iter()
                    .find(|c| c.id == a.id)
                    .map_or(a.id, |c| c.name)
            ),
            (n, _) => format!("Updating {n} apps\u{2026}"),
        });
        let banner = self
            .banner
            .and_then(|id| {
                let name = CATALOG.iter().find(|c| c.id == id)?.name;
                Some(Banner {
                    text: format!("{name} is open: close it and update"),
                    yes: "Close and update".into(),
                    no: "Not now".into(),
                    warn: false,
                    hub: false,
                })
            })
            // Hub's own update, once it is downloaded: after an app's question, never over it.
            .or_else(|| {
                self.hub_banner_shown().then(|| Banner {
                    text: self.own.status.label(),
                    yes: "Restart now".into(),
                    no: "Later".into(),
                    warn: false,
                    hub: true,
                })
            });
        let checked = if self.checking {
            "Checking\u{2026}".to_string()
        } else if let Some(n) = &self.notice {
            n.clone()
        } else if self.failures > 0 && self.last_check.is_some() {
            format!(
                "{} \u{b7} some apps could not be reached",
                check::age_label(self.last_check, now)
            )
        } else if self.fixed.is_some() {
            self.fixed
                .as_ref()
                .map(|v| v.checked.clone())
                .unwrap_or_default()
        } else {
            check::age_label(self.last_check, now)
        };
        Vm {
            sel,
            hover: self.hover.filter(|h| *h < apps.len()),
            view: self.view,
            banner: banner.or_else(|| self.fixed.as_ref().and_then(|v| v.banner.clone())),
            checked,
            hub_version: match &self.own.status {
                Status::Ready(s) => format!("{} ({} ready)", env!("CARGO_PKG_VERSION"), s.version),
                _ => env!("CARGO_PKG_VERSION").to_string(),
            },
            settings: None,
            notes: self
                .notes
                .as_ref()
                .map(|n| self.notes_vm(n, now))
                .or_else(|| self.fixed.as_ref().and_then(|v| v.notes.clone())),
            busy: busy.or_else(|| self.fixed.as_ref().and_then(|v| v.busy.clone())),
            updates,
            filter: self.filter.clone(),
            filter_focus: self.filter_focus,
            apps,
        }
    }

    pub fn set_hover(&mut self, h: Option<usize>) -> bool {
        let changed = self.hover != h;
        self.hover = h;
        changed
    }

    /// Is the offer bar asking whether an open app may be closed? Only a window can show it.
    pub fn asking(&self) -> bool {
        self.fixed.is_none() && self.banner.is_some()
    }

    /// The "..." menu of an app: what it offers and whether each item can be used now.
    pub fn menu_items(&self, id: &str) -> Vec<(MenuItem, &'static str, bool)> {
        let Some(slot) = self.slots.iter().find(|s| s.app.id == id) else {
            return vec![(MenuItem::GitHub, "GitHub", true)];
        };
        let installed = slot.installed.is_some();
        let skippable = matches!(&slot.latest, Latest::Found(_))
            && matches!(slot.phase, Phase::Idle)
            && check::row_state(
                slot.installed.as_ref(),
                &slot.latest,
                slot.skipped.as_deref(),
            ) == Row::Update;
        let mut v = vec![
            (MenuItem::Open, "Open", installed),
            (MenuItem::ShowInFolder, "Show in folder", installed),
            (
                MenuItem::ReleaseNotes,
                "Release notes",
                matches!(slot.latest, Latest::Found(_)),
            ),
            (MenuItem::SkipVersion, "Skip this version", skippable),
        ];
        if self.can_roll_back(slot) {
            v.push((MenuItem::RollBack, "Roll back", true));
        }
        v.push((MenuItem::GitHub, "GitHub", true));
        v
    }

    fn can_roll_back(&self, slot: &Slot) -> bool {
        slot.installed.is_some()
            && matches!(slot.phase, Phase::Idle | Phase::Done(_))
            && self
                .store
                .rollback_dir(slot.app.id)
                .join("rollback.txt")
                .is_file()
    }

    // ---- the tray ----

    /// What the tray's badge, tooltip and menu are built from: the updates waiting and the apps
    /// installed, with the two settings the menu shows. Hub's own update counts in the badge but
    /// not in "Update all".
    pub fn tray_snapshot(&self, now: u64) -> crate::tray::Snapshot {
        let mut snap = crate::tray::Snapshot {
            notify: self.tell.on,
            start_with_windows: self.autostart_on(),
            hub_update: matches!(
                self.own.status,
                Status::Available(_) | Status::Downloading(_) | Status::Ready(_)
            ),
            ..Default::default()
        };
        for s in &self.slots {
            if s.installed.is_some() {
                snap.apps.push((s.app.id, s.app.name.to_string()));
            }
            if row_of(s, now) == Row::Update {
                snap.updates += 1;
                snap.update_names.push(s.app.name.to_string());
            }
        }
        snap
    }

    // ---- scheduling ----

    /// How long the loop may sleep before the engine wants to run again; None: not on a timer.
    pub fn wait(&self, now: u64) -> Option<Duration> {
        if self.fixed.is_some() || self.checking || self.own_downloading() {
            return None;
        }
        check_wait(
            self.first,
            self.started.elapsed(),
            self.last_check,
            now,
            self.interval,
            self.failures,
        )
    }

    /// Starts a check when one is due.
    pub fn tick(&mut self, now: u64) -> bool {
        match self.wait(now) {
            Some(d) if d.is_zero() => self.start_check(),
            _ => false,
        }
    }

    // ---- input ----

    pub fn apply(&mut self, cmd: Cmd) -> bool {
        let now = check::now();
        match cmd {
            Cmd::Select(i) => {
                let changed = self.sel != i;
                self.sel = i;
                self.sel_touched = true;
                self.filter_focus = false;
                changed
            }
            Cmd::SelectRel(d) => {
                let n = self.visible_ids(now).len() as i32;
                if n == 0 {
                    return false;
                }
                let cur = self.sel.min(n as usize - 1) as i32;
                let next = (cur + d).clamp(0, n - 1) as usize;
                let changed = next != self.sel;
                self.sel = next;
                self.sel_touched = true;
                changed
            }
            Cmd::SetView(v) => {
                let changed = self.view != v;
                self.view = v;
                if self.fixed.is_none() {
                    self.store.set_view(mode_of(v));
                }
                changed
            }
            Cmd::FocusFilter => {
                let changed = !self.filter_focus;
                self.filter_focus = true;
                changed
            }
            Cmd::FilterChar(c) => {
                if self.filter.chars().count() < 40 {
                    self.filter.push(c);
                    self.sel = 0;
                    self.sel_touched = false;
                }
                true
            }
            Cmd::FilterBack => {
                let had = self.filter.pop().is_some();
                self.sel = 0;
                self.sel_touched = false;
                had
            }
            Cmd::FilterBlur => {
                let changed = self.filter_focus;
                self.filter_focus = false;
                changed
            }
            Cmd::Esc => {
                if self.notes.is_some() {
                    // Back to the list: the selection is where it was.
                    self.notes = None;
                    true
                } else if self.banner.is_some() {
                    // The same as No: the open apps still waiting are not asked about later.
                    self.pending_close.clear();
                    self.banner = None;
                    true
                } else if self.hub_banner_shown() {
                    // The same as Later.
                    self.own.dismissed = true;
                    true
                } else if !self.filter.is_empty() {
                    self.filter.clear();
                    self.sel = 0;
                    self.sel_touched = false;
                    true
                } else if self.filter_focus {
                    self.filter_focus = false;
                    true
                } else {
                    false
                }
            }
            Cmd::Primary => {
                // Enter acts on a row the user chose, not on the one a fresh window starts with.
                if !self.sel_touched {
                    return false;
                }
                let ids = self.visible_ids(now);
                let Some(id) = ids.get(self.sel.min(ids.len().saturating_sub(1))).copied() else {
                    return false;
                };
                let first = self
                    .all_apps(now)
                    .iter()
                    .find(|a| a.id == id)
                    .and_then(|a| crate::view::row_actions(a).first().map(|b| b.act));
                match first {
                    Some(act) => self.apply(act_cmd(id, act)),
                    None => false,
                }
            }
            // The popup itself is the shell's; picking an item is the engine's.
            Cmd::OpenMenu(_) => false,
            _ if self.fixed.is_some() => false,
            Cmd::Open(id) => self.open(id),
            Cmd::Install(id) | Cmd::TryAgain(id) => self.enqueue(id, JobKind::Install),
            Cmd::Update(id) => {
                // Is it open now? Not what was cached a while ago.
                self.refresh_one(id);
                if self.slot(id).is_some_and(|s| s.running) {
                    self.banner = Some(id);
                    true
                } else {
                    self.enqueue(id, JobKind::Install)
                }
            }
            Cmd::OpenNotes(id) => self.open_notes(id),
            Cmd::NotesOfSelected => {
                let ids = self.visible_ids(now);
                match ids.get(self.sel.min(ids.len().saturating_sub(1))).copied() {
                    Some(id) if self.notes.is_none() => self.open_notes(id),
                    _ => false,
                }
            }
            Cmd::NotesClose => self.notes.take().is_some(),
            Cmd::NotesRetry => self.retry_notes(),
            Cmd::NotesPage => self.notes_page(),
            Cmd::Adopt(id) => self.adopt(id),
            Cmd::Cancel(id) => self.cancel(id),
            Cmd::CancelAll => {
                let ids: Vec<&'static str> = self.queue.iter().map(|(i, _)| *i).collect();
                self.queue.clear();
                self.pending_close.clear();
                self.consented.clear();
                for id in ids {
                    self.set_phase(id, Phase::Idle);
                }
                if let Some(a) = &self.active {
                    a.cancel.store(true, Ordering::Relaxed);
                }
                true
            }
            Cmd::UpdateAll => self.update_all(now),
            Cmd::CheckNow => self.start_check(),
            Cmd::BannerYes => match self.banner.take() {
                Some(id) => {
                    self.pending_close.retain(|p| *p != id);
                    // The yes is for the install it starts, and for no other: one that does not
                    // get queued leaves no permission behind.
                    self.consented.push(id);
                    self.enqueue(id, JobKind::Install);
                    if !self.in_flight(id) {
                        self.consented.retain(|c| *c != id);
                    }
                    true
                }
                // No app is asking (a second click of a double-click): nothing to answer. Hub's
                // own bar has its own command.
                None => false,
            },
            Cmd::BannerNo => {
                self.pending_close.clear();
                self.banner.take().is_some()
            }
            Cmd::HubRestart => self.hub_restart(),
            Cmd::HubLater => {
                if self.hub_banner_shown() {
                    self.own.dismissed = true;
                    true
                } else {
                    false
                }
            }
            Cmd::MenuPick(id, item) => self.menu_pick(id, item),
        }
    }

    fn slot(&self, id: &str) -> Option<&Slot> {
        self.slots.iter().find(|s| s.app.id == id)
    }

    fn slot_mut(&mut self, id: &str) -> Option<&mut Slot> {
        self.slots.iter_mut().find(|s| s.app.id == id)
    }

    fn set_phase(&mut self, id: &str, p: Phase) {
        if let Some(s) = self.slot_mut(id) {
            s.phase = p;
        }
    }

    fn open(&mut self, id: &'static str) -> bool {
        let Some(slot) = self.slot(id) else {
            return false;
        };
        let (app, Some(inst)) = (slot.app, slot.installed.clone()) else {
            return false;
        };
        self.notice = match self.shell.launch(app, &inst) {
            Ok(()) => None,
            Err(e) => Some(format!("Could not start {}: {e}", app.name)),
        };
        true
    }

    fn adopt(&mut self, id: &'static str) -> bool {
        let store = self.store.clone();
        let Some(slot) = self.slot_mut(id) else {
            return false;
        };
        let Some(inst) = slot.installed.as_mut() else {
            return false;
        };
        detect::adopt(slot.app, &store, inst);
        inst.source = Source::Hub;
        true
    }

    fn cancel(&mut self, id: &'static str) -> bool {
        if let Some(a) = &self.active
            && a.id == id
        {
            a.cancel.store(true, Ordering::Relaxed);
            return true;
        }
        let before = self.queue.len();
        self.queue.retain(|(q, _)| *q != id);
        if self.queue.len() != before {
            // A yes to closing the app was for this install: it goes with it.
            self.consented.retain(|c| *c != id);
            self.set_phase(id, Phase::Idle);
            return true;
        }
        false
    }

    /// Is a job of this app running or waiting its turn?
    fn in_flight(&self, id: &str) -> bool {
        self.active.as_ref().is_some_and(|a| a.id == id) || self.queue.iter().any(|(q, _)| *q == id)
    }

    fn update_all(&mut self, _now: u64) -> bool {
        // Every app with an update goes into the queue, in the window's order; one that is open
        // is asked about first (the offer bar), never closed without asking.
        self.refresh_running();
        let mut running = Vec::new();
        let mut todo = Vec::new();
        for s in &self.slots {
            if s.phase == Phase::Idle && row_of(s, 0) == Row::Update {
                if s.running {
                    running.push(s.app.id);
                } else {
                    todo.push(s.app.id);
                }
            }
        }
        if todo.is_empty() && running.is_empty() {
            return false;
        }
        for id in todo {
            self.enqueue(id, JobKind::Install);
        }
        self.pending_close = running;
        self.offer_pending();
        true
    }

    /// With nothing running or queued, asks about the next open app waiting to be updated.
    fn offer_pending(&mut self) {
        if self.active.is_none()
            && self.queue.is_empty()
            && self.banner.is_none()
            && !self.pending_close.is_empty()
        {
            self.banner = Some(self.pending_close.remove(0));
        }
    }

    fn enqueue(&mut self, id: &'static str, kind: JobKind) -> bool {
        if self.fixed.is_some()
            || self.slot(id).is_none()
            || self.active.as_ref().is_some_and(|a| a.id == id)
            || self.queue.iter().any(|(q, _)| *q == id)
        {
            return false;
        }
        if kind == JobKind::Install {
            let usable = self.slot(id).is_some_and(|s| match &s.latest {
                Latest::Found(m) => m.entry().is_some(),
                _ => false,
            });
            if !usable {
                self.set_phase(
                    id,
                    Phase::Failed("There is no release to install from".into()),
                );
                return true;
            }
        }
        self.notice = None;
        // The bar's question about this app is answered by whatever queued it.
        if self.banner == Some(id) {
            self.banner = None;
        }
        self.set_phase(id, Phase::Queued);
        self.queue.push_back((id, kind));
        self.pump();
        true
    }

    /// Starts the next queued job when none is running: one install at a time.
    fn pump(&mut self) {
        if self.active.is_some() {
            return;
        }
        while let Some((id, kind)) = self.queue.pop_front() {
            if self.slot(id).is_none() {
                continue;
            }
            if kind == JobKind::Rollback {
                // Opened since it was queued: not started (see `menu_pick`).
                self.refresh_one(id);
                if let Some(s) = self.slot(id).filter(|s| s.running) {
                    self.notice = Some(roll_back_open(s.app.name));
                    self.set_phase(id, Phase::Idle);
                    continue;
                }
            }
            if kind == JobKind::Install {
                // The app may have been opened since it was queued: ask before closing it.
                let yes = self.consented.iter().position(|c| *c == id);
                if let Some(i) = yes {
                    self.consented.remove(i);
                } else {
                    self.refresh_one(id);
                    if self.slot(id).is_some_and(|s| s.running) {
                        self.set_phase(id, Phase::Idle);
                        if !self.pending_close.contains(&id) {
                            self.pending_close.push(id);
                        }
                        continue;
                    }
                }
            }
            let cancel = Arc::new(AtomicBool::new(false));
            let opts = self.opts(cancel.clone());
            let Some(slot) = self.slot(id) else { continue };
            let (app, cur, latest) = (slot.app, slot.installed.clone(), slot.latest.clone());
            let started = match (kind, latest) {
                (JobKind::Install, Latest::Found(m)) => work::spawn_install(
                    self.post.clone(),
                    app,
                    *m,
                    cur,
                    self.store.clone(),
                    self.transport.clone(),
                    opts,
                ),
                (JobKind::Rollback, _) => {
                    work::spawn_rollback(self.post.clone(), app, self.store.clone(), opts)
                }
                _ => Err("no release".to_string()),
            };
            match started {
                Ok(()) => {
                    self.active = Some(Active { id, cancel });
                    self.set_phase(
                        id,
                        Phase::Downloading {
                            frac: 0.0,
                            text: "Starting".into(),
                        },
                    );
                    return;
                }
                Err(e) => self.set_phase(id, Phase::Failed(format!("Could not start: {e}"))),
            }
        }
        self.offer_pending();
    }

    fn opts(&self, cancel: Arc<AtomicBool>) -> Opts {
        let env = self.env.clone();
        Opts {
            cancel,
            elevate: self.tmpl.elevate.clone(),
            closer: self.tmpl.closer.clone(),
            close_timeout: self.tmpl.close_timeout,
            poll: self.tmpl.poll,
            download_timeout: Duration::from_secs(600),
            programs: Some(self.env.programs.clone()),
            redetect: Some(Arc::new(move |app, store| detect::detect(app, store, &env))),
        }
    }

    /// Hub's own update is being downloaded by the check that found it. That download goes on
    /// after the check itself has reported (`Event::Checked`), so `checking` is already false.
    fn own_downloading(&self) -> bool {
        matches!(self.own.status, Status::Downloading(_))
    }

    fn start_check(&mut self) -> bool {
        // Not while Hub's own file is being written: a second check would start a second download
        // of the same file under the first one.
        if self.checking || self.own_downloading() || self.fixed.is_some() {
            return false;
        }
        self.first = None;
        self.notice = None;
        self.own.told_ready = match &self.own.status {
            Status::Ready(s) => Some(s.version.clone()),
            _ => None,
        };
        let hub = self.own.exe.clone().map(|exe| work::HubJob {
            exe,
            ready: self.own.told_ready.clone(),
        });
        match work::spawn_check(
            self.post.clone(),
            self.transport.clone(),
            self.store.clone(),
            hub,
        ) {
            Ok(()) => {
                self.checking = true;
                true
            }
            Err(e) => {
                self.notice = Some(format!("Could not start a check: {e}"));
                // Try again later, not at the next tick.
                self.last_check = Some(check::now());
                self.failures = self.failures.saturating_add(1);
                true
            }
        }
    }

    // ---- the release notes view ----

    /// Opens the notes of the release Hub knows about. The one line is already there; the full
    /// notes come from the disk when they were fetched before, else from one GET of the release's
    /// notes file, on a worker. Nothing else asks for them: not start, not idle, not a check.
    fn open_notes(&mut self, id: &'static str) -> bool {
        let Some(slot) = self.slot(id) else {
            return false;
        };
        let Latest::Found(m) = &slot.latest else {
            self.notice = Some(format!(
                "Hub has no release information for {} yet",
                slot.app.name
            ));
            return true;
        };
        let version = m.version.to_string();
        let file = m.notes_file.clone();
        let state = match &file {
            None => NotesState::NoFile,
            Some(_) => match self
                .store
                .cached_notes(id, &version)
                .map(|b| hub_core::notes::clean(&b))
                .filter(|t| !t.trim().is_empty())
            {
                Some(text) => NotesState::Loaded(text),
                None => NotesState::Loading,
            },
        };
        self.notes = Some(NotesOpen {
            id,
            version,
            date: fmt_date(&m.date),
            one_liner: m.notes.clone(),
            file,
            state,
        });
        self.fetch_notes();
        true
    }

    /// Starts the fetch when the open view is waiting for one.
    fn fetch_notes(&mut self) {
        let Some(n) = self.notes.as_mut() else {
            return;
        };
        let (NotesState::Loading, Some(file)) = (&n.state, n.file.clone()) else {
            return;
        };
        let (id, version) = (n.id, n.version.clone());
        let Some(app) = CATALOG.iter().find(|a| a.id == id) else {
            return;
        };
        if let Err(e) = work::spawn_notes(
            self.post.clone(),
            self.transport.clone(),
            self.store.clone(),
            app,
            version,
            file,
        ) && let Some(n) = self.notes.as_mut()
        {
            n.state = NotesState::Failed(format!("Could not start the download: {e}"));
        }
    }

    fn retry_notes(&mut self) -> bool {
        match self.notes.as_mut() {
            Some(n) if matches!(n.state, NotesState::Failed(_)) && n.file.is_some() => {
                n.state = NotesState::Loading;
                self.fetch_notes();
                true
            }
            _ => false,
        }
    }

    fn notes_page(&mut self) -> bool {
        if let Some(n) = &self.notes
            && let Some(app) = CATALOG.iter().find(|a| a.id == n.id)
            && let Some(url) = hub_core::notes::page_url(app, &n.version)
        {
            self.shell.open_url(&url);
        }
        false
    }

    fn notes_vm(&self, n: &NotesOpen, now: u64) -> NotesVm {
        let (name, installed) = match self.slot(n.id) {
            Some(s) => (s.app.name, app_vm(s, now).installed),
            None => (n.id, None),
        };
        NotesVm {
            id: n.id,
            name,
            version: n.version.clone(),
            installed,
            date: n.date.clone(),
            one_liner: n.one_liner.clone(),
            state: n.state.clone(),
        }
    }

    fn handle_notes(
        &mut self,
        id: &'static str,
        version: String,
        result: Result<String, String>,
    ) -> bool {
        match self.notes.as_mut() {
            Some(n) if n.id == id && n.version == version && n.state == NotesState::Loading => {
                n.state = match result {
                    Ok(t) => NotesState::Loaded(t),
                    Err(e) => NotesState::Failed(e),
                };
                true
            }
            // The view was closed, or moved to another app: nothing to show it on.
            _ => false,
        }
    }

    fn menu_pick(&mut self, id: &'static str, item: MenuItem) -> bool {
        let Some(slot) = self.slot(id) else {
            return false;
        };
        let app = slot.app;
        match item {
            MenuItem::Open => self.open(id),
            MenuItem::ShowInFolder => {
                if let Some(i) = &slot.installed {
                    self.shell.reveal(&i.exe);
                }
                false
            }
            MenuItem::GitHub => {
                self.shell
                    .open_url(&format!("https://github.com/Kaan88/{}", app.repo));
                false
            }
            MenuItem::ReleaseNotes => self.open_notes(id),
            MenuItem::SkipVersion => {
                let v = match &slot.latest {
                    Latest::Found(m) => m.version.to_string(),
                    _ => return false,
                };
                self.store.set_skipped(id, Some(&v));
                if let Some(s) = self.slot_mut(id) {
                    s.skipped = Some(v);
                }
                true
            }
            MenuItem::RollBack => {
                if !self.can_roll_back(slot) {
                    return false;
                }
                // A roll back replaces the program's files: an open app would have to be closed,
                // and nothing closes one without a yes. The offer bar asks about updates only,
                // so this is not started; the user closes the app and picks it again.
                self.refresh_one(id);
                if self.slot(id).is_some_and(|s| s.running) {
                    self.notice = Some(roll_back_open(app.name));
                    return true;
                }
                self.enqueue(id, JobKind::Rollback)
            }
        }
    }

    /// Looks at one app now: at a click, and before a queued job starts.
    fn refresh_one(&mut self, id: &str) {
        let shell = self.shell.clone();
        if let Some(s) = self.slot_mut(id) {
            s.running = s.installed.as_ref().is_some_and(|i| shell.running(&i.exe));
        }
    }

    /// The same as `refresh_running`, on a worker: the answer comes back as `Event::Running`, so
    /// the window's thread never waits for the process list.
    pub fn refresh_running_async(&mut self) {
        if self.running_busy || self.fixed.is_some() {
            return;
        }
        let exes: Vec<(&'static str, std::path::PathBuf)> = self
            .slots
            .iter()
            .filter_map(|s| s.installed.as_ref().map(|i| (s.app.id, i.exe.clone())))
            .collect();
        if exes.is_empty() {
            return;
        }
        let (shell, post) = (self.shell.clone(), self.post.clone());
        let spawned = std::thread::Builder::new()
            .name("hub-running".into())
            .spawn(move || {
                let r = exes.iter().map(|(id, e)| (*id, shell.running(e))).collect();
                post.post(Event::Running(r));
            });
        self.running_busy = spawned.is_ok();
    }

    /// Looks at which installed apps run now (one pass over the process list; not at start).
    pub fn refresh_running(&mut self) -> bool {
        let shell = self.shell.clone();
        let mut changed = false;
        for s in &mut self.slots {
            let r = s.installed.as_ref().is_some_and(|i| shell.running(&i.exe));
            changed |= r != s.running;
            s.running = r;
        }
        changed
    }

    // ---- Hub's own update ----

    /// The bar over the list asks "Restart now": Hub's own update is downloaded, and "Later" was
    /// not pressed for it.
    fn hub_banner_shown(&self) -> bool {
        !self.own.dismissed && matches!(self.own.status, Status::Ready(_))
    }

    /// Where Hub's own update stands.
    pub fn hub_status(&self) -> &Status {
        &self.own.status
    }

    fn handle_hub(&mut self, ev: HubEvent) -> bool {
        match ev {
            HubEvent::Checked(Ok(Some(o))) => {
                // The worker fetches it next, unless it was told at its start that this version
                // is downloaded already. Then nothing is on its way, and the status is left as it
                // is: ready still, or failed since (a restart that could not swap it in), which a
                // later check fetches again. Calling that "downloading" would wait for an answer
                // that never comes and refuse every check after it.
                if self.own.told_ready.as_ref() != Some(&o.version) {
                    self.own.status = Status::Downloading(o.version);
                }
                true
            }
            HubEvent::Checked(Ok(None)) => {
                if matches!(self.own.status, Status::Ready(_)) {
                    false
                } else {
                    self.own.status = Status::None;
                    true
                }
            }
            // Offline, or no manifest yet: nothing to say about Hub, and the apps' check says
            // what is wrong in its own words.
            HubEvent::Checked(Err(_)) => false,
            HubEvent::Staged(Ok(s)) => {
                if !matches!(&self.own.status, Status::Ready(r) if r.version == s.version) {
                    self.own.dismissed = false;
                }
                self.own.status = Status::Ready(s);
                true
            }
            HubEvent::Staged(Err(e)) => {
                self.own.status = Status::Failed(e);
                true
            }
        }
    }

    /// Swaps the new Hub in and asks the shell to restart, or, with an install running, waits
    /// for the installs to be done: Hub swaps itself last. False when nothing is ready.
    fn hub_restart(&mut self) -> bool {
        let Status::Ready(staged) = self.own.status.clone() else {
            return false;
        };
        match selfupdate::apply_when_idle(&staged, self.installing()) {
            Apply::Deferred => {
                self.own.when_idle = true;
                self.notice = Some("Hub restarts when the installs are done".to_string());
            }
            Apply::Swapped => {
                self.own.when_idle = false;
                self.own.restart = Some(staged.exe);
                self.own.status = Status::None;
            }
            Apply::Failed(e) => {
                self.own.when_idle = false;
                self.own.status = Status::Failed(e);
            }
        }
        true
    }

    /// The swap is done and the old Hub should end now and start the new one from this exe.
    pub fn take_restart(&mut self) -> Option<PathBuf> {
        self.own.restart.take()
    }

    /// Hub is ending (not restarting): a downloaded update is swapped in now, when no install
    /// runs, so the next start is the new Hub. True when it was.
    pub fn apply_hub_on_quit(&mut self) -> bool {
        match self.own.status.clone() {
            Status::Ready(s) if self.fixed.is_none() => {
                selfupdate::apply_when_idle(&s, self.installing()) == Apply::Swapped
            }
            _ => false,
        }
    }

    // ---- telling the user ----

    /// What waits for an update, for the notification rule: Hub's own first, then the apps.
    fn pending_set(&self) -> Vec<Pending> {
        let mut v = Vec::new();
        match &self.own.status {
            Status::Available(o) => v.push(Pending::hub(&o.version.to_string())),
            Status::Downloading(ver) => v.push(Pending::hub(&ver.to_string())),
            Status::Ready(s) => v.push(Pending::hub(&s.version.to_string())),
            Status::None | Status::Failed(_) => {}
        }
        for s in &self.slots {
            if let Latest::Found(m) = &s.latest
                && check::row_state(s.installed.as_ref(), &s.latest, s.skipped.as_deref())
                    == Row::Update
            {
                v.push(Pending::app(s.app.name, &m.version.to_string()));
            }
        }
        v
    }

    /// Whose update status the last check could not see: Hub when its own download failed, an
    /// app whose release could not be read. What was told about them is kept (`decide_keeping`),
    /// so the same update is not announced again when it is seen again.
    fn unseen_names(&self) -> Vec<String> {
        let mut v = Vec::new();
        if matches!(self.own.status, Status::Failed(_)) {
            v.push("Hub".to_string());
        }
        for s in &self.slots {
            if !matches!(s.latest, Latest::Found(_)) {
                v.push(s.app.name.to_string());
            }
        }
        v
    }

    /// After a check: is there news the user was not told? Never when notifications are off or
    /// there is no icon to tell it with (the memory is left alone then, so the news is told
    /// later), and not when the user is looking at the window (it counts as told).
    fn decide_message(&mut self) {
        self.tell.msg = None;
        if !self.tell.on || !self.tell.tray_up || self.fixed.is_some() {
            return;
        }
        let before = self.tell.mem.to_line();
        let msg = self
            .tell
            .mem
            .decide_keeping(&self.pending_set(), &self.unseen_names());
        let line = self.tell.mem.to_line();
        if line != before {
            self.store.set_notified(&line);
        }
        if !self.tell.focused {
            self.tell.msg = msg;
        }
    }

    /// The balloon to show now, once: the shell takes it after each event.
    pub fn take_message(&mut self) -> Option<Message> {
        self.tell.msg.take()
    }

    /// The tray icon is up (or gone): only with one can anything be told.
    pub fn set_tray_up(&mut self, up: bool) {
        self.tell.tray_up = up;
    }

    /// The window is open and has the focus.
    pub fn set_focused(&mut self, focused: bool) {
        self.tell.focused = focused;
    }

    // ---- settings ----

    pub fn notify_on(&self) -> bool {
        self.tell.on
    }

    pub fn tray_on(&self) -> bool {
        self.tray_on
    }

    pub fn interval_hours(&self) -> u32 {
        self.interval
    }

    pub fn set_notify(&mut self, on: bool) {
        self.tell.on = on;
        if self.fixed.is_none() {
            self.store.set_notify_enabled(on);
        }
    }

    pub fn set_tray_enabled(&mut self, on: bool) {
        self.tray_on = on;
        if self.fixed.is_none() {
            self.store.set_tray_enabled(on);
        }
    }

    pub fn set_interval(&mut self, hours: u32) {
        self.interval = hours;
        if self.fixed.is_none() {
            self.store.set_interval_hours(hours);
        }
    }

    /// "Start with Windows" as the registry says (read the first time it is wanted).
    pub fn autostart_on(&self) -> bool {
        if let Some(b) = self.autostart.get() {
            return b;
        }
        let on = !self.run_key.key.is_empty()
            && hub_core::autostart::is_enabled_at(&self.run_key.key, &self.run_key.value);
        self.autostart.set(Some(on));
        on
    }

    /// Writes or removes the Run value. A refusal (an uninstalled copy, a registry error) leaves
    /// the setting as the registry has it and says why in the status bar.
    pub fn set_autostart(&mut self, on: bool) {
        if self.fixed.is_some() {
            return;
        }
        let (key, value) = (self.run_key.key.as_str(), self.run_key.value.as_str());
        // A copy that cannot be started at login has no say in the startup list, either way.
        let r = match &self.run_key.exe {
            Some(exe) if on => hub_core::autostart::enable_at(key, value, exe),
            Some(_) => hub_core::autostart::disable_at(key, value),
            None => Err("this copy of Hub is not installed".to_string()),
        };
        match r {
            Ok(()) => {
                self.autostart.set(Some(on));
                self.store.set_autostart_enabled(on);
            }
            Err(e) => {
                self.autostart.set(None);
                self.notice = Some(format!("Could not change Start with Windows: {e}"));
            }
        }
    }

    fn settings_model(&self) -> crate::settings::Model {
        let (hub_status, hub_button) = match (&self.own.exe, &self.own.status) {
            (None, _) => (
                "This copy was not installed, so it cannot update itself.".to_string(),
                "Update Hub",
            ),
            (Some(_), s @ Status::Ready(_)) => (s.label(), "Restart now"),
            (Some(_), Status::None) => (
                if self.last_check.is_some() {
                    "No update found at the last check".to_string()
                } else {
                    "Not checked yet".to_string()
                },
                "Update Hub",
            ),
            (Some(_), s) => (s.label(), "Update Hub"),
        };
        crate::settings::Model {
            start: self.autostart_on(),
            tray: self.tray_on,
            notify: self.tell.on,
            interval_hours: self.interval,
            version: env!("CARGO_PKG_VERSION").to_string(),
            hub_status,
            hub_button,
        }
    }

    /// The rows of the settings card now: the card is rebuilt from them whenever they differ from
    /// what it shows, so a change made through the tray menu appears in it.
    pub fn settings_rows(&self) -> Vec<SettingRow> {
        crate::settings::rows(&self.settings_model())
    }

    /// A change made in the card: applied and saved. False for an id or a value that is not one.
    pub fn change_setting(&mut self, id: u32, value: Value) -> bool {
        use crate::settings::{INTERVAL, NOTIFY, START, TRAY, interval_hours};
        match (id, value) {
            (START, Value::Bool(on)) => self.set_autostart(on),
            (TRAY, Value::Bool(on)) => self.set_tray_enabled(on),
            (NOTIFY, Value::Bool(on)) => self.set_notify(on),
            (INTERVAL, Value::Choice(i)) => match interval_hours(i) {
                Some(h) => self.set_interval(h),
                None => return false,
            },
            _ => return false,
        }
        true
    }

    /// A button of the card was pressed: "Update Hub" restarts into a downloaded update, or looks
    /// for one (Hub's own manifest is the first request of any check).
    pub fn setting_action(&mut self, id: u32) -> bool {
        if id != crate::settings::UPDATE_HUB || self.fixed.is_some() {
            return false;
        }
        if matches!(self.own.status, Status::Ready(_)) {
            self.own.dismissed = false;
            self.hub_restart()
        } else {
            self.start_check()
        }
    }

    // ---- events from the workers ----

    pub fn handle(&mut self, ev: Event) -> bool {
        let now = check::now();
        match ev {
            Event::Raise | Event::Tray(_) => false,
            Event::Hub(h) => self.handle_hub(h),
            Event::Notes {
                id,
                version,
                result,
            } => self.handle_notes(id, version, result),
            Event::Progress { id, p } => {
                let phase = match p {
                    Progress::Downloading { done, total, .. } => Phase::Downloading {
                        frac: if total == 0 {
                            0.0
                        } else {
                            (done as f32 / total as f32).clamp(0.0, 1.0)
                        },
                        text: progress_text(done, total),
                    },
                    Progress::Verifying(_) => Phase::Verifying,
                    Progress::Waiting(n) => {
                        Phase::Waiting(format!("{n} is open: close it to continue"))
                    }
                    Progress::Placing => Phase::Placing,
                    Progress::Finished => return false,
                };
                // A late event of a job that was cancelled must not bring its row back.
                if self.active.as_ref().is_some_and(|a| a.id == id) {
                    self.set_phase(id, phase);
                    true
                } else {
                    false
                }
            }
            Event::Finished { id, result } => {
                if self.active.as_ref().is_some_and(|a| a.id == id) {
                    self.active = None;
                }
                match result {
                    Ok(inst) => {
                        if let Some(s) = self.slot_mut(id) {
                            s.installed = Some(inst);
                            s.running = false;
                            s.phase = Phase::Done(now);
                        }
                    }
                    Err(InstallError::Cancelled) => {
                        self.set_phase(id, Phase::Idle);
                        self.notice = Some("Cancelled".to_string());
                    }
                    Err(e) => self.set_phase(id, Phase::Failed(fail_text(&e))),
                }
                self.pump();
                // Hub swaps itself last: the installs are done, and it was asked to.
                if self.own.when_idle && !self.installing() {
                    self.hub_restart();
                }
                true
            }
            Event::Checked(results) => {
                self.checking = false;
                for (id, latest) in results {
                    if let Some(s) = self.slot_mut(id) {
                        s.latest = latest;
                        if matches!(s.phase, Phase::Done(_)) {
                            s.phase = Phase::Idle;
                        }
                    }
                }
                // Not read back from the store: a store that cannot be written must not make every
                // tick look overdue.
                self.last_check = Some(now);
                self.failures = self.store.failures();
                self.refresh_running_async();
                self.decide_message();
                true
            }
            Event::Running(list) => {
                self.running_busy = false;
                let mut changed = false;
                for (id, r) in list {
                    if let Some(s) = self.slot_mut(id)
                        && s.installed.is_some()
                        && s.running != r
                    {
                        s.running = r;
                        changed = true;
                    }
                }
                changed
            }
        }
    }
}

// ---- the transport the program uses ---------------------------------------------------------------

/// The network, or in a build with `test-hooks` the folder `HUB_FETCH_ROOT` names; with
/// `HUB_FETCH_LOG` set as well, every URL asked for is appended to that file first (a test
/// uses it to see that nothing is requested at start).
pub fn make_transport() -> Arc<dyn Transport> {
    let inner = hub_core::transport::default_transport();
    #[cfg(feature = "test-hooks")]
    if let Some(log) = std::env::var_os("HUB_FETCH_LOG").filter(|l| !l.is_empty()) {
        struct Logged {
            inner: Box<dyn Transport>,
            log: std::path::PathBuf,
        }
        impl Transport for Logged {
            fn get(&self, url: &str, max: usize, t: Duration) -> Result<Vec<u8>, String> {
                use std::io::Write;
                if let Ok(mut f) = std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.log)
                {
                    let _ = writeln!(f, "{url}");
                }
                self.inner.get(url, max, t)
            }
        }
        return Arc::new(Logged {
            inner,
            log: log.into(),
        });
    }
    Arc::from(inner)
}
