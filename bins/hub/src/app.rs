//! The Hub window: winit for the window and input, softbuffer for the pixels, the drawing code of
//! `view.rs` for everything on it. Frameless like mdedit's: the title row is ours, the resize band
//! is ours. The loop sleeps (`Wait`, or `WaitUntil` the next check or the end of a test run):
//! nothing polls, so an idle Hub uses no CPU. Redraws happen only when something changed.
use crate::cli::{self, Args};
use crate::demo;
use crate::glue::{
    self, Cmd, Deps, Engine, Event, Key, MenuItem, Post, RealShell, Tmpl, command, key_command,
    repeatable,
};
use crate::settings;
use crate::trace::Trace;
use crate::tray::{self, Do, Input, Life, Snapshot, TrayCmd};
use crate::view::{self, HEAD_H, ROW_H};
use crate::vm::{Hit, Layout, View, Vm};
use hub_core::check;
use hub_core::selfupdate;
use hub_core::state::Store;
use mdshared_text::bitmap::Bitmap;
use mdshared_ui::gfx::{Face, Gfx, Painter, Rect, mix};
use mdshared_ui::keys::{Mods, key_name};
use mdshared_ui::settings::{Form, Outcome};
use mdshared_ui::theme::{DARK, LIGHT, Palette};
use mdshared_ui::window::edge_at;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key as WKey, ModifiersState, NamedKey};
use winit::platform::windows::WindowAttributesExtWindows;
use winit::window::{CursorIcon, ResizeDirection, Window, WindowId, WindowLevel};

const DEFAULT_SIZE: (u32, u32) = (860, 600);
const MIN_SIZE: (u32, u32) = (520, 360);
/// The longest the loop sleeps in one go: a clock that jumped (sleep, resume) is noticed.
const MAX_SLEEP: Duration = Duration::from_secs(30 * 60);
/// At most this often are the running apps looked up again on a focus change.
const RUNNING_EVERY: Duration = Duration::from_secs(2);
/// How long a closing window waits for the install it asked to stop.
const CLOSE_WAIT: Duration = Duration::from_secs(8);
const POPUP_W: f32 = 184.0;
const POPUP_ROW: f32 = 30.0;

/// Worker threads post to the loop through this.
struct Proxy(Mutex<EventLoopProxy<Event>>);

impl Post for Proxy {
    fn post(&self, ev: Event) {
        if let Ok(p) = self.0.lock() {
            let _ = p.send_event(ev);
        }
    }
}

/// The "..." menu of a row, open.
struct Popup {
    id: &'static str,
    rect: Rect,
    items: Vec<(MenuItem, &'static str, bool)>,
    hover: Option<usize>,
}

impl Popup {
    fn item_rect(&self, i: usize) -> Rect {
        Rect::new(
            self.rect.x + 4.0,
            self.rect.y + 4.0 + i as f32 * POPUP_ROW,
            self.rect.w - 8.0,
            POPUP_ROW,
        )
    }

    fn at(&self, x: f32, y: f32) -> Option<usize> {
        (0..self.items.len()).find(|i| self.item_rect(*i).contains(x, y))
    }
}

struct App {
    args: Args,
    trace: Trace,
    engine: Engine,
    window: Option<Rc<Window>>,
    surface: Option<softbuffer::Surface<Rc<Window>, Rc<Window>>>,
    bmp: Bitmap,
    /// Fonts and glyph caches: made with the first frame of a window, dropped with the window,
    /// so a Hub that lives under the tray holds none.
    gfx: Option<Gfx>,
    scale: f32,
    system_dark: bool,
    /// What the last frame drew, for the next click.
    layout: Layout,
    vm: Vm,
    scroll: f32,
    max_scroll: f32,
    /// The release notes view's own scroll (the list keeps its own while the view is open), how
    /// far it can go and how tall its window on the text is (from the last frame), and which
    /// notes it was for (a new one starts at the top).
    nscroll: f32,
    nmax: f32,
    nview: f32,
    nsig: Option<(&'static str, String, u8)>,
    mouse: (f32, f32),
    mods: ModifiersState,
    popup: Option<Popup>,
    last_title_click: Option<Instant>,
    cursor: CursorIcon,
    dirty: bool,
    exit_now: bool,
    /// The window was asked to close while an install ran: the install is told to stop and the
    /// window waits for it, up to this time.
    closing: Option<Instant>,
    finished: bool,
    first: bool,
    shown: bool,
    exit_at: Option<Instant>,
    last_running: Instant,
    /// What exists: a window, and a tray that keeps the process alive without one (`tray::step`).
    life: Life,
    /// The tray icon, when there is one (`--no-tray`, `tray = 0` and a failure give none).
    tray: Option<tray::Tray>,
    /// The last snapshot the tray was given: a new one is sent only when it differs.
    last_snap: Snapshot,
    /// The settings card (the sliders button), open: kept as the form the user is working in.
    settings: Option<Form>,
    /// Hub's own update was swapped in: when the loop ends, this exe is started.
    restart_exe: Option<std::path::PathBuf>,
    /// No window was open when the restart was asked for: the new Hub comes back as it was, under
    /// the tray, not with a window nobody asked for.
    restart_windowless: bool,
    /// The tray is wanted and not started yet (a window-first launch starts it after the first
    /// frame). Cleared by the attempt.
    want_tray: bool,
    /// What the tray thread posts into.
    post: Arc<dyn Post>,
    /// How many windows have been made in this process.
    opens: u32,
    /// Kept for as long as the process lives: what second launches asked for. A request from
    /// `hub --quit` arrives here (`QUIT_FROM`); a plain second launch only wakes the loop.
    single: Option<std::sync::mpsc::Receiver<mdshared_app::sister::Request>>,
}

/// The `from` of the request `hub --quit` sends to the running Hub (a second launch has nothing
/// else to say, and the single-instance channel carries file requests: this one is told apart by
/// its sender's name, with `unplace` set so the listener delivers it though it names no file).
const QUIT_FROM: &str = "hub-quit";

fn hwnd(w: &Window) -> Option<isize> {
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match w.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(h) => Some(h.hwnd.get()),
        _ => None,
    }
}

fn map_key(ev: &KeyEvent) -> Option<Key> {
    match &ev.logical_key {
        WKey::Named(n) => match n {
            NamedKey::ArrowUp => Some(Key::Up),
            NamedKey::ArrowDown => Some(Key::Down),
            NamedKey::ArrowLeft => Some(Key::Left),
            NamedKey::ArrowRight => Some(Key::Right),
            NamedKey::Enter => Some(Key::Enter),
            NamedKey::Escape => Some(Key::Esc),
            NamedKey::Tab => Some(Key::Tab),
            NamedKey::Backspace => Some(Key::Backspace),
            NamedKey::F5 => Some(Key::F5),
            NamedKey::PageUp => Some(Key::PageUp),
            NamedKey::PageDown => Some(Key::PageDown),
            NamedKey::Home => Some(Key::Home),
            NamedKey::End => Some(Key::End),
            NamedKey::Space => Some(Key::Char(' ')),
            _ => None,
        },
        WKey::Character(s) => {
            let mut c = s.chars();
            match (c.next(), c.next()) {
                (Some(ch), None) => Some(Key::Char(ch.to_ascii_lowercase())),
                _ => None,
            }
        }
        _ => None,
    }
}

fn edge_cursor(d: ResizeDirection) -> CursorIcon {
    match d {
        ResizeDirection::West | ResizeDirection::East => CursorIcon::EwResize,
        ResizeDirection::North | ResizeDirection::South => CursorIcon::NsResize,
        ResizeDirection::NorthWest | ResizeDirection::SouthEast => CursorIcon::NwseResize,
        ResizeDirection::NorthEast | ResizeDirection::SouthWest => CursorIcon::NeswResize,
    }
}

impl App {
    fn dark(&self) -> bool {
        self.args.dark.unwrap_or(self.system_dark)
    }

