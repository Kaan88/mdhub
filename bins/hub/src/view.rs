//! The Hub window: the title row, the command bar, the list of apps (or the grid of tiles), the
//! pane under it for the selected app, an optional offer bar, and the status bar. `draw` paints a
//! `Vm` and returns the `Layout` of everything a pointer can hit. Every number is a constant.
use crate::changelog;
use crate::grid;
use crate::motion::{self, ButtonLook};
use crate::panes;
use crate::ui::{self, amber, blue, green, red};
use crate::vm::{Act, AppVm, Hit, Layout, Pointer, View, Vm};
use hub_core::model::Row;
use mdshared_ui::anim::Animator;
use mdshared_ui::chrome::{
    BAR_H, TITLE_H, WIN_BTN_W, WinButton, button_fill_animated, draw_win_button_animated,
    win_button_rect,
};
use mdshared_ui::gfx::{Face, Painter, Rect, Rgb, mix};
use mdshared_ui::glyph;
use mdshared_ui::notice::{OfferBarHits, OfferBarState, draw_offer_bar};
use mdshared_ui::theme::Palette;

pub const STATUS_H: f32 = 26.0;
pub const ROW_H: f32 = 52.0;
pub const HEAD_H: f32 = 28.0;
pub const BANNER_H: f32 = 34.0;
pub const PANE_TAB_H: f32 = 34.0;
pub const PANE_MIN: f32 = 150.0;
/// Under this width the Released and Download columns go.
pub const COMPACT_W: f32 = 1000.0;
const NAME_X: f32 = 58.0;
const NAME_GAP: f32 = 8.0;
const NAME_MIN: f32 = 110.0;
/// The filter box is always expanded from this width up.
pub const FILTER_W: f32 = 900.0;

/// One button of a row or tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActionBtn {
    pub act: Act,
    pub label: &'static str,
    pub primary: bool,
}

/// The buttons a row shows, the main one first (the grid shows only that one; the list draws
/// them from the right edge in this order). Empty only for an app that is neither installed nor
/// installable (a `NoInfo` row with nothing installed): its status line then says why.
pub fn row_actions(a: &AppVm) -> Vec<ActionBtn> {
    let b = |act, label, primary| ActionBtn {
        act,
        label,
        primary,
    };
    let open = b(Act::Open, "Open", false);
    match &a.row {
        Row::Update if a.running => vec![b(Act::Update, "Update\u{2026}", true)],
        Row::Update => vec![b(Act::Update, "Update", true), open],
        Row::UpToDate | Row::Done(_) => vec![open],
        Row::NotInstalled => vec![b(Act::Install, "Install", false)],
        Row::External => vec![b(Act::Adopt, "Adopt", false), open],
        Row::Downloading(..) | Row::Verifying | Row::Waiting(_) => {
            vec![b(Act::Cancel, "Cancel", false)]
        }
        Row::Failed(_) => vec![b(Act::TryAgain, "Try again", true)],
        Row::NoInfo(_) if a.installed.is_some() => vec![open],
        Row::NoInfo(_) => vec![],
    }
}

/// Is the app on its way to a newer version (or wanting one)?
pub fn newer(row: &Row) -> bool {
    matches!(
        row,
        Row::Update | Row::Downloading(..) | Row::Verifying | Row::Waiting(_) | Row::Failed(_)
    )
}

pub struct Cols {
    pub installed: f32,
    pub latest: f32,
    pub released: f32,
    pub size: f32,
    pub status: f32,
    pub actions: f32,
}

impl Cols {
    fn starts(&self, right: f32) -> [f32; 5] {
        let status = right - self.actions - self.status;
        let size = status - self.size;
        let released = size - self.released;
        let latest = released - self.latest;
        [latest - self.installed, latest, released, size, status]
    }

    pub fn name_w(&self, w: f32) -> f32 {
        (self.starts(w)[0] - NAME_X - NAME_GAP).max(0.0)
    }
}

pub fn cols(w: f32) -> Cols {
    let mut c = if w >= COMPACT_W {
        Cols {
            installed: 92.0,
            latest: 84.0,
            released: 84.0,
            size: 90.0,
            status: 250.0,
            actions: 200.0,
        }
    } else {
        Cols {
            installed: 78.0,
            latest: 70.0,
            released: 0.0,
            size: 0.0,
            status: 200.0,
            actions: 178.0,
        }
    };
    if c.name_w(w) < NAME_MIN {
        c.installed = 0.0;
        c.latest = 0.0;
        c.status = 140.0;
    }
    c
}

