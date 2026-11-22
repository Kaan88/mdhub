//! The command line of the window: `hub [--behind] [--no-session] [--exit-after-ms N] [--trace]
//! [--demo NAME] [--check-now] [--size WxH] [--light|--dark] [--view list|large|small]`.
//! (`--shot` is a different program: `shot.rs`.)
use crate::vm::View;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Args {
    /// Open without the focus, under every other window: a test run.
    pub behind: bool,
    /// Accepted and ignored: Hub keeps nothing of an app's session.
    pub no_session: bool,
    pub exit_after_ms: Option<u64>,
    pub trace: bool,
    pub demo: Option<String>,
    /// Check at once instead of two minutes after start.
    pub check_now: bool,
    pub size: Option<(u32, u32)>,
    pub dark: Option<bool>,
    pub view: Option<View>,
}

pub fn parse(args: &[String]) -> Result<Args, String> {
    let mut a = Args::default();
    let mut it = args.iter();
    while let Some(s) = it.next() {
        match s.as_str() {
            "--behind" => a.behind = true,
            "--no-session" => a.no_session = true,
            "--trace" => a.trace = true,
            "--check-now" => a.check_now = true,
            "--light" => a.dark = Some(false),
            "--dark" => a.dark = Some(true),
            "--exit-after-ms" => {
                a.exit_after_ms = Some(
                    it.next()
                        .and_then(|v| v.parse().ok())
                        .ok_or("--exit-after-ms needs a number of milliseconds")?,
                );
            }
            "--demo" => a.demo = Some(it.next().ok_or("--demo needs a name")?.clone()),
            "--size" => {
                let v = it.next().ok_or("--size needs WxH")?;
                let (w, h) = v.split_once(['x', 'X']).ok_or("--size is WxH")?;
                let (w, h): (u32, u32) = (
                    w.parse().map_err(|_| "--size is WxH")?,
                    h.parse().map_err(|_| "--size is WxH")?,
                );
                if !(1..=16384).contains(&w) || !(1..=16384).contains(&h) {
                    return Err("--size out of range".into());
                }
                a.size = Some((w, h));
            }
            "--view" => {
                a.view = Some(match it.next().map(String::as_str) {
                    Some("list") => View::List,
                    Some("large") => View::Large,
                    Some("small") => View::Small,
                    _ => return Err("--view is list, large or small".into()),
                })
            }
            other => return Err(format!("unknown option {other}")),
        }
    }
    Ok(a)
}

/// The first check's delay, from `HUB_FIRST_CHECK_MS` (a test) or the default 2 minutes; and
/// whether it runs even when no check is due.
pub fn first_check(a: &Args, var: Option<&str>) -> (std::time::Duration, bool) {
    use std::time::Duration;
    if a.check_now {
        return (Duration::ZERO, true);
    }
    match var.and_then(|v| v.trim().parse::<u64>().ok()) {
        Some(ms) => (Duration::from_millis(ms), true),
        None => (Duration::from_secs(120), false),
    }
}
