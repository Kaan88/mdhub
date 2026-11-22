//! "Start with Windows": a value `Hub` under `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`
//! whose data is `"<path>\hub.exe" --tray`. Off by default; written and removed only by the
//! setting. Per user, no elevation, no scheduled task, no service.
//!
//! Every function takes the registry path and the value name, so a test works on
//! `HKCU\Software\HubTest\<unique>` and never on the real Run key. Only the three wrappers at the
//! bottom ([`enable`], [`disable`], [`is_enabled`]) name the real one.
use std::path::Path;

/// The real key, under `HKCU`. Named here and nowhere else.
pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
/// The real value's name.
pub const RUN_VALUE: &str = "Hub";
/// What Windows starts Hub with, so it opens no window.
pub const TRAY_ARG: &str = "--tray";

/// The command line the value holds: `"<exe>" --tray`. An exe path that is not absolute, or
/// carries a quote, a control character or a `%` (the shell may expand `%NAME%` in a Run value),
/// is refused: the line is parsed by Windows, and nothing may leak out of the quotes.
pub fn command_line(exe: &Path) -> Result<String, String> {
    let s = exe
        .to_str()
        .ok_or_else(|| "the path of Hub is not valid text".to_string())?;
    if !exe.is_absolute() {
        return Err("the path of Hub is not absolute".to_string());
    }
    if s.chars().any(|c| c == '"' || c == '%' || c.is_control()) {
        return Err("the path of Hub has a character a startup command cannot hold".to_string());
    }
    if s.len() > 1024 {
        return Err("the path of Hub is too long".to_string());
    }
    Ok(format!("\"{s}\" {TRAY_ARG}"))
}

fn name_ok(s: &str, allow_slash: bool) -> bool {
    !s.is_empty()
        && s.len() <= 255
        && !s.chars().any(|c| c.is_control())
        && (allow_slash || !s.contains('\\'))
        && !s.starts_with('\\')
        && !s.ends_with('\\')
}

/// Writes the value `value` under `HKCU\<key>`, creating the key if need be.
pub fn enable_at(key: &str, value: &str, exe: &Path) -> Result<(), String> {
    let cmd = command_line(exe)?;
    if !name_ok(key, true) || !name_ok(value, false) {
        return Err("bad registry name".to_string());
    }
    imp::set(key, value, &cmd)
}

/// Removes the value. A value that is not there is success.
pub fn disable_at(key: &str, value: &str) -> Result<(), String> {
    if !name_ok(key, true) || !name_ok(value, false) {
        return Err("bad registry name".to_string());
    }
    imp::delete(key, value)
}

/// The command line stored in the value, if there is one.
pub fn command_at(key: &str, value: &str) -> Option<String> {
    if !name_ok(key, true) || !name_ok(value, false) {
        return None;
    }
    imp::get(key, value)
}

/// Whether the value exists (whoever wrote it).
pub fn is_enabled_at(key: &str, value: &str) -> bool {
    command_at(key, value).is_some_and(|c| !c.trim().is_empty())
}

/// Start with Windows, on: the real Run key.
pub fn enable(exe: &Path) -> Result<(), String> {
    enable_at(RUN_KEY, RUN_VALUE, exe)
}

/// Start with Windows, off: the real Run key.
pub fn disable() -> Result<(), String> {
    disable_at(RUN_KEY, RUN_VALUE)
}

/// Whether Windows will start Hub at sign-in: the real Run key.
pub fn is_enabled() -> bool {
    is_enabled_at(RUN_KEY, RUN_VALUE)
}