    fn pal(&self) -> Palette {
        if self.dark() { DARK } else { LIGHT }
    }

    /// The window's size in logical px.
    fn logical(&self) -> (f32, f32) {
        match &self.window {
            Some(w) => {
                let s = w.inner_size();
                (s.width as f32 / self.scale, s.height as f32 / self.scale)
            }
            None => (DEFAULT_SIZE.0 as f32, DEFAULT_SIZE.1 as f32),
        }
    }

    fn apply(&mut self, cmd: Cmd) {
        if self.engine.apply(cmd) {
            self.dirty = true;
        }
        self.check_restart();
        self.keep_selection_visible();
    }

    /// In the list, a selection moved by the keyboard scrolls into view.
    fn keep_selection_visible(&mut self) {
        if self.vm.view != View::List {
            return;
        }
        let (_, h) = self.logical();
        let vm = self.engine.vm(check::now());
        let geo = view::geometry(self.logical().0, h, &vm);
        let body_h = (geo.list.h - HEAD_H).max(0.0);
        let top = vm.sel as f32 * ROW_H;
        if top < self.scroll {
            self.scroll = top;
            self.dirty = true;
        } else if top + ROW_H > self.scroll + body_h {
            self.scroll = (top + ROW_H - body_h).max(0.0);
            self.dirty = true;
        }
    }

    fn raise(&self) {
        if let Some(w) = &self.window {
            w.set_minimized(false);
            w.focus_window();
        }
    }

    // ---- drawing ----

