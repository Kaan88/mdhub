//! What the tray says: the balloon after a check, and the tooltip. Pure text and one small rule,
//! no window and no Windows call, so every case is a unit test.
//!
//! The rule (the owner, plan section 8): one notification per check that found something, never
//! one per app, and not again for what was already told. [`Memory`] is that "already told".
//! Finished and failed updates are not notified at all: they are in the window and the badge.
use std::collections::BTreeSet;

/// `NOTIFYICONDATAW.szInfo` holds 255 UTF-16 units and a NUL; stay well under it.
pub const BODY_UNITS: usize = 200;
/// `szInfoTitle` holds 63.
pub const TITLE: &str = "Hub";
/// `szTip` holds 127.
pub const TIP_UNITS: usize = 120;
/// At most this many names are listed; the rest are "+N more".
pub const MAX_NAMES: usize = 4;
const NAME_CHARS: usize = 24;

/// One thing with an update waiting: an app by its display name, or Hub itself.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pending {
    pub name: String,
    /// The version on offer. It is what makes a newer release of an already-told app new again.
    pub version: String,
    /// Hub's own update: listed first so a long list never hides it.
    pub own: bool,
}

impl Pending {
    pub fn app(name: &str, version: &str) -> Pending {
        Pending {
            name: name.to_string(),
            version: version.to_string(),
            own: false,
        }
    }

    pub fn hub(version: &str) -> Pending {
        Pending {
            name: "Hub".to_string(),
            version: version.to_string(),
            own: true,
        }
    }
}

/// The text of one balloon.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub title: String,
    pub body: String,
}

fn units(s: &str) -> usize {
    s.encode_utf16().count()
}

/// `s` cut to at most `max` UTF-16 units, never inside a character.
pub fn fit_utf16(s: &str, max: usize) -> String {
    let mut out = String::new();
    let mut n = 0;
    for c in s.chars() {
        n += c.len_utf16();
        if n > max {
            break;
        }
        out.push(c);
    }
    out
}

/// A name as it may be shown: no control characters, trimmed, not long.
fn clean(name: &str) -> String {
    let t: String = name.chars().filter(|c| !c.is_control()).collect();
    t.trim().chars().take(NAME_CHARS).collect()
}

fn count_text(n: usize) -> String {
    if n == 1 {
        "1 update".to_string()
    } else {
        format!("{n} updates")
    }
}

/// The balloon for everything that has an update, or `None` when there is nothing.
/// `1 update: mdedit`, `3 updates: mdedit, Sprawl, Inbound`, `7 updates: Hub, app1, app2, app3
/// +3 more`. Hub's own update comes first. The same name twice counts once.
pub fn compose(pending: &[Pending]) -> Option<Message> {
    let mut names: Vec<String> = Vec::new();
    let mut ordered: Vec<&Pending> = pending.iter().filter(|p| p.own).collect();
    ordered.extend(pending.iter().filter(|p| !p.own));
    for p in ordered {
        let n = clean(&p.name);
        if !n.is_empty() && !names.contains(&n) {
            names.push(n);
        }
    }
    if names.is_empty() {
        return None;
    }
    let total = names.len();
    let head = format!("{}: ", count_text(total));
    // The most names that fit: the cap, then fewer while the text is too long.
    let mut shown = total.min(MAX_NAMES);
    loop {
        let mut body = format!("{head}{}", names[..shown].join(", "));
        if shown < total {
            body.push_str(&format!(" +{} more", total - shown));
        }
        if units(&body) <= BODY_UNITS || shown <= 1 {
            return Some(Message {
                title: TITLE.to_string(),
                body: fit_utf16(&body, BODY_UNITS),
            });
        }
        shown -= 1;
    }
}

/// The tray's tooltip: `Hub`, `Hub: 2 updates available`, `Hub: updating...`. `count` is how
/// many updates wait, `busy` is an install running.
pub fn tooltip(count: usize, busy: bool) -> String {
    let t = if busy {
        "Hub: updating...".to_string()
    } else if count == 0 {
        "Hub".to_string()
    } else {
        format!("Hub: {} available", count_text(count))
    };
    fit_utf16(&t, TIP_UNITS)
}

/// What was already told. A check that finds a set containing something not told yet gives a
/// balloon (everything waiting is in it: the reader wants the whole picture); a set that is the
/// same, or only smaller, gives none. What is no longer waiting is forgotten, so an update that
/// comes back later is news again. A newer version of a told app is news too.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Memory {
    told: BTreeSet<(String, String)>,
}

