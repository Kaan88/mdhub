//! The glue between hub-core and the window: the headless `Engine` that owns what Hub knows (what
//! is installed, what is latest, what an install is doing), turns a `Hit` or a key into work, runs
//! the work on worker threads one install at a time, and builds the `Vm` the drawing code reads.
//! Nothing here touches a window, so every behaviour is tested without one. Nothing here asks the
//! network anything until a check is asked for (the first one is scheduled, never run at start).
use crate::vm::{Act, AppVm, Banner, Hit, View, Vm};
use crate::work;
use hub_core::catalog::{CATALOG, CatalogApp};
use hub_core::check;
use hub_core::detect::{self, Env};
use hub_core::install::{Closer, Elevation, InstallError, Opts, Progress, RealCloser};
use hub_core::manifest;
use hub_core::model::{HubManifest, Installed, Kind, Latest, Row, Scope, Source};
use hub_core::state::{Store, ViewMode};
use hub_core::transport::Transport;
use std::collections::VecDeque;
use std::path::Path;
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
    /// A second launch asked for this window (the shell raises it).
    Raise,
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
        Hit::BannerYes => Some(Cmd::BannerYes),
        Hit::BannerNo => Some(Cmd::BannerNo),
        Hit::Drag | Hit::Min | Hit::Max | Hit::Close | Hit::None => None,
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
        _ => None,
    }
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
    hover: Option<usize>,
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
        }
    }

    pub fn is_demo(&self) -> bool {
        self.fixed.is_some()
    }

    /// Is any work in flight (a check, an install, a queued install)?
    pub fn busy(&self) -> bool {
        self.active.is_some() || self.checking || !self.queue.is_empty()
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
        let banner = self.banner.and_then(|id| {
            let name = CATALOG.iter().find(|c| c.id == id)?.name;
            Some(Banner {
                text: format!("{name} is open: close it and update"),
                yes: "Close and update".into(),
                no: "Not now".into(),
                warn: false,
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
            hub_version: env!("CARGO_PKG_VERSION").to_string(),
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

    // ---- scheduling ----

    /// How long the loop may sleep before the engine wants to run again; None: not on a timer.
    pub fn wait(&self, now: u64) -> Option<Duration> {
        if self.fixed.is_some() || self.checking {
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
                }
                true
            }
            Cmd::FilterBack => {
                let had = self.filter.pop().is_some();
                self.sel = 0;
                had
            }
            Cmd::FilterBlur => {
                let changed = self.filter_focus;
                self.filter_focus = false;
                changed
            }
            Cmd::Esc => {
                if self.banner.is_some() {
                    self.banner = None;
                    true
                } else if !self.filter.is_empty() {
                    self.filter.clear();
                    self.sel = 0;
                    true
                } else if self.filter_focus {
                    self.filter_focus = false;
                    true
                } else {
                    false
                }
            }
            Cmd::Primary => {
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
                if self.slot(id).is_some_and(|s| s.running) {
                    self.banner = Some(id);
                    true
                } else {
                    self.enqueue(id, JobKind::Install)
                }
            }
            Cmd::Adopt(id) => self.adopt(id),
            Cmd::Cancel(id) => self.cancel(id),
            Cmd::CancelAll => {
                let ids: Vec<&'static str> = self.queue.iter().map(|(i, _)| *i).collect();
                self.queue.clear();
                self.pending_close.clear();
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
                    self.enqueue(id, JobKind::Install);
                    true
                }
                None => false,
            },
            Cmd::BannerNo => {
                self.pending_close.clear();
                self.banner.take().is_some()
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
            self.set_phase(id, Phase::Idle);
            return true;
        }
        false
    }

    fn update_all(&mut self, _now: u64) -> bool {
        // Every app with an update goes into the queue, in the window's order; one that is open
        // is asked about first (the offer bar), never closed without asking.
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

    fn start_check(&mut self) -> bool {
        if self.checking || self.fixed.is_some() {
            return false;
        }
        self.first = None;
        self.notice = None;
        match work::spawn_check(
            self.post.clone(),
            self.transport.clone(),
            self.store.clone(),
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
                self.enqueue(id, JobKind::Rollback)
            }
        }
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

    // ---- events from the workers ----

    pub fn handle(&mut self, ev: Event) -> bool {
        let now = check::now();
        match ev {
            Event::Raise => false,
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
                self.refresh_running();
                true
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
