//! Fake Vm states for the pictures and the tests: `hub --shot out.png --demo NAME`.
//! Everything here is made up; nothing reads this PC.
use crate::glue::{MenuItem, Phase, Slot, app_vm, close_question};
use crate::settings;
use crate::vm::{
    Act, AppVm, Banner, Hit, Layout, NotesState, NotesVm, OfferKey, Pointer, View, Vm,
};
use hub_core::catalog::{CATALOG, CatalogApp};
use hub_core::model::{Latest, Row};

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
    "claude-switch",
    "empty",
    "settings-general",
    "settings-updates",
    "settings-about",
    "settings-interval-menu",
    "row-menu",
    "small-install-menu",
    "long-versions",
    "notes",
    "notes-offer",
    "notes-loading",
    "notes-failed",
    "notes-nofile",
    "notes-long",
    "notes-bar-active",
    "notes-focus-back",
    "notes-focus-retry",
    "notes-focus-page",
    "hover-close",
    "hover-min",
    "hover-bar",
    "hover-primary",
    "pressed-primary",
    "hover-open",
    "hover-menu",
    "close-question",
    "close-question-yes",
    "close-question-no",
    "backdrop-mica",
    "backdrop-acrylic",
    "filter-results",
    "filter-edit",
    "filter-no-matches",
    "narrow-columns",
    "maximized-chrome",
    "narrow-long-notice",
];

/// The window backdrop a demo's picture is drawn with: the main window with its chrome keyed
/// out and laid over a stand-in for what the system would show (`frost::preview`).
pub fn backdrop(name: &str) -> mdshared_app::backdrop::Backdrop {
    use mdshared_app::backdrop::Backdrop;
    match name {
        "backdrop-mica" => Backdrop::Mica,
        "backdrop-acrylic" => Backdrop::Acrylic,
        _ => Backdrop::Off,
    }
}

/// Demo overlays are pictures, not input models. Draw through the same paths as a live
/// overlay but force the settled endpoint regardless of the user's system settings.
pub fn draw_overlays(
    p: &mut mdshared_ui::gfx::Painter,
    pal: &mdshared_ui::theme::Palette,
    area: mdshared_ui::gfx::Rect,
    vm: &Vm,
    lay: &Layout,
) {
    if vm.demo_menu
        && let Some((anchor, _)) = lay.hits.iter().find(|(_, h)| *h == Hit::Menu(0))
        && let Some(app) = vm.apps.first()
    {
        let installed = app.installed.is_some();
        let items = vec![
            (MenuItem::Open, "Open", installed),
            (MenuItem::ShowInFolder, "Show in folder", installed),
            (MenuItem::ReleaseNotes, "Release notes", true),
            (MenuItem::SkipVersion, "Skip this version", false),
            (MenuItem::GitHub, "GitHub", true),
        ];
        let items = if vm.view == View::Small {
            crate::glue::small_menu_items(app, items)
        } else {
            items
        };
        crate::popup::Popup::new(app.id, *anchor, vm.view, items).draw(
            p,
            pal,
            area,
            1.0,
            &mut crate::motion::still(),
        );
    }
    if let Some((rows, page)) = &vm.settings {
        let mut card = settings::Card::new(rows.clone(), *page);
        if vm.demo_dropdown {
            card.layout(p, area);
            card.open_dropdown(settings::INTERVAL);
        }
        card.draw_motion(p, pal, area, 1.0, &mut crate::motion::still());
    }
}

/// What a release notes file of mdedit 1.2.1 could look like: every construct the page reads.
const SAMPLE_NOTES: &str = "# mdedit 1.2.1

An option to leave out tabs you barely looked at, and a **tidier AI menu**. This release is mostly *small* things that add up, and one fix people asked for.

## What is new

