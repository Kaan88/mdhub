//! Finding out what is installed, and what is running. Nothing here starts an app or writes
//! anything of the app's: detection reads the registry, the disk and the version resource of
//! an exe (never by running it), and the process list is only read.
//!
//! The order is the plan's 3.2: Hub's own record (while its exe is still there), then the
//! uninstall entries, then the folders installers use, each version read from the exe's
//! version resource when nothing better is known.
use crate::catalog::CatalogApp;
use crate::model::{Installed, Source};
use crate::state::{Record, Store};
use mdshared_app::update::Version;
use std::path::{Path, PathBuf};

/// A registry hive an uninstall list can be read from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hive {
    CurrentUser,
    LocalMachine,
}

const UNINSTALL: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall";
const UNINSTALL_WOW: &str = r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall";

/// Where detection looks. `Env::system()` is the real PC; tests build their own.
#[derive(Clone, Debug)]
pub struct Env {
    /// `%LOCALAPPDATA%\Programs`
    pub programs: PathBuf,
    /// Program Files folders (64-bit and 32-bit).
    pub program_files: Vec<PathBuf>,
    /// Registry keys that hold one subkey per installed program.
    pub uninstall_roots: Vec<(Hive, String)>,
}

impl Env {
    pub fn system() -> Env {
        let var = |n: &str| {
            std::env::var_os(n)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        };
        let programs = var("LOCALAPPDATA")
            .map(|l| l.join("Programs"))
            .unwrap_or_default();
        let mut program_files: Vec<PathBuf> = Vec::new();
        for n in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
            if let Some(p) = var(n)
                && !program_files.iter().any(|q| same_path(q, &p))
            {
                program_files.push(p);
            }
        }
        Env {
            programs,
            program_files,
            uninstall_roots: vec![
                (Hive::CurrentUser, UNINSTALL.to_string()),
                (Hive::LocalMachine, UNINSTALL.to_string()),
                (Hive::LocalMachine, UNINSTALL_WOW.to_string()),
            ],
        }
    }

    /// An environment that sees only the given folders and registry keys (tests, tools).
    pub fn custom(
        programs: impl Into<PathBuf>,
        program_files: Vec<PathBuf>,
        uninstall_roots: Vec<(Hive, String)>,
    ) -> Env {
        Env {
            programs: programs.into(),
            program_files,
            uninstall_roots,
        }
    }
}

/// What is installed, in the order of the plan's 3.2. `None` when nothing is found.
pub fn detect(app: &CatalogApp, store: &Store, env: &Env) -> Option<Installed> {
    // 1. Hub's own record, if its exe is still there.
    if let Some(r) = store.load_installed(app.id)
        && r.exe.is_file()
    {
        return Some(Installed {
            folder: r.folder,
            exe: r.exe,
            version: r.version,
            source: Source::Hub,
            kind: r.kind,
        });
    }
    // 2. Uninstall entries.
    for (hive, root) in &env.uninstall_roots {
        for key in app.uninstall_keys {
            let path = format!("{root}\\{key}");
            let Some(loc) = reg_string(*hive, &path, "InstallLocation") else {
                continue;
            };
            let loc = loc.trim().trim_matches('"').trim();
            if loc.is_empty() {
                continue;
            }
            let folder = PathBuf::from(loc.trim_end_matches(['\\', '/']));
            let exe = folder.join(app.exe);
            if !exe.is_file() {
                continue;
            }
            let version = reg_string(*hive, &path, "DisplayVersion")
                .and_then(|v| Version::parse(v.trim().trim_start_matches(['v', 'V'])).ok())
                .or_else(|| file_version(&exe));
            return Some(Installed {
                folder,
                exe,
                version,
                source: Source::Uninstall,
                kind: String::new(),
            });
        }
    }
    // 3. The folders installers use; the version comes from the exe's resource.
    let mut folders: Vec<PathBuf> = Vec::new();
    if !env.programs.as_os_str().is_empty() {
        folders.push(env.programs.join(app.folder));
    }
    if let Some(m) = app.machine_folder {
        for pf in &env.program_files {
            folders.push(pf.join(m));
        }
    }
    for folder in folders {
        let exe = folder.join(app.exe);
        if exe.is_file() {
            let version = file_version(&exe);
            return Some(Installed {
                folder,
                exe,
                version,
                source: Source::Folder,
                kind: String::new(),
            });
        }
    }
    None
}