/// Where the parts of the window fall at this size.
#[derive(Clone, Copy, Debug, Default)]
pub struct Geo {
    pub list: Rect,
    /// Height 0 when the pane does not fit.
    pub pane: Rect,
    pub status_y: f32,
    /// What the list wants, head included.
    pub content_h: f32,
    /// How far the list can scroll.
    pub max_scroll: f32,
}

pub fn content_height(vm: &Vm, w: f32) -> f32 {
    let n = vm.apps.len();
    if n == 0 {
        return HEAD_H + 80.0;
    }
    match vm.view {
        View::List => HEAD_H + n as f32 * ROW_H,
        v => grid::height(v, w, n),
    }
}

pub fn geometry(w: f32, h: f32, vm: &Vm) -> Geo {
    let top = TITLE_H + BAR_H + if vm.banner.is_some() { BANNER_H } else { 0.0 };
    let status_y = (h - STATUS_H).max(top);
    let room = status_y - top;
    let content_h = content_height(vm, w);
    let has_sel = vm.sel < vm.apps.len();
    let pane_h = if has_sel && room - content_h >= PANE_MIN {
        room - content_h
    } else {
        0.0
    };
    let list = Rect::new(0.0, top, w, room - pane_h);
    Geo {
        list,
        pane: Rect::new(0.0, list.bottom(), w, pane_h),
        status_y,
        content_h,
        max_scroll: (content_h - list.h).max(0.0),
    }
}

/// Draws the window with the list not scrolled.
pub fn draw(p: &mut Painter, pal: &Palette, w: f32, h: f32, vm: &Vm) -> Layout {
    draw_scrolled(p, pal, w, h, vm, 0.0)
}

/// Draws the window with the list scrolled by `scroll` logical px (clamped to
/// `geometry(..).max_scroll`).
pub fn draw_scrolled(
    p: &mut Painter,
    pal: &Palette,
    w: f32,
    h: f32,
    vm: &Vm,
    scroll: f32,
) -> Layout {
    draw_full(p, pal, w, h, vm, scroll, 0.0)
}

/// Draws the window: the list scrolled by `scroll`, or, when the release notes view is open, that
/// view with its text scrolled by `notes_scroll` (both clamped).
pub fn draw_full(
    p: &mut Painter,
    pal: &Palette,
    w: f32,
    h: f32,
    vm: &Vm,
    scroll: f32,
    notes_scroll: f32,
) -> Layout {
    draw_motion(
        p,
        pal,
        w,
        h,
        vm,
        [scroll, notes_scroll],
        &mut motion::still(),
    )
}

pub fn draw_motion(
    p: &mut Painter,
    pal: &Palette,
    w: f32,
    h: f32,
    vm: &Vm,
    [scroll, notes_scroll]: [f32; 2],
    anim: &mut Animator,
) -> Layout {
    let (w, h) = (
        if w.is_finite() { w.max(0.0) } else { 0.0 },
        if h.is_finite() { h.max(0.0) } else { 0.0 },
    );
    let mut lay = Layout::default();
    p.fill(Rect::new(0.0, 0.0, w, h), pal.page);
    let geo = geometry(w, h, vm);
    lay.list_content_h = geo.content_h;
    let scroll = if scroll.is_finite() {
        scroll.clamp(0.0, geo.max_scroll)
    } else {
        0.0
    };
    title(p, pal, w, vm, &mut lay, anim);
    lay.chrome.push(Rect::new(0.0, 0.0, w, TITLE_H));
    if vm.notes.is_none() {
        command_bar(p, pal, w, vm, &mut lay, anim);
        lay.chrome.push(Rect::new(0.0, TITLE_H, w, BAR_H));
    }
    if let Some(b) = &vm.banner {
        let top = if vm.notes.is_some() {
            TITLE_H
        } else {
            TITLE_H + BAR_H
        };
        let r = Rect::new(0.0, top, w, BANNER_H);
        let hits = offer(p, pal, r, b, vm.pointer, anim);
        for (rect, hit) in [
            (hits.yes, Hit::BannerYes),
            (hits.no, Hit::BannerNo),
            (hits.close, Hit::BannerClose),
        ] {
            if let Some(rect) = rect {
                lay.hits.push((rect, hit));
            }
        }
    }
    if let Some(n) = &vm.notes {
        // The release notes take the whole content area: no list, no pane.
        let top = TITLE_H + if vm.banner.is_some() { BANNER_H } else { 0.0 };
        let bottom = geo.status_y.max(top);
        changelog::draw_page(
            p,
            pal,
            Rect::new(0.0, top, w, bottom - top),
            n,
            notes_scroll,
            &vm.pointer,
            &mut lay,
            anim,
        );
        if let Some((r, _)) = lay.hits.iter().find(|(_, hit)| *hit == vm.notes_focus) {
            // The keyboard focus ring follows the very same visible rectangle a click uses.
            mdshared_ui::focus::draw_ring(p, pal, *r, 6.0);
        }
    } else {
        match vm.view {
            _ if vm.apps.is_empty() => empty(p, pal, geo.list, !vm.filter.is_empty()),
            View::List => list(p, pal, geo.list, vm, scroll, &mut lay, anim),
            _ => grid::draw(p, pal, geo.list, vm, scroll, &mut lay, anim),
        }
        if geo.pane.h > 0.0 {
            panes::draw(p, pal, geo.pane, vm, &mut lay, anim);
        }
    }
    let status = Rect::new(0.0, geo.status_y, w, STATUS_H.min(h));
    status_bar(p, pal, status, vm);
    lay.chrome.push(status);
    lay
}

