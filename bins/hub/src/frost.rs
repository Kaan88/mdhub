//! The "Window backdrop" setting in Hub's window: which pixels are chrome. When the chrome is
//! frosted and what the compositor was asked for is `mdshared_ui::frost::Frost`, which mdedit and
//! mdview use too; the drawing is `mdshared_ui::frost` (the chrome is drawn as always, marked,
//! and its flat colour taken out again when the frame goes to the window).
//!
//! The chrome is what `view::draw_full` lists in `Layout::chrome`: the title row, the command bar
//! and the status bar (D-032). The list, the pane under it, the offer bar and the release notes
//! page stay opaque; the row menu and the settings card are drawn over the chrome with opaque
//! fills and stay opaque by that.
pub use mdshared_ui::frost::Frost;

use mdshared_app::backdrop::Backdrop;
use mdshared_text::bitmap::Bitmap;
use mdshared_ui::frost::{Key, key_colour, key_out, mark, veil};
use mdshared_ui::gfx::{Painter, Rect};
use mdshared_ui::theme::Palette;

/// A device-px rectangle: x0, y0, x1, y1.
pub type Area = (usize, usize, usize, usize);

/// While the chrome is frosted (`key` is `Some`), marks the chrome of a frame just drawn
/// (`chrome` from its `Layout`) so the present can key it out, and returns where it lies in
/// device px. Without a key the frame is left exactly as drawn and nothing is returned. Call it
/// after `view::draw_full` and before the row menu or the settings card is drawn over it.
pub fn mark_chrome(key: Option<&Key>, p: &mut Painter, chrome: &[Rect]) -> Vec<Area> {
    if key.is_none() {
        return Vec::new();
    }
    chrome
        .iter()
        .map(|r| {
            let d = p.dev_rect(*r);
            mark(p.bmp, d);
            let u = |v: i32| v.max(0) as usize;
            (u(d.0), u(d.1), u(d.2), u(d.3))
        })
        .collect()
}

/// The key a frosted chrome of `backdrop` is keyed out with in this palette; `None` for `Off`.
pub fn key_for(backdrop: Backdrop, pal: &Palette) -> Option<Key> {
    (backdrop != Backdrop::Off).then(|| Key::new(key_colour(pal), veil(backdrop)))
}

/// Takes the key colour out of the marked pixels of `areas` in a frame of `stride` pixels a row
/// (`0xAARRGGBB`): the window's copy of the frame, never the bitmap the frame was drawn into.
pub fn key_out_areas(key: &Key, pixels: &mut [u32], stride: usize, areas: &[Area]) {
    for a in areas {
        key_out(key, pixels, stride, *a);
    }
}

/// The colour behind the window in a picture of the backdrop (`--demo backdrop-...`): a made-up
/// diagonal of two colours, so what lets it through is plain to see. Not what Windows draws.
fn stand_in(dark: bool, x: usize, y: usize, w: usize, h: usize) -> [u32; 3] {
    let (a, b): ([u32; 3], [u32; 3]) = if dark {
        ([0x1c, 0x3c, 0x64], [0x5a, 0x24, 0x52])
    } else {
        ([0xb4, 0xd2, 0xf0], [0xf0, 0xc4, 0xd8])
    };
    let t = ((x * 255 / w.max(1)) + (y * 255 / h.max(1))) as u32 / 2;
    [0, 1, 2].map(|i| (a[i] * (255 - t) + b[i] * t) / 255)
}

