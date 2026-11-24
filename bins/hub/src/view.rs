//! The Hub window: the title row, the command bar, the list of apps (or the grid of tiles), the
//! pane under it for the selected app, an optional offer bar, and the status bar. `draw` paints a
//! `Vm` and returns the `Layout` of everything a pointer can hit. Every number is a constant.
use crate::changelog;
use crate::grid;
use crate::panes;
use crate::ui::{self, amber, blue, green, red};
use crate::vm::{Act, AppVm, Hit, Layout, View, Vm};
use hub_core::model::Row;
use mdshared_ui::chrome::{
    BAR_H, TITLE_H, WIN_BTN_W, WinButton, button_fill, draw_win_button, win_button_rect,
};
use mdshared_ui::gfx::{Face, Painter, Rect, Rgb, mix};
use mdshared_ui::glyph;
use mdshared_ui::notice::{OfferBarState, draw_offer_bar};
use mdshared_ui::theme::Palette;

pub const STATUS_H: f32 = 26.0;
pub const ROW_H: f32 = 52.0;
pub const HEAD_H: f32 = 28.0;
pub const BANNER_H: f32 = 34.0;
pub const PANE_TAB_H: f32 = 34.0;
pub const PANE_MIN: f32 = 150.0;
/// Under this width the Released and Download columns go.
pub const COMPACT_W: f32 = 1000.0;
/// Under this width the Installed and Latest columns go as well.
pub const NARROW_W: f32 = 640.0;
/// The filter box is drawn from this width up.
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

pub fn cols(w: f32) -> Cols {
    if w >= COMPACT_W {
        Cols {
            installed: 92.0,
            latest: 84.0,
            released: 84.0,
            size: 90.0,
            status: 250.0,
            actions: 200.0,
        }
    } else if w >= NARROW_W {
        Cols {
            installed: 78.0,
            latest: 70.0,
            released: 0.0,
            size: 0.0,
            status: 200.0,
            actions: 178.0,
        }
    } else {
        Cols {
            installed: 0.0,
            latest: 0.0,
            released: 0.0,
            size: 0.0,
            status: 140.0,
            actions: 178.0,
        }
    }
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
    title(p, pal, w, &mut lay);
    lay.chrome.push(Rect::new(0.0, 0.0, w, TITLE_H));
    if vm.notes.is_none() {
        command_bar(p, pal, w, vm, &mut lay);
        lay.chrome.push(Rect::new(0.0, TITLE_H, w, BAR_H));
    }
    if let Some(b) = &vm.banner {
        let r = Rect::new(0.0, TITLE_H + BAR_H, w, BANNER_H);
        let hits = draw_offer_bar(
            p,
            pal,
            r,
            if b.warn { glyph::WARN } else { glyph::DOWNLOAD },
            &ui::cap(&b.text, 300),
            &ui::cap(&b.yes, 40),
            &ui::cap(&b.no, 40),
            OfferBarState::default(),
        );
        for (rect, hit) in [
            (hits.yes, Hit::BannerYes),
            (hits.no, Hit::BannerNo),
            (hits.close, Hit::BannerNo),
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
            &mut lay,
        );
    } else {
        match vm.view {
            _ if vm.apps.is_empty() => empty(p, pal, geo.list),
            View::List => list(p, pal, geo.list, vm, scroll, &mut lay),
            _ => grid::draw(p, pal, geo.list, vm, scroll, &mut lay),
        }
        if geo.pane.h > 0.0 {
            panes::draw(p, pal, geo.pane, vm, &mut lay);
        }
    }
    let status = Rect::new(0.0, geo.status_y, w, STATUS_H.min(h));
    status_bar(p, pal, status, w, vm);
    lay.chrome.push(status);
    lay
}

fn empty(p: &mut Painter, pal: &Palette, r: Rect) {
    p.fill(r, pal.page);
    let old = p.clip(r);
    p.text_center(
        Face::Ui,
        13.0,
        Rect::new(r.x, r.y + HEAD_H, r.w, 80.0),
        "No apps to show",
        pal.muted,
    );
    p.restore(old);
}

// ---- the title row ----------------------------------------------------------------------------

fn title(p: &mut Painter, pal: &Palette, w: f32, lay: &mut Layout) {
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
        draw_win_button(p, pal, kind, b, false, false, false);
        lay.hits.push((b, hit));
    }
}

// ---- the command bar --------------------------------------------------------------------------