fn empty(p: &mut Painter, pal: &Palette, r: Rect, filtered: bool) {
    p.fill(r, pal.page);
    let old = p.clip(r);
    p.text_center(
        Face::Ui,
        13.0,
        Rect::new(r.x, r.y + HEAD_H, r.w, 80.0),
        if filtered {
            "No apps match your filter."
        } else {
            "No apps to show"
        },
        pal.muted,
    );
    p.restore(old);
}

// ---- the title row ----------------------------------------------------------------------------

fn title(p: &mut Painter, pal: &Palette, w: f32, vm: &Vm, lay: &mut Layout, anim: &mut Animator) {
    p.fill(Rect::new(0.0, 0.0, w, TITLE_H), pal.title);
    let right = (w - 3.0 * WIN_BTN_W).max(0.0);
    let drag = Rect::new(0.0, 0.0, right, TITLE_H);
    if drag.w > 0.0 {
        lay.hits.push((drag, Hit::Drag));
    }
    let old = p.clip(drag);
    ui::tile(
        p,
        Rect::new(14.0, 9.0, 22.0, 22.0),
        "system",
        [0x4a, 0x55, 0xb8],
        false,
    );
    p.text(
        Face::Semibold,
        13.0,
        46.0,
        TITLE_H / 2.0,
        "Hub",
        pal.text,
        (right - 56.0).max(0.0),
    );
    p.restore(old);
    for (n, (kind, hit)) in [
        (WinButton::Close, Hit::Close),
        (WinButton::Max, Hit::Max),
        (WinButton::Min, Hit::Min),
    ]
    .into_iter()
    .enumerate()
    {
        let b = win_button_rect(w, n);
        let look = vm.pointer.look(hit);
        draw_win_button_animated(
            p,
            pal,
            anim,
            hit,
            kind,
            b,
            vm.maximized,
            look.hovered,
            look.pressed,
        );
        lay.hits.push((b, hit));
    }
}

// ---- the command bar --------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)] // a button: where, its icon and words, and its look
fn bar_button(
    p: &mut Painter,
    pal: &Palette,
    x: f32,
    y: f32,
    g: &str,
    label: &str,
    enabled: bool,
    look: ButtonLook,
) -> Rect {
    let cy = y + 16.0;
    let w = 44.0 + p.measure(Face::Ui, 13.0, label);
    let b = Rect::new(x, y, w, 32.0);
    if enabled {
        ui::quiet_fill(p, pal, b, 6.0, look);
    }
    let color = if enabled { pal.text } else { pal.faint };
    p.icon(g, 15.0, x + 18.0, cy, color);
    p.text(Face::Ui, 13.0, x + 34.0, cy, label, color, f32::INFINITY);
    b
}

fn primary_button(
    p: &mut Painter,
    pal: &Palette,
    x: f32,
    y: f32,
    g: &str,
    label: &str,
    look: ButtonLook,
) -> Rect {
    let cy = y + 16.0;
    let wd = 44.0 + p.measure(Face::Semibold, 13.0, label);
    let b = Rect::new(x, y, wd, 32.0);
    p.rbox(b, 6.0, ui::accent_fill(pal, look));
    p.icon(g, 14.0, x + 18.0, cy, pal.on_accent);
    p.text(
        Face::Semibold,
        13.0,
        x + 34.0,
        cy,
        label,
        pal.on_accent,
        f32::INFINITY,
    );
    b
}

