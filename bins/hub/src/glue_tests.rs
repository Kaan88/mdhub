//! The pure parts of `glue.rs` and `cli.rs`: the row of every phase, the details and sizes, a hit
//! turned into a command, keys, the schedule, the filter and the selection.
use crate::cli;
use crate::e2e_tests::{Fx, body, ver};
use crate::glue::{
    Cmd, Key, MenuItem, Phase, Slot, act_cmd, app_vm, check_wait, command, fail_text, fmt_date,
    fmt_size, key_command, row_of,
};
use crate::vm::{Act, Hit, Target, View};
use hub_core::catalog::find;
use hub_core::check;
use hub_core::install::InstallError;
use hub_core::model::{
    FileSpec, HubManifest, Installed, Kind, Latest, PlatformEntry, Row, Scope, SetupSpec, Source,
};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::Duration;

fn manifest(version: &str, size: u64) -> HubManifest {
    HubManifest {
        app: "mdedit".into(),
        version: ver(version),
        date: "2026-11-21".into(),
        notes: "A note.".into(),
        notes_file: None,
        platforms: vec![PlatformEntry {
            platform: "windows-x86_64".into(),
            kind: Kind::Files,
            exe: Some("mdedit.exe".into()),
            files: vec![
                FileSpec {
                    name: "mdedit.exe".into(),
                    url: "https://github.com/Kaan88/markdown-editor-rs/releases/download/v1/mdedit.exe".into(),
                    sha256: "0".repeat(64),
                    size,
                },
                FileSpec {
                    name: "mdview-engine.exe".into(),
                    url: "https://github.com/Kaan88/markdown-editor-rs/releases/download/v1/mdview-engine.exe".into(),
                    sha256: "1".repeat(64),
                    size: 100_000,
                },
            ],
            zip: None,
            setup: None,
            uninstall: None,
            min: None,
            wait: vec![],
            updates_self: false,
        }],
    }
}

fn slot(installed: Option<&str>, latest: Latest, phase: Phase) -> Slot {
    Slot {
        app: find("mdedit").unwrap(),
        installed: installed.map(|v| Installed {
            folder: PathBuf::from(r"C:\Apps\mdedit"),
            exe: PathBuf::from(r"C:\Apps\mdedit\mdedit.exe"),
            version: Some(ver(v)),
            source: Source::Hub,
            kind: "files".into(),
        }),
        latest,
        skipped: None,
        running: false,
        phase,
    }
}

fn found(v: &str) -> Latest {
    Latest::Found(Box::new(manifest(v, 35_000_000)))
}

#[test]
fn every_phase_has_its_row() {
    let now = 10_000;
    let idle = |i, l| row_of(&slot(i, l, Phase::Idle), now);
    assert_eq!(
        idle(None, Latest::Unknown),
        Row::NoInfo("Not checked yet".into())
    );
    assert_eq!(
        idle(
            None,
            Latest::NoManifest("no hub.txt in the latest release".into())
        ),
        Row::NoInfo("no hub.txt in the latest release".into())
    );
    assert_eq!(idle(None, found("1.0.0")), Row::NotInstalled);
    assert_eq!(idle(Some("1.0.0"), found("1.0.0")), Row::UpToDate);
    assert_eq!(idle(Some("1.0.0"), found("1.1.0")), Row::Update);
    assert!(matches!(
        idle(Some("1.0.0"), Latest::Unknown),
        Row::NoInfo(_)
    ));
    let mut ext = slot(Some("1.0.0"), found("1.1.0"), Phase::Idle);
    ext.installed.as_mut().unwrap().source = Source::Folder;
    assert_eq!(row_of(&ext, now), Row::External);
    let phase = |p| row_of(&slot(Some("1.0.0"), found("1.1.0"), p), now);
    assert!(matches!(phase(Phase::Queued), Row::Waiting(_)));
    assert_eq!(
        phase(Phase::Downloading {
            frac: 0.5,
            text: "1.0 of 2.0 MB".into()
        }),
        Row::Downloading(0.5, "1.0 of 2.0 MB".into())
    );
    assert_eq!(phase(Phase::Verifying), Row::Verifying);
    assert!(matches!(phase(Phase::Placing), Row::Downloading(f, _) if f == 1.0));
    assert_eq!(phase(Phase::Waiting("x".into())), Row::Waiting("x".into()));
    assert_eq!(
        phase(Phase::Failed("why".into())),
        Row::Failed("why".into())
    );
    assert_eq!(phase(Phase::Done(now)), Row::Done("just now".into()));
    assert_eq!(
        phase(Phase::Done(now - 120)),
        Row::Done("2 minutes ago".into())
    );
    assert_eq!(
        phase(Phase::Done(now - 60)),
        Row::Done("1 minute ago".into())
    );
    assert_eq!(
        phase(Phase::Done(now - 7200)),
        Row::Done("2 hours ago".into())
    );
}