- Close tabs you barely looked at: **Settings > Tabs > Skip quick looks**.
- The AI menu lists *saved prompts* first, then the commands. A long prompt's name no longer runs under its `Modify` button.
- Find in files remembers the last folder.
  - It also remembers the file filter, such as `*.rs`.
  - Use [Reset filters](https://github.com/Kaan88/markdown-editor-rs) to clear both.

## Fixed

1. Opening a file with a very long first line no longer takes a second to draw.
2. Undo after a replace in all tabs puts every tab back.
3. The status bar no longer flickers while a large file loads.

### Settings that changed

The default of `tabs.skip_quick` is now off. To turn it on from a settings file:

```ini
[tabs]
skip_quick = 1
```

---

Thanks to everyone who sent a report. Full history: github.com/Kaan88/markdown-editor-rs/releases
";

fn long_notes() -> String {
    let mut t = String::from(SAMPLE_NOTES);
    for (v, d) in [
        ("1.2.0", "14 Nov"),
        ("1.1.4", "02 Nov"),
        ("1.1.3", "28 Oct"),
    ] {
        t.push_str(&format!(
            "
## mdedit {v}  ({d})

"
        ));
        for i in 1..=6 {
            t.push_str(&format!(
                "- Item {i} of {v}: the editor now keeps the caret column when a line is moved, and a replace across files no longer asks twice about files that did not change.
"
            ));
        }
        t.push_str("
A paragraph about this release, with `inline code`, **bold words** and *soft words*, long enough to wrap onto a second line in the narrow window.
");
    }
    t.push_str("
https://github.com/Kaan88/markdown-editor-rs/releases/download/v1.2.1/mdedit-1.2.1-windows-x86_64-setup-with-a-very-long-name.exe
");
    t
}

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
        "purge" => (
            "43.7 MB",
            "23 Nov",
            "Uninstalls programs and the leftovers they leave behind.",
        ),
        "barbican" => (
            "9.8 MB",
            "22 Nov",
            "Mail, calendar, contacts and files in one tray app, with a faster first sync.",
        ),
        "claude-switch" => (
            "5.6 MB",
            "26 Nov",
            "Open Claude Code sessions in terminal tabs.",
        ),
        _ => (
            "68.7 MB",
            "17 Nov",
            "Resumes interrupted downloads after a restart, and a quieter tray icon.",
        ),
    };
    let file = latest.map_or_else(
        || "not known".to_string(),
        |l| {
            if c.id == "claude-switch" {
                "claude-switch-setup.exe".to_string()
            } else {
                format!("{}-{l}-windows-x86_64.exe", c.id)
            }
        },
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
        maximized: false,
        ..Vm::default()
    }
}

/// A demo state by name; None for a name that is not in [`NAMES`].
pub fn build(name: &str, view: View) -> Option<Vm> {
    let r = Some;
    Some(match name {
        "backdrop-mica" | "backdrop-acrylic" => build("main", view)?,
        "row-menu" | "small-install-menu" => {
            let mut v = build(if name == "row-menu" { "main" } else { "first-run" }, view)?;
            v.demo_menu = true;
            v
        }
        "long-versions" => {
            let mut v = build("main", view)?;
            v.apps[0].installed = Some("1.2.0-rc.20261126.windows.1".into());
            v.apps[0].latest = Some("1.2.1-rc.20261126.windows.2".into());
            v
        }
        "notes-offer" => {
            let mut v = build("notes", view)?;
            v.banner = Some(close_question("mdedit", app_offer("mdedit", "1.2.1")));
            v
        }
        "notes-focus-back" | "notes-focus-retry" | "notes-focus-page" => {
            let mut v = build("notes-failed", view)?;
            v.notes_focus = match name {
                "notes-focus-back" => Hit::NotesBack,
                "notes-focus-retry" => Hit::NotesRetry,
                _ => Hit::NotesPage,
            };
            v
        }
        "filter-results" | "filter-edit" | "filter-no-matches" => {
            let mut v = build("main", view)?;
            v.filter = if name == "filter-no-matches" { "nothing matches" } else { "mdedit" }.into();
            v.apps.retain(|a| a.id == v.filter);
            v.filter_focus = name == "filter-edit";
            v
        }
        "narrow-columns" => build("claude-switch", view)?,
        "maximized-chrome" => {
            let mut v = build("main", view)?;
            v.maximized = true;
            v
        }
        "narrow-long-notice" => {
            let mut v = build("main", view)?;
            v.checked = "Last check failed: GitHub could not be reached. Hub kept saved release information so you can try again later.".into();
            v.hub_version = "0.1.5 (0.2.0 ready)".into();
            v
        }
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
                key: OfferKey { app: None, version: "0.2.0".into(), serial: 0 },
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
        "settings-general" | "settings-updates" | "settings-about" | "settings-interval-menu" => {
            let about = name == "settings-about";
            let mut v = build(if about { "banner" } else { "main" }, view)?;
            let m = settings::Model {
                start: true,
                tray: true,
                notify: true,
                interval_hours: 6,
                backdrop: mdshared_app::backdrop::Backdrop::Off,
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
            v.demo_dropdown = name == "settings-interval-menu";
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
        "claude-switch" => {
            let mut v = build("main", view)?;
            v.apps.extend([
                app("purge", Row::NotInstalled, None, r("0.1.0"), false),
                unreleased("barbican"),
                unreleased("claude-switch"),
                unreleased("sozluk"),
            ]);
            v.sel = v.apps.len() - 1;
            v
        }
        "empty" => vm(Vec::new(), 0, view),
        // The release notes view of mdedit (1.2.0 installed, 1.2.1 the latest): loaded, still
        // loading, failed (the one line stays), a release with no notes file, a long one, and the long
        // one with its scrollbar active (the pointer on it).
        "notes" | "notes-loading" | "notes-failed" | "notes-nofile" | "notes-long"
        | "notes-bar-active" => {
            let mut v = build("main", view)?;
            let one = v.apps.first().map(|a| a.notes.clone()).unwrap_or_default();
            v.notes = Some(NotesVm {
                id: "mdedit",
                name: "mdedit",
                version: "1.2.1".into(),
                installed: Some("1.2.0".into()),
                date: "21 Nov".into(),
                one_liner: one,
                state: match name {
                    "notes-loading" => NotesState::Loading,
                    "notes-failed" => NotesState::Failed(
                        "Could not download the notes: connection timed out".into(),
                    ),
                    "notes-nofile" => NotesState::NoFile,
                    "notes-long" | "notes-bar-active" => NotesState::Loaded(long_notes()),
                    _ => NotesState::Loaded(SAMPLE_NOTES.to_string()),
                },
                // The long notes with the pointer on the scrollbar (or dragging its thumb).
                bar_active: name == "notes-bar-active",
            });
            v
        }
        // The main window with the pointer over one control: the window's close button (red),
        // its minimize button, "Check now", "Update all" (and that one held down), a row's Open
        // button and its "..." menu (the row under them lit as well).
        "hover-close" | "hover-min" | "hover-bar" | "hover-primary" | "pressed-primary"
        | "hover-open" | "hover-menu" => {
            let mut v = build("main", view)?;
            let hot = match name {
                "hover-close" => Hit::Close,
                "hover-min" => Hit::Min,
                "hover-bar" => Hit::CheckNow,
                "hover-primary" | "pressed-primary" => Hit::UpdateAll,
                "hover-open" => Hit::Action(1, Act::Open),
                _ => Hit::Menu(1),
            };
            if matches!(hot, Hit::Action(..) | Hit::Menu(_)) {
                v.hover = Some(1);
            }
            v.pointer = Pointer {
                hot,
                held: if name == "pressed-primary" {
                    hot
                } else {
                    Hit::None
                },
            };
            v
        }
        // The question before an open app is closed for its update (Inbound is running in
        // "main"): the bar alone, then with the pointer on its yes and on its no.
        "close-question" | "close-question-yes" | "close-question-no" => {
            let mut v = build("main", view)?;
            v.banner = Some(close_question("Inbound", app_offer("inbound", "0.9.0")));
            v.sel = 3;
            v.pointer.hot = match name {
                "close-question-yes" => Hit::BannerYes,
                "close-question-no" => Hit::BannerNo,
                _ => Hit::None,
            };
            v
        }
        _ => return None,
    })
}

fn app_offer(id: &'static str, version: &str) -> OfferKey {
    OfferKey {
        app: Some(id),
        version: version.into(),
        serial: 0,
    }
}

fn unreleased(id: &str) -> AppVm {
    app_vm(
        &Slot {
            app: cat(id),
            installed: None,
            latest: Latest::NoManifest("No release information".into()),
            skipped: None,
            running: false,
            phase: Phase::Idle,
        },
        0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_switch_demo_shows_the_full_catalog_and_selects_the_last_unreleased_app() {
        let vm = build("claude-switch", View::List).unwrap();
        let ids: Vec<_> = vm.apps.iter().map(|app| app.id).collect();
        assert_eq!(ids, CATALOG.map(|app| app.id));
        let selected = &vm.apps[vm.sel];
        assert_eq!(selected.id, "sozluk");
        assert!(matches!(selected.row, Row::NoInfo(_)));
        assert!(selected.installed.is_none() && selected.latest.is_none());
        assert!(selected.notes.is_empty());
        assert!(!selected.details.iter().any(|(key, _)| key == "Installs"));
    }
}
