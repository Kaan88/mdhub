//! The full release notes of a version, fetched on demand. A manifest's `notes.file` names an
//! asset of the same release (`https://github.com/Kaan88/<repo>/releases/download/v<version>/<file>`,
//! Markdown). Nothing here runs at start, at idle or in the periodic check: the window asks when
//! the notes view is opened, once per version (the answer is kept on disk by `Store`).
//! Whatever arrives is hostile until cleaned: any bytes, any length of line, control characters.
use crate::catalog::CatalogApp;
use crate::manifest::valid_name;
use crate::transport::Transport;
use std::time::Duration;

/// The most bytes of notes taken (and kept): a larger file is refused, not cut.
pub const MAX_BYTES: usize = 64 * 1024;
/// How long the one GET may take.
pub const TIMEOUT: Duration = Duration::from_secs(10);

/// The address of `file` in the release of `version`, or why it may not be asked for: a file
/// name that is not plain, a version with odd characters, or an address outside the app's own
/// repository's releases (the catalog decides).
pub fn url(app: &CatalogApp, version: &str, file: &str) -> Result<String, String> {
    if !valid_name(file) {
        return Err("the notes file name is not a plain file name".into());
    }
    if version.is_empty()
        || version.len() > 64
        || !version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+'))
    {
        return Err("the version is not a plain version".into());
    }
    let u = format!(
        "https://github.com/Kaan88/{}/releases/download/v{version}/{file}",
        app.repo
    );
    if app.url_allowed(&u) {
        Ok(u)
    } else {
        Err("the notes address is outside the app's own releases".into())
    }
}

/// The release page of `version`, which Hub may open in the browser (never fetched).
pub fn page_url(app: &CatalogApp, version: &str) -> Option<String> {
    let plain = !version.is_empty()
        && version.len() <= 64
        && version
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'+'));
    plain.then(|| {
        format!(
            "https://github.com/Kaan88/{}/releases/tag/v{version}",
            app.repo
        )
    })
}

/// Bytes to text that is safe to lay out: invalid UTF-8 becomes U+FFFD, carriage returns go,
/// control characters other than newline and tab go, and so do the characters that reorder or
/// hide text (bidi controls, zero-width marks, the byte order mark). At most `MAX_BYTES` of
/// result, cut at a character.
pub fn clean(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().min(MAX_BYTES) + 4);
    for c in String::from_utf8_lossy(bytes).chars() {
        let skip = match c {
            '\n' | '\t' => false,
            '\r' => true,
            c if c.is_control() => true,
            '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2060}'..='\u{2069}' => true,
            '\u{feff}' => true,
            _ => false,
        };
        if skip {
            continue;
        }
        if out.len() + c.len_utf8() > MAX_BYTES {
            break;
        }
        out.push(c);
    }
    out
}

/// A short, plain reason for a failed GET.
pub fn reason(e: &str) -> String {
    let t = match e {
        "Status(404)" => "The release has no notes file by that name (404)".to_string(),
        "TooLarge" => "The notes file is bigger than 64 KiB".to_string(),
        other => format!("Could not download the notes: {other}"),
    };
    t.chars().filter(|c| !c.is_control()).take(200).collect()
}