#[test]
fn a_skipped_version_is_up_to_date_until_a_newer_one() {
    let mut s = slot(Some("1.0.0"), found("1.1.0"), Phase::Idle);
    s.skipped = Some("1.1.0".into());
    assert_eq!(row_of(&s, 0), Row::UpToDate);
    s.latest = found("1.2.0");
    assert_eq!(row_of(&s, 0), Row::Update);
}

#[test]
fn the_app_line_carries_the_release() {
    let s = slot(Some("1.0.0"), found("1.1.0"), Phase::Idle);
    let a = app_vm(&s, 0);
    assert_eq!(a.installed.as_deref(), Some("1.0.0"));
    assert_eq!(a.latest.as_deref(), Some("1.1.0"));
    // The two files together: 35.0 MB and 0.1 MB.
    assert_eq!(a.size, "35.1 MB");
    assert_eq!(a.date, "21 Nov");
    assert_eq!(a.notes, "A note.");
    let d: Vec<(&str, &str)> = a
        .details
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
    assert_eq!(d[0], ("Installs", "mdedit.exe, mdview-engine.exe"));
    assert_eq!(d[1], ("From", "github.com/Kaan88/markdown-editor-rs"));
    assert_eq!(d[2], ("Checked with", "SHA-256 listed in hub.txt"));
    assert_eq!(d[3], ("Into", r"C:\Apps\mdedit"));
    assert_eq!(d[4], ("Beside it", "mdedit-ai.exe, mdview-engine.exe"));
    assert_eq!(d[5].0, "Left alone");
    // Not installed: where it would go; not known: no release lines.
    let n = app_vm(&slot(None, Latest::Unknown, Phase::Idle), 0);
    assert_eq!(n.latest, None);
    assert_eq!(n.size, "");
    assert!(
        n.details
            .iter()
            .any(|(k, v)| k == "Into" && v == r"Programs\mdedit")
    );
    assert!(!n.details.iter().any(|(k, _)| k == "Checked with"));
}

#[test]
fn a_setup_release_shows_the_installer_and_where_a_machine_install_goes() {
    let mut m = manifest("1.0.0", 0);
    let e = &mut m.platforms[0];
    e.kind = Kind::Setup;
    e.files.clear();
    e.setup = Some(SetupSpec {
        file: FileSpec {
            name: "sprawl-setup.exe".into(),
            url: String::new(),
            sha256: "0".repeat(64),
            size: 11_700_000,
        },
        silent: "/VERYSILENT".into(),
        scope: Scope::Machine,
    });
    let mut s = slot(None, Latest::Found(Box::new(m)), Phase::Idle);
    s.app = find("sprawl").unwrap();
    let a = app_vm(&s, 0);
    assert_eq!(a.size, "11.7 MB");
    assert!(
        a.details
            .iter()
            .any(|(k, v)| k == "Installs" && v == "sprawl-setup.exe")
    );
    assert!(
        a.details
            .iter()
            .any(|(k, v)| k == "Into" && v == r"Program Files\Sprawl")
    );
}

#[test]
fn sizes_dates_and_failure_words() {
    assert_eq!(fmt_size(0), "");
    assert_eq!(fmt_size(1), "1 KB");
    assert_eq!(fmt_size(999_999), "1000 KB");
    assert_eq!(fmt_size(35_200_000), "35.2 MB");
    assert_eq!(fmt_date("2026-11-21"), "21 Nov");
    assert_eq!(fmt_date("2026-01-05"), "5 Jan");
    assert_eq!(fmt_date("garbage"), "garbage");
    assert_eq!(fmt_date("2026-13-01"), "2026-13-01");
    let t = fail_text(&InstallError::Mismatch {
        file: "x".into(),
        what: "sha256",
        expected: "a".repeat(64),
        received: "b".repeat(64),
    });
    assert_eq!(
        t,
        "The download did not match its SHA-256 (expected aaaaaa\u{2026}, got bbbbbb\u{2026}); nothing was installed"
    );
    assert_eq!(
        fail_text(&InstallError::Running("mdedit.exe".into())),
        "mdedit.exe is still open: close it and try again"
    );
    assert!(
        fail_text(&InstallError::Setup {
            code: Some(3),
            reason: "s.exe".into()
        })
        .starts_with("The installer exited with code 3")
    );
    assert!(
        fail_text(&InstallError::Io("x".repeat(1000)))
            .chars()
            .count()
            <= 300
    );
}