/// Records what was found in `installed.ini`, so Hub owns the row from now on. The app itself
/// is not touched.
pub fn adopt(app: &CatalogApp, store: &Store, installed: &Installed) {
    store.save_installed(
        app.id,
        &Record {
            folder: installed.folder.clone(),
            exe: installed.exe.clone(),
            version: installed.version.clone(),
            kind: installed.kind.clone(),
            date: today(),
        },
    );
}

/// `YYYY-MM-DD` (UTC) from the clock; empty if the clock is before 1970.
fn today() -> String {
    let secs = match std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
        Ok(d) => d.as_secs(),
        Err(_) => return String::new(),
    };
    civil((secs / 86_400) as i64)
}

fn civil(days: i64) -> String {
    // Howard Hinnant's days-to-civil.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Paths compared the way Windows does: case-insensitive, `/` = `\`, no `\\?\` prefix, no
/// trailing separator.
fn norm(p: &Path) -> String {
    let s = p.to_string_lossy().replace('/', "\\");
    let s = s.strip_prefix(r"\\?\").unwrap_or(&s);
    s.trim_end_matches('\\').to_lowercase()
}

fn same_path(a: &Path, b: &Path) -> bool {
    norm(a) == norm(b)
}

/// The file version of an exe or dll from its version resource: `major.minor.patch` of the
/// fixed file version (the fourth part is dropped). `None` when there is no resource, the file
/// is not there, or the numbers are all zero. The file is read, never run.
pub fn file_version(path: &Path) -> Option<Version> {
    let (ms, ls) = imp::fixed_file_version(path)?;
    let (major, minor, patch) = (ms >> 16, ms & 0xffff, ls >> 16);
    if major == 0 && minor == 0 && patch == 0 {
        return None;
    }
    Version::parse(&format!("{major}.{minor}.{patch}")).ok()
}

/// Is a process running from this exe (compared as paths, case-insensitively)?
pub fn running(exe: &Path) -> bool {
    any_running(&[exe.to_path_buf()])
}

/// Is a process running from any of these exes?
pub fn any_running(exes: &[PathBuf]) -> bool {
    if exes.is_empty() {
        return false;
    }
    let want: Vec<String> = exes.iter().map(|p| norm(p)).collect();
    imp::process_images()
        .into_iter()
        .any(|img| want.iter().any(|w| *w == norm(&img)))
}

fn reg_string(hive: Hive, path: &str, name: &str) -> Option<String> {
    imp::reg_string(hive, path, name)
}

#[cfg(windows)]
mod imp {
    use super::Hive;
    use std::os::windows::ffi::OsStrExt;
    use std::path::{Path, PathBuf};
    use windows_sys::Win32::Foundation::{CloseHandle, ERROR_SUCCESS};
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VS_FIXEDFILEINFO, VerQueryValueW,
    };
    use windows_sys::Win32::System::ProcessStatus::EnumProcesses;
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, REG_EXPAND_SZ, REG_SZ, RegCloseKey,
        RegOpenKeyExW, RegQueryValueExW,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    };

    pub fn wide(s: impl AsRef<std::ffi::OsStr>) -> Vec<u16> {
        s.as_ref().encode_wide().chain(Some(0)).collect()
    }

    pub fn root(h: Hive) -> HKEY {
        match h {
            Hive::CurrentUser => HKEY_CURRENT_USER,
            Hive::LocalMachine => HKEY_LOCAL_MACHINE,
        }
    }

    pub fn reg_string(hive: Hive, path: &str, name: &str) -> Option<String> {
        let mut key: HKEY = std::ptr::null_mut();
        let p = wide(path);
        // SAFETY: valid NUL-terminated wide string and out pointer.
        let rc = unsafe { RegOpenKeyExW(root(hive), p.as_ptr(), 0, KEY_READ, &mut key) };
        if rc != ERROR_SUCCESS {
            return None;
        }
        let n = wide(name);
        let mut ty = 0u32;
        let mut size = 0u32;
        // SAFETY: first call only asks for the type and size.
        let rc = unsafe {
            RegQueryValueExW(
                key,
                n.as_ptr(),
                std::ptr::null(),
                &mut ty,
                std::ptr::null_mut(),
                &mut size,
            )
        };
        let mut out = None;
        // A value is read only up to 32 KiB: nothing Hub asks for is longer.
        if rc == ERROR_SUCCESS && (ty == REG_SZ || ty == REG_EXPAND_SZ) && size <= 32 * 1024 {
            let mut buf = vec![0u16; (size as usize).div_ceil(2) + 1];
            let mut got = (buf.len() * 2) as u32;
            // SAFETY: `buf` holds `got` bytes.
            let rc = unsafe {
                RegQueryValueExW(
                    key,
                    n.as_ptr(),
                    std::ptr::null(),
                    &mut ty,
                    buf.as_mut_ptr().cast(),
                    &mut got,
                )
            };
            if rc == ERROR_SUCCESS {
                let units = ((got as usize) / 2).min(buf.len());
                let s = &buf[..units];
                let end = s.iter().position(|&c| c == 0).unwrap_or(s.len());
                out = Some(String::from_utf16_lossy(&s[..end]));
            }
        }
        // SAFETY: `key` was opened above.
        unsafe { RegCloseKey(key) };
        out
    }

    /// (dwFileVersionMS, dwFileVersionLS)
    pub fn fixed_file_version(path: &Path) -> Option<(u32, u32)> {
        let p = wide(path);
        let mut handle = 0u32;
        // SAFETY: `p` is NUL-terminated.
        let size = unsafe { GetFileVersionInfoSizeW(p.as_ptr(), &mut handle) };
        // A version block is a few KiB; refuse nonsense sizes.
        if size == 0 || size > 4 * 1024 * 1024 {
            return None;
        }
        // u64 elements keep the block 8-aligned.
        let mut buf = vec![0u64; (size as usize).div_ceil(8)];
        // SAFETY: the buffer holds at least `size` bytes.
        let ok = unsafe { GetFileVersionInfoW(p.as_ptr(), 0, size, buf.as_mut_ptr().cast()) };
        if ok == 0 {
            return None;
        }
        let sub = wide("\\");
        let mut ptr: *mut std::ffi::c_void = std::ptr::null_mut();
        let mut len = 0u32;
        // SAFETY: `buf` is a version block just filled in; `ptr` points into it.
        let ok = unsafe { VerQueryValueW(buf.as_ptr().cast(), sub.as_ptr(), &mut ptr, &mut len) };
        if ok == 0 || ptr.is_null() || (len as usize) < std::mem::size_of::<VS_FIXEDFILEINFO>() {
            return None;
        }
        // SAFETY: length checked; read unaligned in case the block is not aligned.
        let info = unsafe { std::ptr::read_unaligned(ptr as *const VS_FIXEDFILEINFO) };
        if info.dwSignature != 0xFEEF_04BD {
            return None;
        }
        Some((info.dwFileVersionMS, info.dwFileVersionLS))
    }

    /// The full image path of every process this user can query.
    pub fn process_images() -> Vec<PathBuf> {
        let mut pids = vec![0u32; 1024];
        let count;
        loop {
            let mut needed = 0u32;
            let bytes = (pids.len() * 4) as u32;
            // SAFETY: `pids` holds `bytes` bytes.
            let ok = unsafe { EnumProcesses(pids.as_mut_ptr(), bytes, &mut needed) };
            if ok == 0 {
                return Vec::new();
            }
            if needed < bytes {
                count = (needed / 4) as usize;
                break;
            }
            if pids.len() >= 1 << 20 {
                count = pids.len();
                break;
            }
            let n = pids.len() * 2;
            pids.resize(n, 0);
        }
        let mut out = Vec::new();
        let mut buf = vec![0u16; 1024];
        for &pid in &pids[..count] {
            if pid == 0 {
                continue;
            }
            // SAFETY: plain query; a null handle means no access (another user, protected).
            let h = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
            if h.is_null() {
                continue;
            }
            let mut len = buf.len() as u32;
            // SAFETY: `buf` holds `len` UTF-16 units.
            let ok = unsafe { QueryFullProcessImageNameW(h, 0, buf.as_mut_ptr(), &mut len) };
            // SAFETY: `h` was opened above.
            unsafe { CloseHandle(h) };
            if ok != 0 {
                let n = (len as usize).min(buf.len());
                out.push(PathBuf::from(String::from_utf16_lossy(&buf[..n])));
            }
        }
        out
    }
}

