//! The icon views: the same apps, states and actions as the list, as a grid of tiles. Large tiles
//! carry the versions, the status and the one action of the row; small tiles carry the icon, the
//! name, one word, and a menu for their actions.
use crate::motion;
use crate::ui::{self, amber, blue, green, red};
use crate::view::{HEAD_H, newer, row_actions, status_of};
use crate::vm::{AppVm, Hit, Layout, View, Vm};
use hub_core::model::Row;
use mdshared_ui::anim::Animator;
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
const SMALL_MENU: f32 = 28.0;

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

/// Bounds of a tile row in scroll space, below the fixed header.
fn row_bounds(row: usize, tile_h: f32) -> (f32, f32) {
    let top = PAD + row as f32 * (tile_h + GAP);
    (top, top + tile_h)
}

pub fn selection_bounds(view: View, w: f32, i: usize) -> (f32, f32) {
    let (cols, _, h) = geometry(view, w);
    row_bounds(i / cols, h)
}

/// The one word under a small tile, and its colour.
fn word(pal: &Palette, a: &AppVm) -> (String, Rgb) {
    match &a.row {
        Row::UpToDate => (a.installed.clone().unwrap_or_default(), pal.muted),
        Row::Update => ("Update".into(), blue(pal)),
        Row::NotInstalled => ("Not installed".into(), pal.muted),
        Row::External => ("Not by Hub".into(), pal.muted),
        Row::Downloading(..) => (String::new(), pal.muted),
        Row::Verifying => ("Checking".into(), pal.accent),
        Row::Waiting(_) => ("Waiting".into(), amber(pal)),
        Row::Failed(_) => ("Not updated".into(), red(pal)),
        Row::Done(_) => ("Updated".into(), green(pal)),
        Row::NoInfo(_) => ("No info".into(), pal.muted),
    }
}

fn badge_ink(pal: &Palette) -> Rgb {
    if pal.dark { pal.page } else { [255; 3] }
}

/// The badges on a tile: an update (an arrow), a failure (red, !), done (green check), and
/// running (a green dot at the foot of the icon). `bg` is the colour behind the tile, for the ring.
fn badges(p: &mut Painter, pal: &Palette, t: Rect, a: &AppVm, bg: Rgb, opacity: f32) {
    let d = (t.w * 0.36).clamp(14.0, 20.0);
    let ring = |p: &mut Painter, cx: f32, cy: f32, d: f32, c: Rgb| {
        let o = Rect::new(cx - d / 2.0 - 2.0, cy - d / 2.0 - 2.0, d + 4.0, d + 4.0);
        p.rbox(o, o.w / 2.0, bg);
        p.rbox(Rect::new(cx - d / 2.0, cy - d / 2.0, d, d), d / 2.0, c);
    };
    let (cx, cy) = (t.right() - 2.0, t.y + 2.0);
    if newer(&a.row) || matches!(a.row, Row::Done(_)) {
        let badge = Rect::new(cx - d / 2.0, cy - d / 2.0, d, d);
        let under = (opacity < 1.0).then(|| p.snapshot(badge.inset(-3.0)));
        if matches!(a.row, Row::Failed(_)) {
            ring(p, cx, cy, d, red(pal));
            p.text_center(Face::Semibold, d * 0.7, badge, "!", badge_ink(pal));
        } else if matches!(a.row, Row::Done(_)) {
            ring(p, cx, cy, d, green(pal));
            p.icon(glyph::CHECK, d * 0.62, cx, cy, badge_ink(pal));
        } else {
            ring(p, cx, cy, d, pal.accent);
            p.icon(glyph::DOWNLOAD, d * 0.62, cx, cy, pal.on_accent);
        }
        if let Some(under) = under {
            p.fade(&under, opacity);
        }
    }
    if a.running {
        ring(p, t.right() - 2.0, t.bottom() - 2.0, d * 0.6, green(pal));
    }
}