fn command_bar(
    p: &mut Painter,
    pal: &Palette,
    w: f32,
    vm: &Vm,
    lay: &mut Layout,
    anim: &mut Animator,
) {
    let r = Rect::new(0.0, TITLE_H, w, BAR_H);
    p.fill(r, pal.bar);
    let y = r.y + (BAR_H - 32.0) / 2.0;
    let cy = y + 16.0;
    // The right group first, so the left one knows how much room it has.
    // The sliders button (settings) is the last thing on the right.
    let gear = Rect::new(w - 8.0 - 32.0, y, 32.0, 32.0);
    ui::quiet_fill(
        p,
        pal,
        gear,
        6.0,
        motion::button(anim, Hit::Settings, vm.pointer, Hit::Settings),
    );
    p.icon(glyph::SLIDERS, 15.0, gear.cx(), cy, pal.text);
    lay.hits.push((gear, Hit::Settings));
    let seg = 30.0;
    let sr = Rect::new(gear.x - 8.0 - (3.0 * seg + 2.0), y, 3.0 * seg + 2.0, 32.0);
    p.frame(sr, 6.0, pal.field, pal.line);
    for (k, (g, v)) in [
        (ui::LIST_ICON, View::List),
        (ui::LARGE_ICON, View::Large),
        (ui::SMALL_ICON, View::Small),
    ]
    .into_iter()
    .enumerate()
    {
        let b = Rect::new(sr.x + 1.0 + k as f32 * seg, y + 1.0, seg, 30.0);
        let on = vm.view == v;
        let look = vm.pointer.look(Hit::ViewSwitch(v));
        if let Some((fill, alpha)) = button_fill_animated(
            anim,
            Hit::ViewSwitch(v),
            pal,
            look.hovered,
            look.pressed,
            on,
            false,
        ) {
            p.rbox_a(
                Rect::new(b.x + 2.0, b.y + 2.0, b.w - 4.0, b.h - 4.0),
                4.0,
                fill,
                alpha,
            );
        }
        p.icon(
            g,
            14.0,
            b.cx(),
            b.cy(),
            if on { pal.accent } else { pal.muted },
        );
        lay.hits.push((b, Hit::ViewSwitch(v)));
    }
    let filter_right = sr.x - 8.0;
    let filtered = !vm.filter.is_empty();
    let expanded = w >= FILTER_W || vm.filter_focus;
    let left_x = 8.0;
    let checking = vm.checked.starts_with("Checking");
    let label = if checking {
        "Checking\u{2026}"
    } else {
        "Check now"
    };
    let update_label =
        (vm.busy.is_none() && vm.updates > 0).then(|| format!("Update all ({})", vm.updates));
    let use_left_space = w < FILTER_W && vm.filter_focus && vm.busy.is_none() && {
        let left_right = left_x
            + 44.0
            + p.measure(Face::Ui, 13.0, label)
            + update_label
                .as_ref()
                .map_or(0.0, |s| 63.0 + p.measure(Face::Semibold, 13.0, s));
        left_right > filter_right - 128.0
    };
    // If the two groups cannot fit, editing gets the whole strip up to the view switch.
    let fw = if w >= FILTER_W {
        220.0
    } else if use_left_space {
        (filter_right - left_x).max(0.0)
    } else if expanded {
        128.0
    } else {
        32.0 + if filtered { 32.0 } else { 0.0 }
    };
    let f = Rect::new(filter_right - fw, y, fw, 32.0);
    let limit = f.x - 12.0;
    if expanded {
        p.frame(f, 6.0, pal.field, pal.line);
        if vm.filter_focus {
            p.fill(
                Rect::new(f.x + 4.0, f.bottom() - 2.0, f.w - 8.0, 2.0),
                pal.accent,
            );
        }
        p.icon(glyph::SEARCH, 13.0, f.x + 17.0, cy, pal.muted);
        let inner = fw - 44.0 - if filtered { 24.0 } else { 0.0 };
        let mut caret_x = f.x + 34.0;
        if vm.filter.is_empty() {
            p.text(
                Face::Ui,
                13.0,
                f.x + 34.0,
                cy,
                "Filter apps",
                pal.faint,
                inner,
            );
        } else {
            let t = ui::cap(&vm.filter, 200);
            caret_x += p.text(Face::Ui, 13.0, f.x + 34.0, cy, &t, pal.text, inner);
        }
        if vm.filter_focus {
            let px = p.px();
            p.fill(
                Rect::new(caret_x.min(f.x + 34.0 + inner), cy - 8.0, px, 16.0),
                pal.text,
            );
        }
        let input = Rect::new(f.x, f.y, f.w - if filtered { 30.0 } else { 0.0 }, f.h);
        lay.hits.push((input, Hit::Filter));
    } else {
        let b = Rect::new(f.x, y, 32.0, 32.0);
        if filtered {
            p.rbox(b, 6.0, pal.on);
        }
        ui::quiet_fill(
            p,
            pal,
            b,
            6.0,
            motion::button(anim, Hit::FilterOpen, vm.pointer, Hit::FilterOpen),
        );
        p.icon(
            glyph::SEARCH,
            15.0,
            b.cx(),
            cy,
            if filtered { pal.accent } else { pal.text },
        );
        lay.hits.push((b, Hit::FilterOpen));
    }
    if filtered {
        let b = Rect::new(f.right() - 30.0, y + 2.0, 28.0, 28.0);
        ui::quiet_fill(
            p,
            pal,
            b,
            5.0,
            motion::button(anim, Hit::FilterClear, vm.pointer, Hit::FilterClear),
        );
        p.icon(glyph::CLOSE, 11.0, b.cx(), cy, pal.muted);
        lay.hits.push((b, Hit::FilterClear));
    }
    if use_left_space {
        return;
    }
    // The left group.
    let sep = |p: &mut Painter, x: f32| -> f32 {
        p.fill(
            Rect::new((x + 6.5).round(), y + 6.0, p.px(), 20.0),
            pal.line,
        );
        13.0
    };
    let mut x = left_x;
    if let Some(t) = &vm.busy {
        let look = motion::button(anim, Hit::CancelAll, vm.pointer, Hit::CancelAll);
        let b = bar_button(p, pal, x, y, glyph::CLOSE, "Cancel all", true, look);
        lay.hits.push((b, Hit::CancelAll));
        x += b.w + 2.0;
        x += sep(p, x);
        p.text(
            Face::Ui,
            13.0,
            x + 6.0,
            cy,
            &ui::cap(t, 200),
            pal.muted,
            (limit - x - 6.0).max(0.0),
        );
    } else {
        if let Some(label) = update_label {
            let b = primary_button(
                p,
                pal,
                x,
                y,
                glyph::DOWNLOAD,
                &label,
                motion::button(anim, Hit::UpdateAll, vm.pointer, Hit::UpdateAll),
            );
            lay.hits.push((b, Hit::UpdateAll));
            x += b.w + 6.0;
            x += sep(p, x);
        }
        let b = bar_button(
            p,
            pal,
            x,
            y,
            glyph::RELOAD,
            label,
            !checking,
            motion::button(anim, Hit::CheckNow, vm.pointer, Hit::CheckNow),
        );
        if !checking {
            lay.hits.push((b, Hit::CheckNow));
        }
    }
}

