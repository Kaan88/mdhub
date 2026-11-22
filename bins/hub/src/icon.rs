//! The icon, drawn at run time with the shared painter, and the one drawing of it there is:
//! `assets/hub.ico` is packed from this module's output by `tools/make-icon.ps1`. The mark is a
//! hub and its four apps: a white node in the middle of a dark rounded tile, joined by spokes to
//! four smaller nodes in the apps' colours (mdedit blue, mdview teal, Sprawl orange, Inbound
//! green). When updates wait, a red badge in the corner carries the count. One icon is built per
//! change of the count, never per frame, and the previous `HICON` is destroyed by its owner
//! (`tray.rs`) once the new one is in place.
//!
//! The mark is laid out in device pixels, not scaled from a grid: every node is a whole number of
//! pixels across and the right half mirrors the left exactly, so the 16-pixel tray icon has no
//! blurred or lopsided dots. The badge keeps its 16 x 16 grid.
//!
//! The painter writes opaque pixels and has no alpha channel, so the tile is rendered twice, once
//! over black and once over white, and the straight alpha of every pixel is solved from the two
//! (`alpha = 255 - (white - black)`): the rounded corners and the antialiased edges come out as a
//! real 32-bit icon with no halo. Digits are a 3 x 5 pixel font drawn with rectangles, so no font
//! is loaded for a tray that otherwise holds none.
//!
//! Everything but the calls at the bottom is pure and tested without a window.
use mdshared_text::bitmap::Bitmap;
use mdshared_ui::gfx::{Gfx, Painter, Rect, mix};

/// The tile, top and bottom of its gradient.
pub const TILE_TOP: [u8; 3] = [0x34, 0x3B, 0x5E];
pub const TILE_BOTTOM: [u8; 3] = [0x17, 0x1A, 0x2E];
/// The hub in the middle.
pub const HUB: [u8; 3] = [0xFF, 0xFF, 0xFF];
/// The four apps, top-left, top-right, bottom-left, bottom-right: mdedit, mdview, Sprawl, Inbound.
pub const APPS: [[u8; 3]; 4] = [
    [0x3D, 0x9B, 0xFF],
    [0x2F, 0xD6, 0xC3],
    [0xFF, 0x94, 0x2E],
    [0x6F, 0xD9, 0x6A],
];
/// The badge.
pub const RED: [u8; 3] = [0xE0, 0x33, 0x24];
const WHITE: [u8; 3] = [255, 255, 255];
/// The badge's grid: it is laid out in 16 x 16 units, scaled to the icon's side.
const GRID: f32 = 16.0;

/// The most a badge shows: a larger count is shown as this.
pub const BADGE_MAX: u32 = 99;

/// What the badge says for `count`: nothing for 0, the number up to `BADGE_MAX`, `99` beyond.
pub fn badge_text(count: u32) -> Option<String> {
    (count > 0).then(|| count.min(BADGE_MAX).to_string())
}

/// Rows of a digit, 3 bits wide (bit 2 is the left column).
fn glyph(c: char) -> [u8; 5] {
    match c {
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        '3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        '6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b010, 0b010, 0b010],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        _ => [0; 5],
    }
}

/// A rounded box shaded from `top` to `bottom`, one device row at a time. `p` is in device px.
fn shaded(p: &mut Painter, r: Rect, radius: f32, top: [u8; 3], bottom: [u8; 3]) {
    let rows = r.h.round().max(1.0) as i32;
    for i in 0..rows {
        let t = i as f32 / (rows - 1).max(1) as f32;
        let old = p.clip(Rect::new(r.x, r.y + i as f32, r.w, 1.0));
        p.rbox(r, radius, mix(top, bottom, t));
        p.restore(old);
    }
}

/// Where the mark's parts lie in an icon `side` device px across: whole pixels, mirrored about
/// the middle. Returns the rim's width, a satellite's offset from the edge and its diameter, and
/// the hub's diameter.
fn layout(side: i32) -> (i32, i32, i32, i32) {
    let u = side as f32 / GRID;
    let rim = ((side as f32 / 48.0).round() as i32).max(1);
    let sat = ((3.6 * u).round() as i32).max(3);
    let off = ((2.2 * u).round() as i32).max(rim + 1);
    // The hub is centred, so its width has the parity of the side.
    let mut hub = (5.0 * u).round() as i32;
    if (side - hub) % 2 != 0 {
        hub += 1;
    }
    (rim, off, sat, hub.max(2))
}