    fn draw(&mut self) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return;
        }
        let (lw, lh) = (
            size.width as f32 / self.scale,
            size.height as f32 / self.scale,
        );
        if self.bmp.width != size.width || self.bmp.height != size.height {
            self.bmp = Bitmap::new(size.width, size.height);
        }
        let pal = self.pal();
        let vm = self.engine.vm(check::now());
        let geo = view::geometry(lw, lh, &vm);
        self.max_scroll = geo.max_scroll;
        self.scroll = self.scroll.clamp(0.0, self.max_scroll);
        let maximized = window.is_maximized();
        let gfx = self.gfx.get_or_insert_with(Gfx::new);
        let mut p = Painter::new(&mut self.bmp, gfx, self.scale);
        // A different notes view (or the same one with its text arrived) starts at the top.
        let sig = vm.notes.as_ref().map(|n| {
            (
                n.id,
                n.version.clone(),
                match n.state {
                    crate::vm::NotesState::Loaded(_) => 1,
                    _ => 0,
                },
            )
        });
        if sig != self.nsig {
            self.nscroll = 0.0;
            self.nsig = sig;
        }
        let layout = view::draw_full(&mut p, &pal, lw, lh, &vm, self.scroll, self.nscroll);
        self.nmax = layout.notes_max_scroll;
        self.nview = layout.notes_view_h;
        self.nscroll = self.nscroll.clamp(0.0, self.nmax);
        let _ = maximized;
        if let Some(pop) = &self.popup {
            draw_popup(&mut p, &pal, pop);
        }
        if let Some(form) = &mut self.settings {
            form.draw(&mut p, &pal, Rect::new(0.0, 0.0, lw, lh), false);
        }
        self.layout = layout;
        self.vm = vm;
        self.dirty = false;
        // The rows moved under a still pointer (a scroll, a new filter, a result): the highlight
        // follows the pointer, so a click lands on the row that is lit.
        if self.popup.is_none() && self.engine.set_hover(hover_row(&self.layout, self.mouse)) {
            self.dirty = true;
        }
        self.present();
    }

    fn present(&mut self) {
        let (Some(surface), Some(window)) = (self.surface.as_mut(), self.window.as_ref()) else {
            return;
        };
        let s = window.inner_size();
        let (Some(w), Some(h)) = (NonZeroU32::new(s.width), NonZeroU32::new(s.height)) else {
            return;
        };
        if surface.resize(w, h).is_err() {
            return;
        }
        let Ok(mut buffer) = surface.buffer_mut() else {
            return;
        };
        let bytes: &mut [u8] = bytemuck::cast_slice_mut(&mut buffer[..]);
        let n = bytes.len().min(self.bmp.pixels.len());
        bytes[..n].copy_from_slice(&self.bmp.pixels[..n]);
        let _ = buffer.present();
    }

    // ---- input ----

    fn cursor_moved(&mut self, x: f32, y: f32) {
        self.mouse = (x, y);
        if let Some(form) = &mut self.settings {
            if form.pointer_move(x, y) {
                self.dirty = true;
            }
            return;
        }
        if let Some(pop) = &mut self.popup {
            let h = pop.at(x, y);
            if h != pop.hover {
                pop.hover = h;
                self.dirty = true;
            }
            return;
        }
        let (lw, lh) = self.logical();
        let maximized = self.window.as_ref().is_some_and(|w| w.is_maximized());
        let edge = if maximized {
            None
        } else {
            edge_at(x, y, lw, lh)
        };
        let icon = edge.map_or(CursorIcon::Default, edge_cursor);
        if icon != self.cursor {
            self.cursor = icon;
            if let Some(w) = &self.window {
                w.set_cursor(icon);
            }
        }
        if self.engine.set_hover(hover_row(&self.layout, (x, y))) {
            self.dirty = true;
        }
    }

    fn click(&mut self) {
        let (x, y) = self.mouse;
        if let Some(pop) = self.popup.take() {
            self.dirty = true;
            if let Some(i) = pop.at(x, y)
                && let Some((item, _, true)) = pop.items.get(i).copied()
            {
                self.apply(Cmd::MenuPick(pop.id, item));
            }
            return;
        }
        let Some(window) = self.window.clone() else {
            return;
        };
        let (lw, lh) = self.logical();
        if !window.is_maximized()
            && let Some(dir) = edge_at(x, y, lw, lh)
        {
            let _ = window.drag_resize_window(dir);
            return;
        }
        let hit = self.layout.at(x, y);
        match hit {
            Hit::Drag => {
                let double = self
                    .last_title_click
                    .is_some_and(|t| t.elapsed() < mdshared_ui::platform::double_click_time());
                if double {
                    self.last_title_click = None;
                    window.set_maximized(!window.is_maximized());
                } else {
                    self.last_title_click = Some(Instant::now());
                    let _ = window.drag_window();
                }
            }
            Hit::Min => window.set_minimized(true),
            Hit::Max => window.set_maximized(!window.is_maximized()),
            Hit::Close => self.request_close(),
            Hit::Settings => self.open_settings(0),
            Hit::None => {
                // A click on nothing leaves the filter box.
                self.apply(Cmd::FilterBlur);
            }
            hit => {
                let vm = self.vm.clone();
                match command(hit, &vm) {
                    Some(Cmd::OpenMenu(i)) => self.open_popup(i),
                    Some(cmd) => self.apply(cmd),
                    None => {}
                }
            }
        }
    }

    // ---- the settings card ----

    /// Opens the card from the engine's rows: a form built now, so it shows what is true now.
    fn open_settings(&mut self, page: usize) {
        self.popup = None;
        self.settings = Some(settings::form(self.engine.settings_rows(), page));
        self.dirty = true;
    }

    /// The card shows the engine's rows: a change made elsewhere (the tray menu, a failed write)
    /// replaces them. Rows equal to what it shows are left alone, so an open dropdown stays open.
    fn sync_settings(&mut self) {
        if let Some(f) = &mut self.settings {
            let rows = self.engine.settings_rows();
            if f.rows() != rows.as_slice() {
                f.set_rows(rows);
                self.dirty = true;
            }
        }
    }

    fn settings_key(&mut self, ev: &KeyEvent) {
        let Some(k) = key_name(&ev.logical_key) else {
            return;
        };
        let mods = Mods::from_winit(self.mods);
        let outcome = match &mut self.settings {
            Some(f) => f.key(&k, mods, ev.text.as_ref().map(|t| t.as_str())),
            None => return,
        };
        self.settings_outcome(outcome);
    }

    fn settings_press(&mut self) {
        let (x, y) = self.mouse;
        let mods = Mods::from_winit(self.mods);
        let outcome = match &mut self.settings {
            Some(f) => f.press(x, y, mods, 1),
            None => return,
        };
        self.settings_outcome(outcome);
    }

    fn settings_release(&mut self) {
        let (x, y) = self.mouse;
        let outcome = match &mut self.settings {
            Some(f) => f.release(x, y),
            None => return,
        };
        self.settings_outcome(outcome);
    }

    /// What the card said: apply the change, press the button, or go away.
    fn settings_outcome(&mut self, o: Outcome) {
        match o {
            Outcome::None | Outcome::Unused => return,
            Outcome::Redraw => {}
            Outcome::Closed => self.settings = None,
            Outcome::Changed { id, value } => {
                self.engine.change_setting(id, value);
                self.settings_changed();
            }
            Outcome::Action(id) => {
                self.engine.setting_action(id);
                self.settings_changed();
            }
        }
        self.dirty = true;
    }

    /// A setting changed (in the card or in the tray menu): the icon, the menu and the card
    /// follow the engine.
    fn settings_changed(&mut self) {
        self.apply_tray_setting();
        self.sync_tray();
        self.check_restart();
    }

    /// The icon follows the setting (and `--tray`, `--no-tray`): made when it was turned on,
    /// removed when it was turned off.
    fn apply_tray_setting(&mut self) {
        let want = tray::wants_tray(
            self.args.tray,
            self.args.no_tray,
            self.engine.tray_on(),
            self.args.demo.is_some(),
        );
        if want && self.tray.is_none() {
            self.start_tray();
        } else if !want && self.tray.is_some() {
            self.tray = None;
            self.engine.set_tray_up(false);
        }
    }

    /// A test run (`--features test-hooks`, `HUB_TEST_AUTO_RESTART` naming a marker file that does
    /// not exist yet): presses "Restart now" once Hub's own update is ready, as a click would,
    /// and leaves the marker so the restarted Hub does not do it again. Not in a release build.
    #[cfg(feature = "test-hooks")]
    fn test_auto_restart(&mut self) {
        let Some(marker) = std::env::var_os("HUB_TEST_AUTO_RESTART").map(std::path::PathBuf::from)
        else {
            return;
        };
        if matches!(self.engine.hub_status(), selfupdate::Status::Ready(_)) && !marker.exists() {
            let _ = std::fs::write(&marker, b"restarted");
            self.apply(Cmd::HubRestart);
        }
    }

    #[cfg(not(feature = "test-hooks"))]
    fn test_auto_restart(&mut self) {}

    /// Hub's own update was swapped in: the loop ends and the new Hub is started.
    fn check_restart(&mut self) {
        if let Some(exe) = self.engine.take_restart() {
            self.restart_exe = Some(exe);
            self.restart_windowless = self.window.is_none();
            self.exit_now = true;
        }
    }

    fn open_popup(&mut self, i: usize) {
        let Some(id) = self.vm.apps.get(i).map(|a| a.id) else {
            return;
        };
        let items = self.engine.menu_items(id);
        let Some(anchor) = self
            .layout
            .hits
            .iter()
            .find(|(_, h)| *h == Hit::Menu(i))
            .map(|(r, _)| *r)
        else {
            return;
        };
        let (lw, lh) = self.logical();
        let h = items.len() as f32 * POPUP_ROW + 8.0;
        let x = (anchor.right() - POPUP_W).clamp(4.0, (lw - POPUP_W - 4.0).max(4.0));
        let y = (anchor.bottom() + 2.0).min((lh - h - 4.0).max(4.0));
        self.popup = Some(Popup {
            id,
            rect: Rect::new(x, y, POPUP_W, h),
            items,
            hover: None,
        });
        self.dirty = true;
    }

    fn key(&mut self, ev: KeyEvent) {
        if ev.state != ElementState::Pressed {
            return;
        }
        if self.settings.is_some() {
            self.settings_key(&ev);
            return;
        }
        if self.popup.is_some() {
            if ev.logical_key == WKey::Named(NamedKey::Escape) {
                self.popup = None;
                self.dirty = true;
            }
            return;
        }
        let ctrl = self.mods.control_key();
        let shift = self.mods.shift_key();
        let focus = self.engine.filter_focus();
        if focus
            && !ctrl
            && let Some(t) = &ev.text
        {
            let mut typed = false;
            for c in t.chars().filter(|c| !c.is_control()) {
                typed = true;
                self.apply(Cmd::FilterChar(c));
            }
            if typed {
                return;
            }
        }
        if self.vm.notes.is_some() {
            // The notes view: the keys move the text, Esc or Backspace go back.
            if let Some(k) = map_key(&ev) {
                if let Some(nav) = crate::glue::notes_nav(k) {
                    let next = crate::changelog::scrolled(nav, self.nscroll, self.nmax, self.nview);
                    if next != self.nscroll {
                        self.nscroll = next;
                        self.dirty = true;
                    }
                    return;
                }
                if k == Key::Backspace && !ev.repeat {
                    self.apply(Cmd::NotesClose);
                    return;
                }
            }
        }
        let cols = match self.vm.view {
            View::List => 1,
            v => crate::grid::geometry(v, self.logical().0).0,
        };
        if let Some(k) = map_key(&ev)
            && let Some(cmd) = key_command(focus, k, ctrl, shift, cols)
            && (!ev.repeat || repeatable(&cmd))
        {
            self.apply(cmd);
        }
    }
}

