//! End to end, without a window: Hub's own update (offered, downloaded beside a scratch exe,
//! swapped in only when no install runs, then a restart is asked for), the notification rule as
//! the engine applies it, and the settings (the Run value of a test key, the check interval, the
//! tray menu and the card staying in step). The exe that is swapped is a dummy file in a scratch
//! folder; the Run value is under `HKCU\Software\HubTest\<unique>`, removed at the end; nothing
//! of this PC's Hub, apps or registry is touched and nothing is asked of the network.
use crate::e2e_tests::{Fx, body};
use crate::glue::{Cmd, Event, RunKey, command};
use crate::settings::{INTERVAL, NOTIFY, START, TRAY, UPDATE_HUB};
use crate::vm::Hit;
use hub_core::check;
use hub_core::selfupdate::Status;
use mdshared_app::update::sha256_hex;
use mdshared_ui::settings::{Form, Value};
use std::path::{Path, PathBuf};
use std::time::Duration;

const MANIFEST_URL: &str = hub_core::selfupdate::MANIFEST_URL;

/// A fake github.com folder path for a URL under `Kaan88/hub-rs`.
fn hub_path(fx: &Fx, tail: &str) -> PathBuf {
    let mut p = fx.root.join("release").join("Kaan88").join("hub-rs");
    for part in tail.split('/') {
        p.push(part);
    }
    p
}

/// Hub's release `version` on the fake github: its manifest (format 1) and its exe.
fn publish_hub(fx: &Fx, version: &str, exe_bytes: &[u8]) {
    let dl = hub_path(fx, &format!("releases/download/v{version}"));
    std::fs::create_dir_all(&dl).unwrap();
    std::fs::write(dl.join("hub.exe"), exe_bytes).unwrap();
    let latest = hub_path(fx, "releases/latest/download");
    std::fs::create_dir_all(&latest).unwrap();
    let url = format!("https://github.com/Kaan88/hub-rs/releases/download/v{version}/hub.exe");
    std::fs::write(
        latest.join("hub-update.txt"),
        format!(
            "manifest: 1\nversion: {version}\nnotes: Faster tray.\nwindows-x86_64.asset: hub.exe\nwindows-x86_64.url: {url}\nwindows-x86_64.sha256: {}\n",
            sha256_hex(exe_bytes)
        ),
    )
    .unwrap();
}

/// The scratch exe Hub "is": a dummy file in a folder of its own.
fn scratch_exe(fx: &Fx) -> PathBuf {
    let dir = fx.root.join("hub-home");
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("hub.exe");
    std::fs::write(&exe, b"old hub").unwrap();
    exe
}

/// A fixture whose Hub can update itself (its exe is the scratch one) and can tell the user.
fn world(name: &str) -> (Fx, PathBuf) {
    let mut fx = Fx::new(name);
    let exe = scratch_exe(&fx);
    fx.eng.set_hub_exe(Some(exe.clone()));
    fx.eng.set_tray_up(true);
    (fx, exe)
}

fn staged_of(exe: &Path) -> PathBuf {
    let mut s = exe.as_os_str().to_os_string();
    s.push(".new");
    PathBuf::from(s)
}

fn hub_urls(fx: &Fx) -> Vec<String> {
    fx.tr
        .urls
        .lock()
        .unwrap()
        .iter()
        .filter(|u| u.contains("/hub-rs/"))
        .cloned()
        .collect()
}

// ---- Hub's own update ----

#[test]
fn a_newer_hub_is_asked_for_first_downloaded_beside_the_exe_and_offered_as_a_banner() {
    let (mut fx, exe) = world("own-offer");
    publish_hub(&fx, "0.2.0", b"new hub bytes");
    fx.check();
    // Hub's own manifest is the first request of the run, ahead of every app's.
    let urls = fx.tr.urls.lock().unwrap().clone();
    assert_eq!(urls.first().map(String::as_str), Some(MANIFEST_URL));
    assert!(urls.len() > 2, "the apps were asked after it: {urls:?}");
    // Downloaded and verified, written beside the exe; the exe itself is not touched.
    assert!(matches!(fx.eng.hub_status(), Status::Ready(s) if s.version.to_string() == "0.2.0"));
    assert_eq!(std::fs::read(&exe).unwrap(), b"old hub");
    assert_eq!(std::fs::read(staged_of(&exe)).unwrap(), b"new hub bytes");
    let vm = fx.vm();
    let b = vm.banner.expect("the banner");
    assert_eq!(b.text, "Hub 0.2.0 is ready: restart to finish");
    assert_eq!((b.yes.as_str(), b.no.as_str()), ("Restart now", "Later"));
    assert!(
        vm.hub_version.ends_with("(0.2.0 ready)"),
        "{}",
        vm.hub_version
    );
    // Nothing restarts by itself.
    assert!(fx.eng.take_restart().is_none());
}

