//! The apps' own icons: one 128 px PNG each in `assets/apps/`, compiled in. A PNG is decoded the
//! first time a draw needs it (never at start) and scaled once per size to a cache keyed by the
//! app and the side in device pixels, so a redraw only blends. An app whose bytes do not decode
//! gets `false` from [`draw`] and the caller draws the catalog's coloured tile instead.
use mdshared_text::bitmap::decode_png_rgba;
use mdshared_ui::gfx::{Painter, Rect};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// The icon of each catalog app, by id.
const ICONS: [(&str, &[u8]); 7] = [
    ("mdedit", include_bytes!("../../../assets/apps/mdedit.png")),
    ("mdview", include_bytes!("../../../assets/apps/mdview.png")),
    ("sprawl", include_bytes!("../../../assets/apps/sprawl.png")),
    (
        "inbound",
        include_bytes!("../../../assets/apps/inbound.png"),
    ),
    ("purge", include_bytes!("../../../assets/apps/purge.png")),
    (
        "barbican",
        include_bytes!("../../../assets/apps/barbican.png"),
    ),
    (
        "claude-switch",
        include_bytes!("../../../assets/apps/claude-switch.png"),
    ),
];

/// The embedded PNG of app `id`.
pub fn bytes_of(id: &str) -> Option<&'static [u8]> {
    ICONS.iter().find(|(i, _)| *i == id).map(|(_, b)| *b)
}

/// A decoded icon: straight (not premultiplied) RGBA, top-down.
pub struct Src {
    pub rgba: Vec<u8>,
    pub w: u32,
    pub h: u32,
}

/// Decodes `bytes`; `None` for anything unreadable or whose pixel count does not fit its size.
pub fn decode(bytes: &[u8]) -> Option<Src> {
    let (rgba, w, h) = decode_png_rgba(bytes)?;
    if w == 0 || h == 0 || rgba.len() != w as usize * h as usize * 4 {
        return None;
    }
    Some(Src { rgba, w, h })
}

#[derive(Default)]
struct Cache {
    src: HashMap<&'static str, Option<Rc<Src>>>,
    scaled: HashMap<(&'static str, u32), Rc<Vec<u8>>>,
}

thread_local! {
    static CACHE: RefCell<Cache> = RefCell::new(Cache::default());
}

/// `src` as a `side` x `side` straight-alpha RGBA picture. Shrinking averages the source pixels
/// each one covers (weighted by alpha, so a transparent edge does not darken the rim); growing
/// interpolates between the four nearest.
pub fn scale(src: &Src, side: u32) -> Vec<u8> {
    let side = side.min(1024) as usize;
    let mut out = vec![0u8; side * side * 4];
    let (sw, sh) = (src.w as usize, src.h as usize);
    if side == 0 || sw == 0 || sh == 0 || src.rgba.len() < sw * sh * 4 {
        return out;
    }
    let at = |x: usize, y: usize| -> [f32; 4] {
        let o = (y.min(sh - 1) * sw + x.min(sw - 1)) * 4;
        let a = src.rgba[o + 3] as f32 / 255.0;
        [
            src.rgba[o] as f32 * a,
            src.rgba[o + 1] as f32 * a,
            src.rgba[o + 2] as f32 * a,
            a,
        ]
    };
    let (kx, ky) = (sw as f32 / side as f32, sh as f32 / side as f32);
    for dy in 0..side {
        for dx in 0..side {
            let mut acc = [0.0f32; 4];
            if kx >= 1.0 && ky >= 1.0 {
                let (x0, x1) = (dx as f32 * kx, (dx + 1) as f32 * kx);
                let (y0, y1) = (dy as f32 * ky, (dy + 1) as f32 * ky);
                let mut total = 0.0;
                for sy in y0 as usize..(y1.ceil() as usize).min(sh) {
                    let wy = (y1.min(sy as f32 + 1.0) - y0.max(sy as f32)).max(0.0);
                    for sx in x0 as usize..(x1.ceil() as usize).min(sw) {
                        let wx = (x1.min(sx as f32 + 1.0) - x0.max(sx as f32)).max(0.0);
                        let c = at(sx, sy);
                        for k in 0..4 {
                            acc[k] += c[k] * wx * wy;
                        }
                        total += wx * wy;
                    }
                }
                if total > 0.0 {
                    for v in &mut acc {
                        *v /= total;
                    }
                }
            } else {
                let fx = ((dx as f32 + 0.5) * kx - 0.5).max(0.0);
                let fy = ((dy as f32 + 0.5) * ky - 0.5).max(0.0);
                let (x0, y0) = (fx as usize, fy as usize);
                let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
                for (sx, sy, w) in [
                    (x0, y0, (1.0 - tx) * (1.0 - ty)),
                    (x0 + 1, y0, tx * (1.0 - ty)),
                    (x0, y0 + 1, (1.0 - tx) * ty),
                    (x0 + 1, y0 + 1, tx * ty),
                ] {
                    let c = at(sx, sy);
                    for k in 0..4 {
                        acc[k] += c[k] * w;
                    }
                }
            }
            let o = (dy * side + dx) * 4;
            let a = acc[3];
            if a > 0.0 {
                for k in 0..3 {
                    out[o + k] = (acc[k] / a).round().clamp(0.0, 255.0) as u8;
                }
                out[o + 3] = (a * 255.0).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    out
}

/// Draws app `id`'s own icon into `r`; `dim` fades it (an app that is not installed). `false`
/// when the app has no icon or its bytes do not decode: the caller draws the tile instead.
pub fn draw(p: &mut Painter, r: Rect, id: &'static str, dim: bool) -> bool {
    match bytes_of(id) {
        Some(b) => draw_bytes(p, r, id, b, dim),
        None => false,
    }
}

/// [`draw`] for `bytes` kept under `key`.
pub fn draw_bytes(p: &mut Painter, r: Rect, key: &'static str, bytes: &[u8], dim: bool) -> bool {
    let side = (r.w.min(r.h) * p.scale).round();
    if !side.is_finite() || side < 1.0 {
        return false;
    }
    let side = (side as u32).min(1024);
    let px = CACHE.with(|c| {
        let mut c = c.borrow_mut();
        let src = c
            .src
            .entry(key)
            .or_insert_with(|| decode(bytes).map(Rc::new))
            .clone()?;
        if c.scaled.len() > 64 {
            c.scaled.clear();
        }
        Some(
            c.scaled
                .entry((key, side))
                .or_insert_with(|| Rc::new(scale(&src, side)))
                .clone(),
        )
    });
    let Some(px) = px else { return false };
    let x0 = (r.cx() * p.scale - side as f32 / 2.0).round() as i32;
    let y0 = (r.cy() * p.scale - side as f32 / 2.0).round() as i32;
    let (fade, grey) = if dim { (0.55, 0.5) } else { (1.0, 0.0) };
    for (i, c) in px.as_chunks::<4>().0.iter().enumerate() {
        if c[3] == 0 {
            continue;
        }
        let rgb = [c[0], c[1], c[2]].map(|v| (v as f32 + (128.0 - v as f32) * grey) as u8);
        let (x, y) = (x0 + (i as u32 % side) as i32, y0 + (i as u32 / side) as i32);
        p.fill_dev(x, y, x + 1, y + 1, rgb, c[3] as f32 / 255.0 * fade);
    }
    true
}

/// Every icon's id, for the tests.
#[cfg(test)]
pub fn ids() -> impl Iterator<Item = &'static str> {
    ICONS.iter().map(|(i, _)| *i)
}