#[cfg(not(windows))]
mod imp {
    use super::Hive;
    use std::path::{Path, PathBuf};

    pub fn reg_string(_: Hive, _: &str, _: &str) -> Option<String> {
        None
    }
    pub fn fixed_file_version(_: &Path) -> Option<(u32, u32)> {
        None
    }
    pub fn process_images() -> Vec<PathBuf> {
        Vec::new()
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use crate::catalog::{CATALOG, find};
    use std::os::windows::ffi::OsStrExt;
    use std::sync::atomic::{AtomicU32, Ordering};
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_ALL_ACCESS, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
        RegCreateKeyExW, RegDeleteTreeW, RegSetValueExW,
    };

    fn n() -> u32 {
        static N: AtomicU32 = AtomicU32::new(0);
        N.fetch_add(1, Ordering::Relaxed)
    }

    fn w(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(Some(0))
            .collect()
    }

    /// A scratch folder, deleted when dropped.
    struct Scratch(PathBuf);
    impl Scratch {
        fn new(name: &str) -> Scratch {
            let d = std::env::temp_dir().join(format!(
                "hubtest-detect-{}-{}-{name}",
                std::process::id(),
                n()
            ));
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(&d).unwrap();
            Scratch(d)
        }
        fn path(&self) -> &Path {
            &self.0
        }
        /// A file with junk contents (no version resource), parents created.
        fn exe(&self, rel: &str) -> PathBuf {
            let p = self.0.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, b"MZ not a real program").unwrap();
            p
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// `HKCU\Software\HubTest\<unique>`, deleted when dropped.
    struct TestHive {
        base: String,
    }
    impl TestHive {
        fn new() -> TestHive {
            let base = format!(
                "Software\\HubTest\\detect-{}-{}-{}",
                std::process::id(),
                n(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)
            );
            TestHive { base }
        }
        fn root(&self) -> (Hive, String) {
            (Hive::CurrentUser, format!("{}\\Uninstall", self.base))
        }
        fn entry(&self, key: &str, values: &[(&str, &str)]) {
            let path = w(&format!("{}\\Uninstall\\{key}", self.base));
            let mut h: HKEY = std::ptr::null_mut();
            // SAFETY: valid strings and out pointer; the key is under HubTest only.
            let rc = unsafe {
                RegCreateKeyExW(
                    HKEY_CURRENT_USER,
                    path.as_ptr(),
                    0,
                    std::ptr::null(),
                    REG_OPTION_NON_VOLATILE,
                    KEY_ALL_ACCESS,
                    std::ptr::null(),
                    &mut h,
                    std::ptr::null_mut(),
                )
            };
            assert_eq!(rc, ERROR_SUCCESS);
            for (name, val) in values {
                let n = w(name);
                let v = w(val);
                // SAFETY: `v` is a NUL-terminated wide string of `v.len() * 2` bytes.
                let rc = unsafe {
                    RegSetValueExW(
                        h,
                        n.as_ptr(),
                        0,
                        REG_SZ,
                        v.as_ptr().cast(),
                        (v.len() * 2) as u32,
                    )
                };
                assert_eq!(rc, ERROR_SUCCESS);
            }
            // SAFETY: opened above.
            unsafe { RegCloseKey(h) };
        }
    }
    impl Drop for TestHive {
        fn drop(&mut self) {
            let p = w(&self.base);
            // SAFETY: deletes only this test's own key.
            unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, p.as_ptr()) };
        }
    }

    fn mdedit() -> &'static CatalogApp {
        find("mdedit").unwrap()
    }

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    /// Nothing but the scratch folders and the test hive is seen.
    fn env(sc: &Scratch, hive: Option<&TestHive>) -> Env {
        Env::custom(
            sc.path().join("Programs"),
            vec![sc.path().join("PF")],
            hive.map(|h| vec![h.root()]).unwrap_or_default(),
        )
    }

    #[test]
    fn nothing_found_is_none() {
        let sc = Scratch::new("none");
        let store = Store::at(sc.path().join("hub"));
        for a in &CATALOG {
            assert_eq!(detect(a, &store, &env(&sc, None)), None);
        }
    }

    #[test]
    fn hubs_record_wins_over_everything() {
        let sc = Scratch::new("record");
        let hive = TestHive::new();
        let store = Store::at(sc.path().join("hub"));
        let recorded = sc.exe("mine/mdedit.exe");
        let other = sc.exe("other/mdedit.exe");
        let folder_exe = sc.exe("Programs/mdedit/mdedit.exe");
        let _ = folder_exe;
        hive.entry(
            mdedit().uninstall_keys[0],
            &[
                ("InstallLocation", sc.path().join("other").to_str().unwrap()),
                ("DisplayVersion", "9.9.9"),
            ],
        );
        assert!(other.is_file());
        store.save_installed(
            "mdedit",
            &Record {
                folder: sc.path().join("mine"),
                exe: recorded.clone(),
                version: Some(v("1.0.0")),
                kind: "files".into(),
                date: "2026-01-01".into(),
            },
        );
        let got = detect(mdedit(), &store, &env(&sc, Some(&hive))).unwrap();
        assert_eq!(got.source, Source::Hub);
        assert_eq!(got.exe, recorded);
        assert_eq!(got.version, Some(v("1.0.0")));
        assert_eq!(got.kind, "files");
    }

    #[test]
    fn a_record_whose_exe_vanished_falls_through() {
        let sc = Scratch::new("vanished");
        let hive = TestHive::new();
        let store = Store::at(sc.path().join("hub"));
        let other = sc.exe("other/mdedit.exe");
        hive.entry(
            mdedit().uninstall_keys[0],
            &[
                ("InstallLocation", sc.path().join("other").to_str().unwrap()),
                ("DisplayVersion", "2.3.4"),
            ],
        );
        store.save_installed(
            "mdedit",
            &Record {
                folder: sc.path().join("gone"),
                exe: sc.path().join("gone").join("mdedit.exe"),
                version: Some(v("1.0.0")),
                kind: "setup".into(),
                date: String::new(),
            },
        );
        let got = detect(mdedit(), &store, &env(&sc, Some(&hive))).unwrap();
        assert_eq!(got.source, Source::Uninstall);
        assert_eq!(got.exe, other);
        // And with no registry entry either, it is simply not installed.
        assert_eq!(detect(mdedit(), &store, &env(&sc, None)), None);
    }

    #[test]
    fn an_uninstall_entry_gives_folder_and_version() {
        let sc = Scratch::new("uninstall");
        let hive = TestHive::new();
        let store = Store::at(sc.path().join("hub"));
        let exe = sc.exe("inst/mdedit.exe");
        // A trailing slash and quotes, as some installers write.
        let loc = format!("\"{}\\\"", sc.path().join("inst").display());
        hive.entry(
            mdedit().uninstall_keys[0],
            &[("InstallLocation", &loc), ("DisplayVersion", "v1.7.2")],
        );
        let got = detect(mdedit(), &store, &env(&sc, Some(&hive))).unwrap();
        assert_eq!(got.source, Source::Uninstall);
        assert_eq!(got.folder, sc.path().join("inst"));
        assert_eq!(got.exe, exe);
        assert_eq!(got.version, Some(v("1.7.2")));
        assert_eq!(got.kind, "");
    }

    #[test]
    fn an_uninstall_entry_for_a_missing_exe_or_another_app_is_ignored() {
        let sc = Scratch::new("uninstall-bad");
        let hive = TestHive::new();
        let store = Store::at(sc.path().join("hub"));
        // Right key, folder has no exe.
        std::fs::create_dir_all(sc.path().join("empty")).unwrap();
        hive.entry(
            mdedit().uninstall_keys[0],
            &[("InstallLocation", sc.path().join("empty").to_str().unwrap())],
        );
        // mdview's key does not make mdedit installed.
        sc.exe("v/mdview.exe");
        hive.entry(
            find("mdview").unwrap().uninstall_keys[0],
            &[("InstallLocation", sc.path().join("v").to_str().unwrap())],
        );
        assert_eq!(detect(mdedit(), &store, &env(&sc, Some(&hive))), None);
        let mv = detect(find("mdview").unwrap(), &store, &env(&sc, Some(&hive))).unwrap();
        assert_eq!(mv.source, Source::Uninstall);
    }

    #[test]
    fn an_unparsable_display_version_falls_back_to_the_exes_resource() {
        let sc = Scratch::new("badver");
        let hive = TestHive::new();
        let store = Store::at(sc.path().join("hub"));
        sc.exe("inst/mdedit.exe");
        hive.entry(
            mdedit().uninstall_keys[0],
            &[
                ("InstallLocation", sc.path().join("inst").to_str().unwrap()),
                ("DisplayVersion", "banana"),
            ],
        );
        let got = detect(mdedit(), &store, &env(&sc, Some(&hive))).unwrap();
        assert_eq!(got.source, Source::Uninstall);
        // The fake exe has no resource: unknown, and nothing was run to find out.
        assert_eq!(got.version, None);
    }

    #[test]
    fn the_known_per_user_folder_is_the_fallback() {
        let sc = Scratch::new("folder");
        let store = Store::at(sc.path().join("hub"));
        let exe = sc.exe("Programs/mdedit/mdedit.exe");
        let got = detect(mdedit(), &store, &env(&sc, None)).unwrap();
        assert_eq!(got.source, Source::Folder);
        assert_eq!(got.folder, sc.path().join("Programs").join("mdedit"));
        assert_eq!(got.exe, exe);
        assert_eq!(got.version, None);
        // The uninstall entry, when there is one, outranks the folder.
        let hive = TestHive::new();
        sc.exe("inst/mdedit.exe");
        hive.entry(
            mdedit().uninstall_keys[0],
            &[
                ("InstallLocation", sc.path().join("inst").to_str().unwrap()),
                ("DisplayVersion", "4.5.6"),
            ],
        );
        let got = detect(mdedit(), &store, &env(&sc, Some(&hive))).unwrap();
        assert_eq!(got.source, Source::Uninstall);
        assert_eq!(got.version, Some(v("4.5.6")));
    }

    #[test]
    fn a_program_files_machine_folder_is_found_only_for_apps_that_have_one() {
        let sc = Scratch::new("pf");
        let store = Store::at(sc.path().join("hub"));
        let sprawl = find("sprawl").unwrap();
        let exe = sc.exe("PF/Sprawl/sprawl.exe");
        let got = detect(sprawl, &store, &env(&sc, None)).unwrap();
        assert_eq!(got.source, Source::Folder);
        assert_eq!(got.folder, sc.path().join("PF").join("Sprawl"));
        assert_eq!(got.exe, exe);
        // mdedit has no machine folder: a Program Files\mdedit is not looked at.
        sc.exe("PF/mdedit/mdedit.exe");
        assert_eq!(detect(mdedit(), &store, &env(&sc, None)), None);
    }

    #[test]
    fn an_exe_in_a_subfolder_is_found_for_inbound() {
        let sc = Scratch::new("inbound");
        let store = Store::at(sc.path().join("hub"));
        let inbound = find("inbound").unwrap();
        let exe = sc.exe("Programs/Inbound Download Manager/app/DownloadManager.Host.exe");
        let got = detect(inbound, &store, &env(&sc, None)).unwrap();
        assert_eq!(got.exe, exe);
    }

    #[test]
    fn the_version_resource_of_a_real_system_exe_parses() {
        let root = std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into());
        let cmd = PathBuf::from(root).join("System32").join("cmd.exe");
        let ver = file_version(&cmd).expect("cmd.exe carries a version resource");
        assert!(ver.major >= 6, "{ver}");
    }

    #[test]
    fn file_version_of_junk_or_nothing_is_none() {
        let sc = Scratch::new("fv");
        assert_eq!(file_version(&sc.exe("a.exe")), None);
        assert_eq!(file_version(&sc.path().join("missing.exe")), None);
        assert_eq!(file_version(sc.path()), None);
        assert_eq!(file_version(Path::new("")), None);
    }

    #[test]
    fn adopt_records_it_and_it_then_reads_back_as_hubs() {
        let sc = Scratch::new("adopt");
        let store = Store::at(sc.path().join("hub"));
        sc.exe("Programs/mdedit/mdedit.exe");
        let found = detect(mdedit(), &store, &env(&sc, None)).unwrap();
        assert_eq!(found.source, Source::Folder);
        adopt(mdedit(), &store, &found);
        let again = detect(mdedit(), &store, &env(&sc, None)).unwrap();
        assert_eq!(again.source, Source::Hub);
        assert_eq!(again.exe, found.exe);
        let rec = store.load_installed("mdedit").unwrap();
        assert_eq!(rec.date.len(), 10);
        // The app's files are untouched.
        assert_eq!(
            std::fs::read(&found.exe).unwrap(),
            b"MZ not a real program".to_vec()
        );
    }

    #[test]
    fn garbage_installed_ini_never_panics_and_detection_carries_on() {
        let sc = Scratch::new("garbage");
        let hub = sc.path().join("hub");
        std::fs::create_dir_all(&hub).unwrap();
        let store = Store::at(&hub);
        sc.exe("Programs/mdedit/mdedit.exe");
        for junk in [
            &b"\xff\xfe\x00[[[\x00"[..],
            b"[mdedit]\nfolder=\nexe=\n",
            b"[mdedit]\nexe=Z:\\nowhere\\x.exe\nfolder=Z:\\nowhere\n",
            b"=====\n[\n]\n",
        ] {
            std::fs::write(hub.join("installed.ini"), junk).unwrap();
            let got = detect(mdedit(), &store, &env(&sc, None)).unwrap();
            assert_eq!(got.source, Source::Folder);
        }
    }

    #[test]
    fn running_is_true_for_this_programs_own_exe_and_false_for_a_missing_one() {
        let me = std::env::current_exe().unwrap();
        assert!(running(&me));
        assert!(any_running(&[PathBuf::from("Z:\\nope\\x.exe"), me.clone()]));
        assert!(!running(Path::new("Z:\\no\\such\\folder\\nothing.exe")));
        assert!(!any_running(&[]));
        assert!(!any_running(&[PathBuf::from("Z:\\no\\such.exe")]));
    }

    #[test]
    fn running_compares_paths_case_insensitively_and_ignores_slash_style() {
        let me = std::env::current_exe().unwrap();
        let s = me.to_string_lossy().into_owned();
        assert!(running(Path::new(&s.to_uppercase())));
        assert!(running(Path::new(&s.to_lowercase())));
        assert!(running(Path::new(&s.replace('\\', "/"))));
        assert!(running(Path::new(&format!(r"\\?\{s}"))));
        // A prefix of the path is not the path.
        let parent = me.parent().unwrap();
        assert!(!running(parent));
    }

    #[test]
    fn the_date_is_a_calendar_date() {
        assert_eq!(civil(0), "1970-01-01");
        assert_eq!(civil(19_723), "2024-01-01");
        assert_eq!(civil(19_782), "2024-02-29");
        assert_eq!(today().len(), 10);
    }

    #[test]
    fn the_system_env_names_the_three_uninstall_lists() {
        let e = Env::system();
        assert_eq!(e.uninstall_roots.len(), 3);
        assert!(e.uninstall_roots[2].1.contains("WOW6432Node"));
    }
}