fn bar_button(
    p: &mut Painter,
    pal: &Palette,
    x: f32,
    y: f32,
    g: &str,
    label: &str,
    enabled: bool,
) -> Rect {
    let cy = y + 16.0;
    let w = 44.0 + p.measure(Face::Ui, 13.0, label);
    let b = Rect::new(x, y, w, 32.0);
    if let Some(fill) = button_fill(pal, false, false, false, false).filter(|_| enabled) {
        p.rbox(b, 6.0, fill);
    }
    let color = if enabled { pal.text } else { pal.faint };
    p.icon(g, 15.0, x + 18.0, cy, color);
    p.text(Face::Ui, 13.0, x + 34.0, cy, label, color, f32::INFINITY);
    b
}

fn primary_button(p: &mut Painter, pal: &Palette, x: f32, y: f32, g: &str, label: &str) -> Rect {
    let cy = y + 16.0;
    let wd = 44.0 + p.measure(Face::Semibold, 13.0, label);
    let b = Rect::new(x, y, wd, 32.0);
    p.rbox(b, 6.0, pal.accent);
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

fn command_bar(p: &mut Painter, pal: &Palette, w: f32, vm: &Vm, lay: &mut Layout) {
    let r = Rect::new(0.0, TITLE_H, w, BAR_H);
    p.fill(r, pal.bar);
    let y = r.y + (BAR_H - 32.0) / 2.0;
    let cy = y + 16.0;
    // The right group first, so the left one knows how much room it has.
    // The sliders button (settings) is the last thing on the right.
    let gear = Rect::new(w - 8.0 - 32.0, y, 32.0, 32.0);
    if let Some(fill) = button_fill(pal, false, false, false, false) {
        p.rbox(gear, 6.0, fill);
    }
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
        if on {
            p.rbox(
                Rect::new(b.x + 2.0, b.y + 2.0, b.w - 4.0, b.h - 4.0),
                4.0,
                pal.on,
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
    let mut limit = sr.x - 12.0;
    if w >= FILTER_W {
        let fw = 220.0;
        let f = Rect::new(sr.x - 8.0 - fw, y, fw, 32.0);
        p.frame(f, 6.0, pal.field, pal.line);
        if vm.filter_focus {
            p.fill(
                Rect::new(f.x + 4.0, f.bottom() - 2.0, f.w - 8.0, 2.0),
                pal.accent,
            );
        }
        p.icon(glyph::SEARCH, 13.0, f.x + 17.0, cy, pal.muted);
        let inner = fw - 44.0;
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
                Rect::new(caret_x.min(f.right() - 8.0), cy - 8.0, px, 16.0),
                pal.text,
            );
        }
        lay.hits.push((f, Hit::Filter));
        limit = f.x - 12.0;
    }
    // The left group.
    let sep = |p: &mut Painter, x: f32| -> f32 {
        p.fill(
            Rect::new((x + 6.5).round(), y + 6.0, p.px(), 20.0),
            pal.line,
        );
        13.0
    };
    let mut x = 8.0;
    if let Some(t) = &vm.busy {
        let b = bar_button(p, pal, x, y, glyph::CLOSE, "Cancel all", true);
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
        if vm.updates > 0 {
            let b = primary_button(
                p,
                pal,
                x,
                y,
                glyph::DOWNLOAD,
                &format!("Update all ({})", vm.updates),
            );
            lay.hits.push((b, Hit::UpdateAll));
            x += b.w + 6.0;
            x += sep(p, x);
        }
        let checking = vm.checked.starts_with("Checking");
        let b = bar_button(
            p,
            pal,
            x,
            y,
            glyph::RELOAD,
            if checking {
                "Checking\u{2026}"
            } else {
                "Check now"
            },
            !checking,
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

fn list(p: &mut Painter, pal: &Palette, r: Rect, vm: &Vm, scroll: f32, lay: &mut Layout) {
    p.fill(r, pal.page);
    let c = cols(r.w);
    let px = p.px().max(1.0);
    let act_x = r.right() - c.actions;
    let stat_x = act_x - c.status;
    let size_x = stat_x - c.size;
    let rel_x = size_x - c.released;
    let lat_x = rel_x - c.latest;
    let ins_x = lat_x - c.installed;
    let name_w = (ins_x - 58.0 - 8.0).max(0.0);
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
        row(
            p,
            pal,
            vm,
            i,
            a,
            rr,
            [ins_x, lat_x, rel_x, size_x, stat_x],
            &c,
            name_w,
        );
        let cy = rr.cy();
        let mut hits = vec![(rr, Hit::Row(i))];
        let mut ax = rr.right() - 12.0;
        hits.push((Rect::new(ax - 24.0, cy - 14.0, 28.0, 28.0), Hit::Menu(i)));
        ax -= 32.0;
        for b in row_actions(a) {
            let w = ui::btn_w(p, b.label);
            hits.push((Rect::new(ax - w, cy - 14.0, w, 28.0), Hit::Action(i, b.act)));
            ax -= w + 6.0;
        }
        for (rect, hit) in hits {
            if let Some(v) = ui::intersect(rect, body) {
                lay.hits.push((v, hit));
            }
        }
    }
    p.restore(old);
    // The column heads stay put.
    let old = p.clip(Rect::new(r.x, r.y, r.w, HEAD_H.min(r.h)));
    let head = Rect::new(r.x, r.y, r.w, HEAD_H);
    p.fill(head, pal.pane);
    p.fill(Rect::new(r.x, head.bottom() - px, r.w, px), pal.line);
    let hy = head.cy();
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
    name_w: f32,
) {
    let [ins_x, lat_x, rel_x, size_x, stat_x] = xs;
    let px = p.px().max(1.0);
    let sel = vm.sel == i;
    if sel {
        p.fill(rr, pal.on);
        p.fill(Rect::new(rr.x, rr.y + 2.0, 3.0, rr.h - 4.0), pal.accent);
    } else if vm.hover == Some(i) {
        p.fill(rr, pal.hover);
    }
    if matches!(a.row, Row::Failed(_)) {
        p.fill_a(rr, red(pal), if pal.dark { 0.12 } else { 0.08 });
    }
    p.fill_a(Rect::new(rr.x, rr.bottom() - px, rr.w, px), pal.line, 0.55);
    let bg = if sel { pal.on } else { pal.page };
    let cy = rr.cy();
    // The app.
    let tile = Rect::new(rr.x + 14.0, cy - 16.0, 32.0, 32.0);
    ui::app_tile(p, tile, a.id, a.glyph, a.tile, false);
    let nx = tile.right() + 12.0;
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
    let s = status_of(pal, a, c.released == 0.0);
    let sx = stat_x + 10.0;
    let lead = if s.dot.is_some() { 15.0 } else { 0.0 };
    let two = !s.sub.is_empty() || s.bar.is_some();
    let ly = if two { cy - 9.0 } else { cy };
    if let Some(d) = s.dot {
        ui::dot(p, sx, ly, d);
    }
    let room = (c.status - 20.0 - lead).max(0.0);
    p.text(
        Face::Ui,
        13.0,
        sx + lead,
        ly,
        &ui::cap(&s.line, 200),
        s.line_col,
        room,
    );
    if let Some((f, col)) = s.bar {
        ui::progress(
            p,
            pal,
            Rect::new(sx, cy + 8.0, (c.status - 28.0).max(0.0), 6.0),
            f,
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
            s.sub_col,
            room,
        );
    }
    // Actions, from the right.
    let mut ax = rr.right() - 12.0;
    p.icon(glyph::MORE, 15.0, ax - 10.0, cy, pal.muted);
    ax -= 32.0;
    for b in row_actions(a) {
        let r = ui::sbtn_r(p, pal, ax, cy, b.label, b.primary, true);
        ax = r.x - 6.0;
    }
}

// ---- the status bar ---------------------------------------------------------------------------

fn status_bar(p: &mut Painter, pal: &Palette, r: Rect, w: f32, vm: &Vm) {
    p.fill(r, pal.bar);
    p.fill(Rect::new(r.x, r.y, r.w, p.px().max(1.0)), pal.line);
    let old = p.clip(r);
    let cy = r.cy();
    p.icon(glyph::GLOBE, 12.0, 16.0, cy, pal.muted);
    let left = if w < COMPACT_W {
        "Contacts github.com only"
    } else {
        "Contacts github.com only: release lists and the files you install. No account, no usage data."
    };
    let mut right = ui::cap(&vm.checked, 120);
    if !vm.hub_version.is_empty() {
        if !right.is_empty() {
            right.push_str(" \u{b7} ");
        }
        right.push_str(&format!("Hub {}", ui::cap(&vm.hub_version, 40)));
    }
    let rw = p.text_right(Face::Ui, 12.0, r.right() - 14.0, cy, &right, pal.muted);
    p.text(
        Face::Ui,
        12.0,
        30.0,
        cy,
        left,
        pal.muted,
        (w - 30.0 - rw - 30.0).max(0.0),
    );
    p.restore(old);
}
