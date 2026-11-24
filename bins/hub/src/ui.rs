//! Small drawing helpers the window shares: wrapped text, buttons, tiles, progress bars, dots,
//! the status colours, and rectangle clipping for the hit list.
use crate::vm::Look;
use mdshared_ui::chrome::button_fill;
use mdshared_ui::gfx::{Face, Painter, Rect, Rgb, mix};
use mdshared_ui::glyph;
use mdshared_ui::theme::Palette;

pub const LIST_ICON: &str = glyph::OUTLINE;
pub const LARGE_ICON: &str =
    "M2.5 2.5h4.5v4.5h-4.5zM9 2.5h4.5v4.5H9zM2.5 9h4.5v4.5h-4.5zM9 9h4.5v4.5H9z";
pub const SMALL_ICON: &str = "M2 2h3v3H2zM6.5 2h3v3h-3zM11 2h3v3h-3zM2 6.5h3v3H2zM6.5 6.5h3v3h-3zM11 6.5h3v3h-3zM2 11h3v3H2zM6.5 11h3v3h-3zM11 11h3v3h-3z";

pub fn green(pal: &Palette) -> Rgb {
    if pal.dark {
        [0x6c, 0xcb, 0x5f]
    } else {
        [0x0f, 0x7b, 0x0f]
    }
}

pub fn red(pal: &Palette) -> Rgb {
    if pal.dark {
        [0xff, 0x6e, 0x6e]
    } else {
        [0xc4, 0x2b, 0x1c]
    }
}

pub fn amber(pal: &Palette) -> Rgb {
    if pal.dark {
        [0xf0, 0xb4, 0x4c]
    } else {
        [0x9a, 0x5b, 0x00]
    }
}

pub fn blue(pal: &Palette) -> Rgb {
    if pal.dark {
        pal.accent
    } else {
        [0x00, 0x5f, 0xb8]
    }
}

/// The icon path for a catalog glyph name ("edit", "folder", ...); a document for an unknown one.
pub fn glyph_path(name: &str) -> &'static str {
    glyph::ALL
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, p)| *p)
        .unwrap_or(glyph::DOC)
}

/// The part of `r` inside `clip`, or None when nothing is left.
pub fn intersect(r: Rect, clip: Rect) -> Option<Rect> {
    let x0 = r.x.max(clip.x);
    let y0 = r.y.max(clip.y);
    let x1 = r.right().min(clip.right());
    let y1 = r.bottom().min(clip.bottom());
    (x1 > x0 && y1 > y0).then(|| Rect::new(x0, y0, x1 - x0, y1 - y0))
}

