//! Hub's settings card: the rows of the shared paged form (`mdshared_ui::settings`) built from
//! what the engine knows, and the ids a change or an action comes back with. The form is the
//! card of the sliders button; the engine (`glue.rs`) applies what it returns and saves it, and
//! the tray menu changes the same settings, so the card is rebuilt from the engine's rows
//! whenever they differ from the form's.
use crate::motion::Overlay;
use mdshared_app::backdrop::Backdrop;
use mdshared_ui::anim::{Animator, document, pop};
use mdshared_ui::gfx::{Painter, Rect};
use mdshared_ui::keys::{KeyName, Mods};
use mdshared_ui::menu::{Entrance, Item, Menu};
use mdshared_ui::overlay::{dialog_bounds, draw_backdrop_presence};
use mdshared_ui::settings::{Form, Outcome, Row, Style, Value};
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

/// Keep the shared single-page form for its rows, focus, presses and values. Its built-in
/// choice menu is private and disappears immediately on dismissal. For non-segmented choices,
/// hand the opening off to a shared Menu held in a presence slot instead.
pub struct Card {
    form: Form,
    menu: Overlay<ChoiceMenu>,
    field_press: Option<u32>,
}

struct ChoiceMenu {
    id: u32,
    menu: Menu<usize>,
    pressed: Option<usize>,
}

impl Card {
    pub fn new(rows: Vec<Row>, page: usize) -> Self {
        let mut form = form(rows, page);
        let mut style = Style::CLASSIC;
        style.backdrop = 0.0;
        form.set_style(style);
        Self {
            form,
            menu: Overlay::new(document::MENU),
            field_press: None,
        }
    }

    pub fn rows(&self) -> &[Row] {
        self.form.rows()
    }

    pub fn set_rows(&mut self, rows: Vec<Row>) {
        self.form.set_rows(rows);
        self.menu.hide();
        self.field_press = None;
    }

    fn dropdown_index(&self, id: u32) -> Option<usize> {
        self.rows().iter().enumerate().find_map(|(i, row)| {
            (matches!(row, Row::Choice { id: row_id, .. } if *row_id == id)
                && !self.form.is_segmented(i))
            .then_some(i)
        })
    }

    fn dropdown_at(&self, x: f32, y: f32) -> Option<u32> {
        if !self.form.body_rect().contains(x, y) {
            return None;
        }
        self.rows()
            .iter()
            .enumerate()
            .find_map(|(i, row)| match row {
                Row::Choice { id, .. }
                    if !self.form.is_segmented(i) && self.form.control_rect(i).contains(x, y) =>
                {
                    Some(*id)
                }
                _ => None,
            })
    }

    pub fn open_dropdown(&mut self, id: u32) {
        let Some(i) = self.dropdown_index(id) else {
            return;
        };
        let Row::Choice { options, value, .. } = &self.rows()[i] else {
            return;
        };
        let field = self.form.control_rect(i);
        let items = options
            .iter()
            .enumerate()
            .map(|(k, name)| Item::new(k, name).checked(k == *value))
            .collect();
        let mut menu = Menu::new(
            items,
            field.right(),
            field.bottom() + mdshared_ui::dropdown::MENU_GAP,
            true,
        );
        menu.highlight(*value);
        self.menu.show(ChoiceMenu {
            id,
            menu,
            pressed: None,
        });
    }

    fn pick(&mut self, id: u32, pick: Option<usize>) -> Outcome {
        self.menu.hide();
        if let Some(index) = pick {
            let value = Value::Choice(index);
            if self.form.value(id) != Some(value.clone()) && self.form.set_value(id, value.clone())
            {
                return Outcome::Changed { id, value };
            }
        }
        Outcome::Redraw
    }

