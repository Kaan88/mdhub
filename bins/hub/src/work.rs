//! The worker threads: a check of every app, an install, a roll back. Each runs once, posts its
//! progress and its end through `Post`, and finishes. Nothing here decides what to run or when:
//! that is the engine's (`glue.rs`), which starts one install at a time.
use crate::glue::{Event, Post};
use hub_core::catalog::{CATALOG, CatalogApp};
use hub_core::check;
use hub_core::install::{self, Opts, Progress};
use hub_core::model::{HubManifest, Installed};
use hub_core::state::Store;
use hub_core::transport::Transport;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How often a download's progress is passed on at most.
const PROGRESS_EVERY: Duration = Duration::from_millis(100);

fn spawn(name: &str, f: impl FnOnce() + Send + 'static) -> Result<(), String> {
    std::thread::Builder::new()
        .name(name.to_string())
        .spawn(f)
        .map(drop)
        .map_err(|e| e.to_string())
}

/// A progress callback that posts download counts at most every 100 ms (and the last one).
fn reporter(post: Arc<dyn Post>, id: &'static str) -> impl FnMut(Progress) {
    let mut last: Option<Instant> = None;
    move |p| {
        if let Progress::Downloading { done, total, .. } = &p
            && done != total
            && last.is_some_and(|t| t.elapsed() < PROGRESS_EVERY)
        {
            return;
        }
        if matches!(p, Progress::Downloading { .. }) {
            last = Some(Instant::now());
        }
        post.post(Event::Progress { id, p });
    }
}

pub fn spawn_install(
    post: Arc<dyn Post>,
    app: &'static CatalogApp,
    m: HubManifest,
    cur: Option<Installed>,
    store: Store,
    t: Arc<dyn Transport>,
    opts: Opts,
) -> Result<(), String> {
    spawn("hub-install", move || {
        let mut progress = reporter(post.clone(), app.id);
        let result = install::install(app, &m, cur.as_ref(), &store, &*t, &opts, &mut progress);
        post.post(Event::Finished { id: app.id, result });
    })
}

pub fn spawn_rollback(
    post: Arc<dyn Post>,
    app: &'static CatalogApp,
    store: Store,
    opts: Opts,
) -> Result<(), String> {
    spawn("hub-rollback", move || {
        let mut progress = reporter(post.clone(), app.id);
        let result = install::rollback_with(app, &store, &opts, &mut progress);
        post.post(Event::Finished { id: app.id, result });
    })
}

pub fn spawn_check(post: Arc<dyn Post>, t: Arc<dyn Transport>, store: Store) -> Result<(), String> {
    spawn("hub-check", move || {
        let results = check::check_all(CATALOG.iter(), &*t, &store);
        post.post(Event::Checked(results));
    })
}