/// The tile and the mark on it. `p` is in device px, the icon `side` of them across. `nodes` is
/// how many of the four apps are drawn: the badge takes the place of the last one, so that no
/// sliver of it shows beside the badge.
fn paint_mark(p: &mut Painter, side: i32, nodes: usize) {
    let s = side as f32;
    let u = s / GRID;
    let (rim, off, sat, hub) = layout(side);
    let whole = Rect::new(0.0, 0.0, s, s);
    let radius = 3.6 * u;
    // A lighter rim, brightest at the top, keeps the outline on a dark taskbar.
    shaded(
        p,
        whole,
        radius,
        mix(TILE_TOP, WHITE, 0.30),
        mix(TILE_BOTTOM, WHITE, 0.14),
    );
    shaded(
        p,
        whole.inset(rim as f32),
        radius - rim as f32,
        TILE_TOP,
        TILE_BOTTOM,
    );
    let mid = s / 2.0;
    let near = off as f32 + sat as f32 / 2.0;
    let far = s - near;
    let centres = [(near, near), (far, near), (near, far), (far, far)];
    // The spokes, each in its app's colour, dimmed.
    let half = (0.5 * u).max(0.5) * std::f32::consts::FRAC_1_SQRT_2;
    for (&(x, y), colour) in centres.iter().zip(APPS).take(nodes) {
        // A spoke runs along one diagonal, so its width lies along the other.
        let (nx, ny) = (half * (y - mid).signum(), -half * (x - mid).signum());
        p.polygon(
            &[
                (mid + nx, mid + ny),
                (x + nx, y + ny),
                (x - nx, y - ny),
                (mid - nx, mid - ny),
            ],
            mix(colour, TILE_BOTTOM, 0.35),
        );
    }
    for (&(x, y), colour) in centres.iter().zip(APPS).take(nodes) {
        let d = sat as f32;
        shaded(
            p,
            Rect::new(x - d / 2.0, y - d / 2.0, d, d),
            d / 2.0,
            mix(colour, WHITE, 0.16),
            mix(colour, [0, 0, 0], 0.10),
        );
    }
    let d = hub as f32;
    let disc = Rect::new(mid - d / 2.0, mid - d / 2.0, d, d);
    // A dark ring parts the hub from the spokes once there are pixels for it.
    let gap = (0.45 * u).round();
    if gap >= 1.0 {
        p.rbox(
            disc.inset(-gap),
            d / 2.0 + gap,
            mix(TILE_TOP, TILE_BOTTOM, 0.5),
        );
    }
    shaded(p, disc, d / 2.0, HUB, mix(HUB, TILE_TOP, 0.14));
}

/// The badge over the bottom-right corner. `p` is scaled to the badge's 16 x 16 grid.
fn paint_badge(p: &mut Painter, count: u32) {
    let Some(text) = badge_text(count) else {
        return;
    };
    let n = text.chars().count() as f32;
    let (w, h) = (4.0 * n + 1.0, 7.0);
    let (x, y) = (GRID - w, GRID - h);
    // A dark margin first, to part the badge from the hub and the tile's rim.
    p.rbox(
        Rect::new(x - 1.0, y - 1.0, w + 1.0, h + 1.0),
        3.0,
        TILE_BOTTOM,
    );
    p.rbox(Rect::new(x, y, w, h), 2.0, RED);
    for (i, c) in text.chars().enumerate() {
        for (row, bits) in glyph(c).iter().enumerate() {
            for col in 0..3 {
                if bits & (0b100 >> col) != 0 {
                    p.fill(
                        Rect::new(
                            x + 1.0 + i as f32 * 4.0 + col as f32,
                            y + 1.0 + row as f32,
                            1.0,
                            1.0,
                        ),
                        WHITE,
                    );
                }
            }
        }
    }
}