#[test]
fn restart_now_swaps_the_exe_in_and_asks_for_a_restart() {
    let (mut fx, exe) = world("own-swap");
    publish_hub(&fx, "0.2.0", b"new hub bytes");
    fx.check();
    fx.cmd(Cmd::HubRestart);
    assert_eq!(fx.eng.take_restart().as_deref(), Some(exe.as_path()));
    assert_eq!(std::fs::read(&exe).unwrap(), b"new hub bytes");
    assert!(fx.eng.take_restart().is_none(), "asked once");
    assert!(fx.vm().banner.is_none());
}

#[test]
fn an_install_running_defers_the_swap_until_it_is_done() {
    let mut fx = Fx::with_delay("own-defer", Duration::from_millis(400));
    let exe = scratch_exe(&fx);
    fx.eng.set_hub_exe(Some(exe.clone()));
    fx.publish_files("mdedit", "1.2.0", &[("mdedit.exe", body("v1"))], None);
    publish_hub(&fx, "0.2.0", b"new hub bytes");
    fx.check();
    assert!(matches!(fx.eng.hub_status(), Status::Ready(_)));
    fx.cmd(Cmd::Install("mdedit"));
    assert!(fx.eng.installing(), "the install is running");
    fx.cmd(Cmd::HubRestart);
    // Asked while installing: nothing was touched, nothing restarts yet.
    assert!(fx.eng.take_restart().is_none());
    assert_eq!(std::fs::read(&exe).unwrap(), b"old hub");
    assert!(staged_of(&exe).exists());
    fx.settle();
    // The install ended; Hub swapped itself last.
    assert_eq!(fx.file("mdedit", "mdedit.exe"), Some(body("v1")));
    assert_eq!(fx.eng.take_restart().as_deref(), Some(exe.as_path()));
    assert_eq!(std::fs::read(&exe).unwrap(), b"new hub bytes");
}

#[test]
fn later_hides_the_banner_and_quitting_swaps_the_update_in() {
    let (mut fx, exe) = world("own-later");
    publish_hub(&fx, "0.2.0", b"new hub bytes");
    fx.check();
    fx.cmd(Cmd::HubLater);
    assert!(fx.vm().banner.is_none(), "Later hides it");
    assert!(matches!(fx.eng.hub_status(), Status::Ready(_)));
    // The next check finds the same version already downloaded: it is not fetched again and the
    // banner stays hidden.
    let before = hub_urls(&fx).len();
    fx.check();
    assert!(fx.vm().banner.is_none());
    let files = hub_urls(&fx)
        .iter()
        .filter(|u| u.contains("/releases/download/"))
        .count();
    assert_eq!(files, 1, "downloaded once ({before} hub urls before)");
    assert_eq!(std::fs::read(&exe).unwrap(), b"old hub");
    // Quitting: the downloaded Hub is put in place for the next start.
    assert!(fx.eng.apply_hub_on_quit());
    assert_eq!(std::fs::read(&exe).unwrap(), b"new hub bytes");
}

#[test]
fn quitting_with_an_install_running_leaves_the_exe_alone() {
    let mut fx = Fx::with_delay("own-quit-busy", Duration::from_millis(400));
    let exe = scratch_exe(&fx);
    fx.eng.set_hub_exe(Some(exe.clone()));
    fx.publish_files("mdedit", "1.2.0", &[("mdedit.exe", body("v1"))], None);
    publish_hub(&fx, "0.2.0", b"new hub bytes");
    fx.check();
    fx.cmd(Cmd::Install("mdedit"));
    assert!(!fx.eng.apply_hub_on_quit());
    assert_eq!(std::fs::read(&exe).unwrap(), b"old hub");
    fx.settle();
}