impl Memory {
    pub fn new() -> Memory {
        Memory::default()
    }

    /// Reads `to_line`'s text. Anything unreadable is simply forgotten (a balloon too many, never
    /// one too few, and never a panic).
    pub fn from_line(line: &str) -> Memory {
        let told = line
            .split('|')
            .filter_map(|item| {
                let (n, v) = item.split_once('=')?;
                (!n.is_empty()).then(|| (n.to_string(), v.to_string()))
            })
            .take(64)
            .collect();
        Memory { told }
    }

    /// `name=version|name=version`, one line, for settings.ini.
    pub fn to_line(&self) -> String {
        self.told
            .iter()
            .map(|(n, v)| format!("{n}={v}"))
            .collect::<Vec<_>>()
            .join("|")
    }

    /// The rule. Call it once per finished check with everything that has an update; show what it
    /// returns, then save `to_line` if it changed.
    pub fn decide(&mut self, pending: &[Pending]) -> Option<Message> {
        self.decide_keeping(pending, &[])
    }

    /// [`decide`](Self::decide) for a check that could not see everything: what was told about
    /// the names in `unseen` (an app whose release could not be read this time, Hub's own failed
    /// download) is kept, not forgotten, so the same update is not news again when it is seen
    /// again. A kept entry is never what makes a balloon.
    pub fn decide_keeping(&mut self, pending: &[Pending], unseen: &[String]) -> Option<Message> {
        // `=` and `|` are the line's own characters; no name or version carries them.
        let mut now: BTreeSet<(String, String)> = pending
            .iter()
            .map(|p| {
                (
                    clean(&p.name).replace(['=', '|'], "_"),
                    p.version.replace(['=', '|'], "_"),
                )
            })
            .filter(|(n, _)| !n.is_empty())
            .collect();
        let fresh = now.iter().any(|i| !self.told.contains(i));
        let unseen: Vec<String> = unseen
            .iter()
            .map(|n| clean(n).replace(['=', '|'], "_"))
            .collect();
        let known: BTreeSet<String> = now.iter().map(|(n, _)| n.clone()).collect();
        for (n, v) in &self.told {
            if unseen.contains(n) && !known.contains(n) {
                now.insert((n.clone(), v.clone()));
            }
        }
        self.told = now;
        if fresh { compose(pending) } else { None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(n: &str) -> Pending {
        Pending::app(n, "1.0.0")
    }

    #[test]
    fn what_was_told_about_something_not_seen_this_time_is_kept() {
        let mut m = Memory::new();
        assert!(m.decide(&[Pending::hub("0.2.0"), a("mdedit")]).is_some());
        // Hub could not be seen (its manifest was unreachable): its entry is kept, silently.
        assert_eq!(m.decide_keeping(&[a("mdedit")], &["Hub".to_string()]), None);
        // Seen again, the same version: told already.
        assert_eq!(m.decide(&[Pending::hub("0.2.0"), a("mdedit")]), None);
        // Not kept when it is not unseen: it left, and is news when it returns.
        assert_eq!(m.decide(&[a("mdedit")]), None);
        assert!(m.decide(&[Pending::hub("0.2.0"), a("mdedit")]).is_some());
        // A kept entry never makes a message, and a newer version is news.
        assert_eq!(m.decide_keeping(&[a("mdedit")], &["Hub".to_string()]), None);
        assert!(m.decide(&[Pending::hub("0.3.0"), a("mdedit")]).is_some());
    }

    #[test]
    fn nothing_waiting_is_no_message() {
        assert_eq!(compose(&[]), None);
        assert_eq!(compose(&[a("  "), a("\u{7}")]), None);
    }

    #[test]
    fn one_and_three_read_as_the_plan_says() {
        assert_eq!(compose(&[a("mdedit")]).unwrap().body, "1 update: mdedit");
        let m = compose(&[a("mdedit"), a("Sprawl"), a("Inbound")]).unwrap();
        assert_eq!(m.body, "3 updates: mdedit, Sprawl, Inbound");
        assert_eq!(m.title, "Hub");
    }

    #[test]
    fn a_long_list_is_cut_with_plus_n_more() {
        let many: Vec<Pending> = (1..=7).map(|i| a(&format!("app{i}"))).collect();
        let m = compose(&many).unwrap();
        assert_eq!(m.body, "7 updates: app1, app2, app3, app4 +3 more");
    }

    #[test]
    fn long_names_are_capped_so_the_balloon_always_fits() {
        let l1 = "x".repeat(100);
        let l2 = "y".repeat(100);
        let l3 = "z".repeat(100);
        let l4 = "w".repeat(100);
        let m = compose(&[a(&l1), a(&l2), a(&l3), a(&l4), a("e")]).unwrap();
        assert!(units(&m.body) <= BODY_UNITS, "{}", m.body);
        assert!(m.body.contains(&"x".repeat(NAME_CHARS)));
        assert!(!m.body.contains(&"x".repeat(NAME_CHARS + 1)));
        assert!(m.body.ends_with(" +1 more"), "{}", m.body);
    }

    #[test]
    fn hubs_own_update_is_mentioned_and_first() {
        let m = compose(&[a("mdedit"), Pending::hub("0.2.0")]).unwrap();
        assert_eq!(m.body, "2 updates: Hub, mdedit");
        assert_eq!(
            compose(&[Pending::hub("0.2.0")]).unwrap().body,
            "1 update: Hub"
        );
        // Even in a cut list Hub is not the one that goes.
        let mut v: Vec<Pending> = (1..=6).map(|i| a(&format!("app{i}"))).collect();
        v.push(Pending::hub("0.2.0"));
        assert!(
            compose(&v)
                .unwrap()
                .body
                .starts_with("7 updates: Hub, app1")
        );
    }

    #[test]
    fn the_same_name_counts_once_and_control_characters_are_dropped() {
        let m = compose(&[a("mdedit"), a("mdedit"), a("Spr\nawl")]).unwrap();
        assert_eq!(m.body, "2 updates: mdedit, Sprawl");
    }

    #[test]
    fn the_cut_never_splits_a_character() {
        let s = "a\u{1F600}b"; // the face is two UTF-16 units
        assert_eq!(fit_utf16(s, 2), "a");
        assert_eq!(fit_utf16(s, 3), "a\u{1F600}");
        assert_eq!(fit_utf16(s, 0), "");
    }

    #[test]
    fn the_tooltip_says_how_many_waits_and_fits_the_tip() {
        assert_eq!(tooltip(0, false), "Hub");
        assert_eq!(tooltip(1, false), "Hub: 1 update available");
        assert_eq!(tooltip(3, false), "Hub: 3 updates available");
        assert_eq!(tooltip(3, true), "Hub: updating...");
        assert!(units(&tooltip(usize::MAX, false)) <= TIP_UNITS);
    }

    #[test]
    fn one_balloon_per_check_and_not_again_for_the_same_set() {
        let mut m = Memory::new();
        let set = [a("mdedit"), a("Sprawl")];
        assert!(m.decide(&set).is_some());
        // The next check finds the same: silent.
        assert_eq!(m.decide(&set), None);
        assert_eq!(m.decide(&[a("Sprawl"), a("mdedit")]), None);
    }

    #[test]
    fn a_new_app_or_a_newer_version_is_news_and_the_balloon_lists_everything() {
        let mut m = Memory::new();
        m.decide(&[a("mdedit")]);
        let n = m.decide(&[a("mdedit"), a("mdview")]).unwrap();
        assert_eq!(n.body, "2 updates: mdedit, mdview");
        let newer = [Pending::app("mdedit", "1.1.0"), a("mdview")];
        assert!(m.decide(&newer).is_some());
        assert_eq!(m.decide(&newer), None);
    }

    #[test]
    fn a_smaller_set_is_silent_and_a_cleared_one_forgets() {
        let mut m = Memory::new();
        m.decide(&[a("mdedit"), a("mdview")]);
        // One was installed: nothing new to say about the other.
        assert_eq!(m.decide(&[a("mdview")]), None);
        // Nothing waits; later the same update returns: it is news again.
        assert_eq!(m.decide(&[]), None);
        assert!(m.decide(&[a("mdview")]).is_some());
    }

    #[test]
    fn memory_survives_a_restart_through_its_line_and_garbage_is_harmless() {
        let mut m = Memory::new();
        m.decide(&[a("mdedit"), Pending::hub("0.2.0")]);
        let line = m.to_line();
        let mut again = Memory::from_line(&line);
        assert_eq!(again, m);
        assert_eq!(again.decide(&[a("mdedit"), Pending::hub("0.2.0")]), None);
        let long = "x=1|".repeat(500);
        for junk in [
            "",
            "|||",
            "=",
            "=x",
            "a",
            "\u{0}=\u{1F600}|=|",
            long.as_str(),
        ] {
            let mut g = Memory::from_line(junk);
            let _ = g.decide(&[a("mdedit")]);
        }
    }
}
