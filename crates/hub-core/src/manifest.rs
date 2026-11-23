//! `hub.txt`: the one manifest every app's release carries for Hub (docs/launcher-plan.md 3.4),
//! and the stopgap reader of mdedit's and mdview's format-1/2 manifests.
//!
//! The grammar is the family's (`mdshared_app::update`): at most [`MAX_BYTES`] bytes and
//! [`MAX_LINES`] lines, `key: value` split at the first `:`, both sides trimmed, blank lines and
//! `#` lines ignored, the first meaningful line `hub: 1`, lower-case keys, no duplicates, no
//! unknown keys. On top of that: https only and only under the app's own releases, lower-case
//! 64-digit SHA-256, plain file names, relative `exe`/`wait` paths, a restricted charset for the
//! silent installer arguments.
//!
//! Keys, `P` being one of the six platforms (`windows-x86_64` ... `macos-x86_64`):
//!
//! ```text
//! hub: 1                       app: id            version: X.Y.Z[-pre]    date: YYYY-MM-DD
//! notes: one line              notes.file: name
//! P.kind: setup|files|zip|appimage|app            P.exe: rel\path         P.uninstall: key
//! P.min: X.Y.Z                 P.wait: a.exe, b.exe                        P.updates: self
//! P.file, P.file2 .. P.file9   (each: name, then P.fileN.url / .sha256 / .size)
//! P.zip                        (name, then .url / .sha256 / .size)
//! P.setup                      (name, then .url / .sha256 / .size / .silent / .scope)
//! ```
//!
//! Anything read here comes from outside and the release build aborts on a panic, so nothing in
//! this file indexes, unwraps or slices on input.
use crate::catalog::CatalogApp;
use crate::model::{FileSpec, HubManifest, Kind, PlatformEntry, Scope, SetupSpec};
use mdshared_app::update::{Platform, UpdateManifest, Version};
use std::collections::HashSet;
use std::fmt::Write as _;

/// The platform this Hub runs on.
pub const PLATFORM: &str = "windows-x86_64";

/// The six platforms a manifest may carry entries for, in the order they are read and written.
pub const PLATFORMS: [&str; 6] = [
    "windows-x86_64",
    "windows-aarch64",
    "linux-x86_64",
    "linux-aarch64",
    "macos-aarch64",
    "macos-x86_64",
];

pub const MAX_BYTES: usize = 16 * 1024;
pub const MAX_LINES: usize = 128;
const MAX_NOTES: usize = 1024;
const MAX_URL: usize = 512;
const MAX_NAME: usize = 128;
const MAX_PATH: usize = 128;
const MAX_SILENT: usize = 200;
const MAX_FILES: usize = 9;
const MAX_WAIT: usize = 8;
/// The largest file a manifest may announce (8 GiB).
const MAX_SIZE: u64 = 8 << 30;

impl HubManifest {
    /// The entry for the platform Hub runs on.
    pub fn entry(&self) -> Option<&PlatformEntry> {
        self.platforms.iter().find(|p| p.platform == PLATFORM)
    }
}

// ---- reading ----

#[derive(Default)]
struct DFile {
    name: Option<String>,
    url: Option<String>,
    sha256: Option<String>,
    size: Option<u64>,
}

impl DFile {
    fn any(&self) -> bool {
        self.name.is_some() || self.url.is_some() || self.sha256.is_some() || self.size.is_some()
    }
    fn finish(self, what: &str) -> Result<FileSpec, String> {
        match (self.name, self.url, self.sha256, self.size) {
            (Some(name), Some(url), Some(sha256), Some(size)) => Ok(FileSpec {
                name,
                url,
                sha256,
                size,
            }),
            _ => Err(format!("{what} needs a name, .url, .sha256 and .size")),
        }
    }
}

#[derive(Default)]
struct DSetup {
    file: DFile,
    silent: Option<String>,
    scope: Option<Scope>,
}

#[derive(Default)]
struct Draft {
    present: bool,
    kind: Option<String>,
    exe: Option<String>,
    files: [DFile; MAX_FILES],
    zip: DFile,
    setup: DSetup,
    uninstall: Option<String>,
    min: Option<Version>,
    wait: Vec<String>,
    updates_self: bool,
}