// ---- the list ---------------------------------------------------------------------------------

pub struct Status {
    pub dot: Option<Rgb>,
    pub line: String,
    pub line_col: Rgb,
    pub sub: String,
    pub sub_col: Rgb,
    pub bar: Option<(f32, Rgb)>,
}

/// The words of a row's status cell. Every state has its words; `compact` puts the download
/// size under them, since the Download column is gone.
pub fn status_of(pal: &Palette, a: &AppVm, compact: bool) -> Status {
    let s = |dot, line: &str, lc, sub: &str, sc| Status {
        dot,
        line: line.into(),
        line_col: lc,
        sub: sub.into(),
        sub_col: sc,
        bar: None,
    };
    let size_line = if compact && !a.size.is_empty() {
        format!("{} download", a.size)
    } else {
        String::new()
    };
    match &a.row {
        Row::UpToDate => s(Some(green(pal)), "Up to date", pal.text, "", pal.muted),
        Row::Update => {
            let sub = if a.running {
                "Open now: asks it to close".to_string()
            } else {
                size_line
            };
            s(
                Some(blue(pal)),
                "Update available",
                pal.text,
                &sub,
                pal.muted,
            )
        }
        Row::NotInstalled => s(None, "Not installed", pal.muted, &size_line, pal.faint),
        Row::External => s(
            Some(pal.muted),
            "Installed elsewhere",
            pal.text,
            "Not installed by Hub",
            pal.muted,
        ),
        Row::Downloading(f, t) => Status {
            bar: Some((*f, pal.accent)),
            ..s(
                Some(pal.accent),
                if t.is_empty() { "Downloading" } else { t },
                pal.text,
                "",
                pal.muted,
            )
        },
        Row::Verifying => Status {
            bar: Some((1.0, mix(pal.accent, pal.page, 0.45))),
            ..s(
                Some(pal.accent),
                "Checking SHA-256",
                pal.text,
                "",
                pal.muted,
            )
        },
        Row::Waiting(why) => s(Some(amber(pal)), "Waiting", pal.text, why, pal.muted),
        Row::Failed(why) => s(Some(red(pal)), "Not updated", red(pal), why, pal.muted),
        Row::Done(t) => s(Some(green(pal)), "Updated", pal.text, t, pal.muted),
        Row::NoInfo(why) => s(
            Some(pal.muted),
            "No release information",
            pal.text,
            why,
            pal.muted,
        ),
    }
}

