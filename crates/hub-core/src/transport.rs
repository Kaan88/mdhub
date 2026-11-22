//! How bytes come from github.com. A trait, so tests and the dev loop never touch the network.
//! `Net` is the real thing (`mdshared_fetch::fetch_file`); `FileRoot` stands in for github.com
//! with a folder, for tests and the dev loop.
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

pub const USER_AGENT: &str = "Hub/0.1";

pub trait Transport: Send + Sync {
    /// GET `url` (https only), at most `max_bytes`, within `timeout`. Err is a short reason.
    fn get(&self, url: &str, max_bytes: usize, timeout: Duration) -> Result<Vec<u8>, String>;
}

/// The network: one blocking GET through the shared fetcher, `https` only.
pub struct Net;

impl Transport for Net {
    fn get(&self, url: &str, max_bytes: usize, timeout: Duration) -> Result<Vec<u8>, String> {
        if !is_https(url) {
            return Err("refused: not an https URL".to_string());
        }
        mdshared_fetch::fetch_file(USER_AGENT, url, max_bytes, timeout)
    }
}

fn is_https(url: &str) -> bool {
    url.get(..8)
        .is_some_and(|p| p.eq_ignore_ascii_case("https://"))
}

const GITHUB: &str = "https://github.com/";

/// A Windows device name (CON, NUL, COM1, ...), with or without an extension, and with trailing
/// dots or spaces, which Windows ignores. Opening one would read the device, not a file. Refused
/// on every platform so that a fixture folder behaves the same everywhere.
fn is_device_name(part: &str) -> bool {
    let stem = part.split('.').next().unwrap_or("").trim_end_matches(' ');
    let up = stem.to_ascii_uppercase();
    matches!(
        up.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) || ["COM", "LPT"].iter().any(|p| {
        up.strip_prefix(p).is_some_and(|n| {
            matches!(
                n,
                "1" | "2"
                    | "3"
                    | "4"
                    | "5"
                    | "6"
                    | "7"
                    | "8"
                    | "9"
                    | "\u{b9}"
                    | "\u{b2}"
                    | "\u{b3}"
            )
        })
    })
}

/// A folder standing in for github.com: `https://github.com/X/Y` is read from `<root>/X/Y`.
/// Anything that could leave the root (`..`, `.`, an absolute or drive-lettered part, a
/// backslash, a NUL) is refused.
pub struct FileRoot {
    pub root: PathBuf,
}

impl FileRoot {
    pub fn new(root: impl Into<PathBuf>) -> FileRoot {
        FileRoot { root: root.into() }
    }

    /// The file `url` maps to, or why it does not.
    pub fn resolve(&self, url: &str) -> Result<PathBuf, String> {
        let rest = url
            .get(..GITHUB.len())
            .filter(|p| p.eq_ignore_ascii_case(GITHUB))
            .map(|_| &url[GITHUB.len()..])
            .ok_or_else(|| "not a github.com https URL".to_string())?;
        let mut path = self.root.clone();
        for part in rest.split('/') {
            let ok = !part.is_empty()
                && part != "."
                && part != ".."
                && !part.contains(['\\', ':', '\0']);
            if !ok || is_device_name(part) {
                return Err("refused path".to_string());
            }
            // Belt and braces: the part must be exactly one normal path component.
            let mut comps = Path::new(part).components();
            match (comps.next(), comps.next()) {
                (Some(Component::Normal(_)), None) => {}
                _ => return Err("refused path".to_string()),
            }
            path.push(part);
        }
        Ok(path)
    }
}

impl Transport for FileRoot {
    fn get(&self, url: &str, max_bytes: usize, _timeout: Duration) -> Result<Vec<u8>, String> {
        use std::io::Read;
        let path = self.resolve(url)?;
        // Only a regular file is "there": never a directory or a device.
        let file = std::fs::File::open(&path)
            .ok()
            .filter(|f| f.metadata().is_ok_and(|m| m.is_file()))
            .ok_or_else(|| "Status(404)".to_string())?;
        let mut body = Vec::new();
        file.take(max_bytes as u64 + 1)
            .read_to_end(&mut body)
            .map_err(|e| format!("read: {e}"))?;
        if body.len() > max_bytes {
            return Err("TooLarge".to_string());
        }
        Ok(body)
    }
}

