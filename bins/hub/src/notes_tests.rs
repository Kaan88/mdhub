//! The release notes view end to end, without a window: the engine over the fake github.com
//! (`FileRoot`), a notes file in the fake release, and what the view shows at each step. Nothing
//! here reads this PC or the network.
use crate::e2e_tests::{Fx, body};
use crate::glue::{Cmd, Event, Key, MenuItem, command, key_command};
use crate::vm::{Hit, NotesState};
use hub_core::catalog::find;
use hub_core::manifest;
use std::time::{Duration, Instant};

const NOTES: &str = "# mdedit 1.2.1\n\n- one\n- two with `code`\n";

fn latest_dir(fx: &Fx, id: &str) -> std::path::PathBuf {
    let app = find(id).unwrap();
    fx.root
        .join("release")
        .join("Kaan88")
        .join(app.repo)
        .join("releases")
        .join("latest")
        .join("download")
}

/// Makes the release's manifest name a notes file, and (when `bytes` is given) puts that file in
/// the release.
fn publish_notes(fx: &Fx, id: &str, version: &str, file: &str, bytes: Option<&[u8]>) {
    let app = find(id).unwrap();
    let p = latest_dir(fx, id).join("hub.txt");
    let mut m = manifest::parse(&std::fs::read(&p).unwrap(), app).unwrap();
    m.notes_file = Some(file.to_string());
    std::fs::write(&p, manifest::write(&m)).unwrap();
    if let Some(b) = bytes {
        let d = fx
            .root
            .join("release")
            .join("Kaan88")
            .join(app.repo)
            .join("releases")
            .join("download")
            .join(format!("v{version}"));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join(file), b).unwrap();
    }
}

/// Waits for the notes worker's answer and hands it to the engine.
fn pump_notes(fx: &mut Fx) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        match fx.rx.recv_timeout(Duration::from_millis(100)) {
            Ok(ev @ Event::Notes { .. }) => {
                assert!(fx.eng.handle(ev), "a live view takes the answer");
                return;
            }
            Ok(_) => {}
            Err(_) => assert!(Instant::now() < deadline, "no notes answer came"),
        }
    }
}

fn notes_urls(fx: &Fx) -> Vec<String> {
    fx.tr
        .urls
        .lock()
        .unwrap()
        .iter()
        .filter(|u| !u.ends_with("hub.txt") && !u.contains("manifest"))
        .filter(|u| u.ends_with(".md"))
        .cloned()
        .collect()
}

fn state(fx: &Fx) -> Option<NotesState> {
    fx.vm().notes.map(|n| n.state)
}

/// A fixture whose mdedit 1.2.1 release names `notes-1.2.1.md`, checked, not yet opened.
fn checked(name: &str, with_file: bool) -> Fx {
    let mut fx = Fx::new(name);
    fx.publish_files("mdedit", "1.2.1", &[("mdedit.exe", body("v"))], None);
    publish_notes(
        &fx,
        "mdedit",
        "1.2.1",
        "notes-1.2.1.md",
        with_file.then_some(NOTES.as_bytes()),
    );
    fx.check();
    fx
}

#[test]
fn nothing_asks_for_the_notes_at_start_at_a_check_or_while_the_list_is_used() {
    let mut fx = checked("notes-never", true);
    assert!(notes_urls(&fx).is_empty(), "{:?}", notes_urls(&fx));
    fx.check();
    fx.cmd(Cmd::Select(1));
    fx.cmd(Cmd::OpenMenu(0));
    fx.check();
    assert!(notes_urls(&fx).is_empty());
    assert!(fx.vm().notes.is_none());
    assert!(!fx.root.join("data").join("notes").exists());
    // A new start of Hub over the same folders asks for nothing either.
    fx.restart();
    assert!(fx.vm().notes.is_none());
    assert!(notes_urls(&fx).is_empty());
}