#[test]
fn a_staged_file_that_changed_is_refused_and_the_exe_stays() {
    let (mut fx, exe) = world("own-tamper");
    publish_hub(&fx, "0.2.0", b"new hub bytes");
    fx.check();
    std::fs::write(staged_of(&exe), b"something else").unwrap();
    fx.cmd(Cmd::HubRestart);
    assert!(fx.eng.take_restart().is_none());
    assert_eq!(std::fs::read(&exe).unwrap(), b"old hub");
    assert!(matches!(fx.eng.hub_status(), Status::Failed(_)));
}

#[test]
fn a_download_that_does_not_match_its_hash_is_never_staged() {
    let (mut fx, exe) = world("own-badhash");
    publish_hub(&fx, "0.2.0", b"new hub bytes");
    // The release's file is not what its manifest says.
    std::fs::write(
        hub_path(&fx, "releases/download/v0.2.0/hub.exe"),
        b"not the file",
    )
    .unwrap();
    fx.check();
    assert!(matches!(fx.eng.hub_status(), Status::Failed(_)));
    assert!(!staged_of(&exe).exists());
    assert!(fx.vm().banner.is_none());
}

#[test]
fn a_copy_that_cannot_update_itself_never_asks_for_hubs_manifest() {
    let mut fx = Fx::new("own-none");
    fx.eng.set_tray_up(true);
    publish_hub(&fx, "0.2.0", b"new hub bytes");
    fx.check();
    assert!(hub_urls(&fx).is_empty(), "{:?}", hub_urls(&fx));
    assert!(matches!(fx.eng.hub_status(), Status::None));
    assert!(fx.vm().banner.is_none());
}

#[test]
fn hubs_own_update_counts_in_the_badge_but_not_in_update_all() {
    let (mut fx, _exe) = world("own-badge");
    let now = check::now();
    assert_eq!(fx.eng.tray_snapshot(now).badge(), 0);
    publish_hub(&fx, "0.2.0", b"new hub bytes");
    fx.check();
    let s = fx.eng.tray_snapshot(check::now());
    assert!(s.hub_update);
    assert_eq!((s.updates, s.badge()), (0, 1));
    assert!(
        !crate::tray::menu(&s)
            .iter()
            .any(|e| matches!(e, crate::tray::Entry::Item { label, .. } if label.starts_with("Update all"))),
        "no apps to update"
    );
}

#[test]
fn a_click_that_answers_an_app_banner_never_answers_hubs_restart_bar() {
    let (mut fx, exe) = world("own-banner-double");
    publish_hub(&fx, "0.2.0", b"new hub bytes");
    fx.check();
    // Hub's bar is showing, and its buttons are Hub's own commands.
    let vm = fx.vm();
    assert!(vm.banner.as_ref().is_some_and(|b| b.hub));
    assert_eq!(command(Hit::BannerYes, &vm), Some(Cmd::HubRestart));
    assert_eq!(command(Hit::BannerNo, &vm), Some(Cmd::HubLater));
    // The second click of a double-click on an app's "Close and update" arrives after that bar
    // is gone and Hub's has taken its place: it answers nothing.
    fx.cmd(Cmd::BannerYes);
    assert!(
        fx.eng.take_restart().is_none(),
        "no restart nobody asked for"
    );
    assert_eq!(std::fs::read(&exe).unwrap(), b"old hub");
    assert!(matches!(fx.eng.hub_status(), Status::Ready(_)));
    fx.cmd(Cmd::BannerNo);
    assert!(fx.vm().banner.is_some(), "and does not dismiss it either");
    // An app's bar is the other command.
    let app = crate::vm::Banner::default();
    let mut vm = fx.vm();
    vm.banner = Some(app);
    assert_eq!(command(Hit::BannerYes, &vm), Some(Cmd::BannerYes));
    assert_eq!(command(Hit::BannerNo, &vm), Some(Cmd::BannerNo));
}