#[cfg(windows)]
mod imp {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
        RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW,
        RegSetValueExW,
    };

    fn w(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(Some(0))
            .collect()
    }

    pub fn set(key: &str, value: &str, data: &str) -> Result<(), String> {
        let (k, n, v) = (w(key), w(value), w(data));
        let mut h: HKEY = std::ptr::null_mut();
        // SAFETY: valid NUL-terminated strings and out pointer.
        let rc = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                k.as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                std::ptr::null(),
                &mut h,
                std::ptr::null_mut(),
            )
        };
        if rc != ERROR_SUCCESS {
            return Err(format!("the registry refused the key (error {rc})"));
        }
        // SAFETY: `v` is a NUL-terminated wide string of `v.len() * 2` bytes; `h` is open.
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
        // SAFETY: opened above.
        unsafe { RegCloseKey(h) };
        if rc != ERROR_SUCCESS {
            return Err(format!("the registry refused the value (error {rc})"));
        }
        Ok(())
    }

    pub fn delete(key: &str, value: &str) -> Result<(), String> {
        let (k, n) = (w(key), w(value));
        let mut h: HKEY = std::ptr::null_mut();
        // SAFETY: valid strings and out pointer.
        let rc = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, k.as_ptr(), 0, KEY_SET_VALUE, &mut h) };
        // 2 = the key is not there: nothing to remove.
        if rc == 2 {
            return Ok(());
        }
        if rc != ERROR_SUCCESS {
            return Err(format!("the registry refused the key (error {rc})"));
        }
        // SAFETY: `h` is open.
        let rc = unsafe { RegDeleteValueW(h, n.as_ptr()) };
        // SAFETY: opened above.
        unsafe { RegCloseKey(h) };
        // 2 = the value is not there.
        if rc != ERROR_SUCCESS && rc != 2 {
            return Err(format!(
                "the registry refused to remove the value (error {rc})"
            ));
        }
        Ok(())
    }

    pub fn get(key: &str, value: &str) -> Option<String> {
        let (k, n) = (w(key), w(value));
        let mut h: HKEY = std::ptr::null_mut();
        // SAFETY: valid strings and out pointer.
        let rc =
            unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, k.as_ptr(), 0, KEY_QUERY_VALUE, &mut h) };
        if rc != ERROR_SUCCESS {
            return None;
        }
        let mut ty = 0u32;
        let mut size = 0u32;
        // SAFETY: the first call only asks for the type and size.
        let rc = unsafe {
            RegQueryValueExW(
                h,
                n.as_ptr(),
                std::ptr::null(),
                &mut ty,
                std::ptr::null_mut(),
                &mut size,
            )
        };
        let mut out = None;
        if rc == ERROR_SUCCESS && ty == REG_SZ && size <= 8 * 1024 {
            let mut buf = vec![0u16; (size as usize).div_ceil(2) + 1];
            let mut got = (buf.len() * 2) as u32;
            // SAFETY: `buf` holds `got` bytes.
            let rc = unsafe {
                RegQueryValueExW(
                    h,
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
        // SAFETY: opened above.
        unsafe { RegCloseKey(h) };
        out
    }
}

#[cfg(not(windows))]
mod imp {
    pub fn set(_: &str, _: &str, _: &str) -> Result<(), String> {
        Err("start with Windows exists on Windows only".to_string())
    }
    pub fn delete(_: &str, _: &str) -> Result<(), String> {
        Ok(())
    }
    pub fn get(_: &str, _: &str) -> Option<String> {
        None
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU32, Ordering};
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RegDeleteTreeW};

    /// `HKCU\Software\HubTest\autostart-<pid>-<n>`, deleted when dropped.
    struct TestKey(String);
    impl TestKey {
        fn new() -> TestKey {
            static N: AtomicU32 = AtomicU32::new(0);
            TestKey(format!(
                "Software\\HubTest\\autostart-{}-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0)
            ))
        }
    }
    impl Drop for TestKey {
        fn drop(&mut self) {
            let p: Vec<u16> = self.0.encode_utf16().chain(Some(0)).collect();
            // SAFETY: removes only this test's own key.
            unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, p.as_ptr()) };
        }
    }

    fn exe() -> PathBuf {
        PathBuf::from(r"C:\Program Files\Hub\hub.exe")
    }

    #[test]
    fn the_command_line_is_the_quoted_path_and_the_tray_flag() {
        assert_eq!(
            command_line(&exe()).unwrap(),
            r#""C:\Program Files\Hub\hub.exe" --tray"#
        );
    }

    #[test]
    fn paths_that_could_leave_the_quotes_are_refused() {
        for bad in [
            r#"C:\a"b\hub.exe"#,
            "C:\\a\nb\\hub.exe",
            "C:\\a\u{0}b\\hub.exe",
            r"C:\%PATH%\hub.exe",
            r"hub.exe",
            r"..\hub.exe",
            "",
        ] {
            assert!(command_line(Path::new(bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn enable_writes_the_value_and_disable_removes_it() {
        let k = TestKey::new();
        assert!(!is_enabled_at(&k.0, "Hub"));
        enable_at(&k.0, "Hub", &exe()).unwrap();
        assert!(is_enabled_at(&k.0, "Hub"));
        assert_eq!(
            command_at(&k.0, "Hub").as_deref(),
            Some(r#""C:\Program Files\Hub\hub.exe" --tray"#)
        );
        // Enabling again, from another place (Hub moved), replaces the value.
        enable_at(&k.0, "Hub", Path::new(r"D:\x\hub.exe")).unwrap();
        assert_eq!(
            command_at(&k.0, "Hub").as_deref(),
            Some(r#""D:\x\hub.exe" --tray"#)
        );
        disable_at(&k.0, "Hub").unwrap();
        assert!(!is_enabled_at(&k.0, "Hub"));
        // Removing what is not there, or from a key that is not there, is success.
        disable_at(&k.0, "Hub").unwrap();
        disable_at(&format!("{}\\nothing", k.0), "Hub").unwrap();
    }

    #[test]
    fn disable_removes_only_its_own_value() {
        let k = TestKey::new();
        enable_at(&k.0, "Hub", &exe()).unwrap();
        enable_at(&k.0, "Other", Path::new(r"C:\o\other.exe")).unwrap();
        disable_at(&k.0, "Hub").unwrap();
        assert!(!is_enabled_at(&k.0, "Hub"));
        assert!(is_enabled_at(&k.0, "Other"));
    }

    #[test]
    fn a_refused_path_writes_nothing_and_bad_names_are_refused() {
        let k = TestKey::new();
        assert!(enable_at(&k.0, "Hub", Path::new("C:\\a\"b\\hub.exe")).is_err());
        assert!(!is_enabled_at(&k.0, "Hub"));
        assert!(enable_at(&k.0, "", &exe()).is_err());
        assert!(enable_at(&k.0, "a\\b", &exe()).is_err());
        assert!(enable_at("", "Hub", &exe()).is_err());
        assert!(disable_at(&k.0, "").is_err());
        assert!(!is_enabled_at(&k.0, ""));
    }
}