/// The row menu is drawn at a fixed place: what moves the rows or the window closes it.
fn closes_popup(ev: &WindowEvent) -> bool {
    matches!(
        ev,
        WindowEvent::Resized(_)
            | WindowEvent::ScaleFactorChanged { .. }
            | WindowEvent::MouseWheel { .. }
    )
}

/// The row (or its button) under the pointer.
fn hover_row(layout: &Layout, (x, y): (f32, f32)) -> Option<usize> {
    match layout.at(x, y) {
        Hit::Row(i) | Hit::Action(i, _) | Hit::Menu(i) => Some(i),
        _ => None,
    }
}

fn draw_popup(p: &mut Painter, pal: &Palette, pop: &Popup) {
    p.shadow(pop.rect, 8.0, pal.dark);
    p.frame(pop.rect, 8.0, pal.popup, pal.popup_line);
    for (i, (_, label, enabled)) in pop.items.iter().enumerate() {
        let r = pop.item_rect(i);
        if *enabled && pop.hover == Some(i) {
            p.rbox(r, 5.0, mix(pal.popup, pal.text, 0.10));
        }
        p.text(
            Face::Ui,
            13.0,
            r.x + 12.0,
            r.cy(),
            label,
            if *enabled { pal.text } else { pal.faint },
            r.w - 16.0,
        );
    }
}

