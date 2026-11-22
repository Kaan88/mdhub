//! The tray: the notification-area icon, its balloon and its menu, and the rules for when the
//! window exists.
//!
//! Two halves. The first (everything above `mod os`) is pure and tested without a window: what a
//! menu holds for a snapshot of the engine ([`menu`]), what a click on the icon means
//! ([`classify`]), the tooltip, and the window's life as a state machine ([`step`]). The second
//! (`os`, Windows only) is the icon itself on a thread of its own.
//!
//! **The tray thread** owns one hidden top-level window (never shown, no UI; not a message-only
//! window, which is not sent the broadcast `TaskbarCreated` that tells the icon to come back after
//! Explorer restarts) and a `GetMessageW` loop, so it sleeps in the kernel until something
//! happens: no timer, no polling. It owns no
//! state of Hub's besides a [`Snapshot`] the engine hands it (`Tray::update`); a click is turned
//! into a [`TrayCmd`] and posted to the winit loop as `Event::Tray`, which acts on it. The menu
//! and the balloon are the system's (`TrackPopupMenu`, `NIF_INFO`).
//!
//! **Narrow interface** (what the self-update, settings and notification work calls):
//! `Tray::start(post, snapshot)`, `Tray::update(snapshot)`, `Tray::show_balloon(title, text)`,
//! `Tray::stop()` (also on drop), and the commands in [`TrayCmd`].
//!
//! **When the window exists** (`step`): started with `--tray` there is none; "Open Hub", a click
//! on the icon, a click on the balloon or a second launch makes one; closing it (the X, Alt+F4)
//! destroys it and keeps the process while the tray is on; "Quit Hub" ends the process. Without
//! `--tray` the window opens first and, with the tray on (setting `tray`, default on, and no
//! `--no-tray`), closing it keeps the tray; with the tray off closing it quits.
use crate::glue::Post;
use std::sync::Arc;

/// What the user asked for through the icon. Acted on by the winit loop, never by the tray thread.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayCmd {
    /// Open the window (or raise it).
    Open,
    UpdateAll,
    CheckNow,
    /// Start this installed app (its catalog id).
    OpenApp(&'static str),
    ToggleStart,
    ToggleNotify,
    Quit,
    /// The balloon was clicked: opens the window.
    BalloonClicked,
}

/// Everything the menu and the tooltip are built from. The engine produces it; the tray keeps the
/// last one it was given.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// How many apps have an update waiting: the badge, "Update all (N)".
    pub updates: u32,
    /// Hub's own update is waiting (offered, downloading or ready): it counts in the badge and
    /// the tooltip, not in "Update all".
    pub hub_update: bool,
    /// Display names of those apps, for whoever composes the balloon.
    pub update_names: Vec<String>,
    /// Installed apps in catalog order: (id, display name).
    pub apps: Vec<(&'static str, String)>,
    /// "Start with Windows" is on.
    pub start_with_windows: bool,
    /// "Notify about updates" is on.
    pub notify: bool,
}

impl Snapshot {
    /// What the badge shows and the tooltip counts: the apps, and Hub itself.
    pub fn badge(&self) -> u32 {
        self.updates + u32::from(self.hub_update)
    }
}

/// One line of the menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Entry {
    Item {
        cmd: TrayCmd,
        label: String,
        checked: bool,
        enabled: bool,
    },
    Separator,
}

fn item(cmd: TrayCmd, label: impl Into<String>, checked: bool) -> Entry {
    Entry::Item {
        cmd,
        label: label.into(),
        checked,
        enabled: true,
    }
}

/// The menu for `s`: Open Hub; Update all (N) only when N > 0; Check now; the installed apps; the
/// two settings; Quit Hub.
pub fn menu(s: &Snapshot) -> Vec<Entry> {
    let mut m = vec![item(TrayCmd::Open, "Open Hub", false)];
    if s.updates > 0 {
        m.push(item(
            TrayCmd::UpdateAll,
            format!("Update all ({})", s.updates),
            false,
        ));
    }
    m.push(item(TrayCmd::CheckNow, "Check now", false));
    if !s.apps.is_empty() {
        m.push(Entry::Separator);
        for (id, name) in &s.apps {
            // `&` would make the next letter a keyboard mnemonic.
            m.push(item(TrayCmd::OpenApp(id), name.replace('&', "&&"), false));
        }
    }
    m.push(Entry::Separator);
    m.push(item(
        TrayCmd::ToggleStart,
        "Start with Windows",
        s.start_with_windows,
    ));
    m.push(item(
        TrayCmd::ToggleNotify,
        "Notify about updates",
        s.notify,
    ));
    m.push(Entry::Separator);
    m.push(item(TrayCmd::Quit, "Quit Hub", false));
    m
}

/// What the icon's tooltip says. `suffix` is the test-run tag (`HUB_TRAY_TIP`).
pub fn tooltip(updates: u32, suffix: &str) -> String {
    let mut t = match updates {
        0 => "Hub".to_string(),
        1 => "Hub: 1 update available".to_string(),
        n => format!("Hub: {n} updates available"),
    };
    if !suffix.is_empty() {
        t.push(' ');
        t.push_str(suffix);
    }
    fit16(&t, 120)
}