#[test]
fn a_hit_becomes_the_command_for_the_app_in_that_row_as_shown() {
    let fx = Fx::new("hits");
    let vm = fx.vm();
    let id = |i: usize| vm.apps[i].id;
    assert_eq!(command(Hit::Row(1), &vm), Some(Cmd::Select(1)));
    assert_eq!(command(Hit::Menu(2), &vm), Some(Cmd::OpenMenu(2)));
    for (act, want) in [
        (Act::Open, Cmd::Open(id(3))),
        (Act::Install, Cmd::Install(id(3))),
        (Act::Update, Cmd::Update(id(3))),
        (Act::Adopt, Cmd::Adopt(id(3))),
        (Act::Cancel, Cmd::Cancel(id(3))),
        (Act::TryAgain, Cmd::TryAgain(id(3))),
    ] {
        assert_eq!(command(Hit::Action(3, act), &vm), Some(want), "{act:?}");
        assert_eq!(act_cmd(id(3), act), want);
    }
    assert_eq!(command(Hit::UpdateAll, &vm), Some(Cmd::UpdateAll));
    assert_eq!(command(Hit::CheckNow, &vm), Some(Cmd::CheckNow));
    assert_eq!(command(Hit::CancelAll, &vm), Some(Cmd::CancelAll));
    assert_eq!(
        command(Hit::ViewSwitch(View::Small), &vm),
        Some(Cmd::SetView(View::Small))
    );
    assert_eq!(command(Hit::Filter, &vm), Some(Cmd::FocusFilter));
    assert_eq!(command(Hit::FilterOpen, &vm), Some(Cmd::FocusFilter));
    assert_eq!(command(Hit::FilterClear, &vm), Some(Cmd::FilterClear));
    let offer = crate::demo::build("close-question", View::List).unwrap();
    assert_eq!(command(Hit::BannerYes, &offer), Some(Cmd::BannerYes));
    assert_eq!(command(Hit::BannerNo, &offer), Some(Cmd::BannerNo));
    assert_eq!(command(Hit::BannerClose, &offer), Some(Cmd::BannerNo));
    assert_eq!(command(Hit::BannerYes, &vm), None, "there is no offer");
    // The window's own hits are not commands, and a row that is gone is not an app.
    for h in [Hit::Drag, Hit::Min, Hit::Max, Hit::Close, Hit::None] {
        assert_eq!(command(h, &vm), None);
    }
    assert_eq!(command(Hit::Row(99), &vm), None);
    assert_eq!(command(Hit::Action(99, Act::Open), &vm), None);
}

#[test]
fn a_row_of_a_filtered_list_is_the_app_that_is_shown_not_the_catalog_position() {
    let mut fx = Fx::new("filtered");
    for c in "spr".chars() {
        fx.cmd(Cmd::FilterChar(c));
    }
    let vm = fx.vm();
    assert_eq!(vm.apps.len(), 1);
    assert_eq!(vm.apps[0].id, "sprawl");
    assert_eq!(
        command(Hit::Action(0, Act::Install), &vm),
        Some(Cmd::Install("sprawl"))
    );
}

#[test]
fn keys_map_to_commands() {
    let k = key_command;
    assert_eq!(
        k(false, Key::Down, false, false, 1),
        Some(Cmd::SelectRel(1))
    );
    assert_eq!(k(false, Key::Up, false, false, 1), Some(Cmd::SelectRel(-1)));
    assert_eq!(
        k(false, Key::Down, false, false, 3),
        Some(Cmd::SelectRel(3))
    );
    assert_eq!(
        k(false, Key::Right, false, false, 3),
        Some(Cmd::SelectRel(1))
    );
    assert_eq!(
        k(false, Key::Right, false, false, 1),
        None,
        "the list has no sideways"
    );
    assert_eq!(k(false, Key::Tab, false, false, 1), Some(Cmd::SelectRel(1)));
    assert_eq!(k(false, Key::Tab, false, true, 1), Some(Cmd::SelectRel(-1)));
    assert_eq!(k(false, Key::Enter, false, false, 1), Some(Cmd::Primary));
    assert_eq!(k(false, Key::Esc, false, false, 1), Some(Cmd::Esc));
    assert_eq!(k(false, Key::F5, false, false, 1), Some(Cmd::CheckNow));
    assert_eq!(
        k(false, Key::Char('f'), true, false, 1),
        Some(Cmd::FocusFilter)
    );
    assert_eq!(
        k(false, Key::Char('/'), false, false, 1),
        Some(Cmd::FocusFilter)
    );
    assert_eq!(k(false, Key::Char('x'), false, false, 1), None);
    // In the filter box, characters are typed; the same keys do not move the selection.
    assert_eq!(
        k(true, Key::Char('x'), false, false, 1),
        Some(Cmd::FilterChar('x'))
    );
    assert_eq!(
        k(true, Key::Char('/'), false, false, 1),
        Some(Cmd::FilterChar('/'))
    );
    assert_eq!(
        k(true, Key::Backspace, false, false, 1),
        Some(Cmd::FilterBack)
    );
    assert_eq!(k(true, Key::Enter, false, false, 1), Some(Cmd::FilterBlur));
    assert_eq!(k(true, Key::Esc, false, false, 1), Some(Cmd::FilterEscape));
    assert_eq!(k(true, Key::Up, false, false, 1), None);
}

