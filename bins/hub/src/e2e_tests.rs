//! End to end, without a window: the engine with a folder standing in for github.com (the
//! `FileRoot` transport serving a fake release), a scratch programs folder and a scratch data
//! folder. Installs, updates, a roll back, a hash mismatch, a failing setup, the queue and the
//! "it is open" offer, each asserted on the rows the window would draw. Nothing here touches this
//! PC's apps, its Hub folder or the network.
use crate::glue::{Cmd, Deps, Engine, Event, MenuItem, Post, Shell, Tmpl, app_vm, command};
use crate::vm::{Act, Hit, Vm};
use hub_core::catalog::{CatalogApp, find};
use hub_core::check;
use hub_core::detect::Env;
use hub_core::install::{Closer, Elevation, SetupRunner};
use hub_core::manifest;
use hub_core::model::{
    FileSpec, HubManifest, Installed, Kind, PlatformEntry, Row, Scope, SetupSpec,
};
use hub_core::state::Store;
use hub_core::transport::{FileRoot, Transport};
use mdshared_app::update::{Version, sha256_hex};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub(crate) struct Chan(pub Mutex<Sender<Event>>);

impl Post for Chan {
    fn post(&self, ev: Event) {
        if let Ok(s) = self.0.lock() {
            let _ = s.send(ev);
        }
    }
}

/// A transport over the fake release that counts what is asked, and how many files are in
/// flight at once.
pub(crate) struct Counting {
    pub inner: FileRoot,
    pub inflight: AtomicUsize,
    pub max_files: AtomicUsize,
    pub urls: Mutex<Vec<String>>,
    pub file_delay: Duration,
}

impl Transport for Counting {
    fn get(&self, url: &str, max: usize, t: Duration) -> Result<Vec<u8>, String> {
        if let Ok(mut u) = self.urls.lock() {
            u.push(url.to_string());
        }
        let is_file = url.contains("/releases/download/");
        if is_file {
            let n = self.inflight.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_files.fetch_max(n, Ordering::SeqCst);
            std::thread::sleep(self.file_delay);
        }
        let r = self.inner.get(url, max, t);
        if is_file {
            self.inflight.fetch_sub(1, Ordering::SeqCst);
        }
        r
    }
}

#[derive(Default)]
pub(crate) struct FakeShell {
    pub running: AtomicBool,
    /// With `running`, mdview is open as well.
    pub view_too: AtomicBool,
    pub launched: Mutex<Vec<String>>,
    pub revealed: Mutex<Vec<PathBuf>>,
    pub urls: Mutex<Vec<String>>,
    /// The threads `running` was asked on.
    pub run_threads: Mutex<Vec<std::thread::ThreadId>>,
}

impl Shell for FakeShell {
    fn launch(&self, app: &CatalogApp, _: &Installed) -> std::io::Result<()> {
        self.launched.lock().unwrap().push(app.id.to_string());
        Ok(())
    }
    fn reveal(&self, path: &Path) -> bool {
        self.revealed.lock().unwrap().push(path.to_path_buf());
        true
    }
    fn open_url(&self, url: &str) -> bool {
        self.urls.lock().unwrap().push(url.to_string());
        true
    }
    fn running(&self, exe: &Path) -> bool {
        self.run_threads
            .lock()
            .unwrap()
            .push(std::thread::current().id());
        let name = exe
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        self.running.load(Ordering::SeqCst)
            && (name.starts_with("mdedit")
                || (self.view_too.load(Ordering::SeqCst) && name.starts_with("mdview")))
    }
}

struct NeverRunning;

impl Closer for NeverRunning {
    fn is_running(&self, _: &[PathBuf]) -> bool {
        false
    }
    fn ask_close(&self, _: &[PathBuf]) {}
}

/// The installer of a Setup release: it "installs" by copying the downloaded file to
/// `<programs>\Sprawl\sprawl.exe`, or exits with `code`.
pub(crate) struct FakeSetup {
    pub programs: PathBuf,
    pub code: AtomicU32,
    pub calls: Mutex<Vec<(String, Scope)>>,
}

