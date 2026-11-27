//! Every catalog app that has a release has a sample `hub.txt` (`tools/samples/<id>/hub.txt`,
//! written by `tools/make-samples.ps1`, carrying that release's real sizes and hashes) that Hub's
//! parser accepts and that round-trips through its writer. An app with no release yet cannot have
//! one, so it is named in [`NO_SAMPLE`] instead: the list is checked both ways, so an app cannot
//! quietly lose its sample and an app that gains one is forced out of the list (D-031).
use hub_core::catalog::CATALOG;
use hub_core::manifest::{parse, write};

const SAMPLES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/samples/");

/// Catalog apps with no release to take a sample from (D-031, D-035). The test below fails if
/// an app listed here gains a sample.
const NO_SAMPLE: [&str; 3] = ["barbican", "claude-switch", "sozluk"];

#[test]
fn every_catalog_apps_sample_parses_and_round_trips() {
    for app in CATALOG.iter() {
        let path = format!("{SAMPLES}{}/hub.txt", app.id);
        if NO_SAMPLE.contains(&app.id) {
            assert!(
                !std::path::Path::new(&path).exists(),
                "{} is listed in NO_SAMPLE but {path} exists: it has a release, so give it a \
                 sample and take it out of the list",
                app.id
            );
            continue;
        }
        let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"));
        let m = parse(&bytes, app).unwrap_or_else(|e| panic!("{}: {e}", app.id));
        assert_eq!(m.app, app.id);
        assert_eq!(parse(write(&m).as_bytes(), app), Ok(m), "{}", app.id);
    }
}

/// The list names only catalog apps: a renamed or removed app must not leave a stale exemption
/// behind.
#[test]
fn every_app_with_no_sample_is_a_catalog_app() {
    for id in NO_SAMPLE {
        assert!(
            CATALOG.iter().any(|a| a.id == id),
            "{id} is in NO_SAMPLE but is not in the catalog"
        );
    }
}
