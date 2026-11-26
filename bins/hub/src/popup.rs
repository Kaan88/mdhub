//! The catalog row's context menu: shared placement, hit testing and motion.
use crate::glue::MenuItem;
use mdshared_ui::anim::Animator;
use mdshared_ui::gfx::{Painter, Rect};
use mdshared_ui::menu::{Entrance, Item, Look, Menu};
use mdshared_ui::theme::Palette;

pub struct Popup {
    pub id: &'static str,
    pub menu: Menu<MenuItem>,
}

impl Popup {
    pub fn new(
        id: &'static str,
        anchor: Rect,
        items: impl IntoIterator<Item = (MenuItem, &'static str, bool)>,
    ) -> Self {
        let mut look = Look::ROUNDED;
        look.row = 30.0;
        look.pad = 4.0;
        look.inset = 4.0;
        look.text = 13.0;
        let menu = Menu::new(
            items
                .into_iter()
                .map(|(id, text, enabled)| Item::new(id, text).enabled(enabled))
                .collect(),
            anchor.right(),
            anchor.bottom() + 2.0,
            true,
        )
        .with_look(look)
        .min_width(184.0);
        Self { id, menu }
    }

    pub fn draw(
        &mut self,
        p: &mut Painter,
        pal: &Palette,
        area: Rect,
        t: f32,
        anim: &mut Animator,
    ) {
        // Fade in place: a quick click still hits the same row seen throughout the transition.
        self.menu
            .draw_entering_with(p, area, t, Entrance::Fade, |p, menu| {
                menu.draw_in_animated(p, pal, area, anim, ("row-menu-hover", self.id));
            });
    }
}