/// What a window with `backdrop` looks like over the stand-in colours: the frame in `bmp` (its
/// chrome marked at `areas`) keyed out as the present does and laid over [`stand_in`]. For the
/// pictures only; `Off` leaves the frame as it is.
pub fn preview(bmp: &mut Bitmap, areas: &[Area], backdrop: Backdrop, pal: &Palette) {
    let Some(key) = key_for(backdrop, pal) else {
        return;
    };
    let (w, h) = (bmp.width as usize, bmp.height as usize);
    let mut px: Vec<u32> = bmp
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|c| u32::from_le_bytes(*c))
        .collect();
    key_out_areas(&key, &mut px, w, areas);
    for (i, (out, p)) in bmp
        .pixels
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(&px)
        .enumerate()
    {
        // Premultiplied over the stand-in: an opaque pixel (alpha 255) is the frame itself.
        let a = p >> 24;
        let d = stand_in(pal.dark, i % w.max(1), i / w.max(1), w, h);
        let c = [(p >> 16) & 0xff, (p >> 8) & 0xff, p & 0xff];
        let over = |k: usize| (c[k] + ((255 - a) * d[k] + 127) / 255).min(255) as u8;
        *out = [over(2), over(1), over(0), 255];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo;
    use crate::shot::{render, render_with};
    use crate::view::{self, STATUS_H};
    use crate::vm::View;
    use mdshared_ui::chrome::{BAR_H, TITLE_H};
    use mdshared_ui::frost::MARK;
    use mdshared_ui::gfx::Gfx;
    use mdshared_ui::theme::{DARK, LIGHT};

    const W: f32 = 1280.0;
    const H: f32 = 800.0;

    fn inside(areas: &[Area], x: usize, y: usize) -> bool {
        areas
            .iter()
            .any(|a| (a.0..a.2).contains(&x) && (a.1..a.3).contains(&y))
    }

    fn opaque(bmp: &Bitmap) -> bool {
        bmp.pixels.as_chunks::<4>().0.iter().all(|c| c[3] == 255)
    }

    /// Row `y` of a bitmap, its bytes.
    fn row(bmp: &Bitmap, y: usize) -> &[u8] {
        let w = bmp.width as usize * 4;
        &bmp.pixels[y * w..(y + 1) * w]
    }

    #[test]
    fn the_chrome_is_the_title_row_the_command_bar_and_the_status_bar() {
        let vm = demo::build("main", View::List).unwrap();
        let mut bmp = Bitmap::new(W as u32, H as u32);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        let lay = view::draw(&mut p, &LIGHT, W, H, &vm);
        assert_eq!(
            lay.chrome,
            [
                Rect::new(0.0, 0.0, W, TITLE_H),
                Rect::new(0.0, TITLE_H, W, BAR_H),
                Rect::new(0.0, H - STATUS_H, W, STATUS_H),
            ]
        );
        // The list and the pane under it are not chrome: they stay opaque.
        let geo = view::geometry(W, H, &vm);
        assert!(geo.pane.h > 0.0, "the picture has a pane");
        for r in &lay.chrome {
            for part in [geo.list, geo.pane] {
                assert!(
                    r.bottom() <= part.y || r.y >= part.bottom(),
                    "{r:?} overlaps {part:?}"
                );
            }
        }
        // The release notes page has no command bar: its place is the page's, opaque.
        let vm = demo::build("notes", View::List).unwrap();
        let lay = view::draw(&mut p, &LIGHT, W, H, &vm);
        assert_eq!(
            lay.chrome,
            [
                Rect::new(0.0, 0.0, W, TITLE_H),
                Rect::new(0.0, H - STATUS_H, W, STATUS_H),
            ]
        );
    }

    #[test]
    fn with_a_key_exactly_the_chrome_is_marked_at_any_scale() {
        let vm = demo::build("main", View::List).unwrap();
        for scale in [1.0f32, 1.25, 2.0] {
            let (dw, dh) = ((W * scale) as u32, (H * scale) as u32);
            let mut bmp = Bitmap::new(dw, dh);
            let mut gfx = Gfx::new();
            let mut p = Painter::new(&mut bmp, &mut gfx, scale);
            let lay = view::draw(&mut p, &DARK, W, H, &vm);
            let key = key_for(Backdrop::Mica, &DARK);
            let areas = mark_chrome(key.as_ref(), &mut p, &lay.chrome);
            assert_eq!(areas.len(), 3);
            // The title row starts at the top, the status bar ends at the bottom, edge to edge.
            assert_eq!((areas[0].0, areas[0].1), (0, 0));
            assert_eq!((areas[2].2, areas[2].3), (dw as usize, dh as usize));
            for y in 0..dh as usize {
                for x in (0..dw as usize).step_by(7) {
                    let a = bmp.pixels[(y * dw as usize + x) * 4 + 3];
                    assert_eq!(a == MARK, inside(&areas, x, y), "({x},{y}) at {scale}");
                }
            }
        }
    }

    #[test]
    fn off_draws_exactly_as_without_the_setting() {
        let vm = demo::build("main", View::List).unwrap();
        let mut bmp = Bitmap::new(W as u32, H as u32);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        let lay = view::draw(&mut p, &LIGHT, W, H, &vm);
        let before = p.bmp.pixels.clone();
        // No key (the setting off, no focus, no backdrop on this system): nothing is marked.
        assert_eq!(key_for(Backdrop::Off, &LIGHT), None);
        assert!(mark_chrome(None, &mut p, &lay.chrome).is_empty());
        assert!(bmp.pixels == before);
        assert!(opaque(&bmp));
        // And the pictures with it off are the pictures as they were, for every state.
        for name in demo::NAMES {
            if demo::backdrop(name) != Backdrop::Off {
                continue;
            }
            for pal in [LIGHT, DARK] {
                let vm = demo::build(name, View::List).unwrap();
                let a = render(&vm, &pal, 860, 600, 1.0);
                let b = render_with(&vm, &pal, 860, 600, 1.0, Backdrop::Off);
                assert!(a.pixels == b.pixels, "{name}");
                assert!(opaque(&b), "{name}: every pixel opaque");
            }
        }
    }

    #[test]
    fn a_backdrop_shows_through_the_chrome_only() {
        for (name, b) in [
            ("backdrop-mica", Backdrop::Mica),
            ("backdrop-acrylic", Backdrop::Acrylic),
        ] {
            assert_eq!(demo::backdrop(name), b);
            for pal in [LIGHT, DARK] {
                let vm = demo::build(name, View::List).unwrap();
                let flat = render(&vm, &pal, W as u32, H as u32, 1.0);
                let shot = render_with(&vm, &pal, W as u32, H as u32, 1.0, b);
                let geo = view::geometry(W, H, &vm);
                // The list and the pane: every pixel as drawn.
                for y in geo.list.y as usize..geo.pane.bottom() as usize {
                    assert!(row(&flat, y) == row(&shot, y), "{name} row {y}");
                }
                // The chrome: its flat colour is gone, the stand-in shows through.
                for y in [4.0, TITLE_H + 4.0, H - 4.0] {
                    let y = y as usize;
                    assert!(row(&flat, y) != row(&shot, y), "{name} chrome row {y}");
                    // Across the row the picture is much nearer the stand-in than the flat
                    // chrome is (Mica lets it through; Acrylic's veil keeps some of the chrome).
                    let off = |bmp: &Bitmap| -> u64 {
                        row(bmp, y)
                            .as_chunks::<4>()
                            .0
                            .iter()
                            .enumerate()
                            .map(|(x, c)| {
                                let d = stand_in(pal.dark, x, y, W as usize, H as usize);
                                (0..3)
                                    .map(|k| (c[2 - k] as u32).abs_diff(d[k]) as u64)
                                    .sum::<u64>()
                            })
                            .sum()
                    };
                    let (s, f) = (off(&shot), off(&flat));
                    let bound = if b == Backdrop::Mica { f / 2 } else { f };
                    assert!(s < bound, "{name} row {y}: {s} vs flat {f}");
                }
                assert!(opaque(&shot));
            }
        }
        // Acrylic keeps a veil of the chrome's own colour: nearer the flat look than Mica.
        let vm = demo::build("main", View::List).unwrap();
        let px = |b| {
            let bmp = render_with(&vm, &DARK, W as u32, H as u32, 1.0, b);
            bmp.pixels[16..19].to_vec()
        };
        let (flat, mica, acrylic) = (px(Backdrop::Off), px(Backdrop::Mica), px(Backdrop::Acrylic));
        let dist = |a: &[u8], b: &[u8]| -> u32 {
            a.iter().zip(b).map(|(x, y)| x.abs_diff(*y) as u32).sum()
        };
        assert!(
            dist(&acrylic, &flat) < dist(&mica, &flat),
            "{flat:?} {mica:?} {acrylic:?}"
        );
    }
}