fn list(
    p: &mut Painter,
    pal: &Palette,
    r: Rect,
    vm: &Vm,
    scroll: f32,
    lay: &mut Layout,
    anim: &mut Animator,
) {
    p.fill(r, pal.page);
    let c = cols(r.w);
    let px = p.px().max(1.0);
    let starts = c.starts(r.right());
    let body = Rect::new(r.x, r.y + HEAD_H, r.w, (r.h - HEAD_H).max(0.0));
    let old = p.clip(body);
    let mut y = body.y - scroll;
    for (i, a) in vm.apps.iter().enumerate() {
        let rr = Rect::new(r.x, y, r.w, ROW_H);
        y += ROW_H;
        if rr.bottom() < body.y {
            continue;
        }
        if rr.y > body.bottom() {
            break;
        }
        row(p, pal, vm, i, a, rr, starts, &c, anim, |rect, hit| {
            if let Some(v) = ui::intersect(rect, body) {
                lay.hits.push((v, hit));
            }
        });
    }
    p.restore(old);
    // The column heads stay put.
    let old = p.clip(Rect::new(r.x, r.y, r.w, HEAD_H.min(r.h)));
    let head = Rect::new(r.x, r.y, r.w, HEAD_H);
    p.fill(head, pal.pane);
    p.fill(Rect::new(r.x, head.bottom() - px, r.w, px), pal.line);
    let hy = head.cy();
    let [ins_x, lat_x, rel_x, size_x, stat_x] = starts;
    let hd = |p: &mut Painter, x: f32, t: &str| {
        p.text(Face::Semibold, 12.0, x + 10.0, hy, t, pal.muted, 200.0);
    };
    hd(p, r.x + 4.0, "App");
    if c.installed > 0.0 {
        hd(p, ins_x, "Installed");
        hd(p, lat_x, "Latest");
    }
    if c.released > 0.0 {
        hd(p, rel_x, "Released");
        hd(p, size_x, "Download");
    }
    hd(p, stat_x, "Status");
    p.restore(old);
}