    pub fn key(&mut self, key: &KeyName, mods: Mods, typed: Option<&str>) -> Outcome {
        if let Some(open) = self.menu.input() {
            match key {
                KeyName::Escape | KeyName::Tab => self.menu.hide(),
                KeyName::Up | KeyName::Down => open.menu.step(*key == KeyName::Down),
                KeyName::Enter | KeyName::Space => {
                    let (id, pick) = (open.id, open.menu.chosen());
                    return self.pick(id, pick);
                }
                _ => return Outcome::None,
            }
            return Outcome::Redraw;
        }
        let dropdown = self
            .form
            .focus_id()
            .filter(|id| self.dropdown_index(*id).is_some());
        let result = self.form.key(key, mods, typed);
        if let Some(id) = dropdown
            && matches!(key, KeyName::Enter | KeyName::Space)
        {
            // Let shared reveal keyboard focus and activate its control. Esc closes that
            // private menu, not the card; the replacement uses the same options and geometry.
            if matches!(result, Outcome::Redraw) {
                self.form.key(&KeyName::Escape, mods, None);
                self.open_dropdown(id);
            }
        }
        result
    }

    pub fn pointer_move(&mut self, x: f32, y: f32) -> bool {
        match self.menu.input() {
            Some(open) => open.menu.pointer(x, y),
            None => self.form.pointer_move(x, y),
        }
    }

    pub fn press(&mut self, x: f32, y: f32, mods: Mods, clicks: u8) -> Outcome {
        self.field_press = None;
        if let Some(open) = self.menu.input() {
            if open.menu.contains(x, y) {
                open.menu.pointer(x, y);
                open.pressed = open.menu.hover;
            } else {
                self.menu.hide();
            }
            return Outcome::Redraw;
        }
        self.field_press = self.dropdown_at(x, y);
        self.form.press(x, y, mods, clicks)
    }

    pub fn release(&mut self, x: f32, y: f32) -> Outcome {
        if let Some(open) = self.menu.input() {
            let pressed = open.pressed.take();
            open.menu.pointer(x, y);
            if pressed.is_some() && pressed == open.menu.hover {
                let (id, pick) = (open.id, open.menu.chosen());
                return self.pick(id, pick);
            }
            return Outcome::None;
        }
        let result = self.form.release(x, y);
        if let Some(id) = self.field_press.take()
            && self.dropdown_at(x, y) == Some(id)
            && matches!(result, Outcome::Redraw)
        {
            self.form.key(&KeyName::Escape, Mods::default(), None);
            self.open_dropdown(id);
        }
        result
    }

    pub fn wheel(&mut self, x: f32, y: f32, dy: f32) -> bool {
        match self.menu.input() {
            Some(open) => open.menu.contains(x, y) && open.menu.scroll_by(dy),
            None => self.form.wheel(x, y, dy),
        }
    }

    pub fn layout(&mut self, p: &mut Painter, area: Rect) {
        self.form.layout(p, area);
    }