#[test]
fn the_filter_selection_and_escape() {
    let mut fx = Fx::new("keys");
    let n = hub_core::catalog::CATALOG.len();
    assert_eq!(fx.vm().apps.len(), n);
    assert_eq!(fx.vm().sel, 0);
    // Arrow keys stop at the ends.
    fx.cmd(Cmd::SelectRel(1));
    fx.cmd(Cmd::SelectRel(1));
    assert_eq!(fx.vm().sel, 2);
    fx.cmd(Cmd::SelectRel(10));
    assert_eq!(fx.vm().sel, n - 1);
    fx.cmd(Cmd::SelectRel(-10));
    assert_eq!(fx.vm().sel, 0);
    // Typing filters by name and id, any case; the selection starts again at the top.
    fx.cmd(Cmd::SelectRel(2));
    fx.cmd(Cmd::FocusFilter);
    assert!(fx.vm().filter_focus);
    for c in "MD".chars() {
        fx.cmd(Cmd::FilterChar(c));
    }
    let names: Vec<&str> = fx.vm().apps.iter().map(|a| a.name).collect();
    assert_eq!(names, ["mdedit", "mdview"]);
    assert_eq!(fx.vm().sel, 0);
    fx.cmd(Cmd::FilterChar('v'));
    assert_eq!(fx.vm().apps.len(), 1);
    fx.cmd(Cmd::FilterBack);
    assert_eq!(fx.vm().apps.len(), 2);
    // Nothing matches: an empty list, and Enter does nothing.
    fx.cmd(Cmd::FilterChar('z'));
    assert!(fx.vm().apps.is_empty());
    assert!(!fx.eng.apply(Cmd::Primary));
    // Esc clears the text; a second Esc leaves the box.
    fx.cmd(Cmd::Esc);
    assert_eq!(fx.vm().filter, "");
    assert_eq!(fx.vm().apps.len(), n);
    assert!(fx.vm().filter_focus);
    fx.cmd(Cmd::Esc);
    assert!(!fx.vm().filter_focus);
    // A click on a row leaves the box too.
    fx.cmd(Cmd::FocusFilter);
    fx.cmd(Cmd::Select(1));
    assert!(!fx.vm().filter_focus);
}

#[test]
fn enter_runs_the_main_button_of_the_selected_row() {
    let mut fx = Fx::new("enter");
    fx.publish_files("mdedit", "1.0.0", &[("mdedit.exe", body("a"))], None);
    fx.check();
    // A fresh window's first row is lit, but nothing was chosen: Enter does nothing yet.
    fx.cmd(Cmd::Primary);
    assert!(!fx.eng.busy(), "a stray Enter installs nothing");
    // mdedit is chosen and not installed: its main button is Install.
    fx.cmd(Cmd::Select(0));
    fx.cmd(Cmd::Primary);
    assert!(fx.eng.busy());
    fx.settle();
    assert_eq!(fx.file("mdedit", "mdedit.exe"), Some(body("a")));
    // Now Open is the main button.
    fx.cmd(Cmd::Primary);
    assert_eq!(*fx.shell.launched.lock().unwrap(), ["mdedit"]);
}

#[test]
fn the_view_is_remembered_in_the_settings() {
    let mut fx = Fx::new("view");
    assert_eq!(fx.vm().view, View::List);
    fx.cmd(Cmd::SetView(View::Large));
    assert_eq!(fx.vm().view, View::Large);
    fx.restart();
    assert_eq!(fx.vm().view, View::Large, "the next start has it");
}

#[test]
fn cancel_takes_a_queued_install_out_and_stops_a_running_one_before_it_places_anything() {
    let mut fx = Fx::with_delay("cancel", Duration::from_millis(150));
    fx.publish_files("mdedit", "1.0.0", &[("mdedit.exe", body("a"))], None);
    fx.publish_files("mdview", "1.0.0", &[("mdview.exe", body("m"))], None);
    fx.check();
    fx.cmd(Cmd::Install("mdedit"));
    fx.cmd(Cmd::Install("mdview"));
    assert!(matches!(fx.row("mdview"), Row::Waiting(_)));
    fx.cmd(Cmd::Cancel("mdview"));
    assert_eq!(
        fx.row("mdview"),
        Row::NotInstalled,
        "out of the queue, as it was"
    );
    fx.cmd(Cmd::Cancel("mdedit"));
    fx.settle();
    assert_eq!(fx.file("mdedit", "mdedit.exe"), None, "nothing was placed");
    assert_eq!(fx.row("mdedit"), Row::NotInstalled);
    assert_eq!(fx.vm().checked, "Cancelled");
    assert_eq!(fx.vm().busy, None);
}

#[test]
fn cancel_all_clears_the_queue_and_the_bar() {
    let mut fx = Fx::with_delay("cancelall", Duration::from_millis(150));
    for (id, exe) in [
        ("mdedit", "mdedit.exe"),
        ("mdview", "mdview.exe"),
        ("sprawl", "sprawl.exe"),
    ] {
        fx.publish_files(id, "1.0.0", &[(exe, body("x"))], None);
    }
    fx.check();
    for id in ["mdedit", "mdview", "sprawl"] {
        fx.cmd(Cmd::Install(id));
    }
    assert_eq!(fx.vm().busy.as_deref(), Some("Updating 3 apps\u{2026}"));
    fx.cmd(Cmd::CancelAll);
    fx.settle();
    for id in ["mdedit", "mdview", "sprawl"] {
        assert_eq!(fx.row(id), Row::NotInstalled, "{id}");
    }
    assert_eq!(fx.vm().busy, None);
}