/// The transport the program uses: `Net`, except that a build with the `test-hooks` feature
/// reads from the folder named by `HUB_FETCH_ROOT` when that is set. A release has no such
/// feature, so the variable means nothing there.
pub fn default_transport() -> Box<dyn Transport> {
    #[cfg(feature = "test-hooks")]
    if let Some(root) = std::env::var_os("HUB_FETCH_ROOT").filter(|r| !r.is_empty()) {
        return Box::new(FileRoot::new(root));
    }
    Box::new(Net)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hub-transport-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }
    const T: Duration = Duration::from_secs(1);

    #[test]
    fn file_root_maps_github_to_the_folder() {
        let root = scratch("map");
        let dir = root.join("Kaan88/x/releases/latest/download");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("hub.txt"), b"hello").unwrap();
        let fr = FileRoot::new(&root);
        let url = "https://github.com/Kaan88/x/releases/latest/download/hub.txt";
        assert_eq!(fr.get(url, 100, T).unwrap(), b"hello");
        let missing = "https://github.com/Kaan88/x/releases/latest/download/none.txt";
        // A missing file is "Status(404)": check.rs reads that as "the server answered, not there".
        assert_eq!(fr.get(missing, 100, T).unwrap_err(), "Status(404)");
        // So is a directory.
        let dir_url = "https://github.com/Kaan88/x/releases";
        assert_eq!(fr.get(dir_url, 100, T).unwrap_err(), "Status(404)");
        // The cap is enforced, exactly.
        assert!(fr.get(url, 4, T).is_err());
        assert_eq!(fr.get(url, 5, T).unwrap(), b"hello");
    }

    #[test]
    fn file_root_refuses_escapes_and_other_hosts() {
        let root = scratch("escape");
        std::fs::write(root.join("secret.txt"), b"s").unwrap();
        std::fs::create_dir_all(root.join("inner")).unwrap();
        let fr = FileRoot::new(root.join("inner"));
        for bad in [
            "https://github.com/../secret.txt",
            "https://github.com/a/../../secret.txt",
            "https://github.com/a/./b",
            "https://github.com//secret.txt",
            "https://github.com/C:/Windows/win.ini",
            "https://github.com/a\\..\\secret.txt",
            "https://github.com/",
            "https://github.com/a/",
            "https://example.com/a",
            "http://github.com/a",
            "/etc/passwd",
            "",
        ] {
            let r = fr.resolve(bad);
            assert!(r.is_err(), "{bad} was allowed: {r:?}");
        }
        assert!(
            fr.get("https://github.com/../secret.txt", 10, T).is_err(),
            "read outside the root"
        );
    }

    #[test]
    fn file_root_refuses_windows_device_names() {
        let root = scratch("devices");
        let fr = FileRoot::new(&root);
        for name in [
            "CON",
            "con",
            "NUL",
            "AUX",
            "PRN",
            "COM1",
            "lpt9",
            "CON.txt",
            "nul.tar.gz",
            "CON.",
            "CON ",
            "COM1.",
            "CONIN$",
            "CONOUT$",
        ] {
            let url = format!("https://github.com/Kaan88/{name}");
            assert!(fr.resolve(&url).is_err(), "{name} was allowed");
            assert!(fr.get(&url, 10, T).is_err(), "{name} was read");
        }
        // Names that merely start like one are fine.
        for name in ["console", "COM10", "COM0", "auxiliary", "hub.txt", "x.CON"] {
            let url = format!("https://github.com/Kaan88/{name}");
            assert!(fr.resolve(&url).is_ok(), "{name} was refused");
        }
    }

    #[test]
    fn net_refuses_non_https_without_touching_the_network() {
        for url in ["http://github.com/a", "ftp://x/y", "file:///c:/x", "", "h"] {
            assert_eq!(
                Net.get(url, 10, T).unwrap_err(),
                "refused: not an https URL"
            );
        }
    }
}
