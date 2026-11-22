//! Fake Vm states for the pictures and the tests: `hub --shot out.png --demo NAME`.
//! Everything here is made up; nothing reads this PC.
use crate::settings;
use crate::vm::{AppVm, Banner, View, Vm};
use hub_core::catalog::{CATALOG, CatalogApp};
use hub_core::model::Row;

pub const NAMES: &[&str] = &[
    "main",
    "first-run",
    "updating",
    "failed",
    "done",
    "noinfo",
    "running",
    "banner",
    "all",
    "empty",
    "settings-general",
    "settings-updates",
    "settings-about",
];

fn cat(id: &str) -> &'static CatalogApp {
    CATALOG.iter().find(|a| a.id == id).unwrap_or(&CATALOG[0])
}

fn app(id: &str, row: Row, installed: Option<&str>, latest: Option<&str>, running: bool) -> AppVm {
    let c = cat(id);
    let (size, date, notes) = match c.id {
        "mdedit" => (
            "35.2 MB",
            "21 Nov",
            "An option to leave out tabs you barely looked at, and a tidier AI menu. Fixed: in the AI menu, a long saved prompt's name no longer runs under its Modify button.",
        ),
        "mdview" => (
            "34.1 MB",
            "21 Nov",
            "Opens a folder of notes in one window, and remembers where you stopped reading.",
        ),
        "sprawl" => (
            "11.7 MB",
            "20 Nov",
            "Faster scans of large drives, and a cleaner list of what is safe to remove.",
        ),
        _ => (
            "68.7 MB",
            "17 Nov",
            "Resumes interrupted downloads after a restart, and a quieter tray icon.",
        ),
    };
    let file = latest.map_or_else(
        || "not known".to_string(),
        |l| format!("{}-{l}-windows-x86_64.exe", c.id),
    );
    let mut details = vec![
        ("Installs".to_string(), file),
        ("From".to_string(), format!("github.com/Kaan88/{}", c.repo)),
        (
            "Checked with".to_string(),
            "SHA-256 listed in hub.txt".to_string(),
        ),
        ("Into".to_string(), format!("Programs\\{}", c.folder)),
    ];
    if c.id == "mdedit" {
        details.push((
            "Beside it".to_string(),
            "mdview-engine.exe, mdedit-ai.exe".to_string(),
        ));
        details.push((
            "Left alone".to_string(),
            "settings, session, snippets".to_string(),
        ));
    }
    AppVm {
        id: c.id,
        name: c.name,
        blurb: c.blurb,
        glyph: c.glyph,
        tile: c.tile,
        row,
        installed: installed.map(str::to_string),
        latest: latest.map(str::to_string),
        date: if latest.is_some() {
            date.to_string()
        } else {
            String::new()
        },
        size: if latest.is_some() {
            size.to_string()
        } else {
            String::new()
        },
        running,
        notes: notes.to_string(),
        details,
    }
}

fn vm(apps: Vec<AppVm>, sel: usize, view: View) -> Vm {
    let updates = apps.iter().filter(|a| a.row == Row::Update).count();
    Vm {
        apps,
        sel,
        view,
        checked: "Checked 4 minutes ago".into(),
        hub_version: "0.1.0".into(),
        updates,
        ..Vm::default()
    }
}