/// `s` cut to at most `max` UTF-16 units, never inside a character: the notification area's
/// strings are fixed arrays (tip 128, balloon title 64, balloon text 256, each with its NUL).
pub fn fit16(s: &str, max: usize) -> String {
    let mut out = String::new();
    let mut n = 0;
    for c in s.chars() {
        n += c.len_utf16();
        if n > max {
            break;
        }
        out.push(c);
    }
    out
}

/// What a message the icon sent means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Click {
    Open,
    Menu,
    Balloon,
}

pub const WM_LBUTTONUP: u32 = 0x0202;
pub const WM_RBUTTONUP: u32 = 0x0205;
pub const WM_CONTEXTMENU: u32 = 0x007B;
pub const NIN_SELECT: u32 = 0x0400;
pub const NIN_KEYSELECT: u32 = 0x0401;
pub const NIN_BALLOONUSERCLICK: u32 = 0x0405;

/// The meaning of the event in the low word of the icon's callback `lParam`. With
/// `NOTIFYICON_VERSION_4` (`v4`) a left click is `NIN_SELECT` (or `NIN_KEYSELECT` from the
/// keyboard) and the menu is `WM_CONTEXTMENU`; the raw mouse messages are ignored then, so a click
/// is not taken twice. Without it the raw mouse messages are the clicks.
pub fn classify(v4: bool, event: u32) -> Option<Click> {
    match (v4, event) {
        (_, NIN_BALLOONUSERCLICK) => Some(Click::Balloon),
        (true, NIN_SELECT | NIN_KEYSELECT) => Some(Click::Open),
        (true, WM_CONTEXTMENU) => Some(Click::Menu),
        (false, WM_LBUTTONUP) => Some(Click::Open),
        (false, WM_RBUTTONUP) => Some(Click::Menu),
        _ => None,
    }
}

// ---- when the window exists --------------------------------------------------------------------

/// What exists now: a window, and a tray that keeps the process alive without one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Life {
    pub window: bool,
    pub tray: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    /// A tray click, the menu, the balloon or a second launch asked for the window.
    Open,
    /// The window's X or Alt+F4.
    CloseWindow,
    /// "Quit Hub" or a quit request from another launch.
    Quit,
    /// A window was asked for and could not be made.
    OpenFailed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Do {
    Create,
    Raise,
    Destroy,
    /// End the process.
    Exit,
    Nothing,
}

/// The whole of the window's life. Pure, so it is tested, and so the loop only carries it out.
pub fn step(l: Life, i: Input) -> (Life, Do) {
    match i {
        Input::Open if l.window => (l, Do::Raise),
        Input::Open => (Life { window: true, ..l }, Do::Create),
        Input::CloseWindow if !l.window => (l, Do::Nothing),
        Input::CloseWindow if l.tray => (Life { window: false, ..l }, Do::Destroy),
        Input::CloseWindow => (Life { window: false, ..l }, Do::Exit),
        Input::Quit => (Life { window: false, ..l }, Do::Exit),
        Input::OpenFailed if l.tray => (Life { window: false, ..l }, Do::Nothing),
        Input::OpenFailed => (Life { window: false, ..l }, Do::Exit),
    }
}

/// Is there to be a tray icon? `--no-tray` says no, `--tray` says yes (a window-less Hub must
/// have one), otherwise the setting `tray` (default on). A `--demo` picture never has one.
pub fn wants_tray(flag: bool, no_flag: bool, setting: bool, demo: bool) -> bool {
    !demo && !no_flag && (flag || setting)
}

#[cfg(windows)]
pub use os::Tray;

/// The test-run tag for the tooltip (`HUB_TRAY_TIP`), so a test's icon is told from the owner's.
pub fn tip_suffix() -> String {
    std::env::var("HUB_TRAY_TIP").unwrap_or_default()
}

