//! Asking GitHub what is new, and what a row of the window should say about it.
//!
//! One GET of `hub.txt` per app (64 KiB cap, 10 s); an app whose release carries none but names
//! a legacy manifest (mdedit, mdview) costs exactly one more. Nothing else is requested.
//!
//! Timing: `last_check` in the store is the time of the last *attempt* (a run in which at least
//! one app answered, or not). With no failures the next check is due one interval after it; after
//! failed runs it is due after 30 minutes, doubling per further failure up to the interval.
//! Interval 0 means never: a manual check only.
use crate::catalog::CatalogApp;
use crate::manifest;
use crate::model::{HubManifest, Installed, Latest, Row, Source};
use crate::state::Store;
use crate::transport::Transport;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MANIFEST_MAX: usize = 64 * 1024;
const TIMEOUT: Duration = Duration::from_secs(10);
/// How many apps are asked at once.
const AT_ONCE: usize = 2;
/// The first retry after a failed run, in seconds.
const RETRY_FIRST: u64 = 30 * 60;

/// What one app's lookup came to. `Absent` means the server answered but has no manifest we can
/// use (404, unreadable, wrong app): a successful check with nothing to offer. `Failed` means we
/// could not ask (offline, timeout, server error): the cached answer stands and we retry soon.
enum Look {
    Found(Box<HubManifest>),
    Absent(String),
    Failed(String),
}

/// A reason that says the server answered "not there" rather than "could not reach".
fn is_absent(reason: &str) -> bool {
    reason.contains("Status(404)") || reason.contains("Status(410)")
}

fn look(app: &CatalogApp, t: &dyn Transport) -> Look {
    let mut failed: Option<String> = None;
    let mut absent = String::from("no hub.txt in the latest release");
    match t.get(&app.latest_url("hub.txt"), MANIFEST_MAX, TIMEOUT) {
        Ok(bytes) => match manifest::parse(&bytes, app) {
            Ok(m) => return Look::Found(Box::new(m)),
            Err(e) => absent = format!("hub.txt: {e}"),
        },
        Err(e) if is_absent(&e) => {}
        Err(e) => failed = Some(e),
    }
    if let Some(legacy) = app.legacy_manifest {
        match t.get(&app.latest_url(legacy), MANIFEST_MAX, TIMEOUT) {
            Ok(bytes) => match manifest::from_legacy(&bytes, app) {
                Ok(m) => return Look::Found(Box::new(m)),
                Err(e) => absent = format!("{legacy}: {e}"),
            },
            Err(e) if is_absent(&e) => {}
            Err(e) => failed = Some(e),
        }
    }
    match failed {
        Some(e) => Look::Failed(e),
        None => Look::Absent(absent),
    }
}

/// Ask for one app's latest release. Never more than the requests described at the top.
pub fn check_one(app: &CatalogApp, t: &dyn Transport) -> Latest {
    match look(app, t) {
        Look::Found(m) => Latest::Found(m),
        Look::Absent(r) | Look::Failed(r) => Latest::NoManifest(r),
    }
}

/// Seconds since the Unix epoch (0 if the clock is before it).
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn cached(app: &CatalogApp, store: &Store) -> Option<Box<HubManifest>> {
    let bytes = store.cached_manifest(app.id)?;
    manifest::parse(&bytes, app).ok().map(Box::new)
}