/// A demo state by name; None for a name that is not in [`NAMES`].
pub fn build(name: &str, view: View) -> Option<Vm> {
    let r = Some;
    Some(match name {
        "main" => vm(
            vec![
                app("mdedit", Row::Update, r("1.2.0"), r("1.2.1"), false),
                app("mdview", Row::UpToDate, r("1.0.4"), r("1.0.4"), false),
                app("sprawl", Row::External, r("1.0.3"), r("1.0.5"), false),
                app("inbound", Row::Update, r("0.8.2"), r("0.9.0"), true),
            ],
            0,
            view,
        ),
        "first-run" => {
            let mut v = vm(
                vec![
                    app("mdedit", Row::NotInstalled, None, r("1.2.1"), false),
                    app("mdview", Row::NotInstalled, None, r("1.0.4"), false),
                    app("sprawl", Row::NotInstalled, None, r("1.0.5"), false),
                    app("inbound", Row::NotInstalled, None, r("0.9.0"), false),
                ],
                0,
                view,
            );
            v.checked = "Checking\u{2026}".into();
            v
        }
        "updating" => {
            let mut v = vm(
                vec![
                    app(
                        "mdedit",
                        Row::Downloading(0.42, "12.4 of 35.2 MB".into()),
                        r("1.2.0"),
                        r("1.2.1"),
                        false,
                    ),
                    app("mdview", Row::Verifying, r("1.0.3"), r("1.0.4"), false),
                    app(
                        "sprawl",
                        Row::Waiting("It is open: close it to continue".into()),
                        r("1.0.3"),
                        r("1.0.5"),
                        true,
                    ),
                    app(
                        "inbound",
                        Row::Failed("The connection was reset after 31.0 of 68.7 MB".into()),
                        r("0.8.2"),
                        r("0.9.0"),
                        false,
                    ),
                ],
                0,
                view,
            );
            v.busy = Some("Updating 3 apps\u{2026}".into());
            v
        }
        "failed" => vm(
            vec![
                app(
                    "mdedit",
                    Row::Failed(
                        "The download did not match its SHA-256 (expected 3f2a9c\u{2026}, got 91bc40\u{2026}); nothing was installed".into(),
                    ),
                    r("1.2.0"),
                    r("1.2.1"),
                    false,
                ),
                app("mdview", Row::UpToDate, r("1.0.4"), r("1.0.4"), false),
                app("sprawl", Row::Update, r("1.0.3"), r("1.0.5"), false),
                app("inbound", Row::NotInstalled, None, r("0.9.0"), false),
            ],
            0,
            view,
        ),
        "done" => vm(
            vec![
                app(
                    "mdedit",
                    Row::Done("just now".into()),
                    r("1.2.1"),
                    r("1.2.1"),
                    false,
                ),
                app("mdview", Row::UpToDate, r("1.0.4"), r("1.0.4"), false),
                app(
                    "sprawl",
                    Row::Done("2 minutes ago".into()),
                    r("1.0.5"),
                    r("1.0.5"),
                    false,
                ),
                app(
                    "inbound",
                    Row::NoInfo("Its latest release has no hub.txt yet".into()),
                    r("0.8.2"),
                    None,
                    false,
                ),
            ],
            0,
            view,
        ),
        "noinfo" => vm(
            vec![
                app(
                    "mdedit",
                    Row::NoInfo("Last check failed: no connection".into()),
                    r("1.2.0"),
                    None,
                    false,
                ),
                app("mdview", Row::UpToDate, r("1.0.4"), r("1.0.4"), false),
                app("sprawl", Row::NotInstalled, None, r("1.0.5"), false),
                app(
                    "inbound",
                    Row::NoInfo("Its latest release has no hub.txt yet".into()),
                    None,
                    None,
                    false,
                ),
            ],
            0,
            view,
        ),
        "running" => vm(
            vec![
                app("mdedit", Row::UpToDate, r("1.2.1"), r("1.2.1"), true),
                app("mdview", Row::UpToDate, r("1.0.4"), r("1.0.4"), false),
                app("sprawl", Row::Update, r("1.0.3"), r("1.0.5"), true),
                app("inbound", Row::UpToDate, r("0.9.0"), r("0.9.0"), true),
            ],
            2,
            view,
        ),
        "banner" => {
            let mut v = build("main", view)?;
            v.banner = Some(Banner {
                // The words the engine gives (`selfupdate::Status::label`).
                text: "Hub 0.2.0 is ready: restart to finish".into(),
                yes: "Restart now".into(),
                no: "Later".into(),
                warn: false,
            });
            v.hub_version = "0.1.0 (0.2.0 ready)".into();
            v.hover = Some(1);
            v
        }
        // The sliders button's card over the main window; the About page with Hub's own update
        // downloaded and its banner showing.
        "settings-general" | "settings-updates" | "settings-about" => {
            let about = name == "settings-about";
            let mut v = build(if about { "banner" } else { "main" }, view)?;
            let m = settings::Model {
                start: true,
                tray: true,
                notify: true,
                interval_hours: 6,
                version: "0.1.0".into(),
                hub_status: if about {
                    "Hub 0.2.0 is ready: restart to finish".into()
                } else {
                    "No update found at the last check".into()
                },
                hub_button: if about { "Restart now" } else { "Update Hub" },
            };
            let page = match name {
                "settings-updates" => 1,
                "settings-about" => 2,
                _ => 0,
            };
            v.settings = Some((settings::rows(&m), page));
            v
        }
        "all" => {
            let rows = [
                (Row::NotInstalled, None, r("1.2.1"), false),
                (Row::UpToDate, r("1.0.4"), r("1.0.4"), true),
                (Row::Update, r("1.0.3"), r("1.0.5"), false),
                (Row::External, r("0.8.2"), r("0.9.0"), false),
                (
                    Row::Downloading(0.7, "24.6 of 35.2 MB".into()),
                    r("1.2.0"),
                    r("1.2.1"),
                    false,
                ),
                (Row::Verifying, r("1.0.3"), r("1.0.4"), false),
                (
                    Row::Waiting("It is open: close it to continue".into()),
                    r("1.0.3"),
                    r("1.0.5"),
                    true,
                ),
                (
                    Row::Failed("The connection was reset".into()),
                    r("0.8.2"),
                    r("0.9.0"),
                    false,
                ),
                (Row::Done("just now".into()), r("1.2.1"), r("1.2.1"), false),
                (
                    Row::NoInfo("No release information".into()),
                    r("1.0.3"),
                    None,
                    false,
                ),
            ];
            let apps = rows
                .into_iter()
                .enumerate()
                .map(|(i, (row, inst, lat, run))| {
                    app(CATALOG[i % CATALOG.len()].id, row, inst, lat, run)
                })
                .collect();
            vm(apps, 2, view)
        }
        "empty" => vm(Vec::new(), 0, view),
        _ => return None,
    })
}
