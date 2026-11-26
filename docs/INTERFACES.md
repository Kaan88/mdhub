# Interfaces between the modules (the contract the build agents share)

Hub = `crates/hub-core` (headless) + `bins/hub` (window). Windows first; nothing here needs to
compile off Windows, but gate OS calls with `#[cfg(windows)]` and give the other targets a
harmless stub so `cargo check` does not break the shared-crates habit.

Fixed already (do not change signatures without saying so in your report): `catalog.rs`,
`model.rs`, `transport.rs`'s trait, `bins/hub/src/vm.rs`.

## manifest.rs  (hub.txt)
- `pub const PLATFORM: &str = "windows-x86_64";`
- `pub fn parse(bytes: &[u8], app: &CatalogApp) -> Result<HubManifest, String>` strict, per
  docs/launcher-plan.md 3.4: <=16 KiB, <=128 lines, `hub: 1` first, `key: value`, `#` comments, no
  duplicates, unknown keys refused (platform-prefixed keys of the six known platforms are
  accepted and kept), https only, every URL must satisfy `app.url_allowed`, lower-case 64-hex
  sha256, sizes numeric, file names plain, `exe`/`wait` relative with no `..`, `silent` args from
  `[A-Za-z0-9 /=_.:-]` only, `app` must equal `app.id` (including a dash in `claude-switch`).
  NEVER panic on any input.
- `pub fn from_legacy(bytes: &[u8], app: &CatalogApp) -> Result<HubManifest, String>`: stopgap for
  mdedit/mdview's format 1/2 manifests via `mdshared_app::update::UpdateManifest`; yields a
  `Kind::Files` windows-x86_64 entry (exe + engine + extras as `FileSpec`s, size 0), `exe` =
  `app.exe`.
- `impl HubManifest { pub fn entry(&self) -> Option<&PlatformEntry> }` (windows-x86_64).
- `pub fn write(m: &HubManifest) -> String` (a writer for tests and tools) such that
  `parse(write(m)) == m`.

## state.rs  (Hub's own files)
- `pub struct Store { pub dir: PathBuf }`; `Store::open() -> Store`: directory is
  `$HUB_DATA` if set, else `%LOCALAPPDATA%\Hub`; created on demand, never at open.
- `Record { folder, exe, version, kind, date }`; `installed.ini` (INI sections per app id).
  `load_installed(&self, id) -> Option<Record>`, `save_installed(&self, id, &Record)`,
  `forget(&self, id)`. Writes are atomic (write temp, rename). Reading garbage never panics.
- `skipped(&self, id) -> Option<String>` / `set_skipped(id, Option<&str>)`.
- `checks.ini`: `last_check() -> Option<u64>`, `set_last_check(u64)`, `failures() -> u32`,
  `set_failures(u32)`, and per app the cached `latest` manifest text so the window shows rows
  with no network: `cached_manifest(id) -> Option<Vec<u8>>`, `set_cached_manifest(id, &[u8])`.
  AS BUILT: the manifest text is the file `<dir>\latest\<id>.txt`, not a part of checks.ini;
  `skipped` lives in settings.ini, section `[skipped]`.
- `cache_dir(id, version) -> PathBuf` (`<dir>\cache\<id>-<version>`), `rollback_dir(id) -> PathBuf`
  (parts are made safe: no device names, no trailing dots).
- Settings (`settings.ini`, a tiny reader in state.rs):
  `view() -> ViewMode` (`ViewMode::{List, Large, Small}` in hub-core; `vm::View` in bins/hub is
  mapped from it), `interval_hours` (default 6, 0 = never), `backdrop() -> Backdrop` (the shared
  `mdshared_app::backdrop::Backdrop`, key `backdrop`, default `Off`) and `set_backdrop`.
- Every read-modify-write of an ini holds a lock file `<dir>\.hub.lock`, so the tray and the
  window cannot lose each other's change; temp files are named per process and write.
- AS BUILT, detect: the exe's own version resource wins over Hub's record and DisplayVersion.

## detect.rs
- `pub struct Env { pub programs: PathBuf /* %LOCALAPPDATA%\Programs */, pub program_files: Vec<PathBuf>, pub uninstall_roots: Vec<(Hive, String)> }` with `Env::system()` and a way for tests to
  point it at scratch folders and a test-only registry path (`HKCU\Software\HubTest\...`).
- `pub fn detect(app: &CatalogApp, store: &Store, env: &Env) -> Option<Installed>` in the plan's
  3.2 order: Hub's record (verify the exe still exists), then uninstall entries (HKCU+HKLM and
  WOW6432Node: `InstallLocation`, `DisplayVersion`), then known folders
  (`<programs>\<folder>`, each `program_files\<machine_folder>`), then the exe's version
  resource (`GetFileVersionInfoW`). Version unknown => `version: None` (never run the app).