#[test]
fn a_single_install_says_which_app_in_the_bar() {
    let mut fx = Fx::with_delay("bar", Duration::from_millis(100));
    fx.publish_files("mdview", "1.0.0", &[("mdview.exe", body("m"))], None);
    fx.check();
    fx.cmd(Cmd::Install("mdview"));
    assert_eq!(fx.vm().busy.as_deref(), Some("Installing mdview\u{2026}"));
    fx.settle();
    assert_eq!(fx.vm().busy, None);
}

#[test]
fn an_install_with_no_release_fails_in_the_row_without_a_worker() {
    let mut fx = Fx::new("norelease");
    // Never checked: there is nothing to install from.
    fx.cmd(Cmd::Install("mdedit"));
    assert!(!fx.eng.busy());
    assert!(matches!(fx.row("mdedit"), Row::Failed(_)));
    assert!(fx.tr.urls.lock().unwrap().is_empty());
}

// ---- the schedule ----

const H: u64 = 3600;

#[test]
fn the_first_check_waits_for_its_delay_and_only_runs_when_due() {
    let s = Duration::from_secs;
    // Default: 2 minutes after start, if a check is due.
    let first = Some((s(120), false));
    assert_eq!(check_wait(first, s(0), None, 1000, 6, 0), Some(s(120)));
    assert_eq!(check_wait(first, s(100), None, 1000, 6, 0), Some(s(20)));
    assert_eq!(
        check_wait(first, s(120), None, 1000, 6, 0),
        Some(Duration::ZERO)
    );
    // A check 1 hour ago with a 6 hour interval: not due at 2 minutes; due in 5 hours.
    let last = Some(100_000 - H);
    assert_eq!(
        check_wait(first, s(120), last, 100_000, 6, 0),
        Some(s(5 * H))
    );
    // Forced (a test or --check-now): at its delay whatever the interval says.
    let forced = Some((s(0), true));
    assert_eq!(
        check_wait(forced, s(0), last, 100_000, 6, 0),
        Some(Duration::ZERO)
    );
    assert_eq!(
        check_wait(forced, s(0), last, 100_000, 0, 0),
        Some(Duration::ZERO)
    );
    assert_eq!(
        check_wait(Some((s(500), true)), s(200), last, 100_000, 6, 0),
        Some(s(300))
    );
}

#[test]
fn later_checks_follow_the_interval_and_back_off_after_a_failure() {
    let s = Duration::from_secs;
    let now = 1_000_000;
    assert_eq!(
        check_wait(None, s(9), Some(now - 2 * H), now, 6, 0),
        Some(s(4 * H))
    );
    assert_eq!(
        check_wait(None, s(9), Some(now - 7 * H), now, 6, 0),
        Some(Duration::ZERO)
    );
    // After a failed run: 30 minutes, then doubling, never more than the interval.
    assert_eq!(check_wait(None, s(9), Some(now), now, 6, 1), Some(s(1800)));
    assert_eq!(check_wait(None, s(9), Some(now), now, 6, 2), Some(s(3600)));
    assert_eq!(check_wait(None, s(9), Some(now), now, 6, 9), Some(s(6 * H)));
    // Never (interval 0): no timer at all, a manual check only.
    assert_eq!(check_wait(None, s(9), Some(now - 99 * H), now, 0, 0), None);
    // A clock that went backwards is due, not "in 100 years".
    assert_eq!(
        check_wait(None, s(9), Some(now + 5 * H), now, 6, 0),
        Some(Duration::ZERO)
    );
    // Never checked: due at once.
    assert_eq!(
        check_wait(None, s(9), None, now, 6, 0),
        Some(Duration::ZERO)
    );
}

#[test]
fn nothing_is_asked_at_start_and_the_first_check_is_scheduled_not_run() {
    use crate::glue::{Deps, Engine, Tmpl};
    use hub_core::detect::Env;
    use hub_core::state::Store;
    use std::sync::Arc;
    let fx = Fx::new("sched");
    let mk = |first| {
        Engine::new(Deps {
            store: Store::at(fx.root.join("data")),
            env: Env::custom(fx.root.join("programs"), Vec::new(), Vec::new()),
            transport: fx.tr.clone(),
            post: Arc::new(crate::e2e_tests::Chan(std::sync::Mutex::new(
                std::sync::mpsc::channel().0,
            ))),
            shell: fx.shell.clone(),
            tmpl: Tmpl::real(),
            first_check: first,
            hub_exe: None,
            run_key: crate::glue::RunKey::none(),
        })
    };
    let mut e = mk(Some((Duration::from_secs(120), false)));
    let now = check::now();
    let w = e.wait(now).expect("a timer");
    assert!(
        w > Duration::from_secs(100) && w <= Duration::from_secs(120),
        "{w:?}"
    );
    assert!(!e.tick(now), "not yet");
    assert!(!e.checking());
    assert!(e.vm(now).apps.iter().all(|a| a.latest.is_none()));
    assert!(fx.tr.urls.lock().unwrap().is_empty(), "no request at start");
    // `--check-now`: a forced check with no delay starts at the first tick.
    let mut e = mk(Some((Duration::ZERO, true)));
    assert!(e.tick(now));
    assert!(e.checking());
    assert_eq!(e.vm(now).checked, "Checking\u{2026}");
    assert_eq!(e.wait(now), None, "no timer while it runs");
    fx.shell.running.store(false, Ordering::SeqCst);
}

