//! A read-only look at this PC through `detect::detect`; prints name, version and how it was
//! found. Run by hand: `cargo test -p hub-core --test measure_real_detect -- --ignored --nocapture`
use hub_core::catalog::CATALOG;
use hub_core::detect::{Env, detect};
use hub_core::state::Store;

#[test]
#[ignore = "reads this PC, read-only"]
fn what_is_installed_here() {
    let dir = std::env::temp_dir().join(format!("hub-real-detect-{}", std::process::id()));
    let store = Store { dir };
    let env = Env::system();
    for app in CATALOG.iter() {
        match detect(app, &store, &env) {
            Some(i) => println!(
                "detected {}: version {:?} source {:?}",
                app.name,
                i.version.map(|v| v.to_string()),
                i.source
            ),
            None => println!("detected {}: not found", app.name),
        }
    }
}