#[test]
fn a_failed_look_at_hubs_manifest_does_not_make_the_same_update_news_again() {
    let (mut fx, exe) = world("tell-hub-failed");
    let bad_exe = hub_path(&fx, "releases/download/v0.2.0/hub.exe");
    publish_hub(&fx, "0.2.0", b"new hub bytes");
    std::fs::write(&bad_exe, b"not the file").unwrap();
    fx.check();
    assert!(matches!(fx.eng.hub_status(), Status::Failed(_)));
    assert_eq!(fx.eng.take_message().unwrap().body, "1 update: Hub");
    // Hub's manifest cannot be read at the next check: Hub's status stays failed, nothing said.
    let manifest = hub_path(&fx, "releases/latest/download/hub-update.txt");
    let kept = std::fs::read(&manifest).unwrap();
    std::fs::remove_file(&manifest).unwrap();
    fx.check();
    assert!(matches!(fx.eng.hub_status(), Status::Failed(_)));
    assert!(fx.eng.take_message().is_none());
    // It is readable again and offers the same version: the user was told already.
    std::fs::write(&manifest, kept).unwrap();
    fx.check();
    assert!(
        fx.eng.take_message().is_none(),
        "the same Hub, told once, is not told again"
    );
    assert!(!staged_of(&exe).exists());
}

#[test]
fn no_second_check_starts_while_hubs_own_download_is_still_running() {
    let mut fx = Fx::with_delay("own-double-download", Duration::from_millis(400));
    let exe = scratch_exe(&fx);
    fx.eng.set_hub_exe(Some(exe.clone()));
    publish_hub(&fx, "0.2.0", b"new hub bytes");
    fx.cmd(Cmd::CheckNow);
    // Until the check has reported (the apps' answers come after Hub's manifest) and Hub's
    // download, which goes on after that, is under way.
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while !(matches!(fx.eng.hub_status(), Status::Downloading(_)) && !fx.eng.checking()) {
        let ev = fx.rx.recv_timeout(Duration::from_millis(100));
        if let Ok(ev) = ev {
            fx.eng.handle(ev);
        }
        assert!(std::time::Instant::now() < deadline, "never downloading");
    }
    assert!(!fx.eng.checking(), "the check itself has reported");
    // A manual check (F5, the tray) or a timed one now would write the same file twice.
    fx.cmd(Cmd::CheckNow);
    assert!(!fx.eng.checking(), "no second check");
    assert_eq!(fx.eng.wait(check::now()), None, "and no timer to start one");
    fx.settle();
    let downloads = hub_urls(&fx)
        .iter()
        .filter(|u| u.contains("/releases/download/"))
        .count();
    assert_eq!(downloads, 1, "downloaded once");
    assert!(matches!(fx.eng.hub_status(), Status::Ready(_)));
    // Once it is done a check runs as usual.
    fx.cmd(Cmd::CheckNow);
    assert!(fx.eng.checking());
    fx.settle();
}

#[test]
fn the_card_button_restarts_into_a_ready_update_or_looks_for_one() {
    let (mut fx, exe) = world("own-button");
    // Nothing found yet: the button checks (Hub's manifest is the first request).
    publish_hub(&fx, "0.2.0", b"new hub bytes");
    assert!(fx.eng.setting_action(UPDATE_HUB));
    assert!(fx.eng.checking());
    fx.settle();
    assert!(matches!(fx.eng.hub_status(), Status::Ready(_)));
    let rows = Form::new("s", fx.eng.settings_rows());
    assert!(
        rows.rows().iter().any(|r| matches!(
            r,
            mdshared_ui::settings::Row::Action { button, label, .. }
                if button == "Restart now" && label.contains("0.2.0 is ready")
        )),
        "{:?}",
        rows.rows()
    );
    // Ready: the same button restarts.
    assert!(fx.eng.setting_action(UPDATE_HUB));
    assert_eq!(fx.eng.take_restart().as_deref(), Some(exe.as_path()));
    assert!(!fx.eng.setting_action(99), "not a button of the card");
}

// ---- telling the user ----

/// Installs mdedit and mdview 1.0.0 into the fixture's programs folder.
fn with_two_apps(name: &str) -> Fx {
    let mut fx = Fx::new(name);
    fx.eng.set_tray_up(true);
    fx.publish_files("mdedit", "1.0.0", &[("mdedit.exe", body("e1"))], None);
    fx.publish_files("mdview", "1.0.0", &[("mdview.exe", body("v1"))], None);
    fx.check();
    fx.cmd(Cmd::Install("mdedit"));
    fx.settle();
    fx.cmd(Cmd::Install("mdview"));
    fx.settle();
    assert!(fx.eng.take_message().is_none(), "nothing was waiting");
    fx
}

