//! `hub --shot OUT.png [--size WxH] [--scale N] [--light|--dark] [--view list|large|small]
//! [--demo NAME]`: renders a Vm to a PNG with no window.
use crate::demo;
use crate::view;
use crate::vm::{View, Vm};
use mdshared_text::bitmap::Bitmap;
use mdshared_ui::gfx::{Gfx, Painter};
use mdshared_ui::theme::{DARK, LIGHT, Palette};
use std::path::Path;

/// The most pixels a shot may have (a 4-byte bitmap of this is 256 MB).
pub const MAX_PIXELS: f64 = 64.0e6;

pub struct Args {
    pub out: String,
    pub size: (u32, u32),
    pub scale: f32,
    pub dark: bool,
    pub view: View,
    pub demo: String,
}

pub fn parse(args: &[String]) -> Result<Args, String> {
    let mut a = Args {
        out: String::new(),
        size: (1280, 800),
        scale: 1.0,
        dark: false,
        view: View::List,
        demo: "main".into(),
    };
    let mut it = args.iter();
    while let Some(s) = it.next() {
        match s.as_str() {
            "--shot" => a.out = it.next().ok_or("--shot needs a file")?.clone(),
            "--size" => {
                let v = it.next().ok_or("--size needs WxH")?;
                let (w, h) = v.split_once(['x', 'X']).ok_or("--size is WxH")?;
                a.size = (
                    w.parse().map_err(|_| "--size is WxH")?,
                    h.parse().map_err(|_| "--size is WxH")?,
                );
                if a.size.0 == 0 || a.size.1 == 0 || a.size.0 > 16384 || a.size.1 > 16384 {
                    return Err("--size out of range".into());
                }
            }
            "--scale" => {
                a.scale = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .filter(|s: &f32| s.is_finite() && *s >= 0.5 && *s <= 4.0)
                    .ok_or("--scale is a number from 0.5 to 4")?;
            }
            "--light" => a.dark = false,
            "--dark" => a.dark = true,
            "--view" => {
                a.view = match it.next().map(String::as_str) {
                    Some("list") => View::List,
                    Some("large") => View::Large,
                    Some("small") => View::Small,
                    _ => return Err("--view is list, large or small".into()),
                }
            }
            "--demo" => a.demo = it.next().ok_or("--demo needs a name")?.clone(),
            _ => {}
        }
    }
    if a.out.is_empty() {
        return Err("--shot needs a file".into());
    }
    let px = a.size.0 as f64 * a.size.1 as f64 * (a.scale as f64).powi(2);
    if px > MAX_PIXELS {
        return Err("--size and --scale ask for more than 64 million pixels".into());
    }
    Ok(a)
}

/// The window as a bitmap, `w` x `h` logical px at `scale`.
pub fn render(vm: &Vm, pal: &Palette, w: u32, h: u32, scale: f32) -> Bitmap {
    let mut bmp = Bitmap::new(
        ((w as f32 * scale).round() as u32).max(1),
        ((h as f32 * scale).round() as u32).max(1),
    );
    let mut gfx = Gfx::new();
    let mut p = Painter::new(&mut bmp, &mut gfx, scale);
    view::draw(&mut p, pal, w as f32, h as f32, vm);
    if let Some((rows, page)) = &vm.settings {
        crate::settings::draw_card(&mut p, pal, w as f32, h as f32, rows.clone(), *page);
    }
    bmp
}

/// Entry point of `--shot`; the process exit code.
pub fn run(args: &[String]) -> i32 {
    let a = match parse(args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("hub: {e}");
            return 2;
        }
    };
    let Some(vm) = demo::build(&a.demo, a.view) else {
        eprintln!("hub: no demo '{}'; try: {}", a.demo, demo::NAMES.join(", "));
        return 2;
    };
    let pal = if a.dark { DARK } else { LIGHT };
    let bmp = render(&vm, &pal, a.size.0, a.size.1, a.scale);
    match bmp.write_png(Path::new(&a.out)) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("hub: cannot write {}: {e}", a.out);
            1
        }
    }
}