/// Check every app, two at a time. A manifest that was read is cached in the store (as the
/// canonical hub.txt text); an app that could not be asked gets its cached manifest back, or
/// `Unknown`. Records the time of the run and counts failed runs (a run where nothing answered).
pub fn check_all<'a>(
    apps: impl IntoIterator<Item = &'a CatalogApp>,
    t: &dyn Transport,
    store: &Store,
) -> Vec<(&'static str, Latest)> {
    let apps: Vec<&CatalogApp> = apps.into_iter().collect();
    if apps.is_empty() {
        return Vec::new();
    }
    let looks = look_all(&apps, t);
    let mut out = Vec::with_capacity(apps.len());
    let mut answered = false;
    for (app, look) in apps.iter().zip(looks) {
        let latest = match look {
            Look::Found(m) => {
                answered = true;
                store.set_cached_manifest(app.id, manifest::write(&m).as_bytes());
                Latest::Found(m)
            }
            Look::Absent(r) => {
                answered = true;
                Latest::NoManifest(r)
            }
            Look::Failed(_) => match cached(app, store) {
                Some(m) => Latest::Found(m),
                None => Latest::Unknown,
            },
        };
        out.push((app.id, latest));
    }
    store.set_last_check(now());
    store.set_failures(if answered {
        0
    } else {
        store.failures().saturating_add(1)
    });
    out
}

/// Look every app up with this thread and one more: two requests in flight at most.
fn look_all(apps: &[&CatalogApp], t: &dyn Transport) -> Vec<Look> {
    let next = AtomicUsize::new(0);
    let results: Mutex<Vec<Option<Look>>> = Mutex::new(apps.iter().map(|_| None).collect());
    let work = || {
        loop {
            let i = next.fetch_add(1, Ordering::SeqCst);
            let Some(app) = apps.get(i) else { return };
            let l = look(app, t);
            if let Ok(mut r) = results.lock() {
                r[i] = Some(l);
            }
        }
    };
    std::thread::scope(|s| {
        for _ in 1..AT_ONCE.min(apps.len()) {
            // If a thread cannot be made, this thread simply does all the work.
            let _ = std::thread::Builder::new().spawn_scoped(s, work);
        }
        work();
    });
    let done = results.into_inner().unwrap_or_default();
    done.into_iter()
        .map(|l| l.unwrap_or_else(|| Look::Failed("check did not finish".to_string())))
        .collect()
}

/// The delay after `last` before the next check, in seconds. `u64::MAX` means never.
fn delay(interval_hours: u32, failures: u32) -> u64 {
    if interval_hours == 0 {
        return u64::MAX;
    }
    let interval = u64::from(interval_hours) * 3600;
    if failures == 0 {
        return interval;
    }
    let doublings = (failures - 1).min(32);
    RETRY_FIRST.saturating_mul(1u64 << doublings).min(interval)
}

/// When the next check is due (Unix seconds); `u64::MAX` for never. No check yet: 0.
pub fn next_due(last: Option<u64>, interval_hours: u32, failures: u32) -> u64 {
    match last {
        None if interval_hours == 0 => u64::MAX,
        None => 0,
        Some(l) => l.saturating_add(delay(interval_hours, failures)),
    }
}

/// Is a check due at `now`? Interval 0 is never; a clock that went backwards (the last check is
/// in the future) is due.
pub fn due(last: Option<u64>, now: u64, interval_hours: u32, failures: u32) -> bool {
    if interval_hours == 0 {
        return false;
    }
    match last {
        None => true,
        Some(l) if l > now => true,
        Some(_) => now >= next_due(last, interval_hours, failures),
    }
}

/// What a row of the window says about one app.
pub fn row_state(installed: Option<&Installed>, latest: &Latest, skipped: Option<&str>) -> Row {
    let Some(inst) = installed else {
        return Row::NotInstalled;
    };
    if inst.source != Source::Hub {
        return Row::External;
    }
    match latest {
        Latest::Unknown => Row::NoInfo("not checked yet".to_string()),
        Latest::NoManifest(r) => Row::NoInfo(r.clone()),
        Latest::Found(m) => {
            if skipped == Some(m.version.to_string().as_str()) {
                return Row::UpToDate;
            }
            match &inst.version {
                Some(v) if m.version <= *v => Row::UpToDate,
                _ => Row::Update,
            }
        }
    }
}