fn release(fx: &Fx, id: &str, version: &str) {
    let exe = format!("{id}.exe");
    fx.publish_files(id, version, &[(exe.as_str(), body(version))], None);
}

#[test]
fn one_notification_per_check_that_found_something_new() {
    let mut fx = with_two_apps("tell-rule");
    release(&fx, "mdedit", "1.1.0");
    fx.check();
    let m = fx.eng.take_message().expect("news");
    assert_eq!(m.title, "Hub");
    assert_eq!(m.body, "1 update: mdedit");
    assert!(fx.eng.take_message().is_none(), "told once");
    // The same set again: nothing new, nothing said.
    fx.check();
    assert!(fx.eng.take_message().is_none());
    // Another app: the whole set is in the one message.
    release(&fx, "mdview", "1.1.0");
    fx.check();
    assert_eq!(
        fx.eng.take_message().unwrap().body,
        "2 updates: mdedit, mdview"
    );
    // A smaller set (one was updated): silent. Everything updated: silent.
    fx.cmd(Cmd::UpdateAll);
    fx.settle();
    fx.check();
    assert!(fx.eng.take_message().is_none());
    // An update that comes back after it left is news again.
    release(&fx, "mdedit", "1.2.0");
    fx.check();
    assert_eq!(fx.eng.take_message().unwrap().body, "1 update: mdedit");
}

#[test]
fn what_was_told_is_remembered_across_a_restart() {
    let mut fx = with_two_apps("tell-memory");
    release(&fx, "mdedit", "1.1.0");
    fx.check();
    assert!(fx.eng.take_message().is_some());
    assert!(!fx.eng.store.notified().is_empty(), "saved");
    fx.restart();
    fx.eng.set_tray_up(true);
    fx.check();
    assert!(
        fx.eng.take_message().is_none(),
        "the same update is not told again after a restart"
    );
}

#[test]
fn no_notification_when_off_with_the_window_focused_or_with_no_icon() {
    let mut fx = with_two_apps("tell-off");
    // Off: nothing, and nothing is marked as told, so turning it on tells at the next check.
    fx.eng.set_notify(false);
    release(&fx, "mdedit", "1.1.0");
    fx.check();
    assert!(fx.eng.take_message().is_none());
    assert!(fx.eng.store.notified().is_empty());
    fx.eng.set_notify(true);
    fx.check();
    assert_eq!(fx.eng.take_message().unwrap().body, "1 update: mdedit");
    // The window is open and focused: the user is looking at it, and that counts as told.
    release(&fx, "mdedit", "1.2.0");
    fx.eng.set_focused(true);
    fx.check();
    assert!(fx.eng.take_message().is_none());
    fx.eng.set_focused(false);
    fx.check();
    assert!(fx.eng.take_message().is_none(), "already told");
    // No icon to tell it with: nothing, and the news waits for one.
    release(&fx, "mdedit", "1.3.0");
    fx.eng.set_tray_up(false);
    fx.check();
    assert!(fx.eng.take_message().is_none());
    fx.eng.set_tray_up(true);
    fx.check();
    assert_eq!(fx.eng.take_message().unwrap().body, "1 update: mdedit");
}

#[test]
fn hubs_own_update_is_in_the_notification_first() {
    let (mut fx, _exe) = world("tell-hub");
    fx.publish_files("mdedit", "1.0.0", &[("mdedit.exe", body("e1"))], None);
    publish_hub(&fx, "0.2.0", b"new hub bytes");
    fx.check();
    let m = fx.eng.take_message().expect("news");
    assert!(m.body.starts_with("1 update: Hub"), "{}", m.body);
}

#[test]
fn finished_and_failed_updates_are_not_notified() {
    let mut fx = with_two_apps("tell-finished");
    release(&fx, "mdedit", "1.1.0");
    fx.check();
    let _ = fx.eng.take_message();
    fx.cmd(Cmd::UpdateAll);
    fx.settle();
    // Installs end with no message of their own.
    assert!(fx.eng.take_message().is_none());
    while let Ok(ev) = fx.rx.try_recv() {
        fx.eng.handle(ev);
        assert!(fx.eng.take_message().is_none());
    }
}

// ---- settings ----

#[cfg(windows)]
struct TestKey(String);

#[cfg(windows)]
impl TestKey {
    fn new() -> TestKey {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        TestKey(format!(
            r"Software\HubTest\e2e-{}-{nanos}",
            std::process::id()
        ))
    }
}