#[allow(clippy::too_many_arguments)]
fn row(
    p: &mut Painter,
    pal: &Palette,
    vm: &Vm,
    i: usize,
    a: &AppVm,
    rr: Rect,
    xs: [f32; 5],
    c: &Cols,
    anim: &mut Animator,
    mut hit: impl FnMut(Rect, Hit),
) {
    let [ins_x, lat_x, rel_x, size_x, stat_x] = xs;
    let px = p.px().max(1.0);
    let sel = vm.sel == i;
    let key = motion::app_key(vm, i);
    let ground = motion::row_ground(
        anim,
        vm.view,
        key,
        if sel {
            pal.on
        } else if vm.hover == Some(i) {
            pal.hover
        } else {
            pal.page
        },
    );
    if ground != pal.page {
        p.fill(rr, ground);
    }
    if sel {
        p.fill(Rect::new(rr.x, rr.y + 2.0, 3.0, rr.h - 4.0), pal.accent);
    }
    if matches!(a.row, Row::Failed(_)) {
        p.fill_a(rr, red(pal), if pal.dark { 0.12 } else { 0.08 });
    }
    p.fill_a(Rect::new(rr.x, rr.bottom() - px, rr.w, px), pal.line, 0.55);
    let bg = if sel { ground } else { pal.page };
    let cy = rr.cy();
    // The app.
    let tile = Rect::new(rr.x + 14.0, cy - 16.0, 32.0, 32.0);
    ui::app_tile(p, tile, a.id, a.glyph, a.tile, false);
    let nx = rr.x + NAME_X;
    let name_w = (ins_x - nx - NAME_GAP).max(0.0);
    let nw = p.text(
        Face::Semibold,
        13.5,
        nx,
        cy - 9.0,
        &ui::cap(a.name, 200),
        pal.text,
        name_w,
    );
    if a.running && nw + 10.0 + 12.0 + 40.0 < name_w {
        let tx = nx + nw + 10.0;
        ui::dot(p, tx, cy - 9.0, green(pal));
        p.text(
            Face::Ui,
            11.5,
            tx + 12.0,
            cy - 9.0,
            "running",
            pal.muted,
            (name_w - nw - 22.0).min(80.0),
        );
    }
    p.text(
        Face::Ui,
        11.5,
        nx,
        cy + 9.0,
        &ui::cap(a.blurb, 200),
        pal.muted,
        (name_w - 4.0).max(0.0),
    );
    // Versions.
    if c.installed > 0.0 {
        match &a.installed {
            Some(v) => {
                p.text(
                    Face::Mono,
                    12.0,
                    ins_x + 10.0,
                    cy,
                    &ui::cap(v, 40),
                    pal.text,
                    c.installed - 10.0,
                );
            }
            None => {
                p.text(
                    Face::Ui,
                    13.0,
                    ins_x + 10.0,
                    cy,
                    "\u{2014}",
                    pal.faint,
                    c.installed,
                );
            }
        }
        let nw = newer(&a.row);
        let lat = a.latest.as_deref().unwrap_or("\u{2014}");
        p.text(
            if nw { Face::Semibold } else { Face::Mono },
            12.0,
            lat_x + 10.0,
            cy,
            &ui::cap(lat, 40),
            if nw { blue(pal) } else { pal.muted },
            c.latest - 10.0,
        );
    }
    if c.released > 0.0 {
        p.text(
            Face::Ui,
            13.0,
            rel_x + 10.0,
            cy,
            &a.date,
            pal.muted,
            c.released - 10.0,
        );
        let sz = match a.row {
            Row::UpToDate | Row::External | Row::Done(_) | Row::NoInfo(_) => "",
            _ => a.size.as_str(),
        };
        p.text(
            Face::Ui,
            13.0,
            size_x + 10.0,
            cy,
            sz,
            pal.muted,
            c.size - 10.0,
        );
    }
    // Status.
    let opacity = motion::status(anim, key, &a.row);
    let amount = motion::progress(anim, key, &a.row);
    let s = status_of(pal, a, c.released == 0.0);
    let sx = stat_x + 10.0;
    let lead = if s.dot.is_some() { 15.0 } else { 0.0 };
    let two = !s.sub.is_empty() || s.bar.is_some();
    let ly = if two { cy - 9.0 } else { cy };
    if let Some(d) = s.dot {
        ui::dot(p, sx, ly, mix(ground, d, opacity));
    }
    let room = (c.status - 20.0 - lead).max(0.0);
    p.text(
        Face::Ui,
        13.0,
        sx + lead,
        ly,
        &ui::cap(&s.line, 200),
        mix(ground, s.line_col, opacity),
        room,
    );
    if let Some((_, col)) = s.bar {
        ui::progress(
            p,
            pal,
            Rect::new(sx, cy + 8.0, (c.status - 28.0).max(0.0), 6.0),
            amount,
            col,
            bg,
        );
    } else if !s.sub.is_empty() {
        p.text(
            Face::Ui,
            11.5,
            sx + lead,
            cy + 9.0,
            &ui::cap(&s.sub, 200),
            mix(ground, s.sub_col, opacity),
            room,
        );
    }
    // Actions, from the right.
    hit(rr, Hit::Row(i));
    let actions = Rect::new(stat_x + c.status, rr.y, c.actions, rr.h);
    let old = p.clip(actions);
    let mut ax = rr.right() - 12.0;
    let menu = Rect::new(ax - 24.0, cy - 14.0, 28.0, 28.0);
    ui::icon_fill(
        p,
        pal,
        menu,
        ground,
        motion::button(anim, (vm.view, key, "menu"), vm.pointer, Hit::Menu(i)),
    );
    p.icon(glyph::MORE, 15.0, ax - 10.0, cy, pal.muted);
    hit(menu, Hit::Menu(i));
    ax -= 32.0;
    for b in row_actions(a) {
        let look = motion::button(
            anim,
            (vm.view, key, b.act),
            vm.pointer,
            Hit::Action(i, b.act),
        );
        let r = ui::sbtn_r(p, pal, ax, cy, b.label, b.primary, look);
        if let Some(rect) = ui::intersect(r, actions) {
            hit(rect, Hit::Action(i, b.act));
        }
        ax = r.x - 6.0;
    }
    p.restore(old);
}

// ---- the status bar ---------------------------------------------------------------------------

const STATUS_LEFT: f32 = 30.0;
const STATUS_RIGHT: f32 = 14.0;
const STATUS_GAP: f32 = 16.0;
const STATUS_SEP: &str = " · ";

struct StatusSlots {
    privacy: bool,
    message_max: f32,
    version_w: f32,
}