fn paint(bmp: &mut Bitmap, gfx: &mut Gfx, count: u32) {
    let side = bmp.width;
    let nodes = if count == 0 { 4 } else { 3 };
    paint_mark(&mut Painter::new(bmp, gfx, 1.0), side as i32, nodes);
    paint_badge(&mut Painter::new(bmp, gfx, side as f32 / GRID), count);
}

/// The icon `side` pixels across (8 to 256) as straight-alpha BGRA, `side * side * 4` bytes, top
/// row first. `count` 0 is the plain tile.
pub fn render(side: u32, count: u32) -> Vec<u8> {
    let side = side.clamp(8, 256);
    let mut gfx = Gfx::new();
    let mut shot = |bg: u8| {
        let mut bmp = Bitmap::new(side, side);
        bmp.clear([bg; 3]);
        paint(&mut bmp, &mut gfx, count);
        bmp.pixels
    };
    let black = shot(0);
    let white = shot(255);
    let mut out = vec![0u8; black.len()];
    for ((o, b), w) in out
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(black.as_chunks::<4>().0)
        .zip(white.as_chunks::<4>().0)
    {
        // Where the painter left the background alone, white minus black is 255 in every channel.
        let diff: u32 = (0..3)
            .map(|c| (w[c] as u32).saturating_sub(b[c] as u32))
            .sum();
        let alpha = 255u32.saturating_sub(diff / 3);
        if alpha == 0 {
            continue;
        }
        for c in 0..3 {
            o[c] = ((b[c] as u32 * 255) / alpha).min(255) as u8;
        }
        o[3] = alpha as u8;
    }
    out
}

/// The side the notification area wants: `SM_CXSMICON`, 16 at 100 % and more on a scaled display.
#[cfg(windows)]
pub fn small_icon_side() -> u32 {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSMICON};
    // SAFETY: a plain query with no pointers.
    let n = unsafe { GetSystemMetrics(SM_CXSMICON) };
    if n <= 0 { 16 } else { (n as u32).clamp(16, 64) }
}

/// An `HICON` (as an `isize`, 0 for none) of the tile with `count` in its badge, at the system's
/// small icon size. The caller owns it: [`destroy`] it when it is replaced or the icon is gone.
#[cfg(windows)]
pub fn hicon(count: u32) -> isize {
    hicon_sized(small_icon_side(), count)
}

/// Like [`hicon`] at `side` pixels (8 to 256): the notification balloon takes a large one, so that
/// Windows does not stretch the 16-pixel tray icon into its toast and blur it.
#[cfg(windows)]
pub fn hicon_sized(side: u32, count: u32) -> isize {
    use windows_sys::Win32::Graphics::Gdi::{CreateBitmap, DeleteObject};
    use windows_sys::Win32::UI::WindowsAndMessaging::{CreateIconIndirect, ICONINFO};
    let side = side.clamp(8, 256);
    let bgra = render(side, count);
    let mask = vec![0u8; (side as usize).div_ceil(16) * 2 * side as usize];
    // SAFETY: both buffers are at least as large as the bitmaps they describe (32 bits a pixel,
    // and a 1-bit mask padded to 16-bit rows); GDI copies them. The bitmaps are freed below once
    // the icon has its own copies.
    unsafe {
        let color = CreateBitmap(side as i32, side as i32, 1, 32, bgra.as_ptr().cast());
        let hmask = CreateBitmap(side as i32, side as i32, 1, 1, mask.as_ptr().cast());
        let mut h = 0isize;
        if !color.is_null() && !hmask.is_null() {
            let info = ICONINFO {
                fIcon: 1,
                xHotspot: 0,
                yHotspot: 0,
                hbmMask: hmask,
                hbmColor: color,
            };
            h = CreateIconIndirect(&info) as isize;
        }
        if !color.is_null() {
            DeleteObject(color.cast());
        }
        if !hmask.is_null() {
            DeleteObject(hmask.cast());
        }
        h
    }
}

