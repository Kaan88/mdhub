//! The icon views: the same apps, states and actions as the list, as a grid of tiles. Large tiles
//! carry the versions, the status and the one action of the row; small tiles carry the icon, the
//! name and one word, and leave the rest to the pane under the grid and the row menu.
use crate::ui::{self, amber, blue, green, red};
use crate::view::{HEAD_H, newer, row_actions, status_of};
use crate::vm::{AppVm, Hit, Layout, View, Vm};
use hub_core::model::Row;
use mdshared_ui::gfx::{Face, Painter, Rect, Rgb, mix};
use mdshared_ui::glyph;
use mdshared_ui::theme::Palette;

pub const MARGIN: f32 = 16.0;
pub const GAP: f32 = 8.0;
pub const PAD: f32 = 12.0;
pub const LARGE_MIN_W: f32 = 164.0;
pub const LARGE_H: f32 = 196.0;
pub const SMALL_MIN_W: f32 = 108.0;
pub const SMALL_H: f32 = 104.0;

/// Columns, the width of a tile and its height, at window width `w`.
pub fn geometry(view: View, w: f32) -> (usize, f32, f32) {
    let (min, h) = match view {
        View::Small => (SMALL_MIN_W, SMALL_H),
        _ => (LARGE_MIN_W, LARGE_H),
    };
    let avail = (w - 2.0 * MARGIN).max(0.0);
    let cols = (((avail + GAP) / (min + GAP)).floor() as usize).max(1);
    let cw = ((avail - GAP * (cols as f32 - 1.0)) / cols as f32)
        .floor()
        .max(0.0);
    (cols, cw, h)
}

/// The height the grid wants for `n` apps.
pub fn height(view: View, w: f32, n: usize) -> f32 {
    let (cols, _, h) = geometry(view, w);
    let rows = n.div_ceil(cols);
    HEAD_H + PAD + rows as f32 * (h + GAP) - GAP + PAD
}

/// The one word under a small tile, and its colour.
fn word(pal: &Palette, a: &AppVm) -> (String, Rgb) {
    match &a.row {
        Row::UpToDate => (a.installed.clone().unwrap_or_default(), pal.muted),
        Row::Update => ("Update".into(), blue(pal)),
        Row::NotInstalled => ("Not installed".into(), pal.faint),
        Row::External => ("Not by Hub".into(), pal.muted),
        Row::Downloading(..) => (String::new(), pal.muted),
        Row::Verifying => ("Checking".into(), pal.accent),
        Row::Waiting(_) => ("Waiting".into(), amber(pal)),
        Row::Failed(_) => ("Not updated".into(), red(pal)),
        Row::Done(_) => ("Updated".into(), green(pal)),
        Row::NoInfo(_) => ("No info".into(), pal.muted),
    }
}

/// The badges on a tile: an update (an arrow), a failure (red, !), done (green check), and
/// running (a green dot at the foot of the icon). `bg` is the colour behind the tile, for the ring.
fn badges(p: &mut Painter, pal: &Palette, t: Rect, a: &AppVm, bg: Rgb) {
    let d = (t.w * 0.36).clamp(14.0, 20.0);
    let ring = |p: &mut Painter, cx: f32, cy: f32, d: f32, c: Rgb| {
        let o = Rect::new(cx - d / 2.0 - 2.0, cy - d / 2.0 - 2.0, d + 4.0, d + 4.0);
        p.rbox(o, o.w / 2.0, bg);
        p.rbox(Rect::new(cx - d / 2.0, cy - d / 2.0, d, d), d / 2.0, c);
    };
    let (cx, cy) = (t.right() - 2.0, t.y + 2.0);
    if newer(&a.row) && !matches!(a.row, Row::Failed(_)) {
        ring(p, cx, cy, d, pal.accent);
        p.icon(glyph::DOWNLOAD, d * 0.62, cx, cy, pal.on_accent);
    } else if matches!(a.row, Row::Failed(_)) {
        ring(p, cx, cy, d, red(pal));
        p.text_center(
            Face::Semibold,
            d * 0.7,
            Rect::new(cx - d / 2.0, cy - d / 2.0, d, d),
            "!",
            [255, 255, 255],
        );
    } else if matches!(a.row, Row::Done(_)) {
        ring(p, cx, cy, d, green(pal));
        p.icon(glyph::CHECK, d * 0.62, cx, cy, [255, 255, 255]);
    }
    if a.running {
        ring(p, t.right() - 2.0, t.bottom() - 2.0, d * 0.6, green(pal));
    }
}

