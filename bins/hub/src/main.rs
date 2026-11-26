//! Hub: the launcher and updater for the apps. winit + softbuffer, every control drawn by this
//! crate or by mdshared-ui. `app.rs` is the window, `glue.rs` the engine behind it, `work.rs` its
//! worker threads; `view.rs`, `grid.rs`, `panes.rs`, `ui.rs` draw a `Vm`.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(dead_code)]

/// `eprintln!` for this crate: a stderr that is gone (a closed pipe) is ignored instead of being a
/// panic, which the release build (`panic = "abort"`) would turn into the end of the program.
macro_rules! eprintln {
    ($($arg:tt)*) => {{
        use std::io::Write as _;
        let _ = writeln!(std::io::stderr(), $($arg)*);
    }};
}

#[cfg(windows)]
mod app;
mod appicon;
mod changelog;
mod cli;
mod demo;
mod frost;
mod glue;
mod grid;
mod icon;
mod motion;
mod panes;
#[cfg(any(windows, test))]
mod placement;
mod popup;
mod settings;
mod shot;
mod trace;
mod tray;
mod ui;
mod view;
mod vm;
mod work;

fn main() {
    let started = std::time::Instant::now();
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--shot") {
        std::process::exit(shot::run(&args));
    }
    let parsed = match cli::parse(&args) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("hub: {e}");
            std::process::exit(2);
        }
    };
    #[cfg(windows)]
    std::process::exit(app::run(parsed, started));
    #[cfg(not(windows))]
    {
        let _ = (parsed, started);
        eprintln!("hub: the window is Windows-only for now (hub --shot works everywhere)");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod e2e_tests;
#[cfg(test)]
mod glue_tests;
#[cfg(test)]
mod notes_tests;
#[cfg(test)]
mod own_tests;
#[cfg(test)]
mod tests;
