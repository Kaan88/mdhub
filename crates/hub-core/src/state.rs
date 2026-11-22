//! Hub's own files: `installed.ini` (what Hub put where), `checks.ini` (when it last looked and
//! what it found), `settings.ini`, and the download cache. All in one folder, `$HUB_DATA` or
//! `%LOCALAPPDATA%\Hub`, created on demand and never at open. Everything read from disk is
//! parsed without a path that can panic; a damaged file reads as empty.
use mdshared_app::settings::{decode_path, encode_path, escape, unescape};
use mdshared_app::update::Version;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// A file bigger than this is not read (it is not ours).
const MAX_INI: u64 = 1 << 20;
const MAX_MANIFEST: u64 = 1 << 20;

/// What Hub recorded about an install it made or adopted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub folder: PathBuf,
    pub exe: PathBuf,
    pub version: Option<Version>,
    /// "setup" | "files" | "zip" | "" (adopted).
    pub kind: String,
    /// `YYYY-MM-DD`, or empty.
    pub date: String,
}

/// How the window lists the apps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    List,
    Large,
    Small,
}

impl ViewMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ViewMode::List => "list",
            ViewMode::Large => "large",
            ViewMode::Small => "small",
        }
    }
    pub fn parse(s: &str) -> Option<ViewMode> {
        match s.trim() {
            "list" => Some(ViewMode::List),
            "large" => Some(ViewMode::Large),
            "small" => Some(ViewMode::Small),
            _ => None,
        }
    }
}

pub const DEFAULT_INTERVAL_HOURS: u32 = 6;

#[derive(Clone, Debug)]
pub struct Store {
    pub dir: PathBuf,
}

/// `[section]` / `key=value` text, in order. Lines that are neither are skipped.
#[derive(Default, Debug)]
struct Ini {
    sections: Vec<(String, Vec<(String, String)>)>,
}

impl Ini {
    fn parse(text: &str) -> Ini {
        let mut ini = Ini::default();
        let mut cur: Option<usize> = None;
        for line in text.lines() {
            let t = line.trim_start();
            if let Some(rest) = t.strip_prefix('[') {
                if let Some((name, _)) = rest.split_once(']') {
                    let name = name.trim().to_string();
                    cur = Some(match ini.sections.iter().position(|(n, _)| *n == name) {
                        Some(i) => i,
                        None => {
                            ini.sections.push((name, Vec::new()));
                            ini.sections.len() - 1
                        }
                    });
                } else {
                    cur = None;
                }
            } else if let (Some(i), Some((k, v))) = (cur, line.split_once('=')) {
                let k = k.trim();
                if !k.is_empty() {
                    ini.sections[i].1.push((k.to_string(), v.to_string()));
                }
            }
        }
        ini
    }
    fn get(&self, section: &str, key: &str) -> Option<&str> {
        let (_, kv) = self.sections.iter().find(|(n, _)| n == section)?;
        kv.iter()
            .rev()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }
    fn set_section(&mut self, section: &str, pairs: Vec<(String, String)>) {
        match self.sections.iter_mut().find(|(n, _)| n == section) {
            Some(s) => s.1 = pairs,
            None => self.sections.push((section.to_string(), pairs)),
        }
    }
    fn set(&mut self, section: &str, key: &str, value: Option<&str>) {
        let mut kv: Vec<(String, String)> = self
            .sections
            .iter()
            .find(|(n, _)| n == section)
            .map(|(_, kv)| kv.iter().filter(|(k, _)| k != key).cloned().collect())
            .unwrap_or_default();
        if let Some(v) = value {
            kv.push((key.to_string(), v.to_string()));
        }
        self.set_section(section, kv);
    }
    fn remove_section(&mut self, section: &str) {
        self.sections.retain(|(n, _)| n != section);
    }
    fn render(&self) -> String {
        let mut out = String::new();
        for (n, kv) in &self.sections {
            out.push_str(&format!("[{n}]\n"));
            for (k, v) in kv {
                out.push_str(&format!("{k}={v}\n"));
            }
        }
        out
    }
}