// ---- the command line ----

#[test]
fn the_command_line() {
    let p = |s: &str| cli::parse(&s.split_whitespace().map(String::from).collect::<Vec<_>>());
    let a = p("--behind --no-session --exit-after-ms 1500 --trace --demo main --check-now --size 700x500 --dark --view large").unwrap();
    assert!(a.behind && a.no_session && a.trace && a.check_now);
    assert_eq!(a.exit_after_ms, Some(1500));
    assert_eq!(a.demo.as_deref(), Some("main"));
    assert_eq!(a.size, Some((700, 500)));
    assert_eq!(a.dark, Some(true));
    assert_eq!(a.view, Some(View::Large));
    assert_eq!(p("--light").unwrap().dark, Some(false));
    assert_eq!(p("").unwrap(), cli::Args::default());
    for bad in [
        "--size 0x5",
        "--size 99999x5",
        "--size 5",
        "--exit-after-ms x",
        "--exit-after-ms",
        "--view round",
        "--nope",
        "--demo",
    ] {
        assert!(p(bad).is_err(), "{bad}");
    }
    let a = cli::Args::default();
    assert_eq!(
        cli::first_check(&a, None),
        (Duration::from_secs(120), false)
    );
    assert_eq!(
        cli::first_check(&a, Some("300")),
        (Duration::from_millis(300), true)
    );
    assert_eq!(
        cli::first_check(&a, Some("x")),
        (Duration::from_secs(120), false)
    );
    let now = cli::Args {
        check_now: true,
        ..cli::Args::default()
    };
    assert_eq!(cli::first_check(&now, Some("300")), (Duration::ZERO, true));
}

#[test]
fn the_menu_items_of_an_unknown_app_are_just_the_page() {
    let fx = Fx::new("menu2");
    let v = fx.eng.menu_items("nonesuch");
    assert_eq!(v, vec![(MenuItem::GitHub, "GitHub", true)]);
}

#[test]
fn a_held_key_repeats_only_what_is_harmless() {
    use crate::glue::repeatable;
    assert!(repeatable(&Cmd::SelectRel(1)));
    assert!(repeatable(&Cmd::FilterChar('a')));
    assert!(repeatable(&Cmd::FilterBack));
    for c in [Cmd::Primary, Cmd::CheckNow, Cmd::FocusFilter, Cmd::Esc] {
        assert!(!repeatable(&c), "{c:?}");
    }
}

fn two_open_updates(name: &str) -> Fx {
    let mut fx = Fx::with_delay(name, Duration::from_millis(40));
    for (id, exe) in [("mdedit", "mdedit.exe"), ("mdview", "mdview.exe")] {
        fx.publish_files(id, "1.0.0", &[(exe, body("a"))], None);
    }
    fx.check();
    fx.cmd(Cmd::Install("mdedit"));
    fx.cmd(Cmd::Install("mdview"));
    fx.settle();
    for (id, exe) in [("mdedit", "mdedit.exe"), ("mdview", "mdview.exe")] {
        fx.publish_files(id, "2.0.0", &[(exe, body("b"))], None);
    }
    fx.check();
    fx
}

#[test]
fn update_asks_again_whether_the_app_is_open_now() {
    let mut fx = two_open_updates("stale");
    // The last look said it is not open; since then it was started.
    fx.eng.refresh_running();
    assert!(!fx.app("mdedit").running);
    fx.shell.running.store(true, Ordering::SeqCst);
    fx.cmd(Cmd::Update("mdedit"));
    assert!(fx.vm().banner.is_some(), "asked, not closed");
    assert!(!fx.eng.busy());
    assert_eq!(fx.file("mdedit", "mdedit.exe"), Some(body("a")));
}

#[test]
fn update_all_counts_and_updates_the_catalog_even_when_a_row_is_filtered_out() {
    let mut fx = two_open_updates("filtered-all");
    fx.cmd(Cmd::FocusFilter);
    for c in "mdedit".chars() {
        fx.cmd(Cmd::FilterChar(c));
    }
    let vm = fx.vm();
    assert_eq!(vm.apps.iter().map(|a| a.id).collect::<Vec<_>>(), ["mdedit"]);
    assert_eq!(vm.updates, 2);
    fx.cmd(Cmd::UpdateAll);
    assert_eq!(fx.vm().busy.as_deref(), Some("Updating 2 apps\u{2026}"));
    assert!(fx.eng.apply(Cmd::FilterClear));
    assert!(fx.vm().filter_focus, "Clear leaves the field's focus alone");
    fx.settle();
    for (id, exe) in [("mdedit", "mdedit.exe"), ("mdview", "mdview.exe")] {
        assert_eq!(fx.file(id, exe), Some(body("b")));
    }
    assert_eq!(fx.vm().updates, 0);
}