/// "Checked 4 minutes ago".
pub fn age_label(last: Option<u64>, now: u64) -> String {
    let Some(last) = last else {
        return "Not checked yet".to_string();
    };
    let s = now.saturating_sub(last);
    let plural =
        |n: u64, unit: &str| format!("Checked {n} {unit}{} ago", if n == 1 { "" } else { "s" });
    match s {
        0..=59 => "Checked just now".to_string(),
        60..=3599 => plural(s / 60, "minute"),
        3600..=172_799 => plural(s / 3600, "hour"),
        _ => plural(s / 86_400, "day"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::find;
    use crate::model::{FileSpec, Kind, PlatformEntry};
    use mdshared_app::update::Version;
    use std::collections::HashMap;
    use std::path::PathBuf;

    /// Answers from a map, records every URL asked.
    #[derive(Default)]
    struct Fake {
        answers: Mutex<HashMap<String, Result<Vec<u8>, String>>>,
        asked: Mutex<Vec<String>>,
    }
    impl Fake {
        fn set(&self, url: String, r: Result<Vec<u8>, String>) {
            self.answers.lock().unwrap().insert(url, r);
        }
        fn asked(&self) -> Vec<String> {
            self.asked.lock().unwrap().clone()
        }
        fn count(&self) -> usize {
            self.asked().len()
        }
    }
    impl Transport for Fake {
        fn get(&self, url: &str, max: usize, timeout: Duration) -> Result<Vec<u8>, String> {
            assert_eq!(max, MANIFEST_MAX);
            assert_eq!(timeout, TIMEOUT);
            self.asked.lock().unwrap().push(url.to_string());
            self.answers
                .lock()
                .unwrap()
                .get(url)
                .cloned()
                .unwrap_or_else(|| Err("Status(404)".to_string()))
        }
    }

    fn ver(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    fn manifest_of(app: &CatalogApp, version: &str) -> HubManifest {
        let name = app.exe.rsplit('\\').next().unwrap().to_string();
        let file = FileSpec {
            url: format!(
                "https://github.com/Kaan88/{}/releases/download/v{version}/{name}",
                app.repo
            ),
            name,
            sha256: "0123456789abcdef".repeat(4),
            size: 10,
        };
        HubManifest {
            app: app.id.to_string(),
            version: ver(version),
            date: "2026-11-22".to_string(),
            notes: "n".to_string(),
            notes_file: None,
            platforms: vec![PlatformEntry {
                platform: manifest::PLATFORM.to_string(),
                kind: Kind::Files,
                exe: Some(app.exe.to_string()),
                files: vec![file],
                zip: None,
                setup: None,
                uninstall: None,
                min: None,
                wait: vec![],
                updates_self: false,
            }],
        }
    }

    fn hub_txt(app: &CatalogApp, version: &str) -> Vec<u8> {
        manifest::write(&manifest_of(app, version)).into_bytes()
    }

    fn legacy_txt(app: &CatalogApp, version: &str) -> Vec<u8> {
        let p = "windows-x86_64";
        let base = format!(
            "https://github.com/Kaan88/{}/releases/download/v{version}",
            app.repo
        );
        let sha = "0123456789abcdef".repeat(4);
        format!(
            "manifest: 1\nversion: {version}\n{p}.asset: {}\n{p}.url: {base}/{}\n{p}.sha256: {sha}\n\
             {p}.engine.url: {base}/engine.exe\n{p}.engine.sha256: {sha}\n",
            app.exe, app.exe
        )
        .into_bytes()
    }

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hub-check-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn mdedit() -> &'static CatalogApp {
        find("mdedit").unwrap()
    }
    fn sprawl() -> &'static CatalogApp {
        find("sprawl").unwrap()
    }

    #[test]
    fn one_get_per_app_when_hub_txt_is_there() {
        let t = Fake::default();
        let a = mdedit();
        t.set(a.latest_url("hub.txt"), Ok(hub_txt(a, "1.2.3")));
        match check_one(a, &t) {
            Latest::Found(m) => assert_eq!(m.version, ver("1.2.3")),
            other => panic!("{other:?}"),
        }
        assert_eq!(t.asked(), vec![a.latest_url("hub.txt")]);
    }

    #[test]
    fn legacy_fallback_adds_exactly_one_request() {
        let t = Fake::default();
        let a = mdedit();
        let legacy = a.legacy_manifest.unwrap();
        t.set(a.latest_url(legacy), Ok(legacy_txt(a, "0.9.0")));
        match check_one(a, &t) {
            Latest::Found(m) => {
                assert_eq!(m.version, ver("0.9.0"));
                assert_eq!(m.app, "mdedit");
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            t.asked(),
            vec![a.latest_url("hub.txt"), a.latest_url(legacy)]
        );
    }

    #[test]
    fn a_hub_txt_that_does_not_parse_also_falls_back_once() {
        let t = Fake::default();
        let a = mdedit();
        t.set(a.latest_url("hub.txt"), Ok(b"nonsense".to_vec()));
        t.set(
            a.latest_url(a.legacy_manifest.unwrap()),
            Ok(legacy_txt(a, "0.9.0")),
        );
        assert!(matches!(check_one(a, &t), Latest::Found(_)));
        assert_eq!(t.count(), 2);
    }

    #[test]
    fn no_manifest_anywhere_is_no_manifest_and_costs_what_it_must() {
        // An app with no legacy file: one request.
        let t = Fake::default();
        assert!(matches!(check_one(sprawl(), &t), Latest::NoManifest(_)));
        assert_eq!(t.count(), 1);
        // An app with a legacy file: two, never three.
        let t = Fake::default();
        assert!(matches!(check_one(mdedit(), &t), Latest::NoManifest(_)));
        assert_eq!(t.count(), 2);
    }

    #[test]
    fn a_manifest_for_another_app_or_garbage_is_no_manifest_with_a_reason() {
        let t = Fake::default();
        let a = sprawl();
        t.set(a.latest_url("hub.txt"), Ok(hub_txt(mdedit(), "1.0.0")));
        match check_one(a, &t) {
            Latest::NoManifest(r) => assert!(!r.is_empty()),
            other => panic!("{other:?}"),
        }
        t.set(a.latest_url("hub.txt"), Ok(vec![0xff, 0xfe, 0, 1]));
        assert!(matches!(check_one(a, &t), Latest::NoManifest(_)));
        t.set(a.latest_url("hub.txt"), Ok(Vec::new()));
        assert!(matches!(check_one(a, &t), Latest::NoManifest(_)));
    }

    #[test]
    fn check_all_asks_each_app_once_and_caches() {
        let t = Fake::default();
        let store = Store {
            dir: scratch("all"),
        };
        for a in crate::catalog::CATALOG.iter() {
            t.set(a.latest_url("hub.txt"), Ok(hub_txt(a, "2.0.0")));
        }
        let out = check_all(&crate::catalog::CATALOG, &t, &store);
        assert_eq!(out.len(), 4);
        assert_eq!(out[0].0, "mdedit");
        assert!(out.iter().all(|(_, l)| matches!(l, Latest::Found(_))));
        assert_eq!(t.count(), 4);
        assert_eq!(store.failures(), 0);
        assert!(store.last_check().unwrap() > 0);
        assert!(store.cached_manifest("sprawl").is_some());
    }

    #[test]
    fn a_failed_check_keeps_the_cached_row_and_counts_the_failure() {
        let t = Fake::default();
        let store = Store {
            dir: scratch("fail"),
        };
        let a = sprawl();
        t.set(a.latest_url("hub.txt"), Ok(hub_txt(a, "3.1.0")));
        let first = check_all([a], &t, &store);
        assert!(matches!(&first[0].1, Latest::Found(m) if m.version == ver("3.1.0")));
        // Now the network is down.
        t.set(a.latest_url("hub.txt"), Err("Os(12007)".to_string()));
        let again = check_all([a], &t, &store);
        assert!(matches!(&again[0].1, Latest::Found(m) if m.version == ver("3.1.0")));
        assert_eq!(store.failures(), 1);
        check_all([a], &t, &store);
        assert_eq!(store.failures(), 2);
        // And back up: the count resets.
        t.set(a.latest_url("hub.txt"), Ok(hub_txt(a, "3.2.0")));
        check_all([a], &t, &store);
        assert_eq!(store.failures(), 0);
    }

    #[test]
    fn a_failure_with_nothing_cached_is_unknown_not_no_manifest() {
        let t = Fake::default();
        let store = Store {
            dir: scratch("fail-empty"),
        };
        let a = sprawl();
        t.set(a.latest_url("hub.txt"), Err("Timeout".to_string()));
        let out = check_all([a], &t, &store);
        assert_eq!(out[0].1, Latest::Unknown);
    }

    #[test]
    fn a_404_is_an_answer_not_a_failure() {
        let t = Fake::default();
        let store = Store {
            dir: scratch("404"),
        };
        store.set_failures(3);
        let out = check_all([sprawl()], &t, &store);
        assert!(matches!(out[0].1, Latest::NoManifest(_)));
        assert_eq!(store.failures(), 0);
    }

    #[test]
    fn check_all_never_runs_more_than_two_requests_at_once() {
        struct Slow {
            now: AtomicUsize,
            peak: AtomicUsize,
        }
        impl Transport for Slow {
            fn get(&self, _: &str, _: usize, _: Duration) -> Result<Vec<u8>, String> {
                let n = self.now.fetch_add(1, Ordering::SeqCst) + 1;
                self.peak.fetch_max(n, Ordering::SeqCst);
                std::thread::sleep(Duration::from_millis(40));
                self.now.fetch_sub(1, Ordering::SeqCst);
                Err("Status(404)".to_string())
            }
        }
        let t = Slow {
            now: AtomicUsize::new(0),
            peak: AtomicUsize::new(0),
        };
        let store = Store {
            dir: scratch("two"),
        };
        check_all(&crate::catalog::CATALOG, &t, &store);
        assert_eq!(t.peak.load(Ordering::SeqCst), 2);
    }

    const H: u64 = 3600;

    #[test]
    fn due_follows_the_interval_from_the_last_check() {
        assert!(due(None, 1000, 6, 0));
        assert!(!due(Some(1000), 1000 + 6 * H - 1, 6, 0));
        assert!(due(Some(1000), 1000 + 6 * H, 6, 0));
        assert_eq!(next_due(Some(1000), 6, 0), 1000 + 6 * H);
        assert_eq!(next_due(None, 6, 0), 0);
    }

    #[test]
    fn interval_zero_is_never() {
        assert!(!due(None, 5, 0, 0));
        assert!(!due(Some(0), u64::MAX - 1, 0, 0));
        assert!(!due(Some(1), 1_000_000_000, 0, 7));
        assert_eq!(next_due(Some(10), 0, 0), u64::MAX);
        assert_eq!(next_due(None, 0, 3), u64::MAX);
    }

    #[test]
    fn failures_retry_after_30_minutes_doubling_to_the_interval() {
        assert_eq!(next_due(Some(0), 6, 1), 30 * 60);
        assert_eq!(next_due(Some(0), 6, 2), 60 * 60);
        assert_eq!(next_due(Some(0), 6, 3), 2 * H);
        assert_eq!(next_due(Some(0), 6, 4), 4 * H);
        assert_eq!(next_due(Some(0), 6, 5), 6 * H);
        assert_eq!(next_due(Some(0), 6, 6), 6 * H);
        assert_eq!(next_due(Some(0), 6, u32::MAX), 6 * H);
        // An interval shorter than 30 minutes never waits longer than the interval.
        assert_eq!(next_due(Some(0), 1, 1), 30 * 60);
        assert_eq!(next_due(Some(0), 1, 2), H);
        assert_eq!(next_due(Some(0), 1, 9), H);
        assert!(!due(Some(0), 30 * 60 - 1, 6, 1));
        assert!(due(Some(0), 30 * 60, 6, 1));
        // A huge last value does not overflow.
        assert_eq!(next_due(Some(u64::MAX), 6, 1), u64::MAX);
    }

    #[test]
    fn a_clock_that_went_backwards_is_due() {
        assert!(due(Some(5000), 4000, 6, 0));
        assert!(due(Some(5000), 4000, 6, 3));
        // Not for "never".
        assert!(!due(Some(5000), 4000, 0, 0));
    }

    fn inst(version: Option<&str>, source: Source) -> Installed {
        Installed {
            folder: PathBuf::from("C:\\x"),
            exe: PathBuf::from("C:\\x\\a.exe"),
            version: version.map(ver),
            source,
            kind: String::new(),
        }
    }
    fn found(v: &str) -> Latest {
        Latest::Found(Box::new(manifest_of(sprawl(), v)))
    }

    #[test]
    fn row_state_matrix() {
        let none: Option<&str> = None;
        // Not installed, whatever is known.
        assert_eq!(row_state(None, &found("1.0.0"), none), Row::NotInstalled);
        assert_eq!(row_state(None, &Latest::Unknown, none), Row::NotInstalled);
        let hub = |v| inst(v, Source::Hub);
        // Equal, older, newer installed.
        assert_eq!(
            row_state(Some(&hub(Some("1.0.0"))), &found("1.0.0"), none),
            Row::UpToDate
        );
        assert_eq!(
            row_state(Some(&hub(Some("1.0.0"))), &found("1.0.1"), none),
            Row::Update
        );
        assert_eq!(
            row_state(Some(&hub(Some("2.0.0"))), &found("1.9.9"), none),
            Row::UpToDate
        );
        // A prerelease installed is older than its release.
        assert_eq!(
            row_state(Some(&hub(Some("1.0.0-rc.1"))), &found("1.0.0"), none),
            Row::Update
        );
        // Unknown installed version: offer the update.
        assert_eq!(
            row_state(Some(&hub(None)), &found("1.0.0"), none),
            Row::Update
        );
        // Skipped: only that exact version is hidden.
        assert_eq!(
            row_state(Some(&hub(Some("1.0.0"))), &found("1.1.0"), Some("1.1.0")),
            Row::UpToDate
        );
        assert_eq!(
            row_state(Some(&hub(Some("1.0.0"))), &found("1.2.0"), Some("1.1.0")),
            Row::Update
        );
        assert_eq!(
            row_state(Some(&hub(None)), &found("1.1.0"), Some("1.1.0")),
            Row::UpToDate
        );
        // External until adopted, even with an update available.
        for s in [Source::Uninstall, Source::Folder] {
            assert_eq!(
                row_state(Some(&inst(Some("1.0.0"), s)), &found("1.5.0"), none),
                Row::External
            );
        }
        // No release information.
        assert!(matches!(
            row_state(Some(&hub(Some("1.0.0"))), &Latest::Unknown, none),
            Row::NoInfo(_)
        ));
        assert_eq!(
            row_state(
                Some(&hub(Some("1.0.0"))),
                &Latest::NoManifest("why".into()),
                none
            ),
            Row::NoInfo("why".into())
        );
    }

    #[test]
    fn age_labels() {
        assert_eq!(age_label(None, 100), "Not checked yet");
        assert_eq!(age_label(Some(100), 100), "Checked just now");
        assert_eq!(age_label(Some(100), 159), "Checked just now");
        assert_eq!(age_label(Some(0), 60), "Checked 1 minute ago");
        assert_eq!(age_label(Some(0), 4 * 60 + 5), "Checked 4 minutes ago");
        assert_eq!(age_label(Some(0), 3600), "Checked 1 hour ago");
        assert_eq!(age_label(Some(0), 5 * 3600), "Checked 5 hours ago");
        assert_eq!(age_label(Some(0), 3 * 86_400), "Checked 3 days ago");
        // A clock that went backwards does not underflow.
        assert_eq!(age_label(Some(500), 100), "Checked just now");
    }
}