#[cfg(windows)]
mod os {
    use super::*;
    use crate::glue::Event;
    use crate::icon;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, AtomicUsize, Ordering};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Shell::{
        NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIIF_LARGE_ICON, NIIF_USER, NIM_ADD,
        NIM_DELETE, NIM_MODIFY, NIM_SETVERSION, NOTIFYICON_VERSION_4, NOTIFYICONDATAW,
        Shell_NotifyIconW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
        DispatchMessageW, GetCursorPos, GetMessageW, KillTimer, MF_CHECKED, MF_GRAYED,
        MF_SEPARATOR, MF_STRING, MSG, PostMessageW, PostQuitMessage, RegisterClassExW,
        RegisterWindowMessageW, SetForegroundWindow, SetMenuDefaultItem, SetTimer, TPM_BOTTOMALIGN,
        TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage, WM_APP,
        WM_DESTROY, WM_NULL, WM_TIMER, WNDCLASSEXW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_POPUP,
    };

    /// The icon's callback message, the "something changed, look at the shared state" wake-up, and
    /// the stop request.
    const WM_TRAY: u32 = WM_APP + 1;
    const WM_SYNC: u32 = WM_APP + 2;
    const WM_STOP: u32 = WM_APP + 3;
    const ICON_ID: u32 = 1;
    /// How often adding the icon is retried while the shell has no taskbar yet (a login that
    /// starts Hub before Explorer), and how many times. `TaskbarCreated` normally gets there first.
    const RETRY_MS: u32 = 3000;
    const RETRIES: u32 = 20;
    const RETRY_TIMER: usize = 1;

    struct Inner {
        snap: Snapshot,
        balloon: Option<(String, String)>,
    }

    /// What the engine side and the tray thread share.
    struct Shared {
        hwnd: AtomicIsize,
        inner: Mutex<Inner>,
        tip_suffix: String,
        /// `Tray::start` gave up waiting: the thread ends itself instead of running on unseen.
        cancelled: AtomicBool,
        /// How many times `TaskbarCreated` made the icon come back.
        readds: AtomicU32,
    }

    /// Tray threads running now (a test asks that none is left behind).
    static LIVE: AtomicUsize = AtomicUsize::new(0);

    /// The balloon to show now, if there is an icon to show it from. Without one (the shell has no
    /// taskbar yet, an add is being retried) it stays where it is, to be shown when the icon is
    /// added: the engine already counts it as told.
    fn take_balloon(inner: &mut Inner, added: bool) -> Option<(String, String)> {
        if added { inner.balloon.take() } else { None }
    }

    /// The tray thread's own state, reached from the window procedure.
    /// Side of the icon the balloon shows: Windows scales it down, never up.
    const BALLOON_ICON_SIDE: u32 = 256;

    struct Ctx {
        shared: Arc<Shared>,
        post: Arc<dyn Post>,
        hwnd: Cell<isize>,
        taskbar_created: u32,
        icon: Cell<isize>,
        /// The big icon of the last balloon (256 px), kept until the next or the end.
        balloon_icon: Cell<isize>,
        count: Cell<u32>,
        added: Cell<bool>,
        v4: Cell<bool>,
        tries: Cell<u32>,
        tip: RefCell<String>,
    }

    thread_local! {
        static CTX: RefCell<Option<Rc<Ctx>>> = const { RefCell::new(None) };
    }

    fn ctx() -> Option<Rc<Ctx>> {
        CTX.try_with(|c| c.try_borrow().ok().and_then(|c| c.clone()))
            .ok()
            .flatten()
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// `s` into a fixed array, cut to fit and ended with a NUL.
    fn copy_wide(dst: &mut [u16], s: &str) {
        let n = dst.len().saturating_sub(1);
        let mut i = 0;
        for u in s.encode_utf16() {
            if i >= n {
                break;
            }
            dst[i] = u;
            i += 1;
        }
        if let Some(z) = dst.get_mut(i) {
            *z = 0;
        }
    }

    fn nid(c: &Ctx, flags: u32) -> NOTIFYICONDATAW {
        // SAFETY: an all-zero NOTIFYICONDATAW is a valid value (null handles, empty strings).
        let mut d: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        d.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        d.hWnd = c.hwnd.get() as HWND;
        d.uID = ICON_ID;
        d.uFlags = flags;
        d.uCallbackMessage = WM_TRAY;
        d.hIcon = c.icon.get() as _;
        copy_wide(&mut d.szTip, &c.tip.borrow());
        d
    }

    fn current(c: &Ctx) -> (u32, String) {
        let n = c.shared.inner.lock().map_or(0, |g| g.snap.badge());
        (n, tooltip(n, &c.shared.tip_suffix))
    }

    /// Makes the icon for the current count (replacing, and freeing, the last one).
    fn rebuild_icon(c: &Ctx, count: u32) {
        let new = icon::hicon(count);
        if new != 0 {
            icon::destroy(c.icon.replace(new));
            c.count.set(count);
        }
    }

    fn add_icon(c: &Ctx) {
        let (n, tip) = current(c);
        *c.tip.borrow_mut() = tip;
        rebuild_icon(c, n);
        let mut d = nid(c, NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP);
        // SAFETY: `d` is fully initialised and outlives the call.
        let ok = unsafe { Shell_NotifyIconW(NIM_ADD, &d) } != 0;
        c.added.set(ok);
        if ok {
            d.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            // SAFETY: as above.
            let v4 = unsafe { Shell_NotifyIconW(NIM_SETVERSION, &d) } != 0;
            c.v4.set(v4);
            // SAFETY: the timer belongs to our own window.
            unsafe { KillTimer(c.hwnd.get() as HWND, RETRY_TIMER) };
            // A balloon that came while there was no icon.
            deliver_balloon(c);
        } else if c.tries.get() < RETRIES {
            // SAFETY: as above.
            unsafe { SetTimer(c.hwnd.get() as HWND, RETRY_TIMER, RETRY_MS, None) };
        }
    }

    fn delete_icon(c: &Ctx) {
        if c.added.replace(false) {
            let d = nid(c, 0);
            // SAFETY: as in `add_icon`.
            unsafe { Shell_NotifyIconW(NIM_DELETE, &d) };
        }
        icon::destroy(c.icon.replace(0));
        icon::destroy(c.balloon_icon.replace(0));
    }

    fn deliver_balloon(c: &Ctx) {
        let balloon = c
            .shared
            .inner
            .lock()
            .ok()
            .and_then(|mut g| take_balloon(&mut g, c.added.get()));
        if let Some((title, text)) = balloon {
            let mut d = nid(c, NIF_INFO);
            copy_wide(&mut d.szInfoTitle, &fit16(&title, 63));
            copy_wide(&mut d.szInfo, &fit16(&text, 255));
            // A large icon of its own: the toast shows hBalloonIcon at its own size, and the
            // 16-pixel tray icon stretched to it is what looked blurry.
            let big = icon::hicon_sized(BALLOON_ICON_SIDE, 0);
            d.dwInfoFlags = if big != 0 {
                NIIF_USER | NIIF_LARGE_ICON
            } else {
                NIIF_USER
            };
            d.hBalloonIcon = big as _;
            // SAFETY: as in `add_icon`.
            unsafe { Shell_NotifyIconW(NIM_MODIFY, &d) };
            icon::destroy(c.balloon_icon.replace(big));
        }
    }

    /// The engine changed the snapshot or asked for a balloon: bring the icon in line.
    fn sync(c: &Ctx) {
        if !c.added.get() {
            return; // `add_icon` reads the shared state (and the balloon) when it succeeds
        }
        let (n, tip) = current(c);
        if n != c.count.get() || *c.tip.borrow() != tip {
            *c.tip.borrow_mut() = tip;
            if n != c.count.get() {
                rebuild_icon(c, n);
            }
            let d = nid(c, NIF_ICON | NIF_TIP | NIF_SHOWTIP);
            // SAFETY: as in `add_icon`.
            unsafe { Shell_NotifyIconW(NIM_MODIFY, &d) };
        }
        deliver_balloon(c);
    }

    fn popup(c: &Ctx) {
        let snap = c
            .shared
            .inner
            .lock()
            .map(|g| g.snap.clone())
            .unwrap_or_default();
        let entries = menu(&snap);
        // SAFETY: plain menu calls on handles made here; the labels are NUL-ended and live
        // across each call; the menu is destroyed before returning.
        let picked = unsafe {
            let m = CreatePopupMenu();
            if m.is_null() {
                return;
            }
            for (i, e) in entries.iter().enumerate() {
                match e {
                    Entry::Separator => {
                        AppendMenuW(m, MF_SEPARATOR, 0, std::ptr::null());
                    }
                    Entry::Item {
                        label,
                        checked,
                        enabled,
                        ..
                    } => {
                        let mut f = MF_STRING;
                        if *checked {
                            f |= MF_CHECKED;
                        }
                        if !*enabled {
                            f |= MF_GRAYED;
                        }
                        AppendMenuW(m, f, i + 1, wide(label).as_ptr());
                    }
                }
            }
            SetMenuDefaultItem(m, 0, 1);
            let hwnd = c.hwnd.get() as HWND;
            let mut at = POINT { x: 0, y: 0 };
            GetCursorPos(&mut at);
            // The menu closes on a click elsewhere only if its window is in front.
            SetForegroundWindow(hwnd);
            let id = TrackPopupMenu(
                m,
                TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN,
                at.x,
                at.y,
                0,
                hwnd,
                std::ptr::null(),
            );
            PostMessageW(hwnd, WM_NULL, 0, 0);
            DestroyMenu(m);
            id
        };
        if picked > 0
            && let Some(Entry::Item { cmd, .. }) = entries.get(picked as usize - 1)
        {
            c.post.post(Event::Tray(*cmd));
        }
    }

    unsafe extern "system" fn wndproc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        if let Some(c) = ctx() {
            if msg == c.taskbar_created && msg != 0 {
                // Explorer restarted: its icons are gone. (A taskbar message with the icon still
                // there is dropped first, so the add below cannot fail on a duplicate.)
                if c.added.replace(false) {
                    let d = nid(&c, 0);
                    // SAFETY: as in `add_icon`.
                    unsafe { Shell_NotifyIconW(NIM_DELETE, &d) };
                }
                c.tries.set(0);
                c.shared.readds.fetch_add(1, Ordering::SeqCst);
                add_icon(&c);
                return 0;
            }
            match msg {
                WM_TRAY => {
                    match classify(c.v4.get(), (lparam as u32) & 0xFFFF) {
                        Some(Click::Open) => c.post.post(Event::Tray(TrayCmd::Open)),
                        Some(Click::Balloon) => c.post.post(Event::Tray(TrayCmd::BalloonClicked)),
                        Some(Click::Menu) => popup(&c),
                        None => {}
                    }
                    return 0;
                }
                WM_SYNC => {
                    sync(&c);
                    return 0;
                }
                WM_TIMER if wparam == RETRY_TIMER => {
                    c.tries.set(c.tries.get() + 1);
                    if c.added.get() || c.tries.get() >= RETRIES {
                        // SAFETY: our own timer.
                        unsafe { KillTimer(hwnd, RETRY_TIMER) };
                    }
                    if !c.added.get() {
                        add_icon(&c);
                    }
                    return 0;
                }
                WM_STOP => {
                    delete_icon(&c);
                    // SAFETY: our own window, on its own thread.
                    unsafe { DestroyWindow(hwnd) };
                    return 0;
                }
                _ => {}
            }
        }
        if msg == WM_DESTROY {
            // SAFETY: ends this thread's message loop.
            unsafe { PostQuitMessage(0) };
            return 0;
        }
        // SAFETY: the default handling of a message we did not take.
        unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
    }

    /// Counts a running tray thread for as long as it lives.
    struct Live;
    impl Live {
        fn new() -> Live {
            LIVE.fetch_add(1, Ordering::SeqCst);
            Live
        }
    }
    impl Drop for Live {
        fn drop(&mut self) {
            LIVE.fetch_sub(1, Ordering::SeqCst);
        }
    }

    fn run(
        shared: Arc<Shared>,
        post: Arc<dyn Post>,
        ready: std::sync::mpsc::Sender<bool>,
        _live: Live,
    ) {
        let class = wide("HubTrayWindow");
        // SAFETY: standard class registration and window creation with valid, NUL-ended names.
        // The window is a top-level popup that is never shown (a tool window, never activated, so
        // no taskbar button): a message-only window would not be sent `TaskbarCreated`, which is
        // a broadcast.
        let hwnd = unsafe {
            let hinst = GetModuleHandleW(std::ptr::null());
            let mut wc: WNDCLASSEXW = std::mem::zeroed();
            wc.cbSize = std::mem::size_of::<WNDCLASSEXW>() as u32;
            wc.lpfnWndProc = Some(wndproc);
            wc.hInstance = hinst;
            wc.lpszClassName = class.as_ptr();
            RegisterClassExW(&wc);
            CreateWindowExW(
                WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                class.as_ptr(),
                class.as_ptr(),
                WS_POPUP,
                0,
                0,
                0,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                hinst,
                std::ptr::null(),
            )
        };
        if hwnd.is_null() {
            let _ = ready.send(false);
            return;
        }
        // SAFETY: a NUL-ended name.
        let taskbar_created = unsafe { RegisterWindowMessageW(wide("TaskbarCreated").as_ptr()) };
        let c = Rc::new(Ctx {
            shared: shared.clone(),
            post,
            hwnd: Cell::new(hwnd as isize),
            taskbar_created,
            icon: Cell::new(0),
            balloon_icon: Cell::new(0),
            count: Cell::new(u32::MAX),
            added: Cell::new(false),
            v4: Cell::new(false),
            tries: Cell::new(0),
            tip: RefCell::new(String::new()),
        });
        let _ = CTX.try_with(|slot| *slot.borrow_mut() = Some(c.clone()));
        shared.hwnd.store(hwnd as isize, Ordering::SeqCst);
        // `Tray::start` may have given up already (and found no window to stop): end here, before
        // an icon nobody owns is added.
        if !shared.cancelled.load(Ordering::SeqCst) {
            add_icon(&c);
        }
        let _ = ready.send(true);
        // SAFETY: the usual loop; `msg` is written by GetMessageW. A return of 0 is WM_QUIT and -1
        // an error: both end the loop. A start that gave up while the add blocked has posted
        // `WM_STOP` (or set `cancelled` before the window existed): the loop is not entered then.
        unsafe {
            if shared.cancelled.load(Ordering::SeqCst) {
                DestroyWindow(hwnd);
            } else {
                let mut msg: MSG = std::mem::zeroed();
                while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
        shared.hwnd.store(0, Ordering::SeqCst);
        delete_icon(&c);
        let _ = CTX.try_with(|slot| *slot.borrow_mut() = None);
    }

    /// The icon, its thread and its menu. Dropping it removes the icon.
    pub struct Tray {
        shared: Arc<Shared>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl Tray {
        /// Starts the tray thread and waits (a millisecond or two) until its window exists. `None`
        /// when it could not be made; the caller then has no tray and must not rely on one. A
        /// shell with no taskbar yet is not a failure: the icon is added when it appears.
        pub fn start(post: Arc<dyn Post>, snap: Snapshot) -> Option<Tray> {
            Tray::start_with(post, snap, tip_suffix(), std::time::Duration::from_secs(5))
        }

        /// [`start`](Self::start) with the wait and the tooltip tag given. When the wait runs out
        /// the thread is told to end (it removes whatever it added), so a `None` never leaves an
        /// icon and a message loop behind that nobody can stop.
        fn start_with(
            post: Arc<dyn Post>,
            snap: Snapshot,
            tip_suffix: String,
            wait: std::time::Duration,
        ) -> Option<Tray> {
            let shared = Arc::new(Shared {
                hwnd: AtomicIsize::new(0),
                inner: Mutex::new(Inner {
                    snap,
                    balloon: None,
                }),
                tip_suffix,
                cancelled: AtomicBool::new(false),
                readds: AtomicU32::new(0),
            });
            let (tx, rx) = std::sync::mpsc::channel();
            let s = shared.clone();
            // Counted from here, not from when the thread gets going.
            let live = Live::new();
            let thread = std::thread::Builder::new()
                .name("hub-tray".into())
                .stack_size(512 * 1024)
                .spawn(move || run(s, post, tx, live))
                .ok()?;
            match rx.recv_timeout(wait) {
                Ok(true) => Some(Tray {
                    shared,
                    thread: Some(thread),
                }),
                _ => {
                    // Too late, or no window: the thread ends itself. Not joined (it may be stuck in
                    // the shell, which is why we gave up on it).
                    shared.cancelled.store(true, Ordering::SeqCst);
                    let h = shared.hwnd.load(Ordering::SeqCst);
                    if h != 0 {
                        // SAFETY: posting to a window of the tray thread; a stale handle only fails.
                        unsafe { PostMessageW(h as HWND, WM_STOP, 0, 0) };
                    }
                    None
                }
            }
        }

        fn wake(&self, msg: u32) {
            let h = self.shared.hwnd.load(Ordering::SeqCst);
            if h != 0 {
                // SAFETY: posting to a window of ours; a stale handle only makes the call fail.
                unsafe { PostMessageW(h as HWND, msg, 0, 0) };
            }
        }

        /// The engine's new picture: the badge, the tooltip and the next menu follow it. Cheap
        /// when nothing changed.
        pub fn update(&self, snap: Snapshot) {
            let changed = match self.shared.inner.lock() {
                Ok(mut g) if g.snap != snap => {
                    g.snap = snap;
                    true
                }
                _ => false,
            };
            if changed {
                self.wake(WM_SYNC);
            }
        }

        /// A balloon (a toast on Windows 10 and 11). Clicking it sends `TrayCmd::BalloonClicked`.
        pub fn show_balloon(&self, title: &str, text: &str) {
            if let Ok(mut g) = self.shared.inner.lock() {
                g.balloon = Some((title.to_string(), text.to_string()));
            }
            self.wake(WM_SYNC);
        }

        /// Removes the icon and ends the thread. Also done on drop.
        pub fn stop(&mut self) {
            self.wake(WM_STOP);
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }

    impl Drop for Tray {
        fn drop(&mut self) {
            self.stop();
        }
    }

    /// These start a real icon (a moment, with a tooltip tagged `(hub test)`): few, and one at a
    /// time.
    #[cfg(test)]
    mod tests {
        use super::*;
        use std::time::{Duration, Instant};
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            EnumWindows, GetClassNameW, GetWindowThreadProcessId,
        };

        static SERIAL: Mutex<()> = Mutex::new(());

        struct Nop;
        impl Post for Nop {
            fn post(&self, _: Event) {}
        }

        /// Our own top-level windows of the tray's class. `EnumWindows` lists top-level windows
        /// only, which is also who a broadcast reaches: a message-only window is in neither.
        fn tray_windows() -> Vec<isize> {
            unsafe extern "system" fn each(h: HWND, l: LPARAM) -> i32 {
                // SAFETY: `l` is the `Vec` passed below, alive for the whole enumeration.
                let found = unsafe { &mut *(l as *mut Vec<isize>) };
                let mut name = [0u16; 64];
                // SAFETY: the buffer holds 64 units.
                let n = unsafe { GetClassNameW(h, name.as_mut_ptr(), 64) };
                let mut pid = 0u32;
                // SAFETY: `pid` outlives the call.
                unsafe { GetWindowThreadProcessId(h, &mut pid) };
                if pid == std::process::id()
                    && n > 0
                    && String::from_utf16_lossy(&name[..n as usize]) == "HubTrayWindow"
                {
                    found.push(h as isize);
                }
                1
            }
            let mut found: Vec<isize> = Vec::new();
            // SAFETY: the callback only reads its arguments and fills `found`.
            unsafe { EnumWindows(Some(each), &mut found as *mut Vec<isize> as LPARAM) };
            found
        }

        fn until(what: &str, mut f: impl FnMut() -> bool) {
            let end = Instant::now() + Duration::from_secs(10);
            while !f() {
                assert!(Instant::now() < end, "{what}");
                std::thread::sleep(Duration::from_millis(10));
            }
        }

        fn snap() -> Snapshot {
            Snapshot::default()
        }

        #[test]
        fn a_balloon_waits_for_an_icon_instead_of_being_dropped() {
            let mut i = Inner {
                snap: snap(),
                balloon: Some(("Hub".into(), "1 update: mdedit".into())),
            };
            // No icon in the notification area (no taskbar yet, an add being retried): kept.
            assert_eq!(take_balloon(&mut i, false), None);
            assert!(
                i.balloon.is_some(),
                "still there for when the icon is added"
            );
            // With one: shown once.
            assert_eq!(
                take_balloon(&mut i, true),
                Some(("Hub".to_string(), "1 update: mdedit".to_string()))
            );
            assert_eq!(take_balloon(&mut i, true), None);
        }

        #[test]
        fn the_tray_window_is_one_a_broadcast_reaches_and_taskbar_created_adds_the_icon_again() {
            let _one = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
            let t = Tray::start_with(
                Arc::new(Nop),
                snap(),
                "(hub test)".into(),
                Duration::from_secs(10),
            )
            .expect("a tray");
            let h = t.shared.hwnd.load(Ordering::SeqCst);
            assert!(
                tray_windows().contains(&h),
                "a message-only window would not get the TaskbarCreated broadcast"
            );
            // What Explorer's broadcast is to the window (sent to it alone: a real broadcast
            // would make every other program's icon come back too).
            // SAFETY: a NUL-ended name; posting to our own window.
            unsafe {
                let msg = RegisterWindowMessageW(wide("TaskbarCreated").as_ptr());
                PostMessageW(h as HWND, msg, 0, 0);
            }
            until("the icon was not added again", || {
                t.shared.readds.load(Ordering::SeqCst) == 1
            });
            drop(t);
            until("the window outlived the tray", || tray_windows().is_empty());
        }

        #[test]
        fn a_start_that_gave_up_leaves_no_thread_behind() {
            let _one = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
            // A wait of nothing: the window is not up when it runs out, as with a shell that is
            // not answering.
            let mut gave_up = false;
            for _ in 0..50 {
                let t =
                    Tray::start_with(Arc::new(Nop), snap(), "(hub test)".into(), Duration::ZERO);
                if t.is_none() {
                    gave_up = true;
                    break;
                }
            }
            assert!(gave_up, "a start with no time to wait never gave up");
            until("a tray thread runs on after its start gave up", || {
                LIVE.load(Ordering::SeqCst) == 0 && tray_windows().is_empty()
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(updates: u32) -> Snapshot {
        Snapshot {
            updates,
            hub_update: false,
            update_names: Vec::new(),
            apps: vec![("mdedit", "mdedit".into()), ("sprawl", "Sprawl".into())],
            start_with_windows: true,
            notify: false,
        }
    }

    fn labels(m: &[Entry]) -> Vec<String> {
        m.iter()
            .map(|e| match e {
                Entry::Item { label, checked, .. } => {
                    format!("{}{label}", if *checked { "[x] " } else { "" })
                }
                Entry::Separator => "---".into(),
            })
            .collect()
    }

    #[test]
    fn the_menu_holds_the_planned_lines_in_order() {
        assert_eq!(
            labels(&menu(&snap(3))),
            [
                "Open Hub",
                "Update all (3)",
                "Check now",
                "---",
                "mdedit",
                "Sprawl",
                "---",
                "[x] Start with Windows",
                "Notify about updates",
                "---",
                "Quit Hub"
            ]
        );
    }

    #[test]
    fn no_update_means_no_update_all_line() {
        let m = menu(&snap(0));
        assert!(!labels(&m).iter().any(|l| l.starts_with("Update all")));
        assert!(m.iter().any(|e| matches!(
            e,
            Entry::Item {
                cmd: TrayCmd::CheckNow,
                ..
            }
        )));
    }

    #[test]
    fn no_installed_app_means_no_app_section() {
        let mut s = snap(0);
        s.apps.clear();
        assert_eq!(
            labels(&menu(&s)),
            [
                "Open Hub",
                "Check now",
                "---",
                "[x] Start with Windows",
                "Notify about updates",
                "---",
                "Quit Hub"
            ]
        );
    }

    #[test]
    fn each_line_carries_its_command() {
        let m = menu(&snap(1));
        let cmds: Vec<TrayCmd> = m
            .iter()
            .filter_map(|e| match e {
                Entry::Item { cmd, .. } => Some(*cmd),
                Entry::Separator => None,
            })
            .collect();
        assert_eq!(
            cmds,
            [
                TrayCmd::Open,
                TrayCmd::UpdateAll,
                TrayCmd::CheckNow,
                TrayCmd::OpenApp("mdedit"),
                TrayCmd::OpenApp("sprawl"),
                TrayCmd::ToggleStart,
                TrayCmd::ToggleNotify,
                TrayCmd::Quit
            ]
        );
    }

    #[test]
    fn an_ampersand_in_a_name_is_not_a_mnemonic() {
        let mut s = snap(0);
        s.apps = vec![("x", "A&B".into())];
        assert!(labels(&menu(&s)).contains(&"A&&B".to_string()));
    }

    #[test]
    fn the_tooltip_follows_the_count_and_carries_the_test_tag() {
        assert_eq!(tooltip(0, ""), "Hub");
        assert_eq!(tooltip(1, ""), "Hub: 1 update available");
        assert_eq!(tooltip(3, ""), "Hub: 3 updates available");
        assert_eq!(tooltip(0, "t-42"), "Hub t-42");
        assert_eq!(tooltip(0, &"x".repeat(500)).encode_utf16().count(), 120);
    }

    #[test]
    fn strings_for_the_shell_are_cut_inside_their_array() {
        assert_eq!(fit16("abcdef", 3), "abc");
        // A character of two units is never split.
        assert_eq!(fit16("a\u{1F600}b", 2), "a");
        assert_eq!(fit16("a\u{1F600}b", 3), "a\u{1F600}");
        assert_eq!(fit16("", 5), "");
    }

    #[test]
    fn a_click_means_one_thing_in_version_4_and_in_the_old_scheme() {
        use Click::*;
        assert_eq!(classify(true, NIN_SELECT), Some(Open));
        assert_eq!(classify(true, NIN_KEYSELECT), Some(Open));
        assert_eq!(classify(true, WM_CONTEXTMENU), Some(Menu));
        assert_eq!(classify(true, NIN_BALLOONUSERCLICK), Some(Balloon));
        // The raw mouse messages are ignored in version 4 (the click would count twice).
        assert_eq!(classify(true, WM_LBUTTONUP), None);
        assert_eq!(classify(true, WM_RBUTTONUP), None);
        assert_eq!(classify(false, WM_LBUTTONUP), Some(Open));
        assert_eq!(classify(false, WM_RBUTTONUP), Some(Menu));
        assert_eq!(classify(false, NIN_SELECT), None);
        assert_eq!(classify(false, NIN_BALLOONUSERCLICK), Some(Balloon));
        assert_eq!(classify(true, 0x0200 /* mouse move */), None);
    }

    #[test]
    fn the_tray_is_wanted_by_flag_or_setting_and_refused_by_no_tray_and_demo() {
        // (flag, no_flag, setting, demo)
        assert!(
            wants_tray(false, false, true, false),
            "the setting, default on"
        );
        assert!(!wants_tray(false, false, false, false), "setting off");
        assert!(wants_tray(true, false, false, false), "--tray forces it");
        assert!(!wants_tray(true, true, true, false), "--no-tray wins");
        assert!(!wants_tray(false, true, true, false));
        assert!(
            !wants_tray(true, false, true, true),
            "a demo picture has none"
        );
    }

    // ---- the window's life ----

    const TRAY_ONLY: Life = Life {
        window: false,
        tray: true,
    };
    const WINDOW_AND_TRAY: Life = Life {
        window: true,
        tray: true,
    };
    const WINDOW_ONLY: Life = Life {
        window: true,
        tray: false,
    };

    #[test]
    fn opening_with_no_window_makes_one_and_with_one_raises_it() {
        assert_eq!(step(TRAY_ONLY, Input::Open), (WINDOW_AND_TRAY, Do::Create));
        assert_eq!(
            step(WINDOW_AND_TRAY, Input::Open),
            (WINDOW_AND_TRAY, Do::Raise)
        );
        assert_eq!(step(WINDOW_ONLY, Input::Open), (WINDOW_ONLY, Do::Raise));
    }

    #[test]
    fn closing_the_window_keeps_the_process_only_while_the_tray_is_on() {
        assert_eq!(
            step(WINDOW_AND_TRAY, Input::CloseWindow),
            (TRAY_ONLY, Do::Destroy)
        );
        let (l, d) = step(WINDOW_ONLY, Input::CloseWindow);
        assert_eq!(d, Do::Exit);
        assert!(!l.window);
        // A close with no window is nothing (a late event of a destroyed one).
        assert_eq!(
            step(TRAY_ONLY, Input::CloseWindow),
            (TRAY_ONLY, Do::Nothing)
        );
    }

    #[test]
    fn quit_ends_the_process_from_every_state() {
        for l in [TRAY_ONLY, WINDOW_AND_TRAY, WINDOW_ONLY] {
            assert_eq!(step(l, Input::Quit).1, Do::Exit, "{l:?}");
        }
    }

    #[test]
    fn a_window_that_cannot_be_made_ends_a_process_that_has_no_tray() {
        assert_eq!(step(WINDOW_ONLY, Input::OpenFailed).1, Do::Exit);
        let (l, d) = step(WINDOW_AND_TRAY, Input::OpenFailed);
        assert_eq!((l, d), (TRAY_ONLY, Do::Nothing));
    }

    #[test]
    fn open_close_open_cycles_through_the_states() {
        let (l, d) = step(TRAY_ONLY, Input::Open);
        assert_eq!(d, Do::Create);
        let (l, d) = step(l, Input::CloseWindow);
        assert_eq!(d, Do::Destroy);
        let (l, d) = step(l, Input::Open);
        assert_eq!(d, Do::Create);
        assert_eq!(l, WINDOW_AND_TRAY);
    }
}