pub fn draw(p: &mut Painter, pal: &Palette, r: Rect, vm: &Vm, scroll: f32, lay: &mut Layout) {
    p.fill(r, pal.page);
    let px = p.px().max(1.0);
    let body = Rect::new(r.x, r.y + HEAD_H, r.w, (r.h - HEAD_H).max(0.0));
    let (cols, cw, ch) = geometry(vm.view, r.w);
    let old = p.clip(body);
    for (k, a) in vm.apps.iter().enumerate() {
        let (col, row) = (k % cols, k / cols);
        let c = Rect::new(
            r.x + MARGIN + col as f32 * (cw + GAP),
            body.y + PAD + row as f32 * (ch + GAP) - scroll,
            cw,
            ch,
        );
        if c.bottom() < body.y {
            continue;
        }
        if c.y > body.bottom() {
            break;
        }
        let hits = match vm.view {
            View::Small => small(p, pal, c, vm, k, a),
            _ => large(p, pal, c, vm, k, a),
        };
        for (rect, hit) in std::iter::once((c, Hit::Row(k))).chain(hits) {
            if let Some(v) = ui::intersect(rect, body) {
                lay.hits.push((v, hit));
            }
        }
    }
    p.restore(old);
    // The strip above the tiles, in the place of the list's column heads.
    let old = p.clip(Rect::new(r.x, r.y, r.w, HEAD_H.min(r.h)));
    let head = Rect::new(r.x, r.y, r.w, HEAD_H);
    p.fill(head, pal.pane);
    p.fill(Rect::new(r.x, head.bottom() - px, r.w, px), pal.line);
    let n_upd = vm
        .apps
        .iter()
        .filter(|a| matches!(a.row, Row::Update))
        .count();
    let left = format!(
        "{} apps \u{b7} {} installed \u{b7} {} with an update",
        vm.apps.len(),
        vm.apps.iter().filter(|a| a.installed.is_some()).count(),
        n_upd
    );
    p.text(
        Face::Semibold,
        12.0,
        r.x + 14.0,
        head.cy(),
        &left,
        pal.muted,
        r.w - 28.0,
    );
    p.restore(old);
}

/// The tile's ground: selected, hovered, failed, or plain; returns the colour behind its content.
fn ground(
    p: &mut Painter,
    pal: &Palette,
    c: Rect,
    vm: &Vm,
    i: usize,
    a: &AppVm,
    framed: bool,
) -> Rgb {
    let sel = vm.sel == i;
    let failed = matches!(a.row, Row::Failed(_));
    let base = if sel {
        pal.on
    } else if vm.hover == Some(i) {
        pal.hover
    } else if framed {
        mix(pal.page, pal.text, 0.025)
    } else {
        pal.page
    };
    let bg = if failed {
        mix(base, red(pal), if pal.dark { 0.12 } else { 0.08 })
    } else {
        base
    };
    if sel {
        p.frame(c, 8.0, bg, pal.accent);
    } else if framed {
        p.frame(c, 8.0, bg, mix(pal.line, pal.page, 0.3));
    } else if bg != pal.page {
        p.rbox(c, 8.0, bg);
    }
    bg
}

