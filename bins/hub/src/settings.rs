//! Hub's settings card: the rows of the shared paged form (`mdshared_ui::settings`) built from
//! what the engine knows, and the ids a change or an action comes back with. The form is the
//! card of the sliders button; the engine (`glue.rs`) applies what it returns and saves it, and
//! the tray menu changes the same settings, so the card is rebuilt from the engine's rows
//! whenever they differ from the form's.
use mdshared_app::backdrop::Backdrop;
use mdshared_ui::gfx::{Painter, Rect};
use mdshared_ui::settings::{Form, Row};
use mdshared_ui::theme::Palette;

/// "Start Hub with Windows" (the Run key).
pub const START: u32 = 1;
/// "Show an icon in the tray".
pub const TRAY: u32 = 2;
/// "Tell me when updates are found".
pub const NOTIFY: u32 = 3;
/// "Look for updates": the hours between checks, by index into [`INTERVALS`].
pub const INTERVAL: u32 = 4;
/// "Update Hub": the button of the About group.
pub const UPDATE_HUB: u32 = 5;
/// "Window backdrop": Off, Mica or Acrylic, by index into `Backdrop::LABELS`.
pub const BACKDROP: u32 = 6;

/// The help line of "Window backdrop". Hub's own words: the shared one names a side pane, which
/// Hub does not have (its pane under the list stays opaque, see `frost.rs`).
pub const BACKDROP_HELP: &str = "Windows 11. Behind the title row and the two bars.";

/// The choices of "Look for updates": the words, and the hours (0 is never: a manual check only).
pub const INTERVALS: [(&str, u32); 5] = [
    ("Every hour", 1),
    ("Every 6 hours", 6),
    ("Every day", 24),
    ("Every week", 168),
    ("Never", 0),
];

/// The categories of the card, in order.
pub const PAGES: [&str; 3] = ["General", "Updates", "About"];

/// Everything the rows show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Model {
    pub start: bool,
    pub tray: bool,
    pub notify: bool,
    pub interval_hours: u32,
    pub backdrop: Backdrop,
    pub version: String,
    /// The line under Hub's version: where its own update stands.
    pub hub_status: String,
    /// The label of the About button: "Update Hub", or "Restart now" once an update is ready.
    pub hub_button: &'static str,
}

/// The index into [`INTERVALS`] that shows `hours`: the same hours, or the nearest choice for a
/// value written by hand (it is shown as that, and not rewritten until the user picks).
pub fn interval_index(hours: u32) -> usize {
    INTERVALS
        .iter()
        .position(|(_, h)| *h == hours)
        .unwrap_or_else(|| {
            // Never is 0 hours; for any other count the closest of the timed choices.
            (0..INTERVALS.len() - 1)
                .min_by_key(|i| INTERVALS[*i].1.abs_diff(hours))
                .unwrap_or(1)
        })
}

/// The hours a choice stands for; None for an index that is not a choice.
pub fn interval_hours(index: usize) -> Option<u32> {
    INTERVALS.get(index).map(|(_, h)| *h)
}

pub fn rows(m: &Model) -> Vec<Row> {
    let names: Vec<&str> = INTERVALS.iter().map(|(n, _)| *n).collect();
    vec![
        Row::header(PAGES[0]),
        Row::toggle(START, "Start Hub with Windows", m.start)
            .help("Hub stays in the tray and checks quietly. Closing its window does not quit it."),
        Row::toggle(TRAY, "Show an icon in the tray", m.tray)
            .help("Off: closing the window quits Hub."),
        Row::toggle(NOTIFY, "Tell me when updates are found", m.notify)
            .help("One notification per check, never one per app."),
        Row::choice(
            BACKDROP,
            Backdrop::TITLE,
            &Backdrop::LABELS,
            m.backdrop.index(),
        )
        .help(BACKDROP_HELP),
        Row::header(PAGES[1]),
        Row::choice(
            INTERVAL,
            "Look for updates",
            &names,
            interval_index(m.interval_hours),
        )
        .help("One request per app, to the address its release publishes."),
        Row::header(PAGES[2]),
        // An action row has no help line: where the update stands is part of the label.
        Row::action(
            UPDATE_HUB,
            format!("Hub {} \u{b7} {}", m.version, m.hub_status),
            m.hub_button,
        ),
    ]
}

/// A form over `rows`, scrolled to the heading of group `page`. One page, no category list:
/// Hub has a handful of settings, and the owner wants them all in view (2026-11-24).
pub fn form(rows: Vec<Row>, page: usize) -> Form {
    let mut f = Form::new("Settings", rows).single_page();
    if page > 0 {
        f.jump_to_category(page);
    }
    f
}

/// The card over a window drawn into `p`, for a picture (`--shot`, `--demo settings-...`): a form
/// made at once, never one a user has touched.
pub fn draw_card(p: &mut Painter, pal: &Palette, w: f32, h: f32, rows: Vec<Row>, page: usize) {
    let mut f = form(rows, page);
    f.draw(p, pal, Rect::new(0.0, 0.0, w, h), false);
}

#[cfg(test)]
mod tests {
    use super::*;
    use mdshared_ui::settings::Value;

    fn model() -> Model {
        Model {
            start: false,
            tray: true,
            notify: true,
            interval_hours: 6,
            backdrop: Backdrop::Off,
            version: "0.1.0".into(),
            hub_status: "Up to date".into(),
            hub_button: "Update Hub",
        }
    }

    #[test]
    fn the_card_is_one_page_with_no_category_list() {
        let mut bmp = mdshared_text::bitmap::Bitmap::new(1200, 900);
        let mut gfx = mdshared_ui::gfx::Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        for page in 0..PAGES.len() {
            let mut f = form(rows(&model()), page);
            // A window wide enough for the list, were it a paged form.
            f.layout(&mut p, Rect::new(0.0, 0.0, 1200.0, 900.0));
            assert!(!f.has_nav(), "page {page}: no category list");
            assert_eq!(
                f.categories(),
                PAGES.to_vec(),
                "the groups stay as headings"
            );
        }
    }

    #[test]
    fn the_choices_map_to_hours_and_back() {
        for (i, (_, h)) in INTERVALS.iter().enumerate() {
            assert_eq!(interval_index(*h), i);
            assert_eq!(interval_hours(i), Some(*h));
        }
        assert_eq!(interval_hours(INTERVALS.len()), None);
        // A value written by hand is shown as the nearest timed choice, never as Never.
        assert_eq!(interval_index(12), 1, "12 h is nearer 6 than 24");
        assert_eq!(interval_index(20), 2);
        assert_eq!(interval_index(100_000), 3);
    }

    #[test]
    fn the_rows_carry_the_model_and_the_ids() {
        let f = Form::new("Settings", rows(&model()));
        assert_eq!(f.categories(), PAGES.to_vec());
        assert_eq!(f.value(START), Some(Value::Bool(false)));
        assert_eq!(f.value(TRAY), Some(Value::Bool(true)));
        assert_eq!(f.value(NOTIFY), Some(Value::Bool(true)));
        assert_eq!(f.value(INTERVAL), Some(Value::Choice(1)));
        assert_eq!(f.value(BACKDROP), Some(Value::Choice(0)), "off");
        let mut m = model();
        m.start = true;
        m.interval_hours = 0;
        m.backdrop = Backdrop::Acrylic;
        let f = Form::new("Settings", rows(&m));
        assert_eq!(f.value(START), Some(Value::Bool(true)));
        assert_eq!(f.value(INTERVAL), Some(Value::Choice(4)));
        assert_eq!(f.value(BACKDROP), Some(Value::Choice(2)));
    }
}