impl SetupRunner for FakeSetup {
    fn run(&self, exe: &Path, args: &str, _dir: &Path, scope: Scope) -> Result<u32, String> {
        self.calls.lock().unwrap().push((args.to_string(), scope));
        let code = self.code.load(Ordering::SeqCst);
        if code == 0 {
            let dir = self.programs.join("Sprawl");
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            std::fs::copy(exe, dir.join("sprawl.exe")).map_err(|e| e.to_string())?;
        }
        Ok(code)
    }
}

pub(crate) struct Fx {
    pub root: PathBuf,
    pub programs: PathBuf,
    pub eng: Engine,
    pub rx: Receiver<Event>,
    pub tr: Arc<Counting>,
    pub shell: Arc<FakeShell>,
    pub setup: Arc<FakeSetup>,
    /// Every row an event produced, in order.
    pub seen: Vec<(&'static str, Row)>,
}

pub(crate) fn body(tag: &str) -> Vec<u8> {
    format!("MZ fake program {tag} ").repeat(40).into_bytes()
}

pub(crate) fn ver(v: &str) -> Version {
    Version::parse(v).unwrap()
}

impl Fx {
    pub fn new(name: &str) -> Fx {
        Fx::with_delay(name, Duration::ZERO)
    }

    pub fn with_delay(name: &str, file_delay: Duration) -> Fx {
        let root = std::env::temp_dir().join(format!("hub-e2e-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let programs = root.join("programs");
        for d in ["release", "programs", "data"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        let tr = Arc::new(Counting {
            inner: FileRoot::new(root.join("release")),
            inflight: AtomicUsize::new(0),
            max_files: AtomicUsize::new(0),
            urls: Mutex::new(Vec::new()),
            file_delay,
        });
        let (tx, rx) = channel();
        let shell = Arc::new(FakeShell::default());
        let setup = Arc::new(FakeSetup {
            programs: programs.clone(),
            code: AtomicU32::new(0),
            calls: Mutex::new(Vec::new()),
        });
        let eng = Fx::engine(&root, &programs, &tr, tx, &shell, &setup);
        Fx {
            root,
            programs,
            eng,
            rx,
            tr,
            shell,
            setup,
            seen: Vec::new(),
        }
    }

    fn engine(
        root: &Path,
        programs: &Path,
        tr: &Arc<Counting>,
        tx: Sender<Event>,
        shell: &Arc<FakeShell>,
        setup: &Arc<FakeSetup>,
    ) -> Engine {
        Engine::new(Deps {
            store: Store::at(root.join("data")),
            env: Env::custom(programs, Vec::new(), Vec::new()),
            transport: tr.clone(),
            post: Arc::new(Chan(Mutex::new(tx))),
            shell: shell.clone(),
            tmpl: Tmpl {
                elevate: Elevation::Custom(setup.clone()),
                closer: Arc::new(NeverRunning),
                close_timeout: Duration::from_millis(200),
                poll: Duration::from_millis(10),
            },
            first_check: None,
            hub_exe: None,
            run_key: crate::glue::RunKey::none(),
        })
    }

    /// Starts the engine again over the same folders: what the next start of Hub sees.
    pub fn restart(&mut self) {
        let (tx, rx) = channel();
        self.eng = Fx::engine(
            &self.root,
            &self.programs,
            &self.tr,
            tx,
            &self.shell,
            &self.setup,
        );
        self.rx = rx;
    }

    fn release_dir(&self, app: &CatalogApp, tail: &str) -> PathBuf {
        let mut p = self.root.join("release").join("Kaan88").join(app.repo);
        for part in tail.split('/') {
            p.push(part);
        }
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn url(app: &CatalogApp, version: &str, name: &str) -> String {
        format!(
            "https://github.com/Kaan88/{}/releases/download/v{version}/{name}",
            app.repo
        )
    }

    /// Publishes a `Files` release: `files` are (name, content); the manifest lists the sha of
    /// `claimed` instead of the real one when given (a download that will not match).
    pub fn publish_files(
        &self,
        id: &str,
        version: &str,
        files: &[(&str, Vec<u8>)],
        claimed: Option<&str>,
    ) {
        let app = find(id).unwrap();
        let dl = self.release_dir(app, &format!("releases/download/v{version}"));
        let mut specs = Vec::new();
        for (name, bytes) in files {
            std::fs::write(dl.join(name), bytes).unwrap();
            specs.push(FileSpec {
                name: name.to_string(),
                url: Fx::url(app, version, name),
                sha256: claimed.map_or_else(|| sha256_hex(bytes), str::to_string),
                size: bytes.len() as u64,
            });
        }
        self.write_manifest(app, version, Kind::Files, specs, None);
    }

    /// Publishes a `Setup` release whose installer file is `bytes`.
    pub fn publish_setup(&self, id: &str, version: &str, bytes: &[u8], scope: Scope) {
        let app = find(id).unwrap();
        let name = format!("{id}-setup-{version}.exe");
        let dl = self.release_dir(app, &format!("releases/download/v{version}"));
        std::fs::write(dl.join(&name), bytes).unwrap();
        let file = FileSpec {
            name: name.clone(),
            url: Fx::url(app, version, &name),
            sha256: sha256_hex(bytes),
            size: bytes.len() as u64,
        };
        let setup = SetupSpec {
            file,
            silent: "/VERYSILENT /SUPPRESSMSGBOXES".to_string(),
            scope,
        };
        self.write_manifest(app, version, Kind::Setup, Vec::new(), Some(setup));
    }

    fn write_manifest(
        &self,
        app: &CatalogApp,
        version: &str,
        kind: Kind,
        files: Vec<FileSpec>,
        setup: Option<SetupSpec>,
    ) {
        let m = HubManifest {
            app: app.id.to_string(),
            version: ver(version),
            date: "2026-11-21".to_string(),
            notes: format!("notes of {version}"),
            notes_file: None,
            platforms: vec![PlatformEntry {
                platform: manifest::PLATFORM.to_string(),
                kind,
                exe: Some(app.exe.to_string()),
                files,
                zip: None,
                setup,
                uninstall: None,
                min: None,
                wait: Vec::new(),
                updates_self: false,
            }],
        };
        let latest = self.release_dir(app, "releases/latest/download");
        std::fs::write(latest.join("hub.txt"), manifest::write(&m)).unwrap();
    }

    pub fn vm(&self) -> Vm {
        self.eng.vm(check::now())
    }

    pub fn app(&self, id: &str) -> crate::vm::AppVm {
        self.vm().apps.into_iter().find(|a| a.id == id).unwrap()
    }

    pub fn row(&self, id: &str) -> Row {
        self.app(id).row
    }

    pub fn cmd(&mut self, c: Cmd) {
        self.eng.apply(c);
    }

    /// Runs the engine until nothing is in flight, recording every row seen.
    pub fn settle(&mut self) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while self.eng.busy() {
            match self.rx.recv_timeout(Duration::from_millis(100)) {
                Ok(ev) => {
                    let id = match &ev {
                        Event::Progress { id, .. } | Event::Finished { id, .. } => Some(*id),
                        _ => None,
                    };
                    self.eng.handle(ev);
                    if let Some(id) = id {
                        let r = self.row(id);
                        self.seen.push((id, r));
                    }
                }
                Err(_) => assert!(Instant::now() < deadline, "the engine never settled"),
            }
        }
    }

    pub fn check(&mut self) {
        self.cmd(Cmd::CheckNow);
        assert!(self.eng.checking());
        self.settle();
    }

    pub fn file(&self, id: &str, name: &str) -> Option<Vec<u8>> {
        let app = find(id).unwrap();
        std::fs::read(self.programs.join(app.folder).join(name)).ok()
    }

    pub fn seen_rows(&self, id: &str) -> Vec<Row> {
        self.seen
            .iter()
            .filter(|(i, _)| *i == id)
            .map(|(_, r)| r.clone())
            .collect()
    }

    /// The file URLs asked for, in order.
    pub fn file_urls(&self) -> Vec<String> {
        self.tr
            .urls
            .lock()
            .unwrap()
            .iter()
            .filter(|u| u.contains("/releases/download/"))
            .cloned()
            .collect()
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

fn is_done(r: &Row) -> bool {
    matches!(r, Row::Done(_))
}

#[test]
fn nothing_is_known_until_a_check_and_then_an_app_can_be_installed() {
    let mut fx = Fx::new("fresh");
    fx.publish_files("mdedit", "1.2.0", &[("mdedit.exe", body("v1"))], None);
    // Start: no network, so every row says it has not been asked.
    assert_eq!(fx.row("mdedit"), Row::NoInfo("Not checked yet".into()));
    assert!(
        fx.tr.urls.lock().unwrap().is_empty(),
        "nothing asked at start"
    );
    fx.check();
    // mdedit has a release; mdview's release carries no manifest at all.
    assert_eq!(fx.row("mdedit"), Row::NotInstalled);
    assert_eq!(fx.app("mdedit").latest.as_deref(), Some("1.2.0"));
    assert!(matches!(fx.row("mdview"), Row::NoInfo(_)));
    assert!(fx.vm().checked.starts_with("Checked"));

    fx.cmd(Cmd::Install("mdedit"));
    assert!(fx.eng.busy());
    fx.settle();
    assert!(is_done(&fx.row("mdedit")), "{:?}", fx.row("mdedit"));
    assert_eq!(fx.file("mdedit", "mdedit.exe"), Some(body("v1")));
    assert_eq!(fx.app("mdedit").installed.as_deref(), Some("1.2.0"));
    // The rows went Verifying before they were Done, and never skipped to Done.
    let rows = fx.seen_rows("mdedit");
    let verifying = rows
        .iter()
        .position(|r| *r == Row::Verifying)
        .expect("verifying");
    let done = rows.iter().position(is_done).expect("done");
    assert!(verifying < done, "{rows:?}");
    assert!(
        rows.iter().any(|r| matches!(r, Row::Downloading(..))),
        "{rows:?}"
    );
    // The cache of the finished install is gone, and the install is on record: the next start
    // finds it as Hub's own, with the release the check cached.
    assert!(!fx.eng.store.cache_dir("mdedit", "1.2.0").exists());
    fx.restart();
    let a = app_vm(&fx.eng.slots[0], check::now());
    assert_eq!(a.installed.as_deref(), Some("1.2.0"));
    assert_eq!(a.row, Row::UpToDate, "its release was cached by the check");
}

#[test]
fn an_update_replaces_the_files_and_a_roll_back_puts_them_back() {
    let mut fx = Fx::new("update");
    fx.publish_files(
        "mdedit",
        "1.2.0",
        &[
            ("mdedit.exe", body("v1")),
            ("mdview-engine.exe", body("e1")),
        ],
        None,
    );
    fx.check();
    fx.cmd(Cmd::Install("mdedit"));
    fx.settle();
    // A newer release appears: the next check turns the row into an update.
    fx.publish_files(
        "mdedit",
        "1.3.0",
        &[
            ("mdedit.exe", body("v2")),
            ("mdview-engine.exe", body("e2")),
        ],
        None,
    );
    assert!(is_done(&fx.row("mdedit")), "Done stays until a check");
    fx.check();
    assert_eq!(fx.row("mdedit"), Row::Update);
    assert_eq!(fx.vm().updates, 1);
    assert_eq!(fx.app("mdedit").latest.as_deref(), Some("1.3.0"));
    assert_eq!(fx.app("mdedit").installed.as_deref(), Some("1.2.0"));

    fx.cmd(Cmd::Update("mdedit"));
    fx.settle();
    assert!(is_done(&fx.row("mdedit")));
    assert_eq!(fx.file("mdedit", "mdedit.exe"), Some(body("v2")));
    assert_eq!(fx.file("mdedit", "mdview-engine.exe"), Some(body("e2")));
    assert_eq!(fx.app("mdedit").installed.as_deref(), Some("1.3.0"));
    assert_eq!(fx.vm().updates, 0);

    // The menu offers the roll back now, and taking it puts the retired files back.
    let items = fx.eng.menu_items("mdedit");
    assert!(
        items.contains(&(MenuItem::RollBack, "Roll back", true)),
        "{items:?}"
    );
    fx.cmd(Cmd::MenuPick("mdedit", MenuItem::RollBack));
    fx.settle();
    assert_eq!(fx.file("mdedit", "mdedit.exe"), Some(body("v1")));
    assert_eq!(fx.file("mdedit", "mdview-engine.exe"), Some(body("e1")));
    assert_eq!(fx.app("mdedit").installed.as_deref(), Some("1.2.0"));
    assert!(
        !fx.eng
            .menu_items("mdedit")
            .iter()
            .any(|i| i.0 == MenuItem::RollBack),
        "nothing left to roll back to"
    );
}

#[test]
fn a_hash_mismatch_installs_nothing_and_try_again_works_once_the_release_is_right() {
    let mut fx = Fx::new("mismatch");
    fx.publish_files("mdedit", "1.2.0", &[("mdedit.exe", body("v1"))], None);
    fx.check();
    fx.cmd(Cmd::Install("mdedit"));
    fx.settle();
    // 1.4.0 lists a hash its file does not have.
    let wrong = sha256_hex(b"something else");
    fx.publish_files(
        "mdedit",
        "1.4.0",
        &[("mdedit.exe", body("bad"))],
        Some(&wrong),
    );
    fx.check();
    assert_eq!(fx.row("mdedit"), Row::Update);
    fx.cmd(Cmd::Update("mdedit"));
    fx.settle();
    let Row::Failed(why) = fx.row("mdedit") else {
        panic!("{:?}", fx.row("mdedit"));
    };
    assert!(why.contains("did not match its SHA-256"), "{why}");
    assert!(why.contains("nothing was installed"), "{why}");
    assert_eq!(
        fx.file("mdedit", "mdedit.exe"),
        Some(body("v1")),
        "the old program is untouched"
    );
    assert_eq!(fx.app("mdedit").installed.as_deref(), Some("1.2.0"));
    assert!(
        !fx.eng.store.cache_dir("mdedit", "1.4.0").exists(),
        "the bad download is gone"
    );
    // The row offers Try again; the release is fixed; the same button now installs.
    assert!(
        crate::view::row_actions(&fx.app("mdedit"))
            .iter()
            .any(|b| b.act == Act::TryAgain)
    );
    fx.publish_files("mdedit", "1.4.0", &[("mdedit.exe", body("good"))], None);
    fx.check();
    assert!(
        matches!(fx.row("mdedit"), Row::Failed(_)),
        "a check does not erase a failure"
    );
    fx.cmd(Cmd::TryAgain("mdedit"));
    fx.settle();
    assert!(is_done(&fx.row("mdedit")), "{:?}", fx.row("mdedit"));
    assert_eq!(fx.file("mdedit", "mdedit.exe"), Some(body("good")));
}

#[test]
fn a_machine_setup_runs_only_after_the_click_and_a_failing_one_leaves_things_as_they_were() {
    let mut fx = Fx::new("setup");
    fx.publish_setup("sprawl", "1.0.5", &body("sprawl-105"), Scope::Machine);
    fx.check();
    assert_eq!(fx.row("sprawl"), Row::NotInstalled);
    assert!(
        fx.setup.calls.lock().unwrap().is_empty(),
        "nothing runs elevated before the click"
    );
    // A failing installer: the row says so and nothing is recorded.
    fx.setup.code.store(3, Ordering::SeqCst);
    fx.cmd(Cmd::Install("sprawl"));
    fx.settle();
    let Row::Failed(why) = fx.row("sprawl") else {
        panic!("{:?}", fx.row("sprawl"));
    };
    assert!(why.contains("code 3"), "{why}");
    assert!(fx.file("sprawl", "sprawl.exe").is_none());
    assert_eq!(fx.app("sprawl").installed, None);
    // The user tries again, the installer works this time.
    fx.setup.code.store(0, Ordering::SeqCst);
    fx.cmd(Cmd::TryAgain("sprawl"));
    fx.settle();
    assert!(is_done(&fx.row("sprawl")), "{:?}", fx.row("sprawl"));
    assert_eq!(fx.file("sprawl", "sprawl.exe"), Some(body("sprawl-105")));
    assert_eq!(fx.app("sprawl").installed.as_deref(), Some("1.0.5"));
    let calls = fx.setup.calls.lock().unwrap().clone();
    assert_eq!(calls.len(), 2);
    assert!(
        calls
            .iter()
            .all(|(a, s)| a == "/VERYSILENT /SUPPRESSMSGBOXES" && *s == Scope::Machine)
    );
}

#[test]
fn update_all_installs_one_app_at_a_time() {
    let mut fx = Fx::with_delay("updateall", Duration::from_millis(40));
    let apps = [
        ("mdedit", "mdedit.exe"),
        ("mdview", "mdview.exe"),
        ("sprawl", "sprawl.exe"),
    ];
    for (id, exe) in apps {
        fx.publish_files(id, "1.0.0", &[(exe, body(&format!("{id}1")))], None);
    }
    fx.check();
    for (id, _) in apps {
        fx.cmd(Cmd::Install(id));
    }
    fx.settle();
    for (id, exe) in apps {
        assert_eq!(fx.file(id, exe), Some(body(&format!("{id}1"))), "{id}");
    }
    for (id, exe) in apps {
        fx.publish_files(id, "2.0.0", &[(exe, body(&format!("{id}2")))], None);
    }
    fx.check();
    assert_eq!(fx.vm().updates, 3);
    let before = fx.file_urls().len();
    fx.tr.max_files.store(0, Ordering::SeqCst);
    fx.cmd(Cmd::UpdateAll);
    // The bar says how many; the first has started and the other two wait their turn.
    assert_eq!(fx.vm().busy.as_deref(), Some("Updating 3 apps\u{2026}"));
    let rows: Vec<Row> = apps.iter().map(|(id, _)| fx.row(id)).collect();
    assert!(matches!(rows[0], Row::Downloading(..)), "{rows:?}");
    assert!(matches!(rows[1], Row::Waiting(_)), "{rows:?}");
    assert!(matches!(rows[2], Row::Waiting(_)), "{rows:?}");
    fx.settle();
    for (id, exe) in apps {
        assert_eq!(fx.file(id, exe), Some(body(&format!("{id}2"))), "{id}");
        assert!(is_done(&fx.row(id)), "{id}: {:?}", fx.row(id));
    }
    assert_eq!(
        fx.tr.max_files.load(Ordering::SeqCst),
        1,
        "never two downloads at once"
    );
    // In the window's order, each app's download finished before the next one began.
    let urls: Vec<String> = fx.file_urls().split_off(before);
    let order: Vec<&str> = urls.iter().map(|u| u.split('/').nth(4).unwrap()).collect();
    assert_eq!(
        order,
        ["markdown-editor-rs", "markdown-viewer-rs", "sprawl"],
        "{urls:?}"
    );
}

#[test]
fn an_app_that_is_open_is_asked_about_first_and_never_closed_without_a_yes() {
    let mut fx = Fx::new("open");
    fx.publish_files("mdedit", "1.0.0", &[("mdedit.exe", body("a"))], None);
    fx.publish_files("mdview", "1.0.0", &[("mdview.exe", body("m"))], None);
    fx.check();
    fx.cmd(Cmd::Install("mdedit"));
    fx.cmd(Cmd::Install("mdview"));
    fx.settle();
    fx.publish_files("mdedit", "1.1.0", &[("mdedit.exe", body("b"))], None);
    fx.publish_files("mdview", "1.1.0", &[("mdview.exe", body("n"))], None);
    fx.check();
    fx.shell.running.store(true, Ordering::SeqCst);
    assert!(fx.eng.refresh_running());
    assert!(fx.app("mdedit").running);
    let before = fx.file_urls().len();
    // Its Update button, hit on the window, asks first.
    let vm = fx.vm();
    assert_eq!(
        command(Hit::Action(0, Act::Update), &vm),
        Some(Cmd::Update("mdedit"))
    );
    fx.cmd(Cmd::Update("mdedit"));
    let b = fx.vm().banner.expect("the offer bar");
    assert_eq!(b.text, "mdedit is open: close it and update");
    assert!(!fx.eng.busy(), "nothing started yet");
    assert_eq!(fx.file_urls().len(), before, "nothing downloaded yet");
    // No: it goes away and nothing happened.
    fx.cmd(Cmd::BannerNo);
    assert!(fx.vm().banner.is_none());
    assert_eq!(fx.file("mdedit", "mdedit.exe"), Some(body("a")));
    // Update all: mdview goes at once, mdedit is asked about when mdview is done.
    fx.cmd(Cmd::UpdateAll);
    assert!(fx.vm().banner.is_none(), "mdview first");
    fx.settle();
    assert_eq!(fx.file("mdview", "mdview.exe"), Some(body("n")));
    assert_eq!(
        fx.file("mdedit", "mdedit.exe"),
        Some(body("a")),
        "still open, still old"
    );
    assert!(fx.vm().banner.is_some(), "now mdedit is asked about");
    // Yes: the install runs (the fake closer finds nothing open, as if it had closed at once).
    fx.cmd(Cmd::BannerYes);
    assert!(fx.vm().banner.is_none());
    fx.settle();
    assert_eq!(fx.file("mdedit", "mdedit.exe"), Some(body("b")));
}

#[test]
fn adopting_and_skipping_a_version_are_remembered() {
    let mut fx = Fx::new("skip");
    // A copy of sprawl that Hub did not put there: found by its folder, offered for Adopt.
    let dir = fx.programs.join("Sprawl");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("sprawl.exe"), body("old")).unwrap();
    fx.restart();
    fx.publish_setup("sprawl", "1.0.5", &body("new"), Scope::User);
    fx.check();
    assert_eq!(fx.row("sprawl"), Row::External);
    fx.cmd(Cmd::Adopt("sprawl"));
    // Adopted, it is Hub's now: its version is unknown, so there is an update to offer.
    assert_eq!(fx.row("sprawl"), Row::Update);
    fx.restart();
    assert_ne!(
        fx.row("sprawl"),
        Row::External,
        "the next start still knows it is adopted"
    );
    // Skip this version: the row is up to date, and a fresh start remembers.
    fx.cmd(Cmd::MenuPick("sprawl", MenuItem::SkipVersion));
    assert_eq!(fx.row("sprawl"), Row::UpToDate);
    assert_eq!(fx.eng.store.skipped("sprawl").as_deref(), Some("1.0.5"));
    assert!(
        !fx.eng
            .menu_items("sprawl")
            .iter()
            .any(|i| i.0 == MenuItem::SkipVersion && i.2),
        "nothing to skip now"
    );
}

#[test]
fn the_menu_opens_folders_and_pages_and_the_launch_goes_to_the_shell() {
    let mut fx = Fx::new("menu");
    fx.publish_files("mdview", "1.0.0", &[("mdview.exe", body("v"))], None);
    fx.check();
    fx.cmd(Cmd::Install("mdview"));
    fx.settle();
    fx.cmd(Cmd::Open("mdview"));
    assert_eq!(*fx.shell.launched.lock().unwrap(), ["mdview"]);
    fx.cmd(Cmd::MenuPick("mdview", MenuItem::ShowInFolder));
    let shown = fx.shell.revealed.lock().unwrap().clone();
    assert_eq!(shown.len(), 1);
    assert!(shown[0].ends_with("mdview.exe"));
    fx.cmd(Cmd::MenuPick("mdview", MenuItem::GitHub));
    assert_eq!(
        *fx.shell.urls.lock().unwrap(),
        ["https://github.com/Kaan88/markdown-viewer-rs"]
    );
    // An app that is not installed cannot be opened or shown.
    let items = fx.eng.menu_items("mdedit");
    assert!(items.iter().any(|i| i.0 == MenuItem::Open && !i.2));
    assert!(items.iter().any(|i| i.0 == MenuItem::GitHub && i.2));
}

/// mdedit and mdview installed at 1.0.0 with 1.1.0 out, files served slowly, mdedit open.
fn two_with_updates(name: &str) -> Fx {
    let mut fx = Fx::with_delay(name, Duration::from_millis(300));
    fx.publish_files("mdedit", "1.0.0", &[("mdedit.exe", body("a"))], None);
    fx.publish_files("mdview", "1.0.0", &[("mdview.exe", body("m"))], None);
    fx.check();
    fx.cmd(Cmd::Install("mdedit"));
    fx.cmd(Cmd::Install("mdview"));
    fx.settle();
    fx.publish_files("mdedit", "1.1.0", &[("mdedit.exe", body("b"))], None);
    fx.publish_files("mdview", "1.1.0", &[("mdview.exe", body("n"))], None);
    fx.check();
    fx.shell.running.store(true, Ordering::SeqCst);
    fx
}

/// "Close and update" is a yes to the install it starts, not a standing permission: when that
/// install is cancelled before it began, a later one asks again before it closes the app.
#[test]
fn a_yes_to_closing_an_app_does_not_outlive_the_install_it_was_given_for() {
    let mut fx = two_with_updates("consent-cancel");
    // mdview (not open) is updating; mdedit (open) is asked about, answered yes, and waits.
    fx.cmd(Cmd::Update("mdview"));
    assert!(fx.eng.installing());
    fx.cmd(Cmd::Update("mdedit"));
    assert!(fx.vm().banner.is_some());
    fx.cmd(Cmd::BannerYes);
    assert!(matches!(fx.row("mdedit"), Row::Waiting(_)), "queued");
    // The user thinks better of it.
    fx.cmd(Cmd::Cancel("mdedit"));
    // Later, with mdedit closed, Update all queues it again behind mdview ...
    fx.shell.running.store(false, Ordering::SeqCst);
    fx.cmd(Cmd::UpdateAll);
    assert!(matches!(fx.row("mdedit"), Row::Waiting(_)), "queued again");
    // ... and mdedit is opened before its turn comes.
    fx.shell.running.store(true, Ordering::SeqCst);
    fx.settle();
    assert_eq!(fx.file("mdview", "mdview.exe"), Some(body("n")));
    assert!(
        fx.file("mdedit", "mdedit.exe") == Some(body("a")),
        "open and not asked about this time: left alone"
    );
    assert!(fx.vm().banner.is_some(), "asked again");
}

/// The bar asks about an app; the user closes the app and presses its Update button instead.
/// The question is answered by that: the bar goes, and no yes is left behind for later.
#[test]
fn the_offer_bar_goes_when_its_app_is_updated_another_way() {
    let mut fx = two_with_updates("consent-stale-bar");
    fx.cmd(Cmd::Update("mdedit"));
    assert!(fx.vm().banner.is_some());
    fx.shell.running.store(false, Ordering::SeqCst);
    fx.cmd(Cmd::Update("mdedit"));
    assert!(fx.eng.installing(), "closed by hand: it installs");
    assert!(fx.vm().banner.is_none(), "nothing left to ask");
    // A click that still lands where the bar was answers nothing.
    assert!(!fx.eng.apply(Cmd::BannerYes));
    fx.settle();
    assert_eq!(fx.file("mdedit", "mdedit.exe"), Some(body("b")));
}

/// A progress report that arrives after its job was cancelled and reported (the worker's last
/// words, still in the queue) must not put the row back into "downloading".
#[test]
fn a_late_progress_report_of_a_job_that_is_over_does_not_bring_its_row_back() {
    let mut fx = two_with_updates("late-progress");
    fx.shell.running.store(false, Ordering::SeqCst);
    fx.cmd(Cmd::Update("mdedit"));
    fx.cmd(Cmd::Cancel("mdedit"));
    fx.settle();
    assert_eq!(fx.row("mdedit"), Row::Update, "cancelled: as it was");
    let late = Event::Progress {
        id: "mdedit",
        p: hub_core::install::Progress::Downloading {
            file: "mdedit.exe".into(),
            done: 1,
            total: 2,
        },
    };
    assert!(!fx.eng.handle(late), "nothing changed");
    assert_eq!(fx.row("mdedit"), Row::Update);
}