impl ApplicationHandler<Event> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() || !self.life.window {
            // Started under the tray (`--tray`): no window until one is asked for.
            if !self.life.window {
                self.trace.stage("started under the tray, no window");
                self.trace.memory("tray only");
                self.trace.cpu("tray only");
            }
            return;
        }
        self.create_window(event_loop);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, ev: Event) {
        match ev {
            Event::Raise => {
                if self.quit_requested() {
                    self.trace.stage("quit requested by another launch");
                    self.request_exit();
                } else {
                    self.open(event_loop);
                }
            }
            Event::Tray(cmd) => self.tray_command(event_loop, cmd),
            ev => {
                if self.engine.handle(ev) {
                    self.dirty = true;
                }
                // One balloon per check that found news (the rule is the engine's).
                if let Some(m) = self.engine.take_message()
                    && let Some(t) = &self.tray
                {
                    t.show_balloon(&m.title, &m.body);
                }
                self.sync_tray();
                self.check_restart();
                self.test_auto_restart();
                self.open_to_ask(event_loop);
            }
        }
        if self.exit_now {
            self.finish(event_loop);
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        // A late event of a window that was destroyed is not this window's.
        if self.window.as_ref().map(|w| w.id()) != Some(id) {
            return;
        }
        if self.popup.is_some() && closes_popup(&event) {
            self.popup = None;
            self.dirty = true;
        }
        match event {
            WindowEvent::CloseRequested => self.request_close(),
            WindowEvent::RedrawRequested => self.draw(),
            WindowEvent::Resized(_) => {
                self.dirty = true;
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.scale = scale_factor as f32;
                self.dirty = true;
            }
            WindowEvent::ThemeChanged(t) => {
                self.system_dark = t == winit::window::Theme::Dark;
                if let Some(h) = self.window.as_ref().and_then(|w| hwnd(w)) {
                    mdshared_ui::platform::style_window(h, self.dark());
                }
                self.dirty = true;
            }
            WindowEvent::Focused(focused) => {
                self.engine.set_focused(focused);
                // An app may have been started or closed meanwhile.
                if focused && self.last_running.elapsed() >= RUNNING_EVERY {
                    self.last_running = Instant::now();
                    self.engine.refresh_running_async();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_moved(
                    position.x as f32 / self.scale,
                    position.y as f32 / self.scale,
                );
            }
            WindowEvent::CursorLeft { .. } => {
                if self.engine.set_hover(None) {
                    self.dirty = true;
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                if self.settings.is_some() {
                    self.settings_press();
                } else {
                    self.click();
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                button: MouseButton::Left,
                ..
            } => self.settings_release(),
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y * ROW_H,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / self.scale,
                };
                if let Some(f) = &mut self.settings {
                    // The card scrolls towards its end when the wheel turns down.
                    if f.wheel(self.mouse.0, self.mouse.1, -dy) {
                        self.dirty = true;
                    }
                    return;
                }
                if self.vm.notes.is_some() {
                    let next = (self.nscroll - dy).clamp(0.0, self.nmax);
                    if next != self.nscroll {
                        self.nscroll = next;
                        self.dirty = true;
                    }
                    return;
                }
                let next = (self.scroll - dy).clamp(0.0, self.max_scroll);
                if next != self.scroll {
                    self.scroll = next;
                    self.dirty = true;
                }
            }
            WindowEvent::ModifiersChanged(m) => self.mods = m.state(),
            WindowEvent::KeyboardInput { event, .. } => self.key(event),
            _ => {}
        }
        if self.exit_now {
            self.finish(event_loop);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if let Some(t) = self.closing
            && (!self.engine.installing() || Instant::now() >= t)
        {
            self.exit_now = true;
        }
        if self.exit_now || self.exit_at.is_some_and(|t| Instant::now() >= t) {
            self.finish(event_loop);
            return;
        }
        let now = check::now();
        // Nothing runs before the first frame of a window; with no window at all (under the tray)
        // the engine keeps its schedule and the next check starts when it is due.
        let live = self.shown || self.window.is_none();
        if self.shown && std::mem::take(&mut self.first) {
            // After the first frame: which apps run (one pass over the process list).
            self.last_running = Instant::now();
            self.engine.refresh_running_async();
            self.trace.stage("running apps looked up");
        }
        if self.want_tray && self.shown {
            // A window-first launch: the icon comes after the first frame, never before it.
            self.want_tray = false;
            self.start_tray();
        }
        if live && self.engine.tick(now) {
            self.dirty = true;
        }
        if let Some(text) = self.args.tray_test_balloon.take()
            && let Some(t) = &self.tray
        {
            t.show_balloon("Hub", &text);
        }
        if self.dirty
            && let Some(w) = &self.window
        {
            w.request_redraw();
        }
        let mut until = self.exit_at;
        if let Some(t) = self.closing {
            until = Some(until.map_or(t, |u| u.min(t)));
        }
        if let Some(d) = self.engine.wait(now).filter(|_| live) {
            let t = Instant::now() + d.min(MAX_SLEEP);
            until = Some(until.map_or(t, |u| u.min(t)));
        }
        event_loop.set_control_flow(match until {
            Some(t) => ControlFlow::WaitUntil(t),
            None => ControlFlow::Wait,
        });
    }
}

impl App {
    /// Makes the window, draws its first frame hidden, shows it. Everything a window costs
    /// (surface, bitmap, fonts) is made here and given back by [`App::destroy_window`]. A failure
    /// is `Input::OpenFailed`: with no tray that ends the process.
    fn create_window(&mut self, event_loop: &ActiveEventLoop) {
        let opened = Instant::now();
        let (w, h) = self.args.size.unwrap_or(DEFAULT_SIZE);
        let mut attributes = Window::default_attributes()
            .with_title("Hub")
            .with_decorations(false)
            .with_visible(false)
            .with_inner_size(LogicalSize::new(
                w.max(MIN_SIZE.0) as f64,
                h.max(MIN_SIZE.1) as f64,
            ))
            .with_min_inner_size(LogicalSize::new(MIN_SIZE.0 as f64, MIN_SIZE.1 as f64))
            .with_undecorated_shadow(true);
        // A test run: shown without the focus and kept under every other window.
        if self.args.behind {
            attributes = attributes
                .with_active(false)
                .with_window_level(WindowLevel::AlwaysOnBottom);
        }
        let window = match event_loop.create_window(attributes) {
            Ok(w) => Rc::new(w),
            Err(e) => {
                eprintln!("hub: no window: {e}");
                self.window_failed();
                return;
            }
        };
        self.trace.stage("window created");
        self.scale = window.scale_factor() as f32;
        if let Some(h) = hwnd(&window) {
            mdshared_ui::platform::style_window(h, self.dark());
        }
        let surface = softbuffer::Context::new(window.clone())
            .and_then(|c| softbuffer::Surface::new(&c, window.clone()));
        match surface {
            Ok(s) => self.surface = Some(s),
            Err(e) => {
                eprintln!("hub: no surface: {e}");
                self.window_failed();
                return;
            }
        }
        self.window = Some(window.clone());
        self.trace.stage("surface");
        // The first frame is drawn while the window is hidden, then the window is shown.
        self.draw();
        self.trace.stage("first frame drawn");
        window.set_visible(true);
        if self.args.behind {
            // Showing a window raises it, so a test run's window is sent under the others again
            // (winit skips a level that has not changed, hence Normal first).
            window.set_window_level(WindowLevel::Normal);
            window.set_window_level(WindowLevel::AlwaysOnBottom);
        }
        // The frame went to a hidden window, which shows nothing: send it again.
        self.present();
        self.first = true;
        self.shown = true;
        self.opens += 1;
        if self.opens == 1 && !self.args.tray {
            self.trace
                .line(&format!("first frame after {:.1} ms", self.trace.since()));
        } else {
            self.trace.line(&format!(
                "window opened: first frame after {:.1} ms",
                opened.elapsed().as_secs_f64() * 1000.0
            ));
        }
        self.trace.memory("window open");
        self.trace.cpu("window open");
    }

    /// Starts the tray icon (once). False when it could not be made: Hub then has no tray.
    fn start_tray(&mut self) -> bool {
        if self.tray.is_some() {
            return true;
        }
        let snap = self.engine.tray_snapshot(check::now());
        match tray::Tray::start(self.post.clone(), snap.clone()) {
            Some(t) => {
                self.tray = Some(t);
                self.engine.set_tray_up(true);
                self.last_snap = snap;
                self.trace.stage("tray icon up");
                self.trace.memory("tray up");
                true
            }
            None => false,
        }
    }

    /// A window could not be made: the state machine says whether Hub goes on (a tray holds it).
    fn window_failed(&mut self) {
        self.life.tray = self.tray.is_some();
        let (l, d) = tray::step(self.life, Input::OpenFailed);
        self.life = l;
        if d == Do::Exit {
            self.exit_now = true;
        }
        self.destroy_window();
    }

    /// Gives back everything the window cost: the window itself, its surface, the bitmap, the
    /// fonts and glyph caches. The engine stays: it is what the tray lives on.
    fn destroy_window(&mut self) {
        self.popup = None;
        self.settings = None;
        self.engine.set_focused(false);
        self.surface = None;
        self.window = None;
        self.bmp = Bitmap::new(1, 1);
        self.gfx = None;
        self.layout = Layout::default();
        self.scroll = 0.0;
        self.max_scroll = 0.0;
        self.nscroll = 0.0;
        self.nmax = 0.0;
        self.nview = 0.0;
        self.nsig = None;
        self.mouse = (-1.0, -1.0);
        self.cursor = CursorIcon::Default;
        self.shown = false;
        self.first = false;
        self.dirty = false;
        self.engine.set_hover(None);
        self.trace.memory("window closed");
    }

    /// Something asked for the window: the tray, its balloon, a second launch.
    fn open(&mut self, event_loop: &ActiveEventLoop) {
        self.life.tray = self.tray.is_some();
        let (l, d) = tray::step(self.life, Input::Open);
        self.life = l;
        match d {
            Do::Create => self.create_window(event_loop),
            Do::Raise => self.raise(),
            _ => {}
        }
    }

    /// The window's X or Alt+F4. With the tray on, the window is destroyed and Hub stays (an
    /// install in progress goes on without it); with none, Hub quits as it always did.
    fn request_close(&mut self) {
        self.life.tray = self.tray.is_some();
        let (l, d) = tray::step(self.life, Input::CloseWindow);
        match d {
            Do::Destroy => {
                self.life = l;
                // Closing the window on the offer bar's question is "Not now": the question does
                // not stay behind to open the window again.
                self.engine.apply(Cmd::BannerNo);
                self.destroy_window();
            }
            Do::Exit => self.request_exit(),
            _ => self.life = l,
        }
    }

    /// Has a second launch asked Hub to quit (`hub --quit`)? Drains what the single-instance
    /// listener queued.
    fn quit_requested(&mut self) -> bool {
        let mut quit = false;
        if let Some(rx) = &self.single {
            while let Ok(r) = rx.try_recv() {
                quit |= r.from.as_deref() == Some(QUIT_FROM);
            }
        }
        quit
    }

    fn tray_command(&mut self, event_loop: &ActiveEventLoop, cmd: TrayCmd) {
        match cmd {
            TrayCmd::Open | TrayCmd::BalloonClicked => self.open(event_loop),
            TrayCmd::UpdateAll => self.apply(Cmd::UpdateAll),
            TrayCmd::CheckNow => self.apply(Cmd::CheckNow),
            TrayCmd::OpenApp(id) => self.apply(Cmd::Open(id)),
            // The menu and the card change the same settings through the engine.
            TrayCmd::ToggleNotify => self.engine.set_notify(!self.engine.notify_on()),
            TrayCmd::ToggleStart => {
                let on = self.engine.autostart_on();
                self.engine.set_autostart(!on);
            }
            TrayCmd::Quit => self.request_exit(),
        }
        self.open_to_ask(event_loop);
        self.sync_tray();
        self.dirty = true;
    }

    /// The engine has a question only a window can show (`tray::wants_window`): make one.
    fn open_to_ask(&mut self, event_loop: &ActiveEventLoop) {
        if !self.exit_now && tray::wants_window(self.engine.asking(), self.window.is_some()) {
            self.open(event_loop);
        }
    }

    /// Gives the tray the engine's current picture (badge, tooltip, menu) when it changed, and
    /// the open settings card the engine's rows.
    fn sync_tray(&mut self) {
        self.sync_settings();
        let Some(t) = &self.tray else { return };
        let s = self.engine.tray_snapshot(check::now());
        if s != self.last_snap {
            t.update(s.clone());
            self.last_snap = s;
        }
    }

    /// The close button, Alt+F4 without a tray, "Quit Hub": leaves at once unless an install or a
    /// roll back is running. That
    /// one is told to stop (it places nothing more, or ends what it is placing) and is waited
    /// for, a few seconds at most; a second close does not wait.
    fn request_exit(&mut self) {
        if self.closing.is_some() || !self.engine.installing() {
            self.exit_now = true;
            return;
        }
        self.engine.begin_close();
        self.closing = Some(Instant::now() + CLOSE_WAIT);
        self.dirty = true;
        if !self.engine.installing() {
            self.exit_now = true;
        }
    }

    fn finish(&mut self, event_loop: &ActiveEventLoop) {
        if std::mem::replace(&mut self.finished, true) {
            return;
        }
        self.trace.memory("exit");
        self.trace.cpu("exit");
        // The icon goes before the process does, so no ghost is left in the notification area.
        self.tray = None;
        // Ending (not restarting): a downloaded Hub is swapped in now, so the next start is it.
        if self.restart_exe.is_none() {
            self.engine.apply_hub_on_quit();
        }
        event_loop.exit();
    }
}

/// `hub --quit`: asks the running Hub (if any) to end. Nothing else starts.
fn quit_running() -> i32 {
    let settings = Store::open().dir.join("settings.ini");
    let request = mdshared_app::sister::Request {
        unplace: true,
        from: Some(QUIT_FROM.to_string()),
        ..Default::default()
    };
    // Nobody running is not an error: the wish is already met.
    mdshared_app::single::forward_request("hub", &settings, &request);
    0
}

/// The window (or the picture of `--demo`); the process exit code.
pub fn run(mut args: Args, started: Instant) -> i32 {
    let trace = Trace::new(args.trace, started);
    trace.stage("start");
    if args.quit {
        return quit_running();
    }
    // `--no-tray` wins over `--tray`: with no tray there is nothing to hold a window-less Hub.
    args.tray &= !args.no_tray;
    let event_loop = match EventLoop::<Event>::with_user_event().build() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("hub: no event loop: {e}");
            return 1;
        }
    };
    let proxy = event_loop.create_proxy();
    let post: Arc<dyn Post> = Arc::new(Proxy(Mutex::new(proxy.clone())));
    let mut single = None;
    let engine = match &args.demo {
        Some(name) => match demo::build(name, args.view.unwrap_or(View::List)) {
            Some(vm) => Engine::demo(vm, post.clone()),
            None => {
                eprintln!("hub: no demo '{name}'; try: {}", demo::NAMES.join(", "));
                return 2;
            }
        },
        None => {
            let store = Store::open();
            let settings = store.dir.join("settings.ini");
            // Started by Hub's own update: the old Hub is still ending, and would be mistaken
            // for a running one (and handed this launch) without the wait.
            mdshared_app::relaunch::wait_for_predecessor(
                args.restarted
                    .then(|| mdshared_app::relaunch::FLAG.to_string()),
                &settings,
            );
            if args.tray {
                // Started by the Run key while Hub already runs: nothing to open, nothing to wake.
                if mdshared_app::single::alive(&settings) {
                    return 0;
                }
            } else if mdshared_app::single::forward("hub", &settings, &[]) {
                // One Hub per user: a second launch raises (or makes) the first's window and ends.
                return 0;
            }
            let waker = proxy.clone();
            single = mdshared_app::single::listen_requests_waking("hub", &settings, move || {
                let _ = waker.send_event(Event::Raise);
            });
            // What an earlier update left beside this exe (`.old`, `.new`) goes now that this is
            // the only Hub running: two small file removals, nothing when there are none.
            let hub_exe = selfupdate::installed_exe();
            if let Some(exe) = &hub_exe {
                selfupdate::clean_up(exe);
            }
            let first =
                cli::first_check(&args, std::env::var("HUB_FIRST_CHECK_MS").ok().as_deref());
            let engine = Engine::new(Deps {
                store,
                env: glue::system_env(),
                transport: glue::make_transport(),
                post: post.clone(),
                shell: Arc::new(RealShell),
                tmpl: Tmpl::real(),
                first_check: Some(first),
                hub_exe,
                run_key: glue::system_run_key(),
            });
            trace.stage("engine loaded (cached releases, what is installed)");
            engine
        }
    };
    let vm = engine.vm(check::now());
    let want_tray = tray::wants_tray(
        args.tray,
        args.no_tray,
        engine.tray_on(),
        args.demo.is_some(),
    );
    let mut app = App {
        exit_at: args
            .exit_after_ms
            .map(|ms| started + Duration::from_millis(ms)),
        system_dark: mdshared_ui::platform::system_is_dark(),
        args,
        trace,
        engine,
        window: None,
        surface: None,
        bmp: Bitmap::new(1, 1),
        gfx: None,
        scale: 1.0,
        layout: Layout::default(),
        vm,
        scroll: 0.0,
        max_scroll: 0.0,
        nscroll: 0.0,
        nmax: 0.0,
        nview: 0.0,
        nsig: None,
        mouse: (-1.0, -1.0),
        mods: ModifiersState::empty(),
        popup: None,
        last_title_click: None,
        cursor: CursorIcon::Default,
        dirty: false,
        first: false,
        shown: false,
        last_running: Instant::now(),
        exit_now: false,
        closing: None,
        finished: false,
        life: Life {
            window: true,
            tray: false,
        },
        tray: None,
        want_tray,
        post,
        last_snap: Snapshot::default(),
        settings: None,
        restart_exe: None,
        restart_windowless: false,
        opens: 0,
        single,
    };
    // Under the tray (`--tray`) the icon is the whole program, so it comes first; with a window
    // first, the icon waits until the window's first frame is out (`about_to_wait`).
    if app.args.tray {
        app.want_tray = false;
        let up = app.start_tray();
        app.life.window = !up;
        if !up {
            eprintln!("hub: no tray icon: opening the window instead");
        }
    }
    let result = event_loop.run_app(&mut app);
    // Hub's own update was swapped in: the window is closed, the icon is gone, the single-instance
    // lock goes with this process; the new Hub waits for that (`--restarted`).
    if let Some(exe) = app.restart_exe.take() {
        restart_hub(&exe, &app.args, app.restart_windowless);
    }
    match result {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("hub: {e}");
            1
        }
    }
}

