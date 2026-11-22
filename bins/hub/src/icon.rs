//! The tray icon, drawn at run time with the shared painter: the Hub tile (a white download arrow
//! on a rounded blue square) and, when updates wait, the same tile with a red badge in the corner
//! carrying the count. One icon is built per change of the count, never per frame, and the
//! previous `HICON` is destroyed by its owner (`tray.rs`) once the new one is in place.
//!
//! The painter writes opaque pixels and has no alpha channel, so the tile is rendered twice, once
//! over black and once over white, and the straight alpha of every pixel is solved from the two
//! (`alpha = 255 - (white - black)`): the rounded corners and the antialiased edges come out as a
//! real 32-bit icon with no halo. Digits are a 3 x 5 pixel font drawn with rectangles, so no font
//! is loaded for a tray that otherwise holds none.
//!
//! Everything but the two calls at the bottom is pure and tested without a window.
use mdshared_text::bitmap::Bitmap;
use mdshared_ui::gfx::{Gfx, Painter, Rect};

/// The Hub tile.
pub const BLUE: [u8; 3] = [0x00, 0x67, 0xC0];
/// The badge.
pub const RED: [u8; 3] = [0xC4, 0x2B, 0x1C];
const WHITE: [u8; 3] = [255, 255, 255];
/// The design grid: everything below is in 16 x 16 units, scaled to the icon's side.
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

fn paint(p: &mut Painter, count: u32) {
    p.rbox(Rect::new(0.0, 0.0, GRID, GRID), 3.5, BLUE);
    // The download arrow: a stem, a head, a tray line under it.
    p.fill(Rect::new(7.0, 2.5, 2.0, 6.0), WHITE);
    p.polygon(&[(3.5, 7.5), (12.5, 7.5), (8.0, 12.0)], WHITE);
    p.fill(Rect::new(3.5, 13.0, 9.0, 1.5), WHITE);
    let Some(text) = badge_text(count) else {
        return;
    };
    let n = text.chars().count() as f32;
    let (w, h) = (4.0 * n + 1.0, 7.0);
    let (x, y) = (GRID - w, GRID - h);
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

/// The icon `side` pixels across (8 to 256) as straight-alpha BGRA, `side * side * 4` bytes, top
/// row first. `count` 0 is the plain tile.
pub fn render(side: u32, count: u32) -> Vec<u8> {
    let side = side.clamp(8, 256);
    let scale = side as f32 / GRID;
    let mut gfx = Gfx::new();
    let mut shot = |bg: u8| {
        let mut bmp = Bitmap::new(side, side);
        bmp.clear([bg; 3]);
        paint(&mut Painter::new(&mut bmp, &mut gfx, scale), count);
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
        let mid = px(&v, 16, 2, 8);
        assert_eq!(mid[3], 255, "inside the tile");
        // BGRA: the tile colour, not black.
        assert_eq!([mid[2], mid[1], mid[0]], BLUE);
        // The arrow's stem is white.
        let stem = px(&v, 16, 7, 4);
        assert_eq!(stem, [255, 255, 255, 255]);
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