#[test]
fn filter_escape_clears_then_blurs_even_while_an_offer_is_visible() {
    let mut fx = two_open_updates("filter-escape-offer");
    fx.shell.running.store(true, Ordering::SeqCst);
    fx.cmd(Cmd::Update("mdedit"));
    let key = |focused| key_command(focused, Key::Esc, false, false, 1).unwrap();
    fx.cmd(Cmd::FocusFilter);
    fx.cmd(Cmd::FilterChar('z'));
    let offer = fx.vm().banner.unwrap().key;
    assert!(fx.eng.apply(key(true)));
    assert_eq!(fx.vm().filter, "");
    assert!(fx.vm().filter_focus);
    assert!(fx.eng.apply(key(true)));
    assert!(!fx.vm().filter_focus);
    assert_eq!(fx.vm().banner.unwrap().key, offer);
    assert!(fx.eng.apply(key(false)));
    assert!(fx.vm().banner.is_none());
}

#[test]
fn choosing_the_same_row_redraws_when_it_blurs_the_filter() {
    let mut fx = Fx::new("select-filter-focus");
    assert!(fx.eng.apply(Cmd::FocusFilter));
    assert!(fx.eng.apply(Cmd::Select(0)));
    assert!(!fx.vm().filter_focus);
    assert!(!fx.eng.apply(Cmd::Select(0)));
}

#[test]
fn a_late_release_cannot_answer_a_replaced_offer_before_or_after_it_is_drawn() {
    let mut fx = two_open_updates("replaced-offer");
    fx.shell.running.store(true, Ordering::SeqCst);
    fx.shell.view_too.store(true, Ordering::SeqCst);
    fx.cmd(Cmd::Update("mdedit"));
    let painted = fx.vm();
    let yes = Hit::BannerYes;
    let pressed = Target::at(yes, &painted).unwrap();
    assert_eq!(pressed.release(yes, &painted, &painted), Some(yes));
    assert_eq!(pressed.release(Hit::BannerNo, &painted, &painted), None);
    // A new manifest can change the version without changing the wording or app.
    fx.publish_files("mdedit", "3.0.0", &[("mdedit.exe", body("c"))], None);
    fx.check();
    let newer = fx.vm();
    assert_eq!(newer.banner.as_ref().unwrap().key.version, "3.0.0");
    assert_eq!(pressed.release(yes, &painted, &newer), None);
    assert_eq!(pressed.release(yes, &newer, &newer), None);
    // A newly issued question, even for the same version, has its own identity.
    let pressed = Target::at(yes, &newer).unwrap();
    fx.cmd(Cmd::Update("mdedit"));
    assert_eq!(pressed.release(yes, &newer, &fx.vm()), None);
    fx.cmd(Cmd::Update("mdview"));
    assert_eq!(pressed.release(yes, &newer, &fx.vm()), None);
    let hub = crate::demo::build("banner", View::List).unwrap();
    assert_eq!(pressed.release(yes, &newer, &hub), None);
    assert_eq!(command(yes, &hub), Some(Cmd::HubRestart));
    assert!(!fx.eng.installing());
}

#[test]
fn release_tracks_an_app_action_not_a_filtered_index_or_another_action() {
    let painted = crate::demo::build("main", View::List).unwrap();
    let original = Hit::Action(3, Act::Update);
    let press = Target::at(original, &painted).unwrap();
    assert_eq!(press.release(original, &painted, &painted), Some(original));
    assert_eq!(
        press.release(Hit::Action(0, Act::Update), &painted, &painted),
        None
    );
    let mut filtered = painted.clone();
    filtered.apps.remove(0);
    let moved = Hit::Action(2, Act::Update);
    assert_eq!(press.release(moved, &filtered, &filtered), Some(moved));
    assert_eq!(press.release(original, &painted, &filtered), Some(moved));
    assert_eq!(press.release(original, &filtered, &filtered), None);
    filtered.apps[2].row = Row::UpToDate;
    assert_eq!(press.release(moved, &filtered, &filtered), None);
}

#[test]
fn a_queued_update_of_an_app_opened_meanwhile_is_asked_about_when_its_turn_comes() {
    let mut fx = two_open_updates("queued");
    fx.eng.refresh_running();
    fx.cmd(Cmd::Update("mdview"));
    fx.cmd(Cmd::Update("mdedit"));
    assert!(fx.eng.busy());
    // mdedit is started while mdview downloads.
    fx.shell.running.store(true, Ordering::SeqCst);
    fx.settle();
    assert_eq!(fx.file("mdview", "mdview.exe"), Some(body("b")));
    assert_eq!(
        fx.file("mdedit", "mdedit.exe"),
        Some(body("a")),
        "untouched"
    );
    assert_eq!(fx.row("mdedit"), Row::Update);
    assert!(fx.vm().banner.is_some(), "it is asked about instead");
    // Yes closes it; the install then runs without asking again.
    fx.shell.running.store(false, Ordering::SeqCst);
    fx.cmd(Cmd::BannerYes);
    fx.settle();
    assert_eq!(fx.file("mdedit", "mdedit.exe"), Some(body("b")));
}