fn large(
    p: &mut Painter,
    pal: &Palette,
    c: Rect,
    vm: &Vm,
    i: usize,
    a: &AppVm,
) -> Vec<(Rect, Hit)> {
    let mut hits = Vec::new();
    let bg = ground(p, pal, c, vm, i, a, true);
    let not_in = matches!(a.row, Row::NotInstalled);
    let cx = c.cx();
    // The icon.
    let t = Rect::new(cx - 28.0, c.y + 16.0, 56.0, 56.0);
    ui::app_tile(p, t, a.id, a.glyph, a.tile, not_in);
    badges(p, pal, t, a, bg);
    let menu = Rect::new(c.right() - 30.0, c.y + 4.0, 26.0, 26.0);
    ui::icon_fill(p, pal, menu, bg, vm.pointer.look(Hit::Menu(i)));
    p.icon(glyph::MORE, 15.0, c.right() - 16.0, c.y + 16.0, pal.muted);
    hits.push((menu, Hit::Menu(i)));
    // Name and blurb.
    let tw = (c.w - 20.0).max(0.0);
    let line = |y: f32| Rect::new(c.x + 10.0, y - 10.0, tw, 20.0);
    p.text_center(
        Face::Semibold,
        13.5,
        line(c.y + 88.0),
        &ui::cap(a.name, 200),
        pal.text,
    );
    p.text_center(
        Face::Ui,
        11.5,
        line(c.y + 105.0),
        &ui::cap(a.blurb, 200),
        pal.muted,
    );
    // Versions, or the progress bar while it downloads.
    let vy = c.y + 126.0;
    let bar_w = (c.w - 44.0).max(0.0);
    match &a.row {
        Row::Downloading(f, _) => {
            ui::progress(
                p,
                pal,
                Rect::new(c.x + 22.0, vy - 3.0, bar_w, 6.0),
                *f,
                pal.accent,
                bg,
            );
        }
        Row::Verifying => {
            ui::progress(
                p,
                pal,
                Rect::new(c.x + 22.0, vy - 3.0, bar_w, 6.0),
                1.0,
                mix(pal.accent, pal.page, 0.45),
                bg,
            );
        }
        row => {
            let latest = a.latest.clone().unwrap_or_else(|| "\u{2014}".into());
            let (first, second) = match (&a.installed, newer(row)) {
                (Some(v), true) => (ui::cap(v, 30), Some(ui::cap(&latest, 30))),
                (Some(v), false) => (ui::cap(v, 30), None),
                (None, _) if a.size.is_empty() => (ui::cap(&latest, 30), None),
                (None, _) => (format!("{} \u{b7} {}", ui::cap(&latest, 30), a.size), None),
            };
            let face = if a.installed.is_some() {
                Face::Mono
            } else {
                Face::Ui
            };
            let aw = p.measure(face, 12.0, &first);
            let arrow = " \u{2192} ";
            let bw = second.as_ref().map_or(0.0, |b| {
                p.measure(Face::Ui, 12.0, arrow) + p.measure(Face::Semibold, 12.0, b)
            });
            let mut x = cx - (aw + bw) / 2.0;
            x += p.text(face, 12.0, x, vy, &first, pal.muted, tw);
            if let Some(b) = second {
                x += p.text(Face::Ui, 12.0, x, vy, arrow, pal.faint, 40.0);
                p.text(Face::Semibold, 12.0, x, vy, &b, blue(pal), 80.0);
            }
        }
    }
    // The status line: the tile has no room for a second line, so the reason joins it.
    let s = status_of(pal, a, true);
    let sy = c.y + 146.0;
    let (text, col) = match &a.row {
        Row::Waiting(why) => (format!("Waiting: {why}"), pal.text),
        Row::Failed(why) => (why.clone(), red(pal)),
        Row::Done(t) if !t.is_empty() => (format!("Updated: {t}"), s.line_col),
        Row::NoInfo(why) => (why.clone(), pal.muted),
        Row::NotInstalled => (s.line.clone(), s.line_col),
        _ if a.running && matches!(a.row, Row::Update) => {
            ("Open now: asks it to close".to_string(), pal.text)
        }
        _ => (s.line.clone(), s.line_col),
    };
    let text = ui::cap(&text, 200);
    let lead = if s.dot.is_some() { 13.0 } else { 0.0 };
    let w = p.measure(Face::Ui, 12.0, &text).min((tw - lead).max(0.0));
    let x0 = cx - (w + lead) / 2.0;
    if let Some(d) = s.dot {
        ui::dot(p, x0, sy, d);
    }
    p.text(
        Face::Ui,
        12.0,
        x0 + lead,
        sy,
        &text,
        col,
        (tw - lead).max(0.0),
    );
    // The action.
    if let Some(b) = row_actions(a).first() {
        let bw = ui::btn_w(p, b.label);
        let r = ui::sbtn(
            p,
            pal,
            (cx - bw / 2.0).round(),
            c.y + 174.0,
            b.label,
            b.primary,
            vm.pointer.look(Hit::Action(i, b.act)),
        );
        hits.push((r, Hit::Action(i, b.act)));
    }
    hits
}

fn small(
    p: &mut Painter,
    pal: &Palette,
    c: Rect,
    vm: &Vm,
    i: usize,
    a: &AppVm,
) -> Vec<(Rect, Hit)> {
    let bg = ground(p, pal, c, vm, i, a, false);
    let dim = matches!(a.row, Row::NotInstalled);
    let cx = c.cx();
    let t = Rect::new(cx - 22.0, c.y + 12.0, 44.0, 44.0);
    ui::app_tile(p, t, a.id, a.glyph, a.tile, dim);
    badges(p, pal, t, a, bg);
    let tw = (c.w - 12.0).max(0.0);
    let line = |y: f32| Rect::new(c.x + 6.0, y - 9.0, tw, 18.0);
    p.text_center(
        Face::Semibold,
        12.5,
        line(c.y + 70.0),
        &ui::cap(a.name, 200),
        pal.text,
    );
    match &a.row {
        Row::Downloading(f, _) => ui::progress(
            p,
            pal,
            Rect::new(c.x + 18.0, c.y + 84.0, (c.w - 36.0).max(0.0), 5.0),
            *f,
            pal.accent,
            bg,
        ),
        _ => {
            let (wd, col) = word(pal, a);
            let face = if matches!(a.row, Row::UpToDate) {
                Face::Mono
            } else {
                Face::Ui
            };
            p.text_center(face, 11.5, line(c.y + 87.0), &ui::cap(&wd, 40), col);
        }
    }
    Vec::new()
}