/// A key or value as it may appear in an error: short, printable.
fn short(s: &str) -> String {
    s.chars()
        .take(40)
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

/// A Windows device name (`NUL`, `con.txt`, `COM1`): Win32 maps these to a device whatever the
/// extension, so a file cannot be written under one.
fn reserved_device(v: &str) -> bool {
    let stem = v.split('.').next().unwrap_or("").to_ascii_uppercase();
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || ["COM", "LPT"].iter().any(|p| {
            stem.strip_prefix(p)
                .is_some_and(|d| d.len() == 1 && d.bytes().all(|b| b.is_ascii_digit()))
        })
}

/// A plain file name: letters, digits, `.`, `_`, `-`; not hidden; no trailing `.` (Win32 strips
/// it, so the file written would not be the one named); not a device name; at most 128 bytes.
pub fn valid_name(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= MAX_NAME
        && !v.starts_with('.')
        && !v.ends_with('.')
        && !reserved_device(v)
        && v.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// A relative path under the install folder: plain segments joined by `\` or `/`; no empty
/// segment, no `.` or `..`, no drive, nothing absolute.
fn valid_rel(v: &str) -> bool {
    !v.is_empty() && v.len() <= MAX_PATH && v.split(['\\', '/']).all(valid_name)
}

fn valid_sha(v: &str) -> bool {
    v.len() == 64 && v.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn valid_date(v: &str) -> bool {
    let b = v.as_bytes();
    let digits = |r: std::ops::Range<usize>| {
        b.get(r)
            .filter(|s| s.iter().all(u8::is_ascii_digit))
            .map(|s| s.iter().fold(0u32, |a, d| a * 10 + u32::from(d - b'0')))
    };
    b.len() == 10
        && b.get(4) == Some(&b'-')
        && b.get(7) == Some(&b'-')
        && digits(0..4).is_some()
        && matches!(digits(5..7), Some(1..=12))
        && matches!(digits(8..10), Some(1..=31))
}

fn valid_uninstall(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= 80
        && v.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'{' | b'}' | b'-' | b'_' | b'.'))
}

fn valid_silent(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= MAX_SILENT
        && v.bytes().all(|b| {
            b.is_ascii_alphanumeric() || matches!(b, b' ' | b'/' | b'=' | b'_' | b'.' | b':' | b'-')
        })
}

fn check_url(app: &CatalogApp, v: &str) -> Result<(), String> {
    if !v.starts_with("https://") {
        return Err("bad value: not an https URL".into());
    }
    if v.len() > MAX_URL
        || !v.bytes().all(|b| b.is_ascii_graphic())
        || v.contains("..")
        || v.contains(['%', '?', '#', '\\', '@'])
    {
        return Err("bad value: malformed URL".into());
    }
    if !app.url_allowed(v) || v.ends_with('/') {
        return Err("bad value: URL is outside the app's own releases".into());
    }
    Ok(())
}

fn check_size(v: &str) -> Result<u64, String> {
    if v.is_empty() || v.len() > 12 || !v.bytes().all(|b| b.is_ascii_digit()) {
        return Err("bad value: size is not a number".into());
    }
    match v.parse::<u64>() {
        Ok(n) if n <= MAX_SIZE => Ok(n),
        _ => Err("bad value: size is too large".into()),
    }
}

/// `name`, `name.url`, `name.sha256`, `name.size` of one file.
fn file_field(f: &mut DFile, sub: Option<&str>, v: &str, app: &CatalogApp) -> Result<(), String> {
    match sub {
        None if valid_name(v) => f.name = Some(v.to_string()),
        None => return Err("bad value: not a plain file name".into()),
        Some("url") => {
            check_url(app, v)?;
            f.url = Some(v.to_string());
        }
        Some("sha256") if valid_sha(v) => f.sha256 = Some(v.to_string()),
        Some("sha256") => return Err("bad value: not 64 lower-case hex digits".into()),
        Some("size") => f.size = Some(check_size(v)?),
        Some(_) => return Err("unknown key".into()),
    }
    Ok(())
}

/// Which of `file`, `file2` .. `file9` a head names.
fn file_slot(head: &str) -> Option<usize> {
    match head.strip_prefix("file")? {
        "" => Some(0),
        d => match d.as_bytes() {
            [c @ b'2'..=b'9'] => Some(usize::from(c - b'1')),
            _ => None,
        },
    }
}

fn platform_field(d: &mut Draft, field: &str, v: &str, app: &CatalogApp) -> Result<(), String> {
    let (head, sub) = match field.split_once('.') {
        Some((h, s)) => (h, Some(s)),
        None => (field, None),
    };
    match (head, sub) {
        ("kind", None) => match v {
            "setup" | "files" | "zip" | "appimage" | "app" => d.kind = Some(v.to_string()),
            _ => return Err("bad value: unknown kind".into()),
        },
        ("exe", None) if valid_rel(v) => d.exe = Some(v.to_string()),
        ("exe", None) => return Err("bad value: not a plain relative path".into()),
        ("uninstall", None) if valid_uninstall(v) => d.uninstall = Some(v.to_string()),
        ("uninstall", None) => return Err("bad value: not an uninstall key".into()),
        ("min", None) => d.min = Some(Version::parse(v).map_err(|e| format!("bad value: {e}"))?),
        ("wait", None) => {
            let mut list = Vec::new();
            for item in v.split(',') {
                let item = item.trim();
                if !valid_rel(item) || list.len() >= MAX_WAIT {
                    return Err("bad value: not a list of plain relative paths".into());
                }
                list.push(item.to_string());
            }
            d.wait = list;
        }
        ("updates", None) if v == "self" => d.updates_self = true,
        ("updates", None) => return Err("bad value: only `self` is known".into()),
        ("zip", s) => file_field(&mut d.zip, s, v, app)?,
        ("setup", Some("silent")) if valid_silent(v) => d.setup.silent = Some(v.to_string()),
        ("setup", Some("silent")) => return Err("bad value: characters not allowed".into()),
        ("setup", Some("scope")) => {
            d.setup.scope = Some(match v {
                "user" => Scope::User,
                "machine" => Scope::Machine,
                _ => return Err("bad value: scope is user or machine".into()),
            })
        }
        ("setup", s) => file_field(&mut d.setup.file, s, v, app)?,
        (h, s) => {
            let Some(f) = file_slot(h).and_then(|slot| d.files.get_mut(slot)) else {
                return Err("unknown key".into());
            };
            file_field(f, s, v, app)?;
        }
    }
    Ok(())
}

/// The kind's own requirements, then the entry. `appimage` and `app` are checked like the rest
/// but have no `Kind`, so they yield no entry (Hub does not install them yet).
fn finish(platform: &str, d: Draft) -> Result<Option<PlatformEntry>, String> {
    let kind = d
        .kind
        .as_deref()
        .ok_or_else(|| format!("{platform}: missing .kind"))?;
    let mut files = Vec::new();
    for (i, f) in d.files.into_iter().enumerate() {
        if f.any() {
            let n = if i == 0 {
                String::new()
            } else {
                (i + 1).to_string()
            };
            files.push(f.finish(&format!("{platform}.file{n}"))?);
        }
    }
    let zip = if d.zip.any() {
        Some(d.zip.finish(&format!("{platform}.zip"))?)
    } else {
        None
    };
    let setup = if d.setup.file.any() || d.setup.silent.is_some() || d.setup.scope.is_some() {
        let file = d.setup.file.finish(&format!("{platform}.setup"))?;
        match (d.setup.silent, d.setup.scope) {
            (Some(silent), Some(scope)) => Some(SetupSpec {
                file,
                silent,
                scope,
            }),
            _ => return Err(format!("{platform}.setup needs .silent and .scope")),
        }
    } else {
        None
    };
    // Windows file names are case-insensitive: `MDVIEW.EXE` is `mdview.exe`.
    let mut names = HashSet::new();
    let all = files
        .iter()
        .chain(zip.iter())
        .chain(setup.iter().map(|s| &s.file));
    for f in all {
        if !names.insert(f.name.to_ascii_lowercase()) {
            return Err(format!(
                "{platform}: file name `{}` is used twice",
                short(&f.name)
            ));
        }
    }
    let kind = match kind {
        "setup" => Kind::Setup,
        "files" => Kind::Files,
        "zip" => Kind::Zip,
        _ => return Ok(None),
    };
    let missing = match kind {
        Kind::Setup if setup.is_none() => Some("a .setup"),
        Kind::Files if files.is_empty() => Some("a .file"),
        Kind::Files if d.exe.is_none() => Some("an .exe"),
        Kind::Zip if zip.is_none() => Some("a .zip"),
        Kind::Zip if d.exe.is_none() => Some("an .exe"),
        _ => None,
    };
    if let Some(m) = missing {
        return Err(format!("{platform}: kind needs {m}"));
    }
    Ok(Some(PlatformEntry {
        platform: platform.to_string(),
        kind,
        exe: d.exe,
        files,
        zip,
        setup,
        uninstall: d.uninstall,
        min: d.min,
        wait: d.wait,
        updates_self: d.updates_self,
    }))
}

/// Read a `hub.txt` for `app`. Strict; see the module comment. Never panics.
pub fn parse(bytes: &[u8], app: &CatalogApp) -> Result<HubManifest, String> {
    if bytes.len() > MAX_BYTES {
        return Err("hub.txt is too large".into());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| "hub.txt is not UTF-8".to_string())?;
    if text.lines().count() > MAX_LINES {
        return Err("hub.txt has too many lines".into());
    }
    let mut seen: HashSet<&str> = HashSet::new();
    let mut header = false;
    let mut have_app = false;
    let (mut version, mut date, mut notes, mut notes_file) = (None, None, None, None);
    let mut drafts: [Draft; 6] = Default::default();
    for (i, raw) in text.lines().enumerate() {
        let n = i + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once(':') else {
            return Err(format!("line {n}: expected `key: value`"));
        };
        let (k, v) = (k.trim(), v.trim());
        if !header {
            if k != "hub" || v != "1" {
                return Err("missing or unsupported `hub: 1` header".into());
            }
            header = true;
            continue;
        }
        if !seen.insert(k) {
            return Err(format!("line {n}: duplicate key `{}`", short(k)));
        }
        let fail = |why: String| format!("line {n}: `{}`: {why}", short(k));
        match k {
            "app" if v == app.id => have_app = true,
            "app" => {
                return Err(fail(format!(
                    "this manifest is for `{}`, not `{}`",
                    short(v),
                    app.id
                )));
            }
            "version" => {
                version = Some(Version::parse(v).map_err(|e| fail(format!("bad value: {e}")))?)
            }
            "date" if valid_date(v) => date = Some(v.to_string()),
            "date" => return Err(fail("bad value: not YYYY-MM-DD".into())),
            "notes" if v.len() <= MAX_NOTES && !v.chars().any(char::is_control) => {
                notes = Some(v.to_string())
            }
            "notes" => return Err(fail("bad value: too long or has control characters".into())),
            "notes.file" if valid_name(v) => notes_file = Some(v.to_string()),
            "notes.file" => return Err(fail("bad value: not a plain file name".into())),
            _ => {
                let found = PLATFORMS.iter().zip(drafts.iter_mut()).find_map(|(p, d)| {
                    let field = k.strip_prefix(p)?.strip_prefix('.')?;
                    Some((d, field))
                });
                let Some((d, field)) = found else {
                    return Err(format!("line {n}: unknown key `{}`", short(k)));
                };
                d.present = true;
                platform_field(d, field, v, app).map_err(fail)?;
            }
        }
    }
    if !header {
        return Err("missing or unsupported `hub: 1` header".into());
    }
    if !have_app {
        return Err("missing `app`".into());
    }
    let version = version.ok_or("missing `version`")?;
    let mut platforms = Vec::new();
    let mut any = false;
    for (p, d) in PLATFORMS.iter().zip(drafts) {
        if d.present {
            any = true;
            if let Some(e) = finish(p, d)? {
                platforms.push(e);
            }
        }
    }
    if !any {
        return Err("lists no platform".into());
    }
    Ok(HubManifest {
        app: app.id.to_string(),
        version,
        date: date.unwrap_or_default(),
        notes: notes.unwrap_or_default(),
        notes_file,
        platforms,
    })
}

// ---- the legacy format ----

/// The file name a legacy URL ends in, if it is a plain one.
fn url_file(url: &str) -> Option<String> {
    let last = url.rsplit('/').next()?;
    valid_name(last).then(|| last.to_string())
}

/// Read mdedit's or mdview's format-1/2 manifest as a `Kind::Files` windows entry: the exe, the
/// engine and any further executables as files of unknown size (0), `exe` = `app.exe`. The
/// primary file is installed as `app.exe` whatever the release asset is called (the release
/// scripts name it with the version and platform). The URLs must lie under the app's own
/// releases, as in a `hub.txt`.
pub fn from_legacy(bytes: &[u8], app: &CatalogApp) -> Result<HubManifest, String> {
    let m = UpdateManifest::parse(bytes).map_err(|e| e.to_string())?;
    let a = m
        .asset(Platform::WindowsX64)
        .ok_or("the manifest has no windows-x86_64 build")?;
    let mut files: Vec<FileSpec> = Vec::new();
    let mut add = |name: Option<String>, url: &str, sha256: &str| -> Result<(), String> {
        check_url(app, url)?;
        let name = name.ok_or("a file name is not plain")?;
        if files.iter().any(|f| f.name == name) {
            return Err(format!("file name `{}` is used twice", short(&name)));
        }
        files.push(FileSpec {
            name,
            url: url.to_string(),
            sha256: sha256.to_string(),
            size: 0,
        });
        Ok(())
    };
    // The asset name must be a plain one, but the file is placed under the exe's name.
    add(
        valid_name(&a.name)
            .then(|| app.exe.to_string())
            .filter(|n| valid_name(n)),
        &a.url,
        &a.sha256,
    )?;
    if let Some(e) = &a.engine {
        add(url_file(&e.url), &e.url, &e.sha256)?;
    }
    for (stem, d) in &a.extras {
        let name = url_file(&d.url).or_else(|| Some(format!("{stem}.exe")));
        add(name, &d.url, &d.sha256)?;
    }
    Ok(HubManifest {
        app: app.id.to_string(),
        version: m.version.clone(),
        date: String::new(),
        notes: m.notes.clone().unwrap_or_default(),
        notes_file: None,
        platforms: vec![PlatformEntry {
            platform: PLATFORM.to_string(),
            kind: Kind::Files,
            exe: Some(app.exe.to_string()),
            files,
            zip: None,
            setup: None,
            uninstall: None,
            min: None,
            wait: app.wait.iter().map(|w| w.to_string()).collect(),
            updates_self: false,
        }],
    })
}

// ---- writing ----

fn put_file(out: &mut String, key: &str, f: &FileSpec) {
    let _ = writeln!(out, "{key}: {}", f.name);
    let _ = writeln!(out, "{key}.url: {}", f.url);
    let _ = writeln!(out, "{key}.sha256: {}", f.sha256);
    let _ = writeln!(out, "{key}.size: {}", f.size);
}

/// A manifest as text that `parse` reads back to the same value. For tests and tools.
pub fn write(m: &HubManifest) -> String {
    let mut o = String::from("hub: 1\n");
    let _ = writeln!(o, "app: {}", m.app);
    let _ = writeln!(o, "version: {}", m.version);
    if !m.date.is_empty() {
        let _ = writeln!(o, "date: {}", m.date);
    }
    let notes: String = m
        .notes
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    if !notes.trim().is_empty() {
        let _ = writeln!(o, "notes: {}", notes.trim());
    }
    if let Some(f) = &m.notes_file {
        let _ = writeln!(o, "notes.file: {f}");
    }
    for p in PLATFORMS {
        for e in m.platforms.iter().filter(|e| e.platform == p) {
            let kind = match e.kind {
                Kind::Setup => "setup",
                Kind::Files => "files",
                Kind::Zip => "zip",
            };
            let _ = writeln!(o, "{p}.kind: {kind}");
            if let Some(x) = &e.exe {
                let _ = writeln!(o, "{p}.exe: {x}");
            }
            if let Some(x) = &e.uninstall {
                let _ = writeln!(o, "{p}.uninstall: {x}");
            }
            if let Some(x) = &e.min {
                let _ = writeln!(o, "{p}.min: {x}");
            }
            if !e.wait.is_empty() {
                let _ = writeln!(o, "{p}.wait: {}", e.wait.join(", "));
            }
            if e.updates_self {
                let _ = writeln!(o, "{p}.updates: self");
            }
            for (i, f) in e.files.iter().enumerate() {
                let n = if i == 0 {
                    String::new()
                } else {
                    (i + 1).to_string()
                };
                put_file(&mut o, &format!("{p}.file{n}"), f);
            }
            if let Some(z) = &e.zip {
                put_file(&mut o, &format!("{p}.zip"), z);
            }
            if let Some(s) = &e.setup {
                let key = format!("{p}.setup");
                put_file(&mut o, &key, &s.file);
                let _ = writeln!(o, "{key}.silent: {}", s.silent);
                let scope = match s.scope {
                    Scope::User => "user",
                    Scope::Machine => "machine",
                };
                let _ = writeln!(o, "{key}.scope: {scope}");
            }
        }
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::find;
    use mdshared_app::update::sha256_hex;

    fn app(id: &str) -> &'static CatalogApp {
        find(id).expect("catalog app")
    }
    fn h(tag: &str) -> String {
        sha256_hex(tag.as_bytes())
    }
    fn url(a: &CatalogApp, tag: &str, file: &str) -> String {
        format!(
            "https://github.com/Kaan88/{}/releases/download/{tag}/{file}",
            a.repo
        )
    }
    /// The four lines of one file: name, url, sha256, size.
    fn fl(a: &CatalogApp, key: &str, tag: &str, name: &str, size: u64) -> String {
        format!(
            "{key}: {name}\n{key}.url: {}\n{key}.sha256: {}\n{key}.size: {size}\n",
            url(a, tag, name),
            h(&format!("{tag}/{name}"))
        )
    }
    const W: &str = "windows-x86_64";
    const SILENT: &str = "/VERYSILENT /SUPPRESSMSGBOXES /NORESTART";

    fn mdedit_txt() -> String {
        let a = app("mdedit");
        let mut t = String::from(
            "hub: 1\n# written by the release workflow\napp: mdedit\nversion: 0.30.0\n\
             date: 2026-11-20\nnotes: Faster open; fixes a crash on empty files.\n\
             notes.file: release-notes.md\n",
        );
        t += &format!("{W}.kind: files\n{W}.exe: mdedit.exe\n");
        t += &fl(a, &format!("{W}.file"), "v0.30.0", "mdedit.exe", 3_100_000);
        t += &fl(
            a,
            &format!("{W}.file2"),
            "v0.30.0",
            "mdedit-ai.exe",
            1_900_000,
        );
        t += &fl(
            a,
            &format!("{W}.file3"),
            "v0.30.0",
            "mdview-engine.exe",
            2_400_000,
        );
        t += &fl(
            a,
            &format!("{W}.setup"),
            "v0.30.0",
            "mdedit-setup-0.30.0.exe",
            4_200_000,
        );
        t += &format!("{W}.setup.silent: {SILENT}\n{W}.setup.scope: user\n");
        t += &format!("{W}.uninstall: {{C58A9AB1-8337-4056-9953-04E92C05F414}}_is1\n");
        t += &format!("{W}.min: 0.20.0\n{W}.wait: mdedit-ai.exe, mdview-engine.exe\n");
        t += "linux-x86_64.kind: appimage\nlinux-x86_64.exe: mdedit.AppImage\n";
        t += &fl(
            a,
            "linux-x86_64.file",
            "v0.30.0",
            "mdedit-x86_64.AppImage",
            5_000_000,
        );
        t += "macos-aarch64.kind: app\n";
        t += &fl(
            a,
            "macos-aarch64.zip",
            "v0.30.0",
            "mdedit-macos.zip",
            4_000_000,
        );
        t
    }
    fn mdview_txt() -> String {
        let a = app("mdview");
        let mut t = String::from(
            "hub: 1\napp: mdview\nversion: 0.9.1\ndate: 2026-11-18\nnotes: Search is faster.\n",
        );
        t += &format!("{W}.kind: files\n{W}.exe: mdview.exe\n");
        t += &fl(a, &format!("{W}.file"), "v0.9.1", "mdview.exe", 2_800_000);
        t += &fl(
            a,
            &format!("{W}.file2"),
            "v0.9.1",
            "mdview-engine.exe",
            2_300_000,
        );
        t += &fl(
            a,
            &format!("{W}.setup"),
            "v0.9.1",
            "mdview-setup-0.9.1.exe",
            3_900_000,
        );
        t += &format!("{W}.setup.silent: {SILENT}\n{W}.setup.scope: user\n");
        t += &format!("{W}.uninstall: {{B7E2F4A9-3D1C-4C58-A6E0-91D5C8F27B34}}_is1\n");
        t += &format!("{W}.wait: mdview-engine.exe\n");
        t
    }
    fn sprawl_txt() -> String {
        let a = app("sprawl");
        let mut t = String::from(
            "hub: 1\napp: sprawl\nversion: 1.0.5\ndate: 2026-11-20\n\
             notes: Faster scan of network drives.\n",
        );
        t += &format!("{W}.kind: setup\n{W}.exe: sprawl.exe\n");
        t += &fl(
            a,
            &format!("{W}.setup"),
            "v1.0.5",
            "Sprawl-Setup-1.0.5.exe",
            5_544_567,
        );
        t += &format!("{W}.setup.silent: {SILENT}\n{W}.setup.scope: machine\n");
        t += &format!("{W}.uninstall: Sprawl_is1\n");
        t
    }
    fn inbound_txt() -> String {
        let a = app("inbound");
        let mut t = String::from(
            "hub: 1\napp: inbound\nversion: 2.3.0\ndate: 2026-11-19\nnotes: Torrent fixes.\n",
        );
        t += &format!("{W}.kind: setup\n{W}.exe: app\\DownloadManager.Host.exe\n");
        t += &fl(
            a,
            &format!("{W}.setup"),
            "v2.3.0",
            "Inbound-Setup-2.3.0.exe",
            68_700_000,
        );
        t += &format!(
            "{W}.setup.silent: {SILENT} /CURRENTUSER\n{W}.setup.scope: user\n\
             {W}.uninstall: {{6F4B7C2E-3D91-4E8A-9B57-2C1D0A8E5F13}}_is1\n\
             {W}.wait: app\\DownloadManager.Host.exe, app\\DownloadManager.Desktop.WinUI.exe, \
             app\\DownloadManager.NativeHost.exe\n"
        );
        t
    }
    fn purge_txt() -> String {
        let a = app("purge");
        let mut t = String::from(
            "hub: 1\napp: purge\nversion: 0.1.0\ndate: 2026-11-23\n\
             notes: Uninstalls programs and the leftovers they leave behind.\n",
        );
        t += &format!("{W}.kind: setup\n{W}.exe: Purge.exe\n");
        t += &fl(
            a,
            &format!("{W}.setup"),
            "v0.1.0",
            "Purge-Setup-0.1.0.exe",
            43_710_970,
        );
        t += &format!(
            "{W}.setup.silent: {SILENT}\n{W}.setup.scope: machine\n\
             {W}.uninstall: {{7C1B9E64-2D58-4F1A-9B03-6E4C5A8D7F21}}_is1\n\
             {W}.wait: Purge.exe, purge-engine.exe, purge-broker.exe\n"
        );
        t
    }
    fn fixtures() -> Vec<(&'static str, String)> {
        vec![
            ("mdedit", mdedit_txt()),
            ("mdview", mdview_txt()),
            ("sprawl", sprawl_txt()),
            ("inbound", inbound_txt()),
            ("purge", purge_txt()),
        ]
    }

    // Text editing helpers: each asserts the line was there, so a row cannot pass by accident.
    fn set(text: &str, key: &str, value: &str) -> String {
        let prefix = format!("{key}:");
        let mut found = false;
        let out: Vec<String> = text
            .lines()
            .map(|l| {
                if l.starts_with(&prefix) {
                    found = true;
                    format!("{key}: {value}")
                } else {
                    l.to_string()
                }
            })
            .collect();
        assert!(found, "no line `{key}` in the fixture");
        out.join("\n") + "\n"
    }
    fn del(text: &str, key: &str) -> String {
        let prefix = format!("{key}:");
        assert!(text.lines().any(|l| l.starts_with(&prefix)), "no `{key}`");
        text.lines()
            .filter(|l| !l.starts_with(&prefix))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n"
    }
    fn add(text: &str, line: &str) -> String {
        format!("{text}{line}\n")
    }

    fn ok(id: &str, text: &str) -> HubManifest {
        parse(text.as_bytes(), app(id)).unwrap_or_else(|e| panic!("{id}: {e}\n{text}"))
    }
    fn err(id: &str, text: &[u8]) -> String {
        parse(text, app(id)).expect_err("should be refused")
    }

    // ---------------------------------------------------------------- the four apps

    #[test]
    fn mdedit_files_with_both_engines() {
        let m = ok("mdedit", &mdedit_txt());
        assert_eq!(m.app, "mdedit");
        assert_eq!(m.version, Version::parse("0.30.0").unwrap());
        assert_eq!(m.date, "2026-11-20");
        assert_eq!(m.notes, "Faster open; fixes a crash on empty files.");
        assert_eq!(m.notes_file.as_deref(), Some("release-notes.md"));
        // The AppImage and the macOS app are checked but have no Kind: only windows is kept.
        assert_eq!(m.platforms.len(), 1);
        let e = m.entry().unwrap();
        assert_eq!(e.kind, Kind::Files);
        assert_eq!(e.exe.as_deref(), Some("mdedit.exe"));
        let names: Vec<_> = e.files.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["mdedit.exe", "mdedit-ai.exe", "mdview-engine.exe"]);
        assert_eq!(e.files[0].size, 3_100_000);
        assert_eq!(e.files[1].sha256, h("v0.30.0/mdedit-ai.exe"));
        assert_eq!(
            e.files[2].url,
            url(app("mdedit"), "v0.30.0", "mdview-engine.exe")
        );
        let s = e.setup.as_ref().unwrap();
        assert_eq!(s.file.name, "mdedit-setup-0.30.0.exe");
        assert_eq!(s.silent, SILENT);
        assert_eq!(s.scope, Scope::User);
        assert_eq!(e.min, Some(Version::parse("0.20.0").unwrap()));
        assert_eq!(e.wait, ["mdedit-ai.exe", "mdview-engine.exe"]);
        assert!(e.uninstall.as_deref().unwrap().ends_with("}_is1"));
        assert!(!e.updates_self);
    }

    #[test]
    fn mdview_files_and_engine() {
        let m = ok("mdview", &mdview_txt());
        let e = m.entry().unwrap();
        assert_eq!(e.kind, Kind::Files);
        assert_eq!(e.files.len(), 2);
        assert_eq!(e.files[1].name, "mdview-engine.exe");
        assert_eq!(e.wait, ["mdview-engine.exe"]);
        assert_eq!(e.setup.as_ref().unwrap().scope, Scope::User);
    }

    #[test]
    fn sprawl_setup_for_all_users() {
        let m = ok("sprawl", &sprawl_txt());
        let e = m.entry().unwrap();
        assert_eq!(e.kind, Kind::Setup);
        assert!(e.files.is_empty());
        let s = e.setup.as_ref().unwrap();
        assert_eq!(s.scope, Scope::Machine);
        assert_eq!(s.silent, "/VERYSILENT /SUPPRESSMSGBOXES /NORESTART");
        assert_eq!(s.file.size, 5_544_567);
        assert_eq!(e.uninstall.as_deref(), Some("Sprawl_is1"));
    }

    #[test]
    fn purge_setup_for_all_users_waits_for_its_helpers() {
        let m = ok("purge", &purge_txt());
        let e = m.entry().unwrap();
        assert_eq!(e.kind, Kind::Setup);
        assert_eq!(e.exe.as_deref(), Some("Purge.exe"));
        assert_eq!(
            e.wait,
            ["Purge.exe", "purge-engine.exe", "purge-broker.exe"]
        );
        let s = e.setup.as_ref().unwrap();
        assert_eq!(s.scope, Scope::Machine);
        assert_eq!(s.silent, "/VERYSILENT /SUPPRESSMSGBOXES /NORESTART");
        assert_eq!(
            e.uninstall.as_deref(),
            Some("{7C1B9E64-2D58-4F1A-9B03-6E4C5A8D7F21}_is1")
        );
    }

    #[test]
    fn inbound_setup_waits_for_three_exes() {
        let m = ok("inbound", &inbound_txt());
        let e = m.entry().unwrap();
        assert_eq!(e.kind, Kind::Setup);
        assert_eq!(e.exe.as_deref(), Some("app\\DownloadManager.Host.exe"));
        assert_eq!(
            e.wait,
            [
                "app\\DownloadManager.Host.exe",
                "app\\DownloadManager.Desktop.WinUI.exe",
                "app\\DownloadManager.NativeHost.exe"
            ]
        );
        assert!(e.setup.as_ref().unwrap().silent.ends_with("/CURRENTUSER"));
    }

    // ---------------------------------------------------------------- every key

    #[test]
    fn every_key_is_read() {
        let a = app("mdedit");
        let mut t = String::from("hub: 1\napp: mdedit\nversion: 1.2.3-rc.1\ndate: 2026-02-28\n");
        t += "notes: a: b\nnotes.file: notes-1.2.3.md\n";
        t += &format!("{W}.kind: zip\n{W}.exe: bin\\mdedit.exe\n{W}.updates: self\n");
        for i in 1..=9u64 {
            let key = if i == 1 {
                format!("{W}.file")
            } else {
                format!("{W}.file{i}")
            };
            t += &fl(a, &key, "v1", &format!("f{i}.exe"), 10 * i);
        }
        t += &fl(a, &format!("{W}.zip"), "v1", "mdedit.zip", 7);
        t += &fl(a, &format!("{W}.setup"), "v1", "setup.exe", 8);
        t += &format!("{W}.setup.silent: /S /D=C:.x_y-z\n{W}.setup.scope: machine\n");
        t += &format!("{W}.uninstall: Some-Key_1.0\n{W}.min: 1.0.0\n{W}.wait: a.exe,b/c.exe\n");
        t += "windows-aarch64.kind: files\nwindows-aarch64.exe: mdedit.exe\n";
        t += &fl(a, "windows-aarch64.file", "v1", "mdedit.exe", 5);
        t += "linux-aarch64.kind: appimage\nmacos-x86_64.kind: app\n";
        let m = ok("mdedit", &t);
        assert_eq!(m.version.to_string(), "1.2.3-rc.1");
        assert_eq!(m.date, "2026-02-28");
        assert_eq!(m.notes, "a: b");
        assert_eq!(m.notes_file.as_deref(), Some("notes-1.2.3.md"));
        assert_eq!(
            m.platforms.len(),
            2,
            "windows-x86_64 and windows-aarch64 kept"
        );
        let e = m.entry().unwrap();
        assert_eq!(e.kind, Kind::Zip);
        assert_eq!(e.exe.as_deref(), Some("bin\\mdedit.exe"));
        assert!(e.updates_self);
        assert_eq!(e.files.len(), 9);
        assert_eq!(e.files[8].name, "f9.exe");
        assert_eq!(e.files[8].size, 90);
        assert_eq!(e.zip.as_ref().unwrap().name, "mdedit.zip");
        assert_eq!(e.zip.as_ref().unwrap().size, 7);
        let s = e.setup.as_ref().unwrap();
        assert_eq!((s.file.size, s.scope), (8, Scope::Machine));
        assert_eq!(s.silent, "/S /D=C:.x_y-z");
        assert_eq!(e.uninstall.as_deref(), Some("Some-Key_1.0"));
        assert_eq!(e.min, Some(Version::parse("1.0.0").unwrap()));
        assert_eq!(e.wait, ["a.exe", "b/c.exe"]);
        assert_eq!(m.platforms[1].platform, "windows-aarch64");
    }

    #[test]
    fn notes_and_date_are_optional() {
        let m = ok("sprawl", &del(&del(&sprawl_txt(), "notes"), "date"));
        assert_eq!((m.notes.as_str(), m.date.as_str()), ("", ""));
        assert_eq!(m.notes_file, None);
    }

    #[test]
    fn comments_blank_lines_crlf_and_spacing_are_tolerated() {
        let plain = ok("sprawl", &sprawl_txt());
        let spaced: String = sprawl_txt()
            .lines()
            .map(|l| format!("  {}  \r\n\r\n# c\r\n", l.replacen(": ", "  :   ", 1)))
            .collect();
        assert_eq!(ok("sprawl", &spaced), plain);
    }

    // ---------------------------------------------------------------- refusals

    #[test]
    fn refusals() {
        let m = mdedit_txt();
        let f = format!("{W}.file");
        let base = "https://github.com/Kaan88/markdown-editor-rs/releases/download";
        let u = |rest: &str| set(&m, &format!("{f}.url"), &format!("{base}{rest}"));
        let rows: Vec<(&str, String, &str)> = vec![
            ("unknown key", add(&m, "colour: red"), "unknown key"),
            ("upper-case key", add(&m, "Version: 1.0.0"), "unknown key"),
            (
                "unknown platform",
                add(&m, "plan9-x86_64.kind: files"),
                "unknown key",
            ),
            (
                "unknown field",
                add(&m, &format!("{W}.bogus: 1")),
                "unknown key",
            ),
            (
                "unknown file field",
                add(&m, &format!("{f}.mode: 1")),
                "unknown key",
            ),
            (
                "file1",
                add(&m, &format!("{W}.file1: a.exe")),
                "unknown key",
            ),
            (
                "file10",
                add(&m, &format!("{W}.file10: a.exe")),
                "unknown key",
            ),
            (
                "fileX",
                add(&m, &format!("{W}.filex: a.exe")),
                "unknown key",
            ),
            (
                "kind with a sub key",
                add(&m, &format!("{W}.kind.x: a")),
                "unknown key",
            ),
            (
                "http url",
                set(
                    &m,
                    &format!("{f}.url"),
                    &format!("http://{}", &base["https://".len()..]),
                ),
                "not an https URL",
            ),
            (
                "ftp url",
                set(
                    &m,
                    &format!("{f}.url"),
                    &format!("ftp://{}", &base["https://".len()..]),
                ),
                "not an https URL",
            ),
            (
                "other host",
                set(
                    &m,
                    &format!("{f}.url"),
                    &format!(
                        "https://evil.example/{}",
                        &base["https://github.com/".len()..]
                    ),
                ),
                "outside the app's own releases",
            ),
            (
                "other repository",
                set(&m, &format!("{f}.url"), &url(app("mdview"), "v1", "a.exe")),
                "outside the app's own releases",
            ),
            (
                "same repo, not releases",
                set(
                    &m,
                    &format!("{f}.url"),
                    "https://github.com/Kaan88/markdown-editor-rs/raw/main/a.exe",
                ),
                "outside the app's own releases",
            ),
            (
                "repo name prefix",
                set(
                    &m,
                    &format!("{f}.url"),
                    "https://github.com/Kaan88/markdown-editor-rs2/releases/download/v1/a.exe",
                ),
                "outside the app's own releases",
            ),
            ("dot dot in url", u("/../../../x/a.exe"), "malformed URL"),
            ("userinfo in url", u("/v1/a@b.exe"), "malformed URL"),
            ("query in url", u("/v1/a.exe?x=1"), "malformed URL"),
            ("space in url", u("/v1/a b.exe"), "malformed URL"),
            ("percent in url", u("/v1/a%2e.exe"), "malformed URL"),
            ("fragment in url", u("/v1/a.exe#x"), "malformed URL"),
            ("backslash in url", u("/v1/a\\b.exe"), "malformed URL"),
            (
                "url ends in a slash",
                u("/v1/"),
                "outside the app's own releases",
            ),
            (
                "device name NUL as file",
                set(&m, &f, "NUL"),
                "not a plain file name",
            ),
            (
                "device name with extension",
                set(&m, &f, "aux.exe"),
                "not a plain file name",
            ),
            (
                "device name in lower case",
                set(&m, &f, "con"),
                "not a plain file name",
            ),
            (
                "device name COM1",
                set(&m, &f, "Com1.txt"),
                "not a plain file name",
            ),
            (
                "device name LPT9",
                set(&m, &f, "lpt9"),
                "not a plain file name",
            ),
            (
                "trailing dot in file",
                set(&m, &f, "mdedit.exe."),
                "not a plain file name",
            ),
            (
                "device name as exe",
                set(&m, &format!("{W}.exe"), "NUL"),
                "plain relative path",
            ),
            (
                "device name inside exe path",
                set(&m, &format!("{W}.exe"), "bin\\con.exe"),
                "plain relative path",
            ),
            (
                "trailing dot in exe",
                set(&m, &format!("{W}.exe"), "mdedit.exe."),
                "plain relative path",
            ),
            (
                "device name in wait",
                set(&m, &format!("{W}.wait"), "mdedit-ai.exe, nul"),
                "plain relative paths",
            ),
            (
                "trailing dot in wait",
                set(&m, &format!("{W}.wait"), "mdedit-ai.exe."),
                "plain relative paths",
            ),
            (
                "url too long",
                u(&format!("/v1/{}", "a".repeat(600))),
                "malformed URL",
            ),
            (
                "setup url outside",
                set(
                    &m,
                    &format!("{W}.setup.url"),
                    "https://example.com/setup.exe",
                ),
                "outside the app's own releases",
            ),
            (
                "short hash",
                set(&m, &format!("{f}.sha256"), &h("x")[..63]),
                "64 lower-case",
            ),
            (
                "long hash",
                set(&m, &format!("{f}.sha256"), &format!("{}0", h("x"))),
                "64 lower-case",
            ),
            (
                "upper-case hash",
                set(&m, &format!("{f}.sha256"), &h("x").to_uppercase()),
                "64 lower-case",
            ),
            (
                "non-hex hash",
                set(&m, &format!("{f}.sha256"), &format!("g{}", &h("x")[1..])),
                "64 lower-case",
            ),
            (
                "wrong header number",
                m.replacen("hub: 1", "hub: 2", 1),
                "header",
            ),
            (
                "old header",
                m.replacen("hub: 1", "manifest: 1", 1),
                "header",
            ),
            ("no header", del(&m, "hub"), "header"),
            ("header not first", format!("app: mdedit\n{m}"), "header"),
            (
                "wrong app",
                set(&m, "app", "mdview"),
                "this manifest is for",
            ),
            ("no app", del(&m, "app"), "missing `app`"),
            ("no version", del(&m, "version"), "missing `version`"),
            ("bad version", set(&m, "version", "1.0"), "bad value"),
            (
                "version with a v",
                set(&m, "version", "v1.0.0"),
                "bad value",
            ),
            ("bad date", set(&m, "date", "20-11-2026"), "YYYY-MM-DD"),
            ("date month 13", set(&m, "date", "2026-13-01"), "YYYY-MM-DD"),
            ("date letters", set(&m, "date", "2026-1a-01"), "YYYY-MM-DD"),
            (
                "notes with control char",
                set(&m, "notes", "bad \u{7} bell"),
                "control characters",
            ),
            (
                "notes too long",
                set(&m, "notes", &"n".repeat(1025)),
                "too long",
            ),
            (
                "notes.file path",
                set(&m, "notes.file", "../x.md"),
                "plain file name",
            ),
            ("duplicate key", add(&m, "version: 0.31.0"), "duplicate key"),
            (
                "duplicate platform key",
                add(&m, &format!("{f}.size: 1")),
                "duplicate key",
            ),
            (
                "dot dot in exe",
                set(&m, &format!("{W}.exe"), "..\\evil.exe"),
                "plain relative path",
            ),
            (
                "dot dot inside exe",
                set(&m, &format!("{W}.exe"), "a/../evil.exe"),
                "plain relative path",
            ),
            (
                "absolute exe",
                set(&m, &format!("{W}.exe"), "\\evil.exe"),
                "plain relative path",
            ),
            (
                "drive exe",
                set(&m, &format!("{W}.exe"), "C:\\evil.exe"),
                "plain relative path",
            ),
            (
                "empty segment exe",
                set(&m, &format!("{W}.exe"), "a//evil.exe"),
                "plain relative path",
            ),
            (
                "hidden exe",
                set(&m, &format!("{W}.exe"), ".evil.exe"),
                "plain relative path",
            ),
            (
                "dot dot in wait",
                set(&m, &format!("{W}.wait"), "a.exe, ../b.exe"),
                "list of plain relative paths",
            ),
            (
                "empty wait item",
                set(&m, &format!("{W}.wait"), "a.exe,,b.exe"),
                "list of plain relative paths",
            ),
            (
                "too many waits",
                set(
                    &m,
                    &format!("{W}.wait"),
                    "a.exe,b.exe,c.exe,d.exe,e.exe,f.exe,g.exe,h.exe,i.exe",
                ),
                "list of plain relative paths",
            ),
            (
                "silent with &",
                set(&m, &format!("{W}.setup.silent"), "/S & calc"),
                "characters not allowed",
            ),
            (
                "silent with quote",
                set(&m, &format!("{W}.setup.silent"), "/S \"x\""),
                "characters not allowed",
            ),
            (
                "silent with $",
                set(&m, &format!("{W}.setup.silent"), "/S $(x)"),
                "characters not allowed",
            ),
            (
                "silent with ;",
                set(&m, &format!("{W}.setup.silent"), "/S;x"),
                "characters not allowed",
            ),
            (
                "silent with backslash",
                set(&m, &format!("{W}.setup.silent"), "/D=C:\\x"),
                "characters not allowed",
            ),
            (
                "silent too long",
                set(&m, &format!("{W}.setup.silent"), &"a".repeat(201)),
                "characters not allowed",
            ),
            (
                "size letters",
                set(&m, &format!("{f}.size"), "12x"),
                "size is not a number",
            ),
            (
                "size negative",
                set(&m, &format!("{f}.size"), "-1"),
                "size is not a number",
            ),
            (
                "size empty",
                set(&m, &format!("{f}.size"), ""),
                "size is not a number",
            ),
            (
                "size decimal",
                set(&m, &format!("{f}.size"), "1.5"),
                "size is not a number",
            ),
            (
                "size over u64",
                set(&m, &format!("{f}.size"), "99999999999999999999999"),
                "size is not a number",
            ),
            (
                "size over cap",
                set(&m, &format!("{f}.size"), "9000000000"),
                "size is too large",
            ),
            (
                "bad scope",
                set(&m, &format!("{W}.setup.scope"), "admin"),
                "scope is user or machine",
            ),
            (
                "bad kind",
                set(&m, &format!("{W}.kind"), "msi"),
                "unknown kind",
            ),
            ("bad min", set(&m, &format!("{W}.min"), "old"), "bad value"),
            (
                "bad updates",
                add(&m, &format!("{W}.updates: yes")),
                "only `self`",
            ),
            (
                "bad uninstall",
                set(&m, &format!("{W}.uninstall"), "a b;c"),
                "uninstall key",
            ),
            (
                "bad file name",
                set(&m, &f, "dir/mdedit.exe"),
                "plain file name",
            ),
            (
                "hidden file name",
                set(&m, &f, ".mdedit.exe"),
                "plain file name",
            ),
            (
                "duplicate file name",
                set(&m, &format!("{W}.file2"), "mdedit.exe"),
                "used twice",
            ),
            (
                "setup named as a file",
                set(&m, &format!("{W}.setup"), "mdedit-ai.exe"),
                "used twice",
            ),
            (
                "file without sha",
                del(&m, &format!("{f}.sha256")),
                "needs a name",
            ),
            (
                "file without url",
                del(&m, &format!("{W}.file2.url")),
                "needs a name",
            ),
            (
                "file without size",
                del(&m, &format!("{W}.file3.size")),
                "needs a name",
            ),
            ("url without name", del(&m, &f), "needs a name"),
            (
                "setup without silent",
                del(&m, &format!("{W}.setup.silent")),
                "needs .silent and .scope",
            ),
            (
                "setup without scope",
                del(&m, &format!("{W}.setup.scope")),
                "needs .silent and .scope",
            ),
            (
                "silent without setup file",
                del(&m, &format!("{W}.setup")),
                "needs a name",
            ),
            ("no kind", del(&m, &format!("{W}.kind")), "missing .kind"),
            (
                "files kind without exe",
                del(&m, &format!("{W}.exe")),
                "kind needs an .exe",
            ),
            (
                "line without colon",
                add(&m, "just some words"),
                "expected `key: value`",
            ),
            ("lone colon", add(&m, ":"), "unknown key"),
            (
                "non-windows platform checked too",
                add(&m, "linux-x86_64.file.url: http://x"),
                "duplicate key",
            ),
            (
                "non-windows url checked too",
                add(&m, "linux-aarch64.file.url: http://x"),
                "not an https URL",
            ),
        ];
        for (label, text, want) in rows {
            let e = err("mdedit", text.as_bytes());
            assert!(e.contains(want), "{label}: wanted `{want}`, got `{e}`");
        }
    }

    #[test]
    fn kind_pieces_are_required() {
        let strip = |t: String, pat: &str| {
            t.lines()
                .filter(|l| !l.contains(pat))
                .collect::<Vec<_>>()
                .join("\n")
        };
        // files kind with no file at all (and no setup either, so nothing else complains first)
        let t = strip(strip(mdedit_txt(), ".file"), ".setup");
        let e = err("mdedit", t.as_bytes());
        assert!(e.contains("kind needs a .file"), "{e}");
        // setup kind with no setup
        let t = strip(sprawl_txt(), ".setup");
        assert!(err("sprawl", t.as_bytes()).contains("kind needs a .setup"));
        // zip kind with no zip
        let zip = set(&sprawl_txt(), &format!("{W}.kind"), "zip");
        assert!(err("sprawl", zip.as_bytes()).contains("kind needs a .zip"));
        // zip kind with a zip but no exe
        let t = format!(
            "hub: 1\napp: mdedit\nversion: 1.0.0\n{W}.kind: zip\n{}",
            fl(app("mdedit"), &format!("{W}.zip"), "v1", "a.zip", 1)
        );
        assert!(err("mdedit", t.as_bytes()).contains("kind needs an .exe"));
        // a platform may list a setup beside files; the kind says which one is primary
        let setup_kind = set(&mdview_txt(), &format!("{W}.kind"), "setup");
        let m = ok("mdview", &setup_kind);
        assert_eq!(m.entry().unwrap().kind, Kind::Setup);
        assert_eq!(m.entry().unwrap().files.len(), 2);
    }

    #[test]
    fn device_names_need_the_whole_stem() {
        // Only the stem before the first dot is a device name: these are ordinary files.
        let m = mdview_txt();
        let f = format!("{W}.file2");
        for name in [
            "console.exe",
            "com10.exe",
            "auxiliary.exe",
            "nul-x.exe",
            "com.exe",
        ] {
            ok("mdview", &set(&m, &f, name));
        }
    }

    #[test]
    fn a_file_name_is_unique_whatever_its_case() {
        let m = mdview_txt();
        let f2 = format!("{W}.file2");
        let e = err("mdview", set(&m, &f2, "MDVIEW.EXE").as_bytes());
        assert!(e.contains("used twice"), "{e}");
        // The setup file is a file of the same folder as well.
        let e = err("mdview", set(&m, &f2, "Mdview-Setup-0.9.1.exe").as_bytes());
        assert!(e.contains("used twice"), "{e}");
        ok("mdview", &set(&m, &f2, "other.exe"));
    }

    #[test]
    fn a_stray_piece_of_a_setup_or_zip_is_refused() {
        let a = app("mdview");
        let base = format!(
            "hub: 1\napp: mdview\nversion: 1.0.0\n{W}.kind: files\n{W}.exe: mdview.exe\n{}",
            fl(a, &format!("{W}.file"), "v1", "mdview.exe", 1)
        );
        ok("mdview", &base);
        let rows = [
            ("silent alone", format!("{W}.setup.silent: /S\n"), "setup"),
            ("scope alone", format!("{W}.setup.scope: user\n"), "setup"),
            (
                "silent and scope, no file",
                format!("{W}.setup.silent: /S\n{W}.setup.scope: user\n"),
                "setup",
            ),
            (
                "setup file without silent",
                format!(
                    "{}{W}.setup.scope: user\n",
                    fl(a, &format!("{W}.setup"), "v1", "s.exe", 1)
                ),
                "setup needs .silent and .scope",
            ),
            ("zip name alone", format!("{W}.zip: a.zip\n"), "zip"),
            (
                "zip url alone",
                format!("{W}.zip.url: {}\n", url(a, "v1", "a.zip")),
                "zip",
            ),
            (
                "zip sha alone",
                format!("{W}.zip.sha256: {}\n", h("z")),
                "zip",
            ),
        ];
        for (label, extra, want) in rows {
            let e = err("mdview", format!("{base}{extra}").as_bytes());
            assert!(e.contains(want), "{label}: {e}");
        }
        // The complete zip is accepted beside files.
        let zip = fl(a, &format!("{W}.zip"), "v1", "a.zip", 1);
        ok("mdview", &format!("{base}{zip}"));
    }

    #[test]
    fn the_limits_are_exact() {
        let m = mdview_txt();
        let f = format!("{W}.file2");
        let exe = format!("{W}.exe");
        let un = format!("{W}.uninstall");
        let silent = format!("{W}.setup.silent");
        let good = |t: String| {
            ok("mdview", &t);
        };
        let bad = |t: String, want: &str| {
            let e = err("mdview", t.as_bytes());
            assert!(e.contains(want), "wanted `{want}`, got `{e}`");
        };
        // A file name: 128 bytes, no more.
        let name = |n: usize| format!("{}.exe", "a".repeat(n - 4));
        good(set(&m, &f, &name(MAX_NAME)));
        bad(set(&m, &f, &name(MAX_NAME + 1)), "plain file name");
        // A relative path: 128 bytes, no more.
        let path = |n: usize| format!("{}\\{}", "a".repeat(64), "b".repeat(n - 65));
        good(set(&m, &exe, &path(MAX_PATH)));
        bad(set(&m, &exe, &path(MAX_PATH + 1)), "plain relative path");
        // The uninstall key: 80 bytes, a small alphabet.
        good(set(&m, &un, &"u".repeat(80)));
        bad(set(&m, &un, &"u".repeat(81)), "uninstall key");
        good(set(&m, &un, "{A-b_1.2}"));
        for c in [" ", "/", "\\", ";", "\""] {
            bad(set(&m, &un, &format!("a{c}b")), "uninstall key");
        }
        // The silent arguments: not empty, 200 bytes, a small alphabet.
        good(set(&m, &silent, &"s".repeat(MAX_SILENT)));
        bad(set(&m, &silent, &"s".repeat(MAX_SILENT + 1)), "not allowed");
        bad(set(&m, &silent, ""), "not allowed");
        bad(set(&m, &silent, "/S & calc"), "not allowed");
        bad(set(&m, &silent, "/S;calc"), "not allowed");
        // A size: up to 8 GiB.
        let size = format!("{W}.file.size");
        good(set(&m, &size, &MAX_SIZE.to_string()));
        bad(set(&m, &size, &(MAX_SIZE + 1).to_string()), "too large");
        // A date: ten characters, a real month, a day from 1 to 31.
        good(set(&m, "date", "2026-12-31"));
        good(set(&m, "date", "2026-01-01"));
        for d in [
            "2026-11-200",
            "2026-11-99",
            "2026-11-32",
            "2026-11-00",
            "2026-00-10",
            "2026-11-1",
            "x2026-11-10",
        ] {
            bad(set(&m, "date", d), "YYYY-MM-DD");
        }
    }

    #[test]
    fn no_platform_at_all() {
        let t = "hub: 1\napp: sprawl\nversion: 1.0.0\n";
        assert!(err("sprawl", t.as_bytes()).contains("no platform"));
    }

    #[test]
    fn only_other_platforms_leaves_no_windows_entry() {
        let a = app("mdedit");
        let mut t =
            String::from("hub: 1\napp: mdedit\nversion: 1.0.0\nwindows-aarch64.kind: files\n");
        t += "windows-aarch64.exe: mdedit.exe\n";
        t += &fl(a, "windows-aarch64.file", "v1", "mdedit.exe", 5);
        let m = ok("mdedit", &t);
        assert_eq!(m.platforms.len(), 1);
        assert!(m.entry().is_none());
    }

    #[test]
    fn size_and_line_limits() {
        let base = mdview_txt();
        assert!(base.len() < MAX_BYTES);
        let n = base.lines().count();
        assert!(n < MAX_LINES);
        // Exactly 128 lines is fine, 129 is not.
        let pad = |count: usize| format!("{base}{}", "# pad\n".repeat(count));
        ok("mdview", &pad(MAX_LINES - n));
        let e = err("mdview", pad(MAX_LINES - n + 1).as_bytes());
        assert!(e.contains("too many lines"), "{e}");
        // Exactly 16 KiB is fine, one byte more is not.
        let fill = |total: usize| {
            let mut s = base.clone();
            s += "#";
            while s.len() < total {
                s.push('x');
            }
            s
        };
        ok("mdview", &fill(MAX_BYTES));
        let e = err("mdview", fill(MAX_BYTES + 1).as_bytes());
        assert!(e.contains("too large"), "{e}");
    }

    #[test]
    fn not_utf8_and_empty() {
        let mut b = mdview_txt().into_bytes();
        b.extend_from_slice(b"# \xff\xfe\n");
        assert!(err("mdview", &b).contains("not UTF-8"));
        assert!(err("mdview", b"").contains("header"));
        assert!(err("mdview", b"\n\n# only a comment\n").contains("header"));
        assert!(err("mdview", b"\xef\xbb\xbfhub: 1\napp: mdview\n").contains("header"));
    }

    // ---------------------------------------------------------------- write and round trip

    #[test]
    fn round_trip() {
        for (id, text) in fixtures() {
            let m = ok(id, &text);
            let w = write(&m);
            assert_eq!(ok(id, &w), m, "{id}");
            assert_eq!(write(&ok(id, &w)), w, "{id}: write is stable");
        }
    }

    #[test]
    fn round_trip_of_every_key() {
        let inbound = app("inbound");
        let m = HubManifest {
            app: "inbound".into(),
            version: Version::parse("3.0.0-beta.2").unwrap(),
            date: "2026-12-01".into(),
            notes: "a: b, c; d".into(),
            notes_file: Some("notes-3.md".into()),
            platforms: vec![PlatformEntry {
                platform: PLATFORM.into(),
                kind: Kind::Zip,
                exe: Some("app\\x.exe".into()),
                files: (1..=9u64)
                    .map(|i| FileSpec {
                        name: format!("f{i}.dll"),
                        url: url(inbound, "v3", &format!("f{i}.dll")),
                        sha256: h(&format!("f{i}")),
                        size: i,
                    })
                    .collect(),
                zip: Some(FileSpec {
                    name: "i.zip".into(),
                    url: url(inbound, "v3", "i.zip"),
                    sha256: h("z"),
                    size: 0,
                }),
                setup: Some(SetupSpec {
                    file: FileSpec {
                        name: "s.exe".into(),
                        url: url(inbound, "v3", "s.exe"),
                        sha256: h("s"),
                        size: 123,
                    },
                    silent: "/S /D=x".into(),
                    scope: Scope::Machine,
                }),
                uninstall: Some("{GUID}_is1".into()),
                min: Some(Version::parse("2.0.0").unwrap()),
                wait: vec!["a.exe".into(), "b\\c.exe".into()],
                updates_self: true,
            }],
        };
        assert_eq!(ok("inbound", &write(&m)), m);
    }

    #[test]
    fn entry_is_the_windows_one() {
        let m = ok("mdedit", &mdedit_txt());
        assert_eq!(m.entry().unwrap().platform, PLATFORM);
        assert_eq!(PLATFORM, "windows-x86_64");
    }

    // ---------------------------------------------------------------- the legacy formats

    /// The example in the doc comment of `mdshared_app::update`.
    fn legacy_mdview() -> String {
        let b = "https://github.com/Kaan88/markdown-viewer-rs/releases/download/v0.2.0";
        format!(
            "manifest: 1\nversion: 0.2.0\nnotes: Faster scrolling; fixes a crash on empty files.\n\
             windows-x86_64.asset: mdview.exe\nwindows-x86_64.url: {b}/mdview.exe\n\
             windows-x86_64.sha256: 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef\n\
             windows-x86_64.engine.url: {b}/mdview-engine.exe\n\
             windows-x86_64.engine.sha256: fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210\n"
        )
    }
    /// Format 2: a further executable (`mdedit-ai`), plus platforms this Hub skips.
    fn legacy_mdedit() -> String {
        let b = "https://github.com/Kaan88/markdown-editor-rs/releases/download/v0.30.0";
        format!(
            "manifest: 2\n# mdedit-update-2.txt\nversion: 0.30.0\nnotes: Notes here.\n\
             windows-x86_64.asset: mdedit.exe\nwindows-x86_64.url: {b}/mdedit.exe\n\
             windows-x86_64.sha256: {}\nwindows-x86_64.engine.url: {b}/mdview-engine.exe\n\
             windows-x86_64.engine.sha256: {}\nwindows-x86_64.mdedit-ai.url: {b}/mdedit-ai.exe\n\
             windows-x86_64.mdedit-ai.sha256: {}\nlinux-x86_64.asset: mdedit\n\
             linux-x86_64.url: {b}/mdedit\nlinux-x86_64.sha256: {}\nlinux-aarch64.url: {b}/x\n",
            h("a"),
            h("b"),
            h("c"),
            h("d")
        )
    }
    fn legacy(id: &str, text: &str) -> Result<HubManifest, String> {
        from_legacy(text.as_bytes(), app(id))
    }

    #[test]
    fn legacy_format_one_of_mdview() {
        let m = legacy("mdview", &legacy_mdview()).unwrap();
        assert_eq!(m.app, "mdview");
        assert_eq!(m.version, Version::parse("0.2.0").unwrap());
        assert_eq!(m.notes, "Faster scrolling; fixes a crash on empty files.");
        assert_eq!(m.date, "");
        let e = m.entry().unwrap();
        assert_eq!(e.kind, Kind::Files);
        assert_eq!(e.exe.as_deref(), Some("mdview.exe"));
        let names: Vec<_> = e.files.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["mdview.exe", "mdview-engine.exe"]);
        assert!(e.files.iter().all(|f| f.size == 0));
        assert_eq!(
            e.files[1].sha256,
            "fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210"
        );
        assert_eq!(e.wait, ["mdview-engine.exe"]);
        assert!(e.setup.is_none() && e.zip.is_none());
    }

    #[test]
    fn legacy_format_two_of_mdedit_with_extras() {
        let m = legacy("mdedit", &legacy_mdedit()).unwrap();
        let e = m.entry().unwrap();
        let names: Vec<_> = e.files.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, ["mdedit.exe", "mdview-engine.exe", "mdedit-ai.exe"]);
        assert_eq!(e.exe.as_deref(), Some("mdedit.exe"));
        assert_eq!(e.files[2].sha256, h("c"));
        assert_eq!(e.wait, ["mdedit-ai.exe", "mdview-engine.exe"]);
        assert_eq!(m.platforms.len(), 1);
    }

    #[test]
    fn legacy_without_engine() {
        let t: String = legacy_mdview()
            .lines()
            .filter(|l| !l.contains("engine"))
            .map(|l| format!("{l}\n"))
            .collect();
        let m = legacy("mdview", &t).unwrap();
        assert_eq!(m.entry().unwrap().files.len(), 1);
    }

    #[test]
    fn legacy_places_the_primary_file_under_the_exe_name() {
        // The release scripts name the asset with the version and the platform.
        let t = legacy_mdedit().replace(
            "windows-x86_64.asset: mdedit.exe",
            "windows-x86_64.asset: mdedit-0.30.0-windows-x86_64.exe",
        );
        let m = legacy("mdedit", &t).unwrap();
        let e = m.entry().unwrap();
        assert_eq!(e.exe.as_deref(), Some("mdedit.exe"));
        assert_eq!(e.files[0].name, "mdedit.exe");
    }

    #[test]
    fn legacy_name_checks() {
        let b = "https://github.com/Kaan88/markdown-editor-rs/releases/download/v0.30.0";
        let extra = |url: &str| {
            format!(
                "{}windows-x86_64.tool.url: {url}
windows-x86_64.tool.sha256: {}
",
                legacy_mdedit(),
                h("t")
            )
        };
        // A file name used twice: an extra that has the engine's name.
        let e = legacy("mdedit", &extra(&format!("{b}/mdview-engine.exe"))).unwrap_err();
        assert!(e.contains("used twice"), "{e}");
        // ... or one that has the exe's name, whatever the asset is called.
        let e = legacy("mdedit", &extra(&format!("{b}/mdedit.exe"))).unwrap_err();
        assert!(e.contains("used twice"), "{e}");
        // An asset name that is not a plain one.
        for bad in ["a b.exe", ".hidden", "nul", "x.exe."] {
            let t = legacy_mdedit().replace(
                "windows-x86_64.asset: mdedit.exe",
                &format!("windows-x86_64.asset: {bad}"),
            );
            assert!(legacy("mdedit", &t).is_err(), "{bad}");
        }
        // An extra whose URL does not end in a plain name is called `<stem>.exe`.
        let m = legacy("mdedit", &extra(&format!("{b}/.tool"))).unwrap();
        let names: Vec<_> = m
            .entry()
            .unwrap()
            .files
            .iter()
            .map(|f| f.name.as_str())
            .collect();
        assert!(names.contains(&"tool.exe"), "{names:?}");
        assert_eq!(names.len(), 4);
    }

    #[test]
    fn legacy_round_trips_through_write() {
        for (id, t) in [("mdview", legacy_mdview()), ("mdedit", legacy_mdedit())] {
            let m = legacy(id, &t).unwrap();
            assert_eq!(ok(id, &write(&m)), m, "{id}");
        }
    }

    #[test]
    fn legacy_refusals() {
        // No windows build.
        let linux_only = legacy_mdview().replace("windows-x86_64", "linux-x86_64");
        assert!(
            legacy("mdview", &linux_only)
                .unwrap_err()
                .contains("no windows-x86_64")
        );
        // A URL outside the app's own repository.
        let other = legacy_mdview().replace("markdown-viewer-rs", "evil");
        assert!(legacy("mdview", &other).unwrap_err().contains("outside"));
        // mdedit's manifest is not mdview's.
        assert!(
            legacy("mdview", &legacy_mdedit())
                .unwrap_err()
                .contains("outside")
        );
        // Garbage, a hub.txt, bytes that are not UTF-8.
        assert!(legacy("mdview", "hello").is_err());
        assert!(legacy("mdview", &mdview_txt()).is_err());
        assert!(from_legacy(b"\xff\xfe", app("mdview")).is_err());
        // And the legacy format is not a hub.txt.
        assert!(err("mdview", legacy_mdview().as_bytes()).contains("header"));
    }

    // ---------------------------------------------------------------- never panic

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n.max(1) as u64) as usize
        }
    }

    /// Both readers, two apps; an `Ok` must survive a write and a read.
    fn poke(bytes: &[u8]) {
        for a in [app("mdedit"), app("inbound")] {
            if let Ok(m) = parse(bytes, a) {
                assert_eq!(parse(write(&m).as_bytes(), a), Ok(m));
            }
            let _ = from_legacy(bytes, a);
        }
    }

    fn corpus() -> Vec<Vec<u8>> {
        let mut c: Vec<Vec<u8>> = fixtures()
            .into_iter()
            .map(|(_, t)| t.into_bytes())
            .collect();
        c.push(legacy_mdview().into_bytes());
        c.push(legacy_mdedit().into_bytes());
        c
    }

    #[test]
    fn fuzz_every_truncation() {
        for doc in corpus() {
            for n in 0..=doc.len() {
                poke(&doc[..n]);
            }
            // Cut inside a multi-byte character.
            let mut d = doc.clone();
            d.extend_from_slice("\n# \u{20ac}\u{1f600}".as_bytes());
            for n in doc.len()..=d.len() {
                poke(&d[..n]);
            }
        }
    }

    #[test]
    fn fuzz_mutations() {
        let corpus = corpus();
        let mut r = Rng(0x9e37_79b9_7f4a_7c15);
        const SPICE: &[u8] = b":\n\r\t\0 #.,\\/{}%@-_09azAZ\xff\xc3\x28";
        for _ in 0..6000 {
            let mut d = corpus[r.below(corpus.len())].clone();
            for _ in 0..1 + r.below(8) {
                if d.is_empty() {
                    break;
                }
                let i = r.below(d.len());
                match r.below(5) {
                    0 => d[i] = r.next() as u8,
                    1 => d[i] = SPICE[r.below(SPICE.len())],
                    2 => {
                        d.remove(i);
                    }
                    3 => d.insert(i, SPICE[r.below(SPICE.len())]),
                    _ => {
                        let j = r.below(d.len());
                        d.swap(i, j);
                    }
                }
            }
            poke(&d);
        }
    }

    #[test]
    fn fuzz_binary_and_shapes() {
        let mut r = Rng(0x1234_5678_9abc_def1);
        for len in [0, 1, 2, 3, 15, 255, 4096, 16384, 16385, 40000] {
            for _ in 0..8 {
                let d: Vec<u8> = (0..len).map(|_| r.next() as u8).collect();
                poke(&d);
            }
        }
        let hdr = "hub: 1\napp: mdedit\n";
        let w = format!("{W}.file");
        let shapes: Vec<String> = vec![
            format!("{hdr}version: {}.0.0\n", "9".repeat(40)),
            format!("{hdr}version: 1.0.0-{}\n", "a.".repeat(5000)),
            format!("{hdr}version: 1.0.0\n{w}.size: {}\n", "9".repeat(5000)),
            format!("{hdr}version: 1.0.0\n{w}.size: 18446744073709551616\n"),
            format!("{hdr}version: 1.0.0\n{w}.size: 18446744073709551615\n"),
            format!("{hdr}version: 1.0.0\n{W}.min: {}.0.0\n", "9".repeat(5000)),
            format!("{hdr}notes: {}\n", "n".repeat(15000)),
            format!("{hdr}{}: x\n", "k".repeat(15000)),
            format!("{hdr}{W}.wait: {}\n", "a.exe,".repeat(2000)),
            format!("{hdr}{W}.exe: {}\n", "a/".repeat(7000)),
            format!("{hdr}{}\n", ":".repeat(15000)),
            format!("{hdr}{}", ":\n".repeat(60)),
            format!("{hdr}{}", "#\n".repeat(500)),
            format!("{hdr}{}", "\r".repeat(15000)),
            "hub: 1".repeat(2000),
            "hub:".into(),
            "hub: 1\n\u{0}\n".into(),
            format!("hub: 1\napp: mdedit\nversion: 1.0.0\n{W}.setup.silent: \u{0}\n"),
            format!("hub: 1\napp: mdedit\nversion: 1.0.0\n{W}.file9.url: https://\n"),
            format!("hub: 1\napp: mdedit\nversion: 1.0.0\n\u{feff}{W}.kind: files\n"),
            "\u{1f600}: \u{1f600}\n".repeat(100),
        ];
        for s in shapes {
            poke(s.as_bytes());
        }
    }

    /// `tools/write-hub-txt.ps1` writes the file every app's release carries, and repeats this
    /// parser's checks so that a bad release fails in its workflow. What it writes must be read
    /// here, and what would not be read it must refuse: a `hub.txt` the script was happy with and
    /// Hub is not is a release nobody can install from.
    #[cfg(windows)]
    #[test]
    fn the_release_script_writes_only_what_this_parser_reads() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let dir = std::env::temp_dir().join(format!("hub-script-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let files = dir.join("files.json");
        std::fs::write(
            &files,
            format!(
                r#"[{{"Name":"mdedit.exe","Sha256":"{}","Size":5}}]"#,
                h("a")
            ),
        )
        .unwrap();
        let out = dir.join("hub.txt");
        let run = |exe: &str, notes: &str| -> Option<Vec<u8>> {
            let _ = std::fs::remove_file(&out);
            let st = std::process::Command::new("powershell.exe")
                .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
                .arg(root.join("tools").join("write-hub-txt.ps1"))
                .args([
                    "-App",
                    "mdedit",
                    "-Repo",
                    "markdown-editor-rs",
                    "-Version",
                    "1.2.3",
                ])
                .args(["-Kind", "files", "-Exe", exe, "-Notes", notes, "-FilesJson"])
                .arg(&files)
                .arg("-Out")
                .arg(&out)
                .output()
                .expect("powershell.exe runs");
            st.status
                .success()
                .then(|| std::fs::read(&out).expect("the file it said it wrote"))
        };
        // The plain case.
        let text = run("mdedit.exe", "Faster.").expect("a plain release is written");
        let m = parse(&text, app("mdedit")).expect("and read");
        assert_eq!(m.notes, "Faster.");
        // Notes are limited in bytes here, not in characters: 700 two-byte letters are 1400 bytes.
        let long = "\u{e9}".repeat(700);
        let text = run("mdedit.exe", &long).expect("long notes are cut, not refused");
        let m = parse(&text, app("mdedit")).expect("notes the script kept are notes Hub reads");
        assert!(m.notes.len() <= MAX_NOTES && m.notes.starts_with('\u{e9}'));
        // A path with a space is not a plain relative path here: the script must say so.
        assert!(
            run(r"my app\mdedit.exe", "x").is_none(),
            "a path Hub refuses was written"
        );
        assert!(
            parse(
                String::from_utf8_lossy(&text)
                    .replace("exe: mdedit.exe", r"exe: my app\mdedit.exe")
                    .as_bytes(),
                app("mdedit")
            )
            .is_err()
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