/// The flags the new Hub is started with: `--tray` when the old one had no window open (it comes
/// back as the icon it was; a Hub whose window is open comes back with a window, whatever started
/// it), and the test flags `--behind`, `--no-tray` as they were.
fn restart_flags(args: &Args, windowless: bool) -> Vec<&'static str> {
    [
        (windowless && !args.no_tray, "--tray"),
        (args.behind, "--behind"),
        (args.no_tray, "--no-tray"),
    ]
    .into_iter()
    .filter(|(on, _)| *on)
    .map(|(_, f)| f)
    .collect()
}

/// Starts the new Hub from `exe` (see [`restart_flags`]). Nothing is started when `exe` is not
/// there; the old Hub has ended by then and the next launch is the new one.
fn restart_hub(exe: &std::path::Path, args: &Args, windowless: bool) {
    let Some(target) = selfupdate::restart_target(exe) else {
        return;
    };
    let mut cmd = mdshared_app::relaunch::command(&target);
    cmd.args(restart_flags(args, windowless));
    let _ = cmd.spawn();
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::dpi::PhysicalSize;
    use winit::window::Theme;

    /// `hub --quit` reaches the running Hub's listener as a request whose sender is `QUIT_FROM`,
    /// and a plain second launch (no request) does not look like one.
    #[test]
    fn a_quit_request_travels_to_the_listener_and_a_plain_launch_is_not_one() {
        let dir = std::env::temp_dir().join(format!("hub-quit-wire-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let settings = dir.join("settings.ini");
        let (tx, wake) = std::sync::mpsc::channel();
        let rx = mdshared_app::single::listen_requests_waking("hub", &settings, move || {
            let _ = tx.send(());
        })
        .expect("listen");
        // A plain second launch: the listener wakes, no request is queued.
        assert!(mdshared_app::single::forward("hub", &settings, &[]));
        wake.recv_timeout(Duration::from_secs(5)).expect("woken");
        assert!(rx.try_recv().is_err(), "a plain launch carries no request");
        // The quit request.
        let request = mdshared_app::sister::Request {
            unplace: true,
            from: Some(QUIT_FROM.to_string()),
            ..Default::default()
        };
        assert!(mdshared_app::single::forward_request(
            "hub", &settings, &request
        ));
        wake.recv_timeout(Duration::from_secs(5)).expect("woken");
        let got = rx
            .try_recv()
            .expect("the request arrives before the wake-up");
        assert_eq!(got.from.as_deref(), Some(QUIT_FROM));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_restart_comes_back_under_the_tray_only_when_no_window_was_open() {
        let tray_start = Args {
            tray: true,
            behind: true,
            ..Default::default()
        };
        // Started by the Run key, window closed: back as the icon.
        assert_eq!(restart_flags(&tray_start, true), ["--tray", "--behind"]);
        // The same Hub with its window opened from the icon: back with a window.
        assert_eq!(restart_flags(&tray_start, false), ["--behind"]);
        // Started with a window, then closed to the tray: not back with a window nobody asked for.
        assert_eq!(restart_flags(&Args::default(), true), ["--tray"]);
        assert!(restart_flags(&Args::default(), false).is_empty());
        // No tray at all: never `--tray`.
        let no_tray = Args {
            no_tray: true,
            ..Default::default()
        };
        assert_eq!(restart_flags(&no_tray, true), ["--no-tray"]);
    }

    #[test]
    fn what_moves_the_rows_or_the_window_closes_the_row_menu() {
        assert!(closes_popup(&WindowEvent::Resized(PhysicalSize::new(
            800, 600
        ))));
        assert!(!closes_popup(&WindowEvent::ThemeChanged(Theme::Dark)));
        assert!(!closes_popup(&WindowEvent::Focused(true)));
    }
}