/// One GET of the notes asset: the raw bytes (for the cache) and the cleaned text. At most one
/// request; an empty file is a failure (there is nothing to show).
pub fn fetch(
    app: &CatalogApp,
    version: &str,
    file: &str,
    t: &dyn Transport,
) -> Result<(Vec<u8>, String), String> {
    let u = url(app, version, file)?;
    let bytes = t.get(&u, MAX_BYTES, TIMEOUT).map_err(|e| reason(&e))?;
    let text = clean(&bytes);
    if text.trim().is_empty() {
        return Err("The notes file is empty".into());
    }
    Ok((bytes, text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::find;
    use std::sync::Mutex;

    struct One(Mutex<Vec<String>>, Result<Vec<u8>, String>);

    impl Transport for One {
        fn get(&self, url: &str, max: usize, t: Duration) -> Result<Vec<u8>, String> {
            self.0.lock().unwrap().push(url.to_string());
            assert_eq!((max, t), (MAX_BYTES, TIMEOUT));
            self.1.clone()
        }
    }

    #[test]
    fn the_url_is_the_apps_own_release_asset() {
        let a = find("mdedit").unwrap();
        assert_eq!(
            url(a, "1.2.1", "release-notes.md").unwrap(),
            "https://github.com/Kaan88/markdown-editor-rs/releases/download/v1.2.1/release-notes.md"
        );
    }

    #[test]
    fn a_name_or_version_that_could_leave_the_release_is_refused() {
        let a = find("mdedit").unwrap();
        for f in [
            "../x.md", "a/b.md", "a\\b.md", "", ".md", "x.md.", "con.md", "a b.md", "a%2fb.md",
            "a?b.md",
        ] {
            assert!(url(a, "1.2.1", f).is_err(), "{f}");
        }
        for v in [
            "",
            "1.2/../..",
            "1.2.1/x",
            "1 2",
            "1.2.1?x",
            "1.2.1#x",
            "1.2@x",
        ] {
            assert!(url(a, v, "n.md").is_err(), "{v}");
        }
        assert!(page_url(a, "1.2/../x").is_none());
        assert_eq!(
            page_url(a, "1.2.1").unwrap(),
            "https://github.com/Kaan88/markdown-editor-rs/releases/tag/v1.2.1"
        );
    }

    #[test]
    fn clean_makes_any_bytes_safe_text() {
        let mut b = b"# T\r\nok\x00\x07 \x1b[31mred\x1b[0m\ttab\n".to_vec();
        b.extend_from_slice(&[0xff, 0xfe, 0xc3]);
        b.extend_from_slice("a\u{202e}b\u{200b}c\u{feff}d".as_bytes());
        let s = clean(&b);
        assert!(!s.contains('\r') && !s.contains('\u{0}') && !s.contains('\u{1b}'));
        assert!(!s.contains('\u{202e}') && !s.contains('\u{200b}') && !s.contains('\u{feff}'));
        assert!(s.contains("# T\nok") && s.contains("\ttab\n") && s.contains('\u{fffd}'));
        assert!(s.contains("abcd"));
        // Never more than the cap, even when every byte grows (invalid byte -> 3 bytes).
        let big = vec![0xffu8; MAX_BYTES * 2];
        assert!(clean(&big).len() <= MAX_BYTES);
    }

    #[test]
    fn fetch_asks_once_and_maps_the_failures() {
        let a = find("mdview").unwrap();
        let t = One(Mutex::new(Vec::new()), Ok(b"# Notes\nhello".to_vec()));
        let (raw, text) = fetch(a, "0.9.1", "n.md", &t).unwrap();
        assert_eq!((raw.len(), text.as_str()), (13, "# Notes\nhello"));
        assert_eq!(t.0.lock().unwrap().len(), 1);
        for (e, want) in [
            ("Status(404)", "404"),
            ("TooLarge", "64 KiB"),
            ("timed out", "timed out"),
        ] {
            let t = One(Mutex::new(Vec::new()), Err(e.to_string()));
            assert!(fetch(a, "0.9.1", "n.md", &t).unwrap_err().contains(want));
        }
        let t = One(Mutex::new(Vec::new()), Ok(b" \r\n\x00".to_vec()));
        assert!(fetch(a, "0.9.1", "n.md", &t).unwrap_err().contains("empty"));
        // A refused name asks nothing at all.
        let t = One(Mutex::new(Vec::new()), Ok(Vec::new()));
        assert!(fetch(a, "0.9.1", "../n.md", &t).is_err());
        assert!(t.0.lock().unwrap().is_empty());
    }
}
