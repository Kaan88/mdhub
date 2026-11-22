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
use crate::trace::Trace;
use crate::view::{self, HEAD_H, ROW_H};
use crate::vm::{Hit, Layout, View, Vm};
use hub_core::check;
use hub_core::state::Store;
use mdshared_text::bitmap::Bitmap;
use mdshared_ui::gfx::{Face, Gfx, Painter, Rect, mix};
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
    gfx: Gfx,
    scale: f32,
    system_dark: bool,
    /// What the last frame drew, for the next click.
    layout: Layout,
    vm: Vm,
    scroll: f32,
    max_scroll: f32,
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
    /// Kept for as long as the window lives (the second-launch listener sends into it).
    _single: Option<std::sync::mpsc::Receiver<std::path::PathBuf>>,
}

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
        let mut p = Painter::new(&mut self.bmp, &mut self.gfx, self.scale);
        let layout = view::draw_scrolled(&mut p, &pal, lw, lh, &vm, self.scroll);
        let _ = maximized;
        if let Some(pop) = &self.popup {
            draw_popup(&mut p, &pal, pop);
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
            Hit::Close => self.request_exit(),
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
        if self.window.is_some() {
            return;
        }
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
                event_loop.exit();
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
                event_loop.exit();
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
        self.trace
            .line(&format!("first frame after {:.1} ms", self.trace.since()));
        self.trace.memory("first frame");
        self.trace.cpu("first frame");
    }

    fn user_event(&mut self, _: &ActiveEventLoop, ev: Event) {
        if matches!(ev, Event::Raise) {
            self.raise();
            return;
        }
        if self.engine.handle(ev) {
            self.dirty = true;
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        if self.popup.is_some() && closes_popup(&event) {
            self.popup = None;
            self.dirty = true;
        }
        match event {
            WindowEvent::CloseRequested => self.request_exit(),
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
            WindowEvent::Focused(true) => {
                // An app may have been started or closed meanwhile.
                if self.last_running.elapsed() >= RUNNING_EVERY {
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
            } => self.click(),
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y * ROW_H,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / self.scale,
                };
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
        if self.shown {
            if std::mem::take(&mut self.first) {
                // After the first frame: which apps run (one pass over the process list).
                self.last_running = Instant::now();
                self.engine.refresh_running_async();
                self.trace.stage("running apps looked up");
            }
            if self.engine.tick(now) {
                self.dirty = true;
            }
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
        if let Some(d) = self.engine.wait(now).filter(|_| self.shown) {
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
    /// The close button, Alt+F4: leaves at once unless an install or a roll back is running. That
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
        event_loop.exit();
    }
}

/// The window (or the picture of `--demo`); the process exit code.
pub fn run(args: Args, started: Instant) -> i32 {
    let trace = Trace::new(args.trace, started);
    trace.stage("start");
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
            Some(vm) => Engine::demo(vm, post),
            None => {
                eprintln!("hub: no demo '{name}'; try: {}", demo::NAMES.join(", "));
                return 2;
            }
        },
        None => {
            let store = Store::open();
            // One Hub per user: a second launch raises the first and ends.
            let settings = store.dir.join("settings.ini");
            if mdshared_app::single::forward("hub", &settings, &[]) {
                return 0;
            }
            let waker = proxy.clone();
            single = mdshared_app::single::listen_waking("hub", &settings, move || {
                let _ = waker.send_event(Event::Raise);
            });
            let first =
                cli::first_check(&args, std::env::var("HUB_FIRST_CHECK_MS").ok().as_deref());
            let engine = Engine::new(Deps {
                store,
                env: glue::system_env(),
                transport: glue::make_transport(),
                post,
                shell: Arc::new(RealShell),
                tmpl: Tmpl::real(),
                first_check: Some(first),
            });
            trace.stage("engine loaded (cached releases, what is installed)");
            engine
        }
    };
    let vm = engine.vm(check::now());
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
        gfx: Gfx::new(),
        scale: 1.0,
        layout: Layout::default(),
        vm,
        scroll: 0.0,
        max_scroll: 0.0,
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
        _single: single,
    };
    match event_loop.run_app(&mut app) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("hub: {e}");
            1
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use winit::dpi::PhysicalSize;
    use winit::window::Theme;

    #[test]
    fn what_moves_the_rows_or_the_window_closes_the_row_menu() {
        assert!(closes_popup(&WindowEvent::Resized(PhysicalSize::new(
            800, 600
        ))));
        assert!(!closes_popup(&WindowEvent::ThemeChanged(Theme::Dark)));
        assert!(!closes_popup(&WindowEvent::Focused(true)));
    }
}