    pub fn draw_motion(
        &mut self,
        p: &mut Painter,
        pal: &Palette,
        area: Rect,
        t: f32,
        anim: &mut Animator,
    ) {
        if t == 0.0 {
            self.layout(p, area);
            return;
        }
        // If a dropdown sticks out of the card, fade both as a group. Otherwise save only
        // the card and its shadow. The footprint stays beneath the same hit rectangles.
        let bounds = if t < 1.0 && !self.menu.is_alive() {
            dialog_bounds(self.form.layout(p, area))
        } else {
            area
        };
        draw_backdrop_presence(p, pal, area.w, area.h, t);
        pop(p, bounds, t, (0.0, 0.0), |p| {
            self.form
                .draw_animated(p, pal, area, false, anim, "hub-settings-form");
            self.menu.draw(anim, "settings-dropdown", |open, t, anim| {
                open.menu
                    .draw_entering_with(p, area, t, Entrance::Fade, |p, menu| {
                        menu.draw_in_animated(
                            p,
                            pal,
                            area,
                            anim,
                            ("settings-dropdown-hover", open.id),
                        );
                    });
            });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mdshared_ui::anim::{Clock, ManualClock};
    use mdshared_ui::settings::Value;
    use mdshared_ui::theme::{DARK, LIGHT};

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

    #[test]
    fn animated_card_at_rest_is_the_shared_single_page_form() {
        for pal in [LIGHT, DARK] {
            let (mut bmp, mut gfx) = (
                mdshared_text::Bitmap::new(860, 600),
                mdshared_ui::gfx::Gfx::new(),
            );
            let area = Rect::new(0.0, 0.0, 860.0, 600.0);
            let rows = rows(&model());
            let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
            p.fill(area, pal.page);
            form(rows.clone(), 0).draw(&mut p, &pal, area, false);
            let old = p.bmp.pixels.clone();
            p.fill(area, pal.page);
            let mut anim = crate::motion::still();
            Card::new(rows, 0).draw_motion(&mut p, &pal, area, 1.0, &mut anim);
            assert_eq!(p.bmp.pixels, old);
            assert_eq!(anim.end_frame(), None);
        }
    }

    #[test]
    fn the_card_and_its_dropdown_open_and_close_then_stop_asking_for_frames() {
        let (mut bmp, mut gfx) = (
            mdshared_text::Bitmap::new(860, 600),
            mdshared_ui::gfx::Gfx::new(),
        );
        let area = Rect::new(0.0, 0.0, 860.0, 600.0);
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        let clock = ManualClock::new();
        let mut anim = Animator::new();
        let mut slot = Overlay::new(document::DIALOG);
        let mut frame = |slot: &mut Overlay<Card>| {
            p.fill(area, LIGHT.page);
            anim.begin_frame(clock.now());
            slot.draw(&mut anim, "settings-card", |card, t, anim| {
                card.draw_motion(&mut p, &LIGHT, area, t, anim);
            });
            (p.bmp.pixels.clone(), anim.end_frame())
        };
        let base = frame(&mut slot);
        slot.show(Card::new(rows(&model()), 0));
        let entering = frame(&mut slot);
        assert_eq!(entering.0, base.0);
        assert!(entering.1.is_some());
        for _ in 0..17 {
            clock.advance_ms(16);
            frame(&mut slot);
        }
        let card = frame(&mut slot);
        assert_ne!(card.0, base.0);
        assert_eq!(card.1, None);

        let settings = slot.input().unwrap();
        let mods = Mods::default();
        let i = settings.dropdown_index(INTERVAL).unwrap();
        let r = settings.form.control_rect(i);
        assert!(matches!(
            settings.press(r.cx(), r.cy(), mods, 1),
            Outcome::Redraw
        ));
        assert!(matches!(settings.release(r.cx(), r.cy()), Outcome::Redraw));
        assert!(settings.menu.is_open(), "a click opened the retained menu");
        assert_eq!(settings.form.focus_id(), Some(INTERVAL));
        let first = frame(&mut slot);
        assert!(first.1.is_some());
        // Focus can update the field; after the first frame the menu must also change pixels.
        clock.advance_ms(40);
        assert_ne!(frame(&mut slot).0, first.0);
        for _ in 0..10 {
            clock.advance_ms(16);
            frame(&mut slot);
        }
        let open = frame(&mut slot);
        assert_eq!(open.1, None);
        let settings = slot.input().unwrap();
        assert!(matches!(
            settings.key(&KeyName::Escape, mods, None),
            Outcome::Redraw
        ));
        assert!(!settings.menu.is_open());
        let closing = frame(&mut slot);
        assert_eq!(closing.0, open.0);
        assert!(closing.1.is_some());
        for _ in 0..12 {
            clock.advance_ms(16);
            frame(&mut slot);
        }
        assert!(!slot.input().unwrap().menu.is_alive());
        let settings = slot.input().unwrap();
        assert!(matches!(
            settings.key(&KeyName::Enter, mods, None),
            Outcome::Redraw
        ));
        assert!(settings.menu.is_open(), "keyboard opens the same menu");
        settings.key(&KeyName::Down, mods, None);
        assert!(matches!(
            settings.key(&KeyName::Enter, mods, None),
            Outcome::Changed {
                id: INTERVAL,
                value: Value::Choice(2)
            },
        ));
        assert_eq!(settings.form.value(INTERVAL), Some(Value::Choice(2)));
        assert!(!settings.menu.is_open());
        slot.hide();
        assert!(slot.input().is_none());
        assert!(frame(&mut slot).1.is_some());
        for _ in 0..12 {
            clock.advance_ms(16);
            frame(&mut slot);
        }
        assert_eq!(frame(&mut slot), base);
    }
}