/// At most `n` characters of `s` (a bound on what a stray huge string can cost to lay out).
pub fn cap(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

pub fn wrap(p: &mut Painter, face: Face, size: f32, text: &str, w: f32) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for word in text.split_whitespace() {
        let t = if cur.is_empty() {
            word.to_string()
        } else {
            format!("{cur} {word}")
        };
        if !cur.is_empty() && p.measure(face, size, &t) > w {
            lines.push(std::mem::take(&mut cur));
            cur = word.to_string();
        } else {
            cur = t;
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

/// A paragraph; stops below `limit_y`; returns the y below it.
#[allow(clippy::too_many_arguments)]
pub fn para(
    p: &mut Painter,
    face: Face,
    size: f32,
    x: f32,
    y: f32,
    w: f32,
    limit_y: f32,
    text: &str,
    color: Rgb,
) -> f32 {
    let lh = (size * 1.45).round();
    let mut y = y;
    for l in wrap(p, face, size, &cap(text, 2000), w) {
        if y > limit_y {
            break;
        }
        p.text(face, size, x, y + lh / 2.0, &l, color, w + 4.0);
        y += lh;
    }
    y
}

pub fn tile(p: &mut Painter, r: Rect, glyph_name: &str, color: Rgb, dim: bool) {
    let c = if dim {
        mix([0x80, 0x80, 0x80], color, 0.35)
    } else {
        color
    };
    p.rbox(r, r.w * 0.22, c);
    p.rbox_a(
        Rect::new(r.x, r.y, r.w, r.h / 2.0),
        r.w * 0.22,
        [255, 255, 255],
        0.08,
    );
    p.icon(
        glyph_path(glyph_name),
        r.w * 0.56,
        r.cx(),
        r.cy(),
        [255, 255, 255],
    );
}

/// An app's tile: its own icon, or the catalog's coloured glyph tile when none decodes.
pub fn app_tile(
    p: &mut Painter,
    r: Rect,
    id: &'static str,
    glyph_name: &str,
    color: Rgb,
    dim: bool,
) {
    if !crate::appicon::draw(p, r, id, dim) {
        tile(p, r, glyph_name, color, dim);
    }
}

pub fn btn_w(p: &mut Painter, label: &str) -> f32 {
    (p.measure(Face::Semibold, 12.5, label) + 26.0).round()
}

/// The fill of an accent button: lighter (dark theme: darker) while hovered, more while pressed,
/// as the shared offer bar's buttons do.
pub fn accent_fill(pal: &Palette, look: Look) -> Rgb {
    match (look.hovered, look.pressed) {
        (true, true) => mix(pal.accent, pal.page, 0.2),
        (true, false) => mix(pal.accent, pal.page, 0.1),
        _ => pal.accent,
    }
}

/// A 28 px button at `x`, centred on `cy`, drawn as `look` says; returns its rectangle.
pub fn sbtn(
    p: &mut Painter,
    pal: &Palette,
    x: f32,
    cy: f32,
    label: &str,
    primary: bool,
    look: Look,
) -> Rect {
    let w = btn_w(p, label);
    let r = Rect::new(x, cy - 14.0, w, 28.0);
    if primary {
        p.rbox(r, 5.0, accent_fill(pal, look));
        p.text_center(Face::Semibold, 12.5, r, label, pal.on_accent);
    } else {
        // The row under a hovered button is lit too (`pal.hover`): the button takes a step past
        // that colour, and a stronger frame, so it still stands apart from the row.
        let (fill, line) = match (look.hovered, look.pressed) {
            (true, true) => (
                mix(pal.press, pal.text, 0.12),
                mix(pal.line, pal.text, 0.35),
            ),
            (true, false) => (
                mix(pal.hover, pal.text, 0.09),
                mix(pal.line, pal.text, 0.25),
            ),
            _ => (mix(pal.page, pal.text, 0.04), pal.line),
        };
        p.frame(r, 5.0, fill, line);
        p.text_center(Face::Ui, 12.5, r, label, pal.text);
    }
    r
}

/// Same, ending at `right`.
pub fn sbtn_r(
    p: &mut Painter,
    pal: &Palette,
    right: f32,
    cy: f32,
    label: &str,
    primary: bool,
    look: Look,
) -> Rect {
    let w = btn_w(p, label);
    sbtn(p, pal, right - w, cy, label, primary, look)
}

/// The fill behind an icon button that sits on `ground` (a row or a tile, itself lit while the
/// pointer is over it): a step darker than that ground while hovered, two while pressed.
pub fn icon_fill(p: &mut Painter, pal: &Palette, r: Rect, ground: Rgb, look: Look) {
    let lift = match (look.hovered, look.pressed) {
        (true, true) => 0.16,
        (true, false) => 0.09,
        _ => return,
    };
    p.rbox(r, 5.0, mix(ground, pal.text, lift));
}

/// The fill behind a quiet icon or bar button (the shared rule: none at rest, hover, press).
pub fn quiet_fill(p: &mut Painter, pal: &Palette, r: Rect, radius: f32, look: Look) {
    if let Some(fill) = button_fill(pal, look.hovered, look.pressed, false, false) {
        p.rbox(r, radius, fill);
    }
}

pub fn progress(p: &mut Painter, pal: &Palette, r: Rect, f: f32, c: Rgb, bg: Rgb) {
    if r.w <= 0.0 || r.h <= 0.0 {
        return;
    }
    let f = if f.is_finite() {
        f.clamp(0.0, 1.0)
    } else {
        0.0
    };
    p.rbox(r, r.h / 2.0, mix(bg, pal.text, 0.14));
    if f > 0.0 {
        p.rbox(Rect::new(r.x, r.y, (r.w * f).max(r.h), r.h), r.h / 2.0, c);
    }
}

pub fn dot(p: &mut Painter, x: f32, cy: f32, c: Rgb) {
    p.rbox(Rect::new(x, cy - 3.5, 7.0, 7.0), 3.5, c);
}

/// `label` muted at `x`, `value` at `x + lw`; stops below `limit_y`; returns the y below.
#[allow(clippy::too_many_arguments)]
pub fn kv(
    p: &mut Painter,
    pal: &Palette,
    x: f32,
    y: f32,
    lw: f32,
    w: f32,
    limit_y: f32,
    label: &str,
    value: &str,
) -> f32 {
    p.text(Face::Ui, 12.0, x, y + 9.0, label, pal.muted, lw);
    let lines = wrap(p, Face::Ui, 12.5, &cap(value, 400), (w - lw).max(1.0));
    let mut yy = y;
    for l in &lines {
        if yy > limit_y {
            break;
        }
        p.text(Face::Ui, 12.5, x + lw, yy + 9.0, l, pal.text, w - lw + 4.0);
        yy += 18.0;
    }
    yy.max(y + 18.0) + 4.0
}