#[test]
fn opening_loads_the_notes_once_and_shows_the_one_line_meanwhile() {
    let mut fx = checked("notes-load", true);
    fx.cmd(Cmd::OpenNotes("mdedit"));
    let v = fx.vm().notes.expect("the view is open");
    assert_eq!(v.state, NotesState::Loading);
    assert_eq!(
        v.one_liner, "notes of 1.2.1",
        "the one line is there at once"
    );
    assert_eq!(
        (v.id, v.name, v.version.as_str()),
        ("mdedit", "mdedit", "1.2.1")
    );
    assert_eq!(v.date, "21 Nov");
    pump_notes(&mut fx);
    assert_eq!(state(&fx), Some(NotesState::Loaded(NOTES.to_string())));
    assert_eq!(
        notes_urls(&fx),
        vec![
            "https://github.com/Kaan88/markdown-editor-rs/releases/download/v1.2.1/notes-1.2.1.md"
                .to_string()
        ]
    );
}

#[test]
fn reopening_is_offline_from_the_cache_also_after_a_restart() {
    let mut fx = checked("notes-cache", true);
    fx.cmd(Cmd::OpenNotes("mdedit"));
    pump_notes(&mut fx);
    fx.cmd(Cmd::NotesClose);
    assert!(fx.vm().notes.is_none());
    fx.cmd(Cmd::OpenNotes("mdedit"));
    // Loaded at once: no worker, no wait.
    assert_eq!(state(&fx), Some(NotesState::Loaded(NOTES.to_string())));
    assert_eq!(notes_urls(&fx).len(), 1, "no second request");
    // Another start of Hub; the release is gone from the fake github: the disk still has it.
    std::fs::remove_dir_all(
        fx.root
            .join("release")
            .join("Kaan88")
            .join("markdown-editor-rs")
            .join("releases")
            .join("download"),
    )
    .unwrap();
    fx.restart();
    fx.check();
    fx.cmd(Cmd::OpenNotes("mdedit"));
    assert_eq!(state(&fx), Some(NotesState::Loaded(NOTES.to_string())));
    assert_eq!(notes_urls(&fx).len(), 1);
    // The cache is by version: a new version asks again.
    fx.publish_files("mdedit", "1.2.2", &[("mdedit.exe", body("w"))], None);
    publish_notes(
        &fx,
        "mdedit",
        "1.2.2",
        "notes-1.2.2.md",
        Some(b"newer notes"),
    );
    fx.check();
    fx.cmd(Cmd::NotesClose);
    fx.cmd(Cmd::OpenNotes("mdedit"));
    assert_eq!(state(&fx), Some(NotesState::Loading));
    pump_notes(&mut fx);
    assert_eq!(state(&fx), Some(NotesState::Loaded("newer notes".into())));
    assert_eq!(notes_urls(&fx).len(), 2);
    // And only the newest is kept on disk.
    let files: Vec<_> = std::fs::read_dir(fx.root.join("data").join("notes"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    assert_eq!(files, vec!["mdedit-1.2.2.md".to_string()]);
}

#[test]
fn a_failed_fetch_keeps_the_one_line_says_why_and_try_again_asks_again() {
    // The release names a notes file that is not there.
    let mut fx = checked("notes-fail", false);
    fx.cmd(Cmd::OpenNotes("mdedit"));
    pump_notes(&mut fx);
    let v = fx.vm().notes.unwrap();
    match &v.state {
        NotesState::Failed(why) => assert!(why.contains("404"), "{why}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(v.one_liner, "notes of 1.2.1", "the one line stays");
    assert!(
        fx.root.join("data").join("notes").read_dir().is_err(),
        "nothing cached"
    );
    // Try again retries: still failing, then published, then loaded.
    fx.cmd(Cmd::NotesRetry);
    assert_eq!(state(&fx), Some(NotesState::Loading));
    pump_notes(&mut fx);
    assert!(matches!(state(&fx), Some(NotesState::Failed(_))));
    assert_eq!(notes_urls(&fx).len(), 2);
    publish_notes(
        &fx,
        "mdedit",
        "1.2.1",
        "notes-1.2.1.md",
        Some(NOTES.as_bytes()),
    );
    fx.cmd(Cmd::NotesRetry);
    pump_notes(&mut fx);
    assert_eq!(state(&fx), Some(NotesState::Loaded(NOTES.to_string())));
    assert_eq!(notes_urls(&fx).len(), 3);
    // Retry does nothing when nothing failed.
    assert!(!fx.eng.apply(Cmd::NotesRetry));
    assert_eq!(notes_urls(&fx).len(), 3);
}

#[test]
fn a_release_without_a_notes_file_shows_the_one_line_and_asks_for_nothing() {
    let mut fx = Fx::new("notes-nofile");
    fx.publish_files("mdedit", "1.2.1", &[("mdedit.exe", body("v"))], None);
    fx.check();
    fx.cmd(Cmd::OpenNotes("mdedit"));
    let v = fx.vm().notes.unwrap();
    assert_eq!(v.state, NotesState::NoFile);
    assert_eq!(v.one_liner, "notes of 1.2.1");
    assert!(notes_urls(&fx).is_empty());
    assert!(!fx.eng.apply(Cmd::NotesRetry));
}

#[test]
fn an_app_hub_knows_no_release_of_has_no_view() {
    let mut fx = Fx::new("notes-unknown");
    fx.publish_files("mdedit", "1.2.1", &[("mdedit.exe", body("v"))], None);
    fx.check();
    // mdview's release has no manifest: nothing to open.
    assert!(fx.eng.apply(Cmd::OpenNotes("mdview")));
    assert!(fx.vm().notes.is_none());
    assert!(fx.vm().checked.contains("no release information"));
    let items = fx.eng.menu_items("mdview");
    assert!(items.contains(&(MenuItem::ReleaseNotes, "Release notes", false)));
    assert!(
        fx.eng
            .menu_items("mdedit")
            .contains(&(MenuItem::ReleaseNotes, "Release notes", true))
    );
}

#[test]
fn escape_and_back_return_to_the_list_with_the_selection_kept() {
    let mut fx = checked("notes-back", true);
    fx.cmd(Cmd::Select(2));
    assert_eq!(fx.vm().sel, 2);
    fx.cmd(Cmd::OpenNotes("mdedit"));
    assert!(fx.vm().notes.is_some());
    assert_eq!(fx.vm().sel, 2, "opening does not move the selection");
    // Esc closes the view first, whatever else is waiting for it.
    assert!(fx.eng.apply(Cmd::Esc));
    assert!(fx.vm().notes.is_none());
    assert_eq!(fx.vm().sel, 2);
    // Back (the arrow) does the same.
    fx.cmd(Cmd::OpenNotes("mdedit"));
    let hit_cmd = command(Hit::NotesBack, &fx.vm());
    assert_eq!(hit_cmd, Some(Cmd::NotesClose));
    fx.cmd(hit_cmd.unwrap());
    assert!(fx.vm().notes.is_none());
    assert_eq!(fx.vm().sel, 2);
    assert!(!fx.eng.apply(Cmd::NotesClose), "nothing to close");
}

#[test]
fn an_answer_for_a_view_that_was_closed_is_dropped() {
    let mut fx = checked("notes-stale", true);
    fx.cmd(Cmd::OpenNotes("mdedit"));
    fx.cmd(Cmd::Esc);
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        match fx.rx.recv_timeout(Duration::from_millis(100)) {
            Ok(ev @ Event::Notes { .. }) => {
                assert!(!fx.eng.handle(ev), "nothing to show it on");
                break;
            }
            Ok(_) => {}
            Err(_) => assert!(Instant::now() < deadline),
        }
    }
    assert!(fx.vm().notes.is_none());
    // The answer was kept for the next time all the same.
    fx.cmd(Cmd::OpenNotes("mdedit"));
    assert!(matches!(state(&fx), Some(NotesState::Loaded(_))));
}

#[test]
fn the_view_shows_the_latest_above_what_is_installed() {
    let mut fx = Fx::new("notes-installed");
    fx.publish_files("mdedit", "1.2.0", &[("mdedit.exe", body("a"))], None);
    fx.check();
    fx.cmd(Cmd::Install("mdedit"));
    fx.settle();
    fx.publish_files("mdedit", "1.2.1", &[("mdedit.exe", body("b"))], None);
    publish_notes(&fx, "mdedit", "1.2.1", "n.md", Some(NOTES.as_bytes()));
    fx.check();
    fx.cmd(Cmd::OpenNotes("mdedit"));
    let v = fx.vm().notes.unwrap();
    assert_eq!(v.installed.as_deref(), Some("1.2.0"));
    assert_eq!(v.version, "1.2.1");
    assert_eq!(
        crate::changelog::version_line(&v),
        "mdedit  1.2.0 \u{2192} 1.2.1"
    );
}

#[test]
fn hostile_bytes_in_the_notes_file_arrive_as_clean_text() {
    let mut fx = Fx::new("notes-hostile");
    fx.publish_files("mdedit", "1.2.1", &[("mdedit.exe", body("v"))], None);
    let mut bad = b"# T\r\n\x00\x07\x1b[2J text\n".to_vec();
    bad.extend_from_slice(&[0xff, 0xfe, 0xc0, 0x80]);
    bad.extend_from_slice("\u{202e}evil".as_bytes());
    publish_notes(&fx, "mdedit", "1.2.1", "n.md", Some(&bad));
    fx.check();
    fx.cmd(Cmd::OpenNotes("mdedit"));
    pump_notes(&mut fx);
    match state(&fx) {
        Some(NotesState::Loaded(t)) => {
            assert!(t.starts_with("# T\n"), "{t:?}");
            assert!(!t.chars().any(|c| c.is_control() && c != '\n' && c != '\t'));
            assert!(!t.contains('\u{202e}'));
            assert!(t.contains('\u{fffd}'));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_file_over_the_cap_fails_with_its_reason_and_nothing_is_kept() {
    let mut fx = Fx::new("notes-big");
    fx.publish_files("mdedit", "1.2.1", &[("mdedit.exe", body("v"))], None);
    publish_notes(&fx, "mdedit", "1.2.1", "n.md", Some(&vec![b'a'; 70 * 1024]));
    fx.check();
    fx.cmd(Cmd::OpenNotes("mdedit"));
    pump_notes(&mut fx);
    match state(&fx) {
        Some(NotesState::Failed(w)) => assert!(w.contains("64 KiB"), "{w}"),
        other => panic!("{other:?}"),
    }
    assert!(fx.root.join("data").join("notes").read_dir().is_err());
}

#[test]
fn the_release_page_opens_in_the_browser_and_only_that() {
    let mut fx = checked("notes-page", true);
    fx.cmd(Cmd::OpenNotes("mdedit"));
    fx.cmd(Cmd::NotesPage);
    assert_eq!(
        *fx.shell.urls.lock().unwrap(),
        vec!["https://github.com/Kaan88/markdown-editor-rs/releases/tag/v1.2.1".to_string()]
    );
    // With the view closed, the command does nothing.
    fx.cmd(Cmd::NotesClose);
    fx.cmd(Cmd::NotesPage);
    assert_eq!(fx.shell.urls.lock().unwrap().len(), 1);
}

#[test]
fn the_pane_link_the_menu_and_the_key_all_open_it() {
    let mut fx = checked("notes-ways", true);
    let vm = fx.vm();
    // The pane's link (a hit of the drawn pane) is the command for that row's app.
    assert_eq!(command(Hit::Notes(0), &vm), Some(Cmd::OpenNotes("mdedit")));
    assert_eq!(command(Hit::NotesRetry, &vm), Some(Cmd::NotesRetry));
    assert_eq!(command(Hit::NotesPage, &vm), Some(Cmd::NotesPage));
    assert_eq!(command(Hit::Notes(99), &vm), None);
    // The key, not while typing in the filter.
    assert_eq!(
        key_command(false, Key::Char('n'), false, false, 1),
        Some(Cmd::NotesOfSelected)
    );
    assert_eq!(key_command(false, Key::Char('n'), true, false, 1), None);
    assert_eq!(
        key_command(true, Key::Char('n'), false, false, 1),
        Some(Cmd::FilterChar('n'))
    );
    fx.cmd(Cmd::NotesOfSelected);
    assert_eq!(fx.vm().notes.map(|n| n.id), Some("mdedit"));
    fx.cmd(Cmd::NotesClose);
    // The row menu.
    fx.cmd(Cmd::MenuPick("mdedit", MenuItem::ReleaseNotes));
    assert_eq!(fx.vm().notes.map(|n| n.id), Some("mdedit"));
}