#[test]
fn esc_on_the_offer_bar_is_the_same_as_no() {
    let mut fx = two_open_updates("esc");
    fx.shell.running.store(true, Ordering::SeqCst);
    fx.shell.view_too.store(true, Ordering::SeqCst);
    fx.eng.refresh_running();
    // Both are open: mdedit is asked about, mdview waits behind it.
    fx.cmd(Cmd::UpdateAll);
    assert!(fx.vm().banner.is_some());
    fx.cmd(Cmd::Esc);
    assert!(fx.vm().banner.is_none());
    // Later work ending must not bring the other open app's offer back.
    fx.shell.running.store(false, Ordering::SeqCst);
    fx.cmd(Cmd::Update("mdedit"));
    fx.settle();
    assert!(fx.vm().banner.is_none(), "nothing is asked about later");
}

#[test]
fn the_running_lookup_is_done_off_the_window_thread() {
    let mut fx = two_open_updates("asyncrun");
    fx.shell.run_threads.lock().unwrap().clear();
    fx.shell.running.store(true, Ordering::SeqCst);
    fx.eng.refresh_running_async();
    let ev = loop {
        let e = fx
            .rx
            .recv_timeout(Duration::from_secs(5))
            .expect("an event");
        if matches!(e, crate::glue::Event::Running(_)) {
            break e;
        }
    };
    assert!(fx.eng.handle(ev));
    assert!(fx.app("mdedit").running);
    let me = std::thread::current().id();
    let threads = fx.shell.run_threads.lock().unwrap().clone();
    assert!(!threads.is_empty());
    assert!(threads.iter().all(|t| *t != me), "asked on the caller");
}

#[test]
fn closing_stops_the_install_and_the_queue_and_waits_for_the_worker() {
    let mut fx = Fx::with_delay("close", Duration::from_millis(150));
    fx.publish_files("mdedit", "1.0.0", &[("mdedit.exe", body("a"))], None);
    fx.publish_files("mdview", "1.0.0", &[("mdview.exe", body("m"))], None);
    fx.check();
    fx.cmd(Cmd::Install("mdedit"));
    fx.cmd(Cmd::Install("mdview"));
    assert!(fx.eng.installing());
    assert!(fx.eng.begin_close(), "the worker is still ending");
    assert!(fx.eng.installing(), "not safe to exit yet");
    fx.settle();
    assert!(!fx.eng.installing(), "now it is");
    assert_eq!(fx.file("mdedit", "mdedit.exe"), None);
    assert_eq!(
        fx.file("mdview", "mdview.exe"),
        None,
        "the queue did not run"
    );
}

#[test]
fn a_progress_report_of_a_self_closing_app_says_the_installer_closes_it() {
    use hub_core::install::Progress;
    match crate::glue::phase_of(Progress::SetupCloses("Inbound".into())) {
        Some(crate::glue::Phase::Waiting(t)) => assert_eq!(t, "The installer will close Inbound"),
        other => panic!("{other:?}"),
    }
    match crate::glue::phase_of(Progress::Waiting("mdedit.exe".into())) {
        Some(crate::glue::Phase::Waiting(t)) => assert!(t.contains("close it to continue"), "{t}"),
        other => panic!("{other:?}"),
    }
    assert!(crate::glue::phase_of(Progress::Finished).is_none());
}

#[test]
fn the_control_under_the_pointer_reaches_the_drawing_and_redraws_only_on_a_change() {
    let mut fx = Fx::new("pointer");
    assert_eq!(fx.vm().pointer, crate::vm::Pointer::default());
    // A move onto the close button: a change, and the Vm carries it to the drawing.
    assert!(fx.eng.set_hot(Hit::Close));
    assert_eq!(fx.vm().pointer.hot, Hit::Close);
    assert!(fx.vm().pointer.look(Hit::Close).hovered);
    assert!(!fx.vm().pointer.look(Hit::Min).hovered);
    // Moves within it: no change, so no redraw.
    assert!(!fx.eng.set_hot(Hit::Close));
    // Pressed on it: pressed, once.
    assert!(fx.eng.set_held(Hit::Close));
    assert!(!fx.eng.set_held(Hit::Close));
    assert!(fx.vm().pointer.look(Hit::Close).pressed);
    // Off it while held: neither lit nor pressed; released there: nothing to redraw.
    assert!(fx.eng.set_hot(Hit::Min));
    assert!(!fx.vm().pointer.look(Hit::Close).pressed);
    assert!(!fx.eng.set_held(Hit::None));
    // Back on it, and released: the press goes.
    assert!(fx.eng.set_hot(Hit::Close));
    assert!(fx.eng.set_held(Hit::Close));
    assert!(fx.eng.set_held(Hit::None));
    assert!(!fx.vm().pointer.look(Hit::Close).pressed);
    assert!(fx.eng.set_hot(Hit::None));
    assert_eq!(fx.vm().pointer, crate::vm::Pointer::default());
}