/// Frees an icon from [`hicon`]. 0 is ignored.
#[cfg(windows)]
pub fn destroy(h: isize) {
    use windows_sys::Win32::UI::WindowsAndMessaging::DestroyIcon;
    if h != 0 {
        // SAFETY: `h` came from `CreateIconIndirect` and is destroyed once.
        unsafe { DestroyIcon(h as _) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The balloon's icon is a real 256-pixel one, not the tray's 16 stretched: it has an
    /// opaque tile body and soft anti-aliased corner pixels at full size.
    #[test]
    fn the_balloon_icon_is_drawn_at_full_size() {
        let v = render(256, 0);
        assert_eq!(v.len(), 256 * 256 * 4);
        assert_eq!(px(&v, 256, 128, 40)[3], 255, "the tile body is opaque");
        assert_eq!(
            px(&v, 256, 0, 0)[3],
            0,
            "the corner outside the rounding is clear"
        );
        let soft = v.chunks(4).filter(|p| p[3] > 0 && p[3] < 255).count();
        assert!(soft > 200, "edges are anti-aliased at this size: {soft}");
    }

    #[cfg(windows)]
    #[test]
    fn a_large_hicon_is_made_and_freed() {
        let h = hicon_sized(256, 0);
        assert_ne!(h, 0);
        destroy(h);
        let big = hicon_sized(1000, 0);
        assert_ne!(big, 0, "the side is clamped, not refused");
        destroy(big);
    }

    fn px(v: &[u8], side: u32, x: u32, y: u32) -> [u8; 4] {
        let o = ((y * side + x) * 4) as usize;
        [v[o], v[o + 1], v[o + 2], v[o + 3]]
    }

    #[test]
    fn the_badge_text_is_the_count_up_to_99() {
        assert_eq!(badge_text(0), None);
        assert_eq!(badge_text(1).as_deref(), Some("1"));
        assert_eq!(badge_text(4).as_deref(), Some("4"));
        assert_eq!(badge_text(99).as_deref(), Some("99"));
        assert_eq!(badge_text(100).as_deref(), Some("99"));
        assert_eq!(badge_text(100_000).as_deref(), Some("99"));
        assert_eq!(badge_text(u32::MAX).as_deref(), Some("99"));
    }

    #[test]
    fn no_count_makes_a_panic_or_a_wrong_sized_icon() {
        for side in [1, 8, 16, 24, 32, 48, 100_000] {
            for count in [0, 1, 2, 9, 10, 99, 100, 100_000, u32::MAX] {
                let v = render(side, count);
                let s = side.clamp(8, 256) as usize;
                assert_eq!(v.len(), s * s * 4, "side {side} count {count}");
            }
        }
    }

    #[test]
    fn the_tile_is_opaque_inside_and_transparent_at_the_rounded_corners() {
        let v = render(16, 0);
        assert_eq!(px(&v, 16, 0, 0)[3], 0, "the corner shows what is behind");
        assert_eq!(px(&v, 16, 15, 0)[3], 0);
        assert_eq!(px(&v, 16, 0, 15)[3], 0);
        assert_eq!(px(&v, 16, 15, 15)[3], 0);
        for y in 2..14 {
            for x in 2..14 {
                assert_eq!(px(&v, 16, x, y)[3], 255, "inside the tile at {x},{y}");
            }
        }
        // BGRA: a pixel of the tile between the nodes is dark, the hub in the middle is white.
        let tile = px(&v, 16, 8, 2);
        assert!(tile[..3].iter().all(|&c| c < 0x70), "dark, got {tile:?}");
        let hub = px(&v, 16, 8, 8);
        assert!(
            hub.iter().all(|&c| c > 225),
            "the hub is white, got {hub:?}"
        );
    }

    /// Each app's node is in its own corner and in its own colour, and the right half of the
    /// mark mirrors the left pixel for pixel in shape (alpha), at every size the icon file holds.
    #[test]
    fn the_four_nodes_are_coloured_and_the_mark_is_symmetric() {
        let v = render(16, 0);
        // BGRA, so index 2 is red, 1 green, 0 blue.
        let (tl, tr) = (px(&v, 16, 3, 3), px(&v, 16, 12, 3));
        let (bl, br) = (px(&v, 16, 3, 12), px(&v, 16, 12, 12));
        assert!(tl[0] > 200 && tl[2] < 120, "mdedit blue, got {tl:?}");
        assert!(tr[1] > 180 && tr[2] < 120, "mdview teal, got {tr:?}");
        assert!(bl[2] > 200 && bl[0] < 120, "Sprawl orange, got {bl:?}");
        assert!(br[1] > 180 && br[0] < 160, "Inbound green, got {br:?}");
        for side in ICON_FILE_SIDES {
            let (rim, off, sat, hub) = layout(side as i32);
            assert_eq!((side as i32 - hub) % 2, 0, "the hub is centred at {side}");
            assert!(rim >= 1 && sat >= 3 && off > rim, "side {side}");
            let v = render(side, 0);
            for y in 0..side {
                for x in 0..side / 2 {
                    assert_eq!(
                        px(&v, side, x, y)[3],
                        px(&v, side, side - 1 - x, y)[3],
                        "side {side} at {x},{y}"
                    );
                }
            }
        }
    }

    /// The sizes `assets/hub.ico` holds.
    const ICON_FILE_SIDES: [u32; 10] = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256];

    /// Not a test of anything: writes the icon as raw BGRA, `<side>.bgra` for each size of the
    /// icon file, into the folder `HUB_ICON_DUMP` names, with `HUB_ICON_COUNT` in the badge.
    /// `tools/make-icon.ps1` runs it and packs `assets/hub.ico` from what it wrote.
    #[test]
    #[ignore = "run by tools/make-icon.ps1 to draw assets/hub.ico"]
    fn dump_the_icon_sizes() {
        let Some(dir) = std::env::var_os("HUB_ICON_DUMP").map(std::path::PathBuf::from) else {
            return;
        };
        let count = std::env::var("HUB_ICON_COUNT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        std::fs::create_dir_all(&dir).unwrap();
        for side in ICON_FILE_SIDES {
            std::fs::write(dir.join(format!("{side}.bgra")), render(side, count)).unwrap();
        }
    }

    #[test]
    fn the_badge_changes_the_corner_and_leaves_the_rest() {
        let plain = render(16, 0);
        let badged = render(16, 3);
        assert_ne!(plain, badged);
        // The badge's red fills the bottom-right corner.
        let c = px(&badged, 16, 11, 12); // the left padding of the box
        assert_eq!(c[3], 255);
        assert!(c[2] > 150 && c[0] < 80, "red, got {c:?}");
        assert_ne!(px(&plain, 16, 11, 12), c);
        // The top-left quarter is the same either way.
        for y in 0..8 {
            for x in 0..8 {
                assert_eq!(px(&plain, 16, x, y), px(&badged, 16, x, y), "{x},{y}");
            }
        }
    }

    #[test]
    fn the_icon_shows_the_number() {
        let (one, two, big) = (render(16, 1), render(16, 2), render(16, 99));
        assert_ne!(one, two, "1 and 2 are drawn differently");
        assert_ne!(two, big);
        // 100 and 100000 are shown as 99.
        assert_eq!(render(16, 100), big);
        assert_eq!(render(16, 100_000), big);
        // A two-digit badge is wider than a one-digit one: a pixel left of the one-digit box.
        assert_eq!(px(&one, 16, 8, 12)[3], 255);
        let wide = px(&big, 16, 7, 12);
        assert_eq!(wide[3], 255);
        assert!(
            wide[2] > 150 && wide[0] < 80,
            "red at the left of two digits"
        );
        assert_ne!(px(&one, 16, 7, 12), wide);
    }

    #[test]
    fn a_larger_icon_is_the_same_picture_larger() {
        let v = render(32, 3);
        assert_eq!(px(&v, 32, 0, 0)[3], 0);
        let c = px(&v, 32, 22, 24);
        assert!(c[2] > 150 && c[0] < 80, "badge corner at 32 px, got {c:?}");
    }

    #[cfg(windows)]
    #[test]
    fn an_hicon_is_made_and_freed_for_every_count() {
        for n in [0, 1, 7, 100_000] {
            let h = hicon(n);
            assert_ne!(h, 0, "count {n}");
            destroy(h);
        }
        destroy(0);
    }
}