/// An app id as a file or section name: letters, digits, `-` and `_`. Anything else is refused.
fn id_ok(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// A piece of a file name: anything outside `[A-Za-z0-9._-]` becomes `_`, trailing dots are
/// dropped (Windows drops them, so `x.` and `x` would be one folder), and a Windows device name
/// (`con`, `nul`, `com1`, ...) gets a `_` in front. No input can name another folder or a device.
fn plain(s: &str) -> String {
    let t: String = s
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    let t = t.trim_end_matches('.');
    if t.is_empty() {
        return "_".into();
    }
    let stem = t.split('.').next().unwrap_or("").to_ascii_lowercase();
    let device = matches!(stem.as_str(), "con" | "prn" | "aux" | "nul")
        || (stem.len() == 4
            && (stem.starts_with("com") || stem.starts_with("lpt"))
            && matches!(stem.as_bytes()[3], b'1'..=b'9'));
    if device {
        format!("_{t}")
    } else {
        t.to_string()
    }
}

impl Store {
    /// `$HUB_DATA` if set, else `%LOCALAPPDATA%\Hub`. Nothing is created here.
    pub fn open() -> Store {
        Store {
            dir: Self::dir_from(
                std::env::var_os("HUB_DATA"),
                std::env::var_os("LOCALAPPDATA"),
            ),
        }
    }

    /// The folder rule on its own, for tests: a non-empty `hub_data` wins, then `local\Hub`,
    /// then a folder under the system temp folder.
    pub fn dir_from(hub_data: Option<OsString>, local_app_data: Option<OsString>) -> PathBuf {
        match (hub_data, local_app_data) {
            (Some(d), _) if !d.is_empty() => PathBuf::from(d),
            (_, Some(l)) if !l.is_empty() => PathBuf::from(l).join("Hub"),
            _ => std::env::temp_dir().join("Hub"),
        }
    }

    /// A store in a chosen folder (tests, tools).
    pub fn at(dir: impl Into<PathBuf>) -> Store {
        Store { dir: dir.into() }
    }

    fn read_ini(&self, name: &str) -> Ini {
        let p = self.dir.join(name);
        match std::fs::metadata(&p) {
            Ok(m) if m.len() <= MAX_INI => {}
            _ => return Ini::default(),
        }
        match std::fs::read(&p) {
            Ok(b) => Ini::parse(&String::from_utf8_lossy(&b)),
            Err(_) => Ini::default(),
        }
    }

    fn write_ini(&self, name: &str, ini: &Ini) {
        // A failed write loses a bookkeeping line, never the app: nothing to report upward.
        let p = self.dir.join(name);
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = write_bytes_atomic(&p, ini.render().as_bytes());
    }

    /// Reads the ini, lets `f` change it (true = write it back), all while holding the store's
    /// lock, so two Hub processes (tray and window) cannot lose each other's change.
    fn modify(&self, name: &str, f: impl FnOnce(&mut Ini) -> bool) {
        let _lock = Lock::take(&self.dir);
        // Under the lock no write of this file is in flight: a temp file left by a crash is
        // dead, and goes.
        if let Ok(rd) = std::fs::read_dir(&self.dir) {
            for e in rd.flatten() {
                let n = e.file_name().to_string_lossy().into_owned();
                if n.starts_with(&format!("{name}.")) && n.ends_with(".tmp") {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
        let mut ini = self.read_ini(name);
        if f(&mut ini) {
            self.write_ini(name, &ini);
        }
    }

    // ---- installed.ini ----

    pub fn load_installed(&self, id: &str) -> Option<Record> {
        if !id_ok(id) {
            return None;
        }
        let ini = self.read_ini("installed.ini");
        let folder = ini.get(id, "folder").map(decode_path)?;
        let exe = ini.get(id, "exe").map(decode_path)?;
        if folder.as_os_str().is_empty() || exe.as_os_str().is_empty() {
            return None;
        }
        Some(Record {
            folder,
            exe,
            version: ini
                .get(id, "version")
                .and_then(|v| Version::parse(unescape(v).trim()).ok()),
            kind: ini.get(id, "kind").map(unescape).unwrap_or_default(),
            date: ini.get(id, "date").map(unescape).unwrap_or_default(),
        })
    }

    pub fn save_installed(&self, id: &str, r: &Record) {
        if !id_ok(id) {
            return;
        }
        let mut kv = vec![
            ("folder".to_string(), encode_path(&r.folder)),
            ("exe".to_string(), encode_path(&r.exe)),
        ];
        if let Some(v) = &r.version {
            kv.push(("version".into(), v.to_string()));
        }
        kv.push(("kind".into(), escape(&r.kind)));
        kv.push(("date".into(), escape(&r.date)));
        self.modify("installed.ini", |ini| {
            ini.set_section(id, kv);
            true
        });
    }

    /// Drops Hub's record of the app (the app itself is untouched).
    pub fn forget(&self, id: &str) {
        if !id_ok(id) {
            return;
        }
        self.modify("installed.ini", |ini| {
            let before = ini.sections.len();
            ini.remove_section(id);
            ini.sections.len() != before
        });
    }

    // ---- skipped versions (settings.ini, section [skipped]) ----

    pub fn skipped(&self, id: &str) -> Option<String> {
        if !id_ok(id) {
            return None;
        }
        let v = self
            .read_ini("settings.ini")
            .get("skipped", id)
            .map(unescape)?;
        let v = v.trim().to_string();
        (!v.is_empty()).then_some(v)
    }

    pub fn set_skipped(&self, id: &str, version: Option<&str>) {
        if !id_ok(id) {
            return;
        }
        let v = version.map(escape);
        self.modify("settings.ini", |ini| {
            ini.set("skipped", id, v.as_deref());
            true
        });
    }

    // ---- checks.ini ----

    pub fn last_check(&self) -> Option<u64> {
        self.read_ini("checks.ini")
            .get("checks", "last_check")
            .and_then(|v| v.trim().parse().ok())
    }

    pub fn set_last_check(&self, t: u64) {
        let v = t.to_string();
        self.modify("checks.ini", |ini| {
            ini.set("checks", "last_check", Some(&v));
            true
        });
    }

    pub fn failures(&self) -> u32 {
        self.read_ini("checks.ini")
            .get("checks", "failures")
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(0)
    }

    pub fn set_failures(&self, n: u32) {
        let v = n.to_string();
        self.modify("checks.ini", |ini| {
            ini.set("checks", "failures", Some(&v));
            true
        });
    }

    /// The last manifest text that arrived for the app, so the window has rows with no network.
    /// Kept as `<dir>\latest\<id>.txt` (several lines of arbitrary bytes), not inside checks.ini.
    pub fn cached_manifest(&self, id: &str) -> Option<Vec<u8>> {
        if !id_ok(id) {
            return None;
        }
        let p = self.dir.join("latest").join(format!("{id}.txt"));
        match std::fs::metadata(&p) {
            Ok(m) if m.len() <= MAX_MANIFEST => std::fs::read(&p).ok(),
            _ => None,
        }
    }

    pub fn set_cached_manifest(&self, id: &str, bytes: &[u8]) {
        if !id_ok(id) || bytes.len() as u64 > MAX_MANIFEST {
            return;
        }
        let p = self.dir.join("latest").join(format!("{id}.txt"));
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let _ = write_bytes_atomic(&p, bytes);
    }

    // ---- folders ----

    /// `<dir>\cache\<id>-<version>`: downloads wait here until verified.
    pub fn cache_dir(&self, id: &str, version: impl std::fmt::Display) -> PathBuf {
        self.dir
            .join("cache")
            .join(format!("{}-{}", plain(id), plain(&version.to_string())))
    }

    /// `<dir>\rollback\<id>`: the files an update retired.
    pub fn rollback_dir(&self, id: &str) -> PathBuf {
        self.dir.join("rollback").join(plain(id))
    }

    // ---- settings.ini ----

    pub fn view(&self) -> ViewMode {
        self.read_ini("settings.ini")
            .get("settings", "view")
            .and_then(ViewMode::parse)
            .unwrap_or(ViewMode::List)
    }

    pub fn set_view(&self, v: ViewMode) {
        self.modify("settings.ini", |ini| {
            ini.set("settings", "view", Some(v.as_str()));
            true
        });
    }

    /// Hours between checks; 0 = never (manual only). Default 6.
    pub fn interval_hours(&self) -> u32 {
        self.read_ini("settings.ini")
            .get("settings", "interval_hours")
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(DEFAULT_INTERVAL_HOURS)
    }

    pub fn set_interval_hours(&self, h: u32) {
        let v = h.to_string();
        self.modify("settings.ini", |ini| {
            ini.set("settings", "interval_hours", Some(&v));
            true
        });
    }
}

/// Writes beside the file under a name no other writer shares, then renames over it.
fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU32, Ordering};
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(
        ".{}-{}.tmp",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let tmp = PathBuf::from(tmp);
    let r = (|| {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)
    })();
    if r.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    r
}

/// A lock between Hub processes on one store folder: a file made with `create_new`, removed on
/// drop. A lock older than 10 seconds is a crashed holder's and is broken; after 3 seconds of
/// waiting the caller goes on without it (a bookkeeping write is never worth a hang).
struct Lock(Option<PathBuf>);

impl Lock {
    fn take(dir: &Path) -> Lock {
        let _ = std::fs::create_dir_all(dir);
        let p = dir.join(".hub.lock");
        let start = std::time::Instant::now();
        loop {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&p)
            {
                Ok(_) => return Lock(Some(p)),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    let stale = std::fs::metadata(&p)
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.elapsed().ok())
                        .is_some_and(|age| age > std::time::Duration::from_secs(10));
                    if stale {
                        let _ = std::fs::remove_file(&p);
                        continue;
                    }
                }
                Err(_) => {}
            }
            if start.elapsed() > std::time::Duration::from_secs(3) {
                return Lock(None);
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        if let Some(p) = &self.0 {
            let _ = std::fs::remove_file(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn scratch(name: &str) -> PathBuf {
        static N: AtomicU32 = AtomicU32::new(0);
        let d = std::env::temp_dir().join(format!(
            "hubtest-state-{}-{}-{name}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn rec() -> Record {
        Record {
            folder: PathBuf::from(r"C:\Apps\mdedit"),
            exe: PathBuf::from(r"C:\Apps\mdedit\mdedit.exe"),
            version: Version::parse("1.2.3").ok(),
            kind: "setup".into(),
            date: "2026-11-22".into(),
        }
    }

    #[test]
    fn open_creates_nothing_and_the_dir_rule_prefers_hub_data() {
        let d = Store::dir_from(Some("X:\\h".into()), Some("Y:\\l".into()));
        assert_eq!(d, PathBuf::from("X:\\h"));
        let d = Store::dir_from(None, Some("Y:\\l".into()));
        assert_eq!(d, PathBuf::from("Y:\\l").join("Hub"));
        let d = Store::dir_from(Some("".into()), Some("Y:\\l".into()));
        assert_eq!(d, PathBuf::from("Y:\\l").join("Hub"));
        let dir = scratch("never");
        let s = Store::at(&dir);
        assert!(s.load_installed("mdedit").is_none());
        assert_eq!(s.last_check(), None);
        assert!(!dir.exists());
    }

    #[test]
    fn a_record_round_trips_and_forget_removes_only_that_app() {
        let dir = scratch("rt");
        let s = Store::at(&dir);
        s.save_installed("mdedit", &rec());
        let mut other = rec();
        other.kind = "files".into();
        other.version = None;
        s.save_installed("mdview", &other);
        assert_eq!(s.load_installed("mdedit"), Some(rec()));
        assert_eq!(s.load_installed("mdview"), Some(other.clone()));
        s.forget("mdedit");
        assert_eq!(s.load_installed("mdedit"), None);
        assert_eq!(s.load_installed("mdview"), Some(other));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_path_with_odd_characters_survives() {
        let dir = scratch("odd");
        let s = Store::at(&dir);
        let mut r = rec();
        r.folder = PathBuf::from("C:\\a b\\100%\\é=x");
        r.exe = r.folder.join("m.exe");
        s.save_installed("mdedit", &r);
        assert_eq!(s.load_installed("mdedit"), Some(r));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn garbage_files_never_panic_and_read_as_empty() {
        let dir = scratch("garbage");
        std::fs::create_dir_all(&dir).unwrap();
        let junk: Vec<Vec<u8>> = vec![
            vec![],
            vec![0xff, 0xfe, 0x00, b'[', 0xc3],
            b"[mdedit\nfolder=\n=\n[]\n[[a]]\nexe".to_vec(),
            b"[mdedit]\nfolder=\nexe=\n".to_vec(),
            b"[mdedit]\nfolder=C:\\x\nexe=C:\\x\\a.exe\nversion=banana\n".to_vec(),
            b"\r\n\r\n[mdedit]\r\nfolder=%ZZ%\r\n".to_vec(),
            vec![b'['; 5000],
        ];
        let s = Store::at(&dir);
        for j in junk {
            for f in ["installed.ini", "checks.ini", "settings.ini"] {
                std::fs::write(dir.join(f), &j).unwrap();
            }
            let _ = s.load_installed("mdedit");
            let _ = s.skipped("mdedit");
            let _ = s.last_check();
            let _ = s.failures();
            let _ = s.view();
            let _ = s.interval_hours();
        }
        // A record with an unreadable version stays, the version does not.
        std::fs::write(
            dir.join("installed.ini"),
            "[mdedit]\nfolder=C:\\x\nexe=C:\\x\\a.exe\nversion=banana\n",
        )
        .unwrap();
        let r = s.load_installed("mdedit").unwrap();
        assert_eq!(r.version, None);
        // A save over garbage works.
        std::fs::write(dir.join("installed.ini"), [0xffu8, 0xfe, 0x00]).unwrap();
        s.save_installed("mdedit", &rec());
        assert_eq!(s.load_installed("mdedit"), Some(rec()));
        // A file too big to be ours is not read.
        std::fs::write(dir.join("checks.ini"), vec![b'a'; (MAX_INI as usize) + 1]).unwrap();
        assert_eq!(s.failures(), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hostile_ids_are_refused_not_turned_into_paths() {
        let dir = scratch("ids");
        let s = Store::at(&dir);
        s.save_installed("..\\evil", &rec());
        s.set_cached_manifest("..\\evil", b"x");
        s.set_skipped("a]b", Some("1.0.0"));
        assert!(!dir.exists());
        assert_eq!(s.load_installed("..\\evil"), None);
        let c = s.cache_dir("..\\x", "1/2");
        assert!(c.starts_with(dir.join("cache")));
        assert_eq!(c.file_name().unwrap(), ".._x-1_2");
        assert_eq!(s.rollback_dir("..").file_name().unwrap(), "_");
    }

    #[test]
    fn writes_are_atomic_no_temp_left_and_a_stale_temp_is_harmless() {
        let dir = scratch("atomic");
        std::fs::create_dir_all(&dir).unwrap();
        // A previous crash left a half-written temp file behind.
        std::fs::write(dir.join("installed.ini.tmp"), "[mdedit]\nfol").unwrap();
        let s = Store::at(&dir);
        s.save_installed("mdedit", &rec());
        s.save_installed("mdedit", &rec());
        let names: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["installed.ini".to_string()]);
        // The cached manifest goes through the same rename.
        s.set_cached_manifest("mdedit", b"hub: 1\n");
        let names: Vec<String> = std::fs::read_dir(dir.join("latest"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, vec!["mdedit.txt".to_string()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn skipped_checks_and_settings_round_trip() {
        let dir = scratch("misc");
        let s = Store::at(&dir);
        assert_eq!(s.skipped("mdedit"), None);
        s.set_skipped("mdedit", Some("2.0.0"));
        s.set_skipped("mdview", Some("3.1.0"));
        assert_eq!(s.skipped("mdedit").as_deref(), Some("2.0.0"));
        s.set_skipped("mdedit", None);
        assert_eq!(s.skipped("mdedit"), None);
        assert_eq!(s.skipped("mdview").as_deref(), Some("3.1.0"));

        assert_eq!(s.failures(), 0);
        s.set_last_check(1_700_000_000);
        s.set_failures(3);
        assert_eq!(s.last_check(), Some(1_700_000_000));
        assert_eq!(s.failures(), 3);
        s.set_last_check(5);
        assert_eq!(s.failures(), 3);

        assert_eq!(s.cached_manifest("mdedit"), None);
        s.set_cached_manifest("mdedit", b"hub: 1\napp: mdedit\n\xff");
        assert_eq!(
            s.cached_manifest("mdedit").as_deref(),
            Some(&b"hub: 1\napp: mdedit\n\xff"[..])
        );

        assert_eq!(s.view(), ViewMode::List);
        assert_eq!(s.interval_hours(), 6);
        s.set_view(ViewMode::Small);
        s.set_interval_hours(0);
        assert_eq!(s.view(), ViewMode::Small);
        assert_eq!(s.interval_hours(), 0);
        // Settings and skipped versions share settings.ini without clobbering each other.
        assert_eq!(s.skipped("mdview").as_deref(), Some("3.1.0"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn device_names_and_trailing_dots_never_name_a_device_or_a_twin_folder() {
        let s = Store::at(PathBuf::from("base"));
        for bad in [
            "con", "NUL", "aux", "prn", "com1", "LPT9", "con.txt", "Com3.x",
        ] {
            let p = s.rollback_dir(bad);
            let leaf = p.file_name().unwrap().to_string_lossy().into_owned();
            assert!(leaf.starts_with('_'), "{bad} -> {leaf}");
        }
        // com0 and com10 are not devices.
        assert_eq!(plain("com10"), "com10");
        assert_eq!(plain("com0"), "com0");
        assert_eq!(plain("x-1."), plain("x-1"));
        assert_eq!(s.cache_dir("mdedit", "1.2."), s.cache_dir("mdedit", "1.2"));
        assert_eq!(plain("..."), "_");
        assert_eq!(plain("a b"), "a_b");
    }

    #[test]
    fn two_writers_on_one_file_lose_nothing() {
        let dir = scratch("race");
        let a = Store::at(&dir);
        let b = Store::at(&dir);
        let ta = std::thread::spawn(move || {
            for i in 0..150u32 {
                a.set_failures(i);
            }
        });
        let tb = std::thread::spawn(move || {
            for i in 0..150u64 {
                b.set_last_check(i);
            }
        });
        ta.join().unwrap();
        tb.join().unwrap();
        let s = Store::at(&dir);
        assert_eq!(s.failures(), 149);
        assert_eq!(s.last_check(), Some(149));
        assert!(!dir.join(".hub.lock").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_stale_lock_is_broken_and_a_failed_write_leaves_no_temp_file() {
        let dir = scratch("stale");
        std::fs::create_dir_all(&dir).unwrap();
        let lock = dir.join(".hub.lock");
        std::fs::write(&lock, b"").unwrap();
        let old = std::time::SystemTime::now() - std::time::Duration::from_secs(60);
        std::fs::File::options()
            .write(true)
            .open(&lock)
            .unwrap()
            .set_modified(old)
            .unwrap();
        let s = Store::at(&dir);
        let t = std::time::Instant::now();
        s.set_failures(7);
        assert!(t.elapsed() < std::time::Duration::from_secs(2));
        assert_eq!(s.failures(), 7);
        // A rename onto a folder fails; the temp file must not stay behind.
        let target = dir.join("blocked");
        std::fs::create_dir_all(&target).unwrap();
        assert!(write_bytes_atomic(&target, b"x").is_err());
        let left: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(left.is_empty(), "{left:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn cache_and_rollback_folders_follow_the_layout() {
        let s = Store::at("D:\\hub");
        assert_eq!(
            s.cache_dir("mdedit", "1.2.3"),
            PathBuf::from("D:\\hub").join("cache").join("mdedit-1.2.3")
        );
        assert_eq!(
            s.rollback_dir("mdedit"),
            PathBuf::from("D:\\hub").join("rollback").join("mdedit")
        );
    }
}
