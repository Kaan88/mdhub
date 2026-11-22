//! A measurement, not a behaviour: the transient private memory of installing a 70 MB setup
//! through an in-memory transport. Run by hand:
//! `cargo test -p hub-core --release --test measure_install_memory -- --ignored --nocapture`
#![cfg(windows)]
use hub_core::catalog::CATALOG;
use hub_core::install::{Elevation, Opts, Progress, SetupRunner, install};
use hub_core::model::{
    FileSpec, HubManifest, Installed, Kind, PlatformEntry, Scope, SetupSpec, Source,
};
use hub_core::state::Store;
use hub_core::transport::Transport;
use mdshared_app::update::{Version, sha256_hex};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use windows_sys::Win32::System::ProcessStatus::{
    K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS, PROCESS_MEMORY_COUNTERS_EX,
};
use windows_sys::Win32::System::Threading::GetCurrentProcess;

fn mem() -> (f64, f64) {
    let mut c: PROCESS_MEMORY_COUNTERS_EX = unsafe { std::mem::zeroed() };
    c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS_EX>() as u32;
    unsafe {
        K32GetProcessMemoryInfo(
            GetCurrentProcess(),
            (&mut c as *mut PROCESS_MEMORY_COUNTERS_EX).cast::<PROCESS_MEMORY_COUNTERS>(),
            c.cb,
        );
    }
    (
        c.PrivateUsage as f64 / 1048576.0,
        c.PeakPagefileUsage as f64 / 1048576.0,
    )
}

struct Mem(Arc<Vec<u8>>);
impl Transport for Mem {
    fn get(&self, _u: &str, _m: usize, _t: Duration) -> Result<Vec<u8>, String> {
        Ok((*self.0).clone())
    }
}
struct Runner(PathBuf);
impl SetupRunner for Runner {
    fn run(&self, _e: &Path, _a: &str, dir: &Path, _s: Scope) -> Result<u32, String> {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        std::fs::write(dir.join(&self.0), b"x").map_err(|e| e.to_string())?;
        Ok(0)
    }
}

#[test]
#[ignore = "a measurement"]
fn install_of_a_70_mb_setup() {
    let root = std::env::temp_dir().join(format!("hub-measure-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let app = &CATALOG[0];
    let body: Arc<Vec<u8>> = Arc::new((0..70 * 1024 * 1024u32).map(|i| (i % 251) as u8).collect());
    let file = FileSpec {
        name: "setup.exe".into(),
        url: "https://x/setup.exe".into(),
        sha256: sha256_hex(&body),
        size: body.len() as u64,
    };
    let entry = PlatformEntry {
        platform: "windows-x86_64".into(),
        kind: Kind::Setup,
        exe: Some(app.exe.into()),
        files: vec![],
        zip: None,
        setup: Some(SetupSpec {
            file,
            silent: "/S".into(),
            scope: Scope::User,
        }),
        uninstall: None,
        min: None,
        wait: vec![],
        updates_self: false,
    };
    let m = HubManifest {
        app: app.id.into(),
        version: Version::parse("9.0.0").unwrap(),
        date: "2026-11-22".into(),
        notes: String::new(),
        notes_file: None,
        platforms: vec![entry],
    };
    let programs = root.join("Programs");
    let folder = programs.join(app.folder);
    let (f2, exe) = (folder.clone(), folder.join(app.exe));
    let opts = Opts {
        elevate: Elevation::Custom(Arc::new(Runner(PathBuf::from(app.exe)))),
        programs: Some(programs),
        redetect: Some(Arc::new(move |_a, _s| {
            Some(Installed {
                folder: f2.clone(),
                exe: exe.clone(),
                version: Version::parse("9.0.0").ok(),
                source: Source::Hub,
                kind: "setup".into(),
            })
        })),
        ..Opts::default()
    };
    let store = Store {
        dir: root.join("hub"),
    };
    let t = Mem(body.clone());
    let (before, _) = mem();
    let peak = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let (p2, stop) = (
        peak.clone(),
        Arc::new(std::sync::atomic::AtomicBool::new(false)),
    );
    let s2 = stop.clone();
    let h = std::thread::spawn(move || {
        while !s2.load(std::sync::atomic::Ordering::Relaxed) {
            p2.fetch_max(
                (mem().0 * 100.0) as u64,
                std::sync::atomic::Ordering::Relaxed,
            );
            std::thread::sleep(Duration::from_millis(1));
        }
    });
    let r = install(app, &m, None, &store, &t, &opts, &mut |_p: Progress| {});
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    h.join().unwrap();
    let (after, peak_commit) = mem();
    println!(
        "install ok={} private before {before:.1} MiB (incl. the 70 MiB test body) sampled peak {:.1} MiB after {after:.1} MiB peak commit {peak_commit:.1} MiB",
        r.is_ok(),
        peak.load(std::sync::atomic::Ordering::Relaxed) as f64 / 100.0
    );
    let _ = std::fs::remove_dir_all(&root);
}