- `pub fn file_version(path: &Path) -> Option<Version>`.
- `pub fn running(exe: &Path) -> bool` (EnumProcesses + QueryFullProcessImageNameW, path
  compare case-insensitively). `pub fn any_running(exes: &[PathBuf]) -> bool`.
- `pub fn adopt(app, store, installed)` records it in installed.ini without touching the app.

## check.rs
- `pub fn check_one(app: &CatalogApp, t: &dyn Transport) -> Latest`: GET
  `app.latest_url("hub.txt")` (64 KiB cap, 10 s); on any failure and `app.legacy_manifest` is
  Some, GET that and `manifest::from_legacy`; no manifest -> `Latest::NoManifest(reason)`.
  Never more than these requests per call. A manifest whose `app` is wrong or whose parse fails
  is `NoManifest("...")` with the parser's reason.
- `pub fn check_all(apps, t, store) -> Vec<(&'static str, Latest)>` at most two at a time,
  caches successes in the store, sets last_check/failures, returns cached for failed ones.
- `pub fn due(last: Option<u64>, now: u64, interval_hours: u32, failures: u32) -> bool` and
  `pub fn next_due(...) -> u64`: interval measured from the last success; after a failure retry
  after 30 min doubling to the interval; interval 0 = never (manual only); a clock that went
  backwards is treated as due.
- `pub fn row_state(installed: Option<&Installed>, latest: &Latest, skipped: Option<&str>) -> Row`:
  NotInstalled / UpToDate / Update / External (installed.source != Hub, until adopted) /
  NoInfo.  Update only if latest > installed version (unknown installed version => Update).
- `pub fn age_label(last: Option<u64>, now: u64) -> String` ("Checked 4 minutes ago").

## install.rs
- `pub enum Progress { Downloading{file:String, done:u64, total:u64}, Verifying(String), Waiting(String), Placing, Finished }`
- `pub struct Opts { pub cancel: Arc<AtomicBool>, pub elevate: Elevation /* real or test */ , .. }`
- `pub fn install(app: &CatalogApp, m: &HubManifest, cur: Option<&Installed>, store: &Store, t: &dyn Transport, opts: &Opts, progress: &mut dyn FnMut(Progress)) -> Result<Installed, InstallError>`.
  Kinds: **Files** (download every file to `cache_dir` as `.part`, check size and SHA-256 BEFORE
  anything is placed, stage beside targets, `mdshared_app::selfupdate::swap_in`, keep the
  retired files in `rollback_dir`), **Setup** (download, verify, run with `silent` args,
  `scope: Machine` through `ShellExecuteExW` `runas` and wait for the process, read the exit
  code; 0 = success, else error with the code; then re-detect and write installed.ini), Zip is
  refused ("not supported"). Before placing: if the app's exes are running, ask their windows to
  close (`WM_CLOSE` to visible top-level windows of those processes), wait up to 30 s, else
  `InstallError::Running`; NEVER `TerminateProcess`. Every failure leaves the old version in
  place and deletes the bad download. A hash or size mismatch is `InstallError::Mismatch{..}` with
  expected and received.
- `pub fn rollback(app, store) -> Result<..>` for Files installs (put the retired files back).
- `pub fn launch(app: &CatalogApp, inst: &Installed) -> std::io::Result<()>`: CreateProcess
  detached, working dir = install folder, `app.launch_args`, nothing waited on.
- Test hooks: the installers in tests are small programs/scripts the test builds; nothing in
  tests touches the owner's installs (`HUB_DATA` in a scratch dir, fake programs root).

## Window (bins/hub)
- `view.rs`/`grid.rs`/`ui.rs`: `pub fn draw(p: &mut Painter, pal: &Palette, w: f32, h: f32, vm: &Vm) -> Layout`,
  adapted from the mock (branch `mock/launcher`, worktree C:\Users\kaank\Desktop\mdedit-hub,
  `bins/mdedit/src/mock/{win,grid,ui,panes}.rs`): title row, command bar, list / large icons /
  small icons, the pane under the list, the offer bar, the status bar. Hit rects go in `Layout`.
- `app.rs`/`main.rs`: winit loop, frameless window like mdedit's, softbuffer present, input to
  `Hit`s, worker threads for checks and installs posting events to the loop, `--shot out.png
  --size WxH --light|--dark`, `--behind`, `--no-session`-style isolation via `HUB_DATA`,
  `--exit-after-ms`, and a `--demo STATE` that fills the Vm with fake apps for the shots.