#[cfg(windows)]
impl Drop for TestKey {
    fn drop(&mut self) {
        // Only ever this test's own key, under HubTest; the empty HubTest goes with it.
        assert!(hub_core::autostart::remove_test_key(&self.0));
    }
}

#[cfg(windows)]
#[test]
fn start_with_windows_writes_and_removes_the_run_value_of_the_test_key() {
    use hub_core::autostart;
    let mut fx = Fx::new("set-start");
    let key = TestKey::new();
    let exe = scratch_exe(&fx);
    fx.eng.set_run_key(RunKey {
        key: key.0.clone(),
        value: "Hub".into(),
        exe: Some(exe.clone()),
    });
    assert!(!fx.eng.autostart_on());
    assert!(fx.eng.change_setting(START, Value::Bool(true)));
    assert!(autostart::is_enabled_at(&key.0, "Hub"));
    assert_eq!(
        autostart::command_at(&key.0, "Hub"),
        Some(format!("\"{}\" --tray", exe.display()))
    );
    // The card, the tray menu and the settings record all say on.
    assert!(fx.eng.autostart_on());
    assert!(fx.eng.tray_snapshot(check::now()).start_with_windows);
    assert_eq!(
        Form::new("s", fx.eng.settings_rows()).value(START),
        Some(Value::Bool(true))
    );
    assert!(fx.eng.store.autostart_enabled());
    // Off removes it again.
    assert!(fx.eng.change_setting(START, Value::Bool(false)));
    assert!(!autostart::is_enabled_at(&key.0, "Hub"));
    assert!(!fx.eng.autostart_on());
    assert!(!fx.eng.store.autostart_enabled());
}

#[cfg(windows)]
#[test]
fn a_copy_with_no_exe_to_start_refuses_start_with_windows_and_says_why() {
    let mut fx = Fx::new("set-start-refused");
    let key = TestKey::new();
    fx.eng.set_run_key(RunKey {
        key: key.0.clone(),
        value: "Hub".into(),
        exe: None,
    });
    assert!(fx.eng.change_setting(START, Value::Bool(true)));
    assert!(!hub_core::autostart::is_enabled_at(&key.0, "Hub"));
    assert!(!fx.eng.autostart_on());
    assert!(
        fx.vm()
            .checked
            .starts_with("Could not change Start with Windows"),
        "{}",
        fx.vm().checked
    );
}

#[cfg(windows)]
#[test]
fn a_copy_with_no_exe_does_not_touch_the_startup_list_when_turning_it_off_either() {
    let mut fx = Fx::new("set-start-off-refused");
    let key = TestKey::new();
    // Someone (the installed Hub) wrote the value; this copy is a build folder's.
    hub_core::autostart::enable_at(&key.0, "Hub", &scratch_exe(&fx)).unwrap();
    fx.eng.set_run_key(RunKey {
        key: key.0.clone(),
        value: "Hub".into(),
        exe: None,
    });
    assert!(fx.eng.change_setting(START, Value::Bool(false)));
    assert!(
        hub_core::autostart::is_enabled_at(&key.0, "Hub"),
        "the installed Hub's value is not this copy's to remove"
    );
    assert!(
        fx.vm()
            .checked
            .starts_with("Could not change Start with Windows"),
        "{}",
        fx.vm().checked
    );
}

#[test]
fn the_run_key_a_build_gets_is_never_the_real_one_unless_it_is_installed() {
    use crate::glue::run_key_for;
    use hub_core::autostart::{RUN_KEY, TEST_ROOT};
    let scratch = Some(PathBuf::from(r"C:\scratch\h\hub.exe"));
    let test_key = format!("{TEST_ROOT}\\unique-1");
    // A normal build: the real key for the installed Hub, none for a build folder's copy.
    let k = run_key_for(false, None, scratch.clone(), scratch.clone());
    assert_eq!((k.key.as_str(), k.exe.clone()), (RUN_KEY, scratch.clone()));
    assert_eq!(run_key_for(false, None, scratch.clone(), None).key, "");
    // A test-hooks build (a scratch copy of the exe counts as installed): no key without the
    // variable, the test key with it, and never the real Run key or anything outside HubTest.
    assert_eq!(
        run_key_for(true, None, scratch.clone(), scratch.clone()).key,
        ""
    );
    let k = run_key_for(
        true,
        Some(test_key.clone()),
        scratch.clone(),
        scratch.clone(),
    );
    assert_eq!((k.key, k.exe), (test_key, scratch.clone()));
    for bad in [RUN_KEY, TEST_ROOT, "Software\\HubTestX\\a", ""] {
        let k = run_key_for(
            true,
            Some(bad.to_string()),
            scratch.clone(),
            scratch.clone(),
        );
        assert_eq!((k.key.as_str(), k.exe), ("", None), "{bad:?}");
    }
}