fn status_slots(w: f32, privacy: f32, msg: f32, version: f32, sep: f32) -> StatusSlots {
    let usable = (w - STATUS_LEFT - STATUS_RIGHT).max(0.0);
    let version_w = version.min(usable);
    let privacy = privacy + STATUS_GAP + msg + version_w + sep <= usable;
    StatusSlots {
        privacy,
        message_max: (usable - version_w - sep).max(0.0),
        version_w,
    }
}

fn status_bar(p: &mut Painter, pal: &Palette, r: Rect, vm: &Vm) {
    p.fill(r, pal.bar);
    p.fill(Rect::new(r.x, r.y, r.w, p.px().max(1.0)), pal.line);
    let old = p.clip(r);
    let cy = r.cy();
    p.icon(glyph::GLOBE, 12.0, 16.0, cy, pal.muted);
    let left = if r.w < COMPACT_W {
        "Contacts github.com only"
    } else {
        "Contacts github.com only: release lists and the files you install. No account, no usage data."
    };
    let msg = ui::cap(&vm.checked, 120);
    let version = if vm.hub_version.is_empty() {
        String::new()
    } else {
        format!("Hub {}", ui::cap(&vm.hub_version, 40))
    };
    let measure = |p: &mut Painter, s: &str| p.measure(Face::Ui, 12.0, s);
    let sep = if msg.is_empty() || version.is_empty() {
        ""
    } else {
        STATUS_SEP
    };
    let sep_w = measure(p, sep);
    let msg_w = measure(p, &msg);
    let slots = status_slots(r.w, measure(p, left), msg_w, measure(p, &version), sep_w);
    let msg = if msg_w <= slots.message_max {
        msg
    } else if measure(p, "…") <= slots.message_max {
        p.elide(Face::Ui, 12.0, &msg, slots.message_max)
    } else {
        String::new()
    };
    let version_x = r.right() - STATUS_RIGHT - slots.version_w;
    if slots.version_w > 0.0 {
        let clip = p.clip(Rect::new(version_x, r.y, slots.version_w, r.h));
        p.text(
            Face::Ui,
            12.0,
            version_x,
            cy,
            &version,
            pal.muted,
            f32::INFINITY,
        );
        p.restore(clip);
    }
    if !msg.is_empty() {
        p.text(
            Face::Ui,
            12.0,
            version_x - sep_w,
            cy,
            sep,
            pal.muted,
            f32::INFINITY,
        );
        p.text_right(Face::Ui, 12.0, version_x - sep_w, cy, &msg, pal.muted);
    }
    if slots.privacy {
        p.text(
            Face::Ui,
            12.0,
            STATUS_LEFT,
            cy,
            left,
            pal.muted,
            f32::INFINITY,
        );
    }
    p.restore(old);
}

/// Shared's offer bar takes boolean looks. Only during a fade, draw its lit buttons over their
/// resting pixels, with the whole bar's geometry and hit testing still owned by shared.
fn offer(
    p: &mut Painter,
    pal: &Palette,
    r: Rect,
    b: &crate::vm::Banner,
    ptr: Pointer,
    anim: &mut Animator,
) -> OfferBarHits {
    let warn_pal = Palette {
        bar: pal.warn_bg,
        muted: pal.warn,
        ..*pal
    };
    let pal = if b.warn { &warn_pal } else { pal };
    let [yes, no, close] = [Hit::BannerYes, Hit::BannerNo, Hit::BannerClose]
        .map(|hit| motion::button(anim, (b.key.app, b.key.serial, hit), ptr, hit));
    let yes_text = ui::cap(&b.yes, 40);
    let no_text = ui::cap(&b.no, 40);
    let body = ui::cap(&b.text, 300);
    let draw = |p: &mut Painter, cutoff: f32| {
        draw_offer_bar(
            p,
            pal,
            r,
            if b.warn { glyph::WARN } else { glyph::DOWNLOAD },
            &body,
            &yes_text,
            &no_text,
            OfferBarState {
                yes_hovered: yes.pressed || yes.hover > cutoff,
                yes_pressed: yes.pressed,
                no_hovered: no.pressed || no.hover > cutoff,
                no_pressed: no.pressed,
                close_hovered: close.hover > cutoff,
            },
        )
    };
    let hits = draw(p, 0.999_999);
    for (rect, look) in [(hits.yes, yes), (hits.no, no), (hits.close, close)] {
        if let Some(rect) = rect
            && !look.pressed
            && look.hover > 0.0
            && look.hover < 1.0
        {
            let under = p.snapshot(rect);
            let clip = p.clip(rect);
            draw(p, 0.0);
            p.fade(&under, look.hover);
            p.restore(clip);
        }
    }
    hits
}
