//! Every catalog app has a sample `hub.txt` (`tools/samples/<id>/hub.txt`, written by
//! `tools/make-samples.ps1`) that Hub's parser accepts and that round-trips through its writer.
use hub_core::catalog::CATALOG;
use hub_core::manifest::{parse, write};

const SAMPLES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/samples/");

#[test]
fn every_catalog_apps_sample_parses_and_round_trips() {
    for app in CATALOG.iter() {
        let path = format!("{SAMPLES}{}/hub.txt", app.id);
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let m = parse(&bytes, app).unwrap_or_else(|e| panic!("{}: {e}", app.id));
        assert_eq!(m.app, app.id);
        assert_eq!(parse(write(&m).as_bytes(), app), Ok(m), "{}", app.id);
    }
}