#[test]
fn the_check_interval_setting_changes_when_the_next_check_is_due() {
    let mut fx = Fx::new("set-interval");
    fx.check();
    let now = check::now();
    let hours = |d: Duration| d.as_secs() as f64 / 3600.0;
    // The default is every 6 hours.
    let w = fx.eng.wait(now).expect("a timer");
    assert!(hours(w) > 5.0 && hours(w) <= 6.0, "{w:?}");
    // Every hour.
    assert!(fx.eng.change_setting(INTERVAL, Value::Choice(0)));
    let w = fx.eng.wait(now).expect("a timer");
    assert!(hours(w) <= 1.0, "{w:?}");
    // Every week.
    assert!(fx.eng.change_setting(INTERVAL, Value::Choice(3)));
    let w = fx.eng.wait(now).expect("a timer");
    assert!(hours(w) > 24.0 * 6.0, "{w:?}");
    // Never: no timer at all.
    assert!(fx.eng.change_setting(INTERVAL, Value::Choice(4)));
    assert_eq!(fx.eng.wait(now), None);
    assert_eq!(fx.eng.store.interval_hours(), 0, "saved");
    // Not a choice: refused, nothing changed.
    assert!(!fx.eng.change_setting(INTERVAL, Value::Choice(9)));
    assert!(!fx.eng.change_setting(INTERVAL, Value::Bool(true)));
    assert_eq!(fx.eng.interval_hours(), 0);
    // The next start reads it back.
    fx.restart();
    assert_eq!(fx.eng.interval_hours(), 0);
    assert_eq!(
        Form::new("s", fx.eng.settings_rows()).value(INTERVAL),
        Some(Value::Choice(4))
    );
}

#[test]
fn the_tray_menu_and_the_card_change_the_same_settings() {
    let mut fx = Fx::new("set-sync");
    let card = |fx: &Fx, id| Form::new("s", fx.eng.settings_rows()).value(id);
    assert_eq!(card(&fx, NOTIFY), Some(Value::Bool(true)));
    assert!(fx.eng.tray_snapshot(check::now()).notify);
    // The menu's "Notify about updates" goes through the engine: the card follows, and it is saved.
    fx.eng.set_notify(false);
    assert_eq!(card(&fx, NOTIFY), Some(Value::Bool(false)));
    assert!(!fx.eng.tray_snapshot(check::now()).notify);
    assert!(!fx.eng.store.notify_enabled());
    // The card's toggle: the menu follows.
    assert!(fx.eng.change_setting(NOTIFY, Value::Bool(true)));
    assert!(fx.eng.tray_snapshot(check::now()).notify);
    assert!(fx.eng.store.notify_enabled());
    // The tray icon setting is saved and read back by the next start.
    assert!(fx.eng.change_setting(TRAY, Value::Bool(false)));
    assert!(!fx.eng.tray_on());
    assert!(!fx.eng.store.tray_enabled());
    fx.restart();
    assert!(!fx.eng.tray_on());
    assert!(matches!(card(&fx, TRAY), Some(Value::Bool(false))));
    // A value of the wrong kind, or an unknown id, changes nothing.
    assert!(!fx.eng.change_setting(TRAY, Value::Choice(1)));
    assert!(!fx.eng.change_setting(77, Value::Bool(true)));
}

#[test]
fn the_restart_flag_is_part_of_the_command_line() {
    let a = crate::cli::parse(&["--restarted".to_string(), "--tray".to_string()]).unwrap();
    assert!(a.restarted && a.tray);
    assert!(!crate::cli::parse(&[]).unwrap().restarted);
}

/// Silences the unused-import lint of `Event` when a build leaves a test out.
#[allow(dead_code)]
fn _uses(_: Event) {}