pub fn draw(
    p: &mut Painter,
    pal: &Palette,
    r: Rect,
    vm: &Vm,
    scroll: f32,
    lay: &mut Layout,
    anim: &mut Animator,
) {
    p.fill(r, pal.page);
    let px = p.px().max(1.0);
    let body = Rect::new(r.x, r.y + HEAD_H, r.w, (r.h - HEAD_H).max(0.0));
    let (cols, cw, ch) = geometry(vm.view, r.w);
    let old = p.clip(body);
    for (k, a) in vm.apps.iter().enumerate() {
        let (col, row) = (k % cols, k / cols);
        let c = Rect::new(
            r.x + MARGIN + col as f32 * (cw + GAP),
            body.y + row_bounds(row, ch).0 - scroll,
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
            View::Small => small(p, pal, c, vm, k, a, anim),
            _ => large(p, pal, c, vm, k, a, anim),
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
    anim: &mut Animator,
    framed: bool,
) -> Rgb {
    let sel = vm.sel == i;
    let failed = matches!(vm.apps[i].row, Row::Failed(_));
    let base = if sel {
        pal.on
    } else if vm.hover == Some(i) {
        pal.hover
    } else if framed {
        mix(pal.page, pal.text, 0.025)
    } else {
        pal.page
    };
    let target = if failed {
        mix(base, red(pal), if pal.dark { 0.12 } else { 0.08 })
    } else {
        base
    };
    let bg = motion::row_ground(anim, vm.view, motion::app_key(vm, i), target);
    if sel {
        p.frame(c, 8.0, bg, pal.accent);
    } else if framed {
        p.frame(c, 8.0, bg, mix(pal.line, pal.page, 0.3));
    } else if bg != pal.page {
        p.rbox(c, 8.0, bg);
    }
    bg
}

const VERSION_SIZE: f32 = 12.0;
const ARROW: &str = " \u{2192} ";

/// Fit the versions before centring them. A short version gives its spare width to the other.
fn version_line(
    p: &mut Painter,
    line: Rect,
    face: Face,
    first: &str,
    second: Option<&str>,
) -> (f32, String, Option<String>) {
    let fit = |p: &mut Painter, face, text: &str, width| {
        let s = p.elide(face, VERSION_SIZE, text, width);
        // Shared elide can return an ellipsis even when there is no space for it.
        if p.measure(face, VERSION_SIZE, &s) <= width {
            s
        } else {
            String::new()
        }
    };
    if let Some(second) = second {
        let arrow = p.measure(Face::Ui, VERSION_SIZE, ARROW);
        if line.w > arrow {
            let available = line.w - arrow;
            let aw = p.measure(face, VERSION_SIZE, first);
            let bw = p.measure(Face::Semibold, VERSION_SIZE, second);
            let a_max = (available - bw.min(available / 2.0)).min(aw);
            let a = fit(p, face, first, a_max);
            let aw = p.measure(face, VERSION_SIZE, &a);
            let b = fit(p, Face::Semibold, second, available - aw);
            if !a.is_empty() && !b.is_empty() {
                let w = aw + arrow + p.measure(Face::Semibold, VERSION_SIZE, &b);
                return (line.cx() - w / 2.0, a, Some(b));
            }
        }
    }
    let a = fit(p, face, first, line.w);
    (line.cx() - p.measure(face, VERSION_SIZE, &a) / 2.0, a, None)
}

fn large(
    p: &mut Painter,
    pal: &Palette,
    c: Rect,
    vm: &Vm,
    i: usize,
    a: &AppVm,
    anim: &mut Animator,
) -> Vec<(Rect, Hit)> {
    let mut hits = Vec::new();
    let key = motion::app_key(vm, i);
    let bg = ground(p, pal, c, vm, i, anim, true);
    let opacity = motion::status(anim, key, &a.row);
    let amount = motion::progress(anim, key, &a.row);
    let not_in = matches!(a.row, Row::NotInstalled);
    let cx = c.cx();
    // The icon.
    let t = Rect::new(cx - 28.0, c.y + 16.0, 56.0, 56.0);
    ui::app_tile(p, t, a.id, a.glyph, a.tile, not_in);
    badges(p, pal, t, a, bg, opacity);
    let menu = Rect::new(c.right() - 30.0, c.y + 4.0, 26.0, 26.0);
    ui::icon_fill(
        p,
        pal,
        menu,
        bg,
        motion::button(anim, (vm.view, key, "menu"), vm.pointer, Hit::Menu(i)),
    );
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
        Row::Downloading(..) => {
            ui::progress(
                p,
                pal,
                Rect::new(c.x + 22.0, vy - 3.0, bar_w, 6.0),
                amount,
                pal.accent,
                bg,
            );
        }
        Row::Verifying => {
            ui::progress(
                p,
                pal,
                Rect::new(c.x + 22.0, vy - 3.0, bar_w, 6.0),
                amount,
                mix(pal.accent, pal.page, 0.45),
                bg,
            );
        }
        row => {
            let latest = a.latest.clone().unwrap_or_else(|| "\u{2014}".into());
            let (first, second) = match (&a.installed, newer(row)) {
                // A full parsed version may be up to 62 characters.
                (Some(v), true) => (ui::cap(v, 64), Some(ui::cap(&latest, 64))),
                (Some(v), false) => (ui::cap(v, 64), None),
                (None, _) if a.size.is_empty() => (ui::cap(&latest, 64), None),
                (None, _) => (format!("{} \u{b7} {}", ui::cap(&latest, 64), a.size), None),
            };
            let face = if a.installed.is_some() {
                Face::Mono
            } else {
                Face::Ui
            };
            let (mut x, first, second) = version_line(p, line(vy), face, &first, second.as_deref());
            let old = p.clip(c);
            let max = f32::INFINITY; // already fitted; keep the measured advances
            x += p.text(face, VERSION_SIZE, x, vy, &first, pal.muted, max);
            if let Some(b) = second {
                x += p.text(Face::Ui, VERSION_SIZE, x, vy, ARROW, pal.faint, max);
                p.text(Face::Semibold, VERSION_SIZE, x, vy, &b, blue(pal), max);
            }
            p.restore(old);
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
        ui::dot(p, x0, sy, mix(bg, d, opacity));
    }
    p.text(
        Face::Ui,
        12.0,
        x0 + lead,
        sy,
        &text,
        mix(bg, col, opacity),
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
            motion::button(
                anim,
                (vm.view, key, b.act),
                vm.pointer,
                Hit::Action(i, b.act),
            ),
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
    anim: &mut Animator,
) -> Vec<(Rect, Hit)> {
    let key = motion::app_key(vm, i);
    let bg = ground(p, pal, c, vm, i, anim, false);
    let opacity = motion::status(anim, key, &a.row);
    let dim = matches!(a.row, Row::NotInstalled);
    let cx = c.cx();
    let t = Rect::new(cx - 22.0, c.y + 12.0, 44.0, 44.0);
    ui::app_tile(p, t, a.id, a.glyph, a.tile, dim);
    badges(p, pal, t, a, bg, opacity);
    let menu = Rect::new(c.x + 4.0, c.y + 4.0, SMALL_MENU, SMALL_MENU);
    ui::icon_fill(
        p,
        pal,
        menu,
        bg,
        motion::button(anim, (vm.view, key, "menu"), vm.pointer, Hit::Menu(i)),
    );
    p.icon(glyph::MORE, 15.0, menu.cx(), menu.cy(), pal.muted);
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
        Row::Downloading(..) => ui::progress(
            p,
            pal,
            Rect::new(c.x + 18.0, c.y + 84.0, (c.w - 36.0).max(0.0), 5.0),
            motion::progress(anim, key, &a.row),
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
            p.text_center(
                face,
                11.5,
                line(c.y + 87.0),
                &ui::cap(&wd, 40),
                mix(bg, col, opacity),
            );
        }
    }
    vec![(menu, Hit::Menu(i))]
}

#[cfg(test)]
mod tests {
    use super::*;
    use mdshared_text::bitmap::Bitmap;
    use mdshared_ui::gfx::Gfx;
    use mdshared_ui::theme::{DARK, LIGHT};

    #[test]
    fn the_small_tile_has_an_independent_overflow_hit_away_from_the_badge() {
        let mut bmp = Bitmap::new(520, 360);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        let vm = crate::demo::build("all", View::Small).unwrap();
        for w in [520.0, 488.0] {
            let mut lay = Layout::default();
            draw(
                &mut p,
                &DARK,
                Rect::new(0.0, 0.0, w, 360.0),
                &vm,
                0.0,
                &mut lay,
                &mut crate::motion::still(),
            );
            for i in [0, 2] {
                // Not installed and update badge.
                let rect = |hit| lay.hits.iter().find(|(_, h)| *h == hit).unwrap().0;
                let tile = rect(Hit::Row(i));
                let menu = rect(Hit::Menu(i));
                assert_eq!(menu, Rect::new(tile.x + 4.0, tile.y + 4.0, 28.0, 28.0));
                assert_eq!(lay.at(menu.cx(), menu.cy()), Hit::Menu(i));
                assert_eq!(lay.at(tile.cx() + 20.0, tile.y + 14.0), Hit::Row(i));
            }
        }
    }

    #[test]
    fn not_installed_is_muted_and_badge_glyphs_contrast_in_both_themes() {
        let luminance = |color: Rgb| {
            let linear = |c: u8| {
                let c = f64::from(c) / 255.0;
                if c <= 0.04045 {
                    c / 12.92
                } else {
                    ((c + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * linear(color[0]) + 0.7152 * linear(color[1]) + 0.0722 * linear(color[2])
        };
        let vm = crate::demo::build("first-run", View::Small).unwrap();
        for pal in [LIGHT, DARK] {
            assert_eq!(word(&pal, &vm.apps[0]), ("Not installed".into(), pal.muted));
            let ink = badge_ink(&pal);
            assert_eq!(ink, if pal.dark { pal.page } else { [255; 3] });
            for fill in [red(&pal), green(&pal)] {
                let (a, b) = (luminance(ink), luminance(fill));
                assert!((a.max(b) + 0.05) / (a.min(b) + 0.05) >= 4.5);
            }
        }
    }

    #[test]
    fn two_long_versions_are_elided_then_centered_inside_a_minimum_width_tile() {
        let mut bmp = Bitmap::new(520, 360);
        let mut gfx = Gfx::new();
        let (_, cw, _) = geometry(View::Large, 520.0);
        let line = Rect::new(26.0, 126.0, cw - 20.0, 20.0);
        let vm = crate::demo::build("long-versions", View::Large).unwrap();
        let a = vm.apps[0].installed.as_deref().unwrap();
        let b = vm.apps[0].latest.as_deref().unwrap();
        for scale in [1.0, 1.25, 2.0] {
            let mut p = Painter::new(&mut bmp, &mut gfx, scale);
            let (x, shown_a, shown_b) = version_line(&mut p, line, Face::Mono, a, Some(b));
            let shown_b = shown_b.unwrap();
            assert!(shown_a.ends_with('…') && shown_b.ends_with('…'));
            let w = p.measure(Face::Mono, VERSION_SIZE, &shown_a)
                + p.measure(Face::Ui, VERSION_SIZE, ARROW)
                + p.measure(Face::Semibold, VERSION_SIZE, &shown_b);
            assert!(x >= line.x && x + w <= line.right() + 0.001);
            assert!((x + w / 2.0 - line.cx()).abs() < 0.001);
            let (_, short, long) = version_line(&mut p, line, Face::Mono, "1.0.0", Some(b));
            assert_eq!(short, "1.0.0");
            assert!(long.unwrap().len() >= shown_b.len(), "use the spare width");
        }
    }
}
