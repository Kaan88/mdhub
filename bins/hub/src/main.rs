// The window loop (app.rs) comes later and will use what the drawing code offers.
#![allow(dead_code)]
mod demo;
mod grid;
mod panes;
mod shot;
mod ui;
mod view;
mod vm;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--shot") {
        std::process::exit(shot::run(&args));
    }
}
#[cfg(test)]
mod tests;
