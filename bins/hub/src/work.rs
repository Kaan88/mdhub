//! The worker threads: a check of every app, an install, a roll back. Each runs once, posts its
//! progress and its end through `Post`, and finishes. Nothing here decides what to run or when:
//! that is the engine's (`glue.rs`), which starts one install at a time.
use crate::glue::{Event, HubEvent, Post};
use hub_core::catalog::{CATALOG, CatalogApp};
use hub_core::check;
use hub_core::install::{self, Opts, Progress};
use hub_core::model::{HubManifest, Installed};
use hub_core::selfupdate;
use hub_core::state::Store;
use hub_core::transport::Transport;
use mdshared_app::update::Version;
use std::path::PathBuf;
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

/// What a check does about Hub's own update: the exe that would be replaced, and the version
/// already staged beside it (not downloaded again).
pub struct HubJob {
    pub exe: PathBuf,
    pub ready: Option<Version>,
}

/// A check: Hub's own manifest first (when Hub can update itself), then the apps', then Hub's
/// download when a newer one was found. Hub's own request is the first of the run, so its update
/// shows at the top; its download is the last, so it never delays the apps' answers.
pub fn spawn_check(
    post: Arc<dyn Post>,
    t: Arc<dyn Transport>,
    store: Store,
    hub: Option<HubJob>,
) -> Result<(), String> {
    spawn("hub-check", move || {
        let own = hub
            .as_ref()
            .map(|_| selfupdate::check(&selfupdate::app(), &*t));
        let offer = match &own {
            Some(Ok(Some(o))) => Some(o.clone()),
            _ => None,
        };
        if let Some(r) = own {
            post.post(Event::Hub(HubEvent::Checked(r)));
        }
        let results = check::check_all(CATALOG.iter(), &*t, &store);
        post.post(Event::Checked(results));
        if let (Some(job), Some(offer)) = (&hub, offer)
            && job.ready.as_ref() != Some(&offer.version)
        {
            let r = selfupdate::stage(&offer, &job.exe, &*t);
            post.post(Event::Hub(HubEvent::Staged(r)));
        }
    })
}
