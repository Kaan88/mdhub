//! The catalog row's context menu: shared placement, hit testing and motion.
use crate::glue::MenuItem;
use crate::vm::View;
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
        view: View,
        items: impl IntoIterator<Item = (MenuItem, &'static str, bool)>,
    ) -> Self {
        let mut look = Look::ROUNDED;
        look.row = 30.0;
        look.pad = 4.0;
        look.inset = 4.0;
        look.text = 13.0;
        let align_right = view != View::Small;
        let menu = Menu::new(
            items
                .into_iter()
                .map(|(id, text, enabled)| Item::new(id, text).enabled(enabled))
                .collect(),
            if align_right {
                anchor.right()
            } else {
                anchor.x
            },
            anchor.bottom() + 2.0,
            align_right,
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

#[cfg(test)]
mod tests {
    use super::*;
    use mdshared_text::bitmap::Bitmap;
    use mdshared_ui::gfx::Gfx;
    use mdshared_ui::theme::LIGHT;

    #[test]
    fn small_popup_aligns_left_while_list_and_large_align_right() {
        let mut bmp = Bitmap::new(860, 660);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        let area = Rect::new(0.0, 0.0, 860.0, 660.0);
        let anchor = Rect::new(412.0, 160.0, 28.0, 28.0);
        for view in [View::List, View::Large, View::Small] {
            let mut popup = Popup::new("mdedit", anchor, view, [(MenuItem::Open, "Open", true)]);
            popup.draw(&mut p, &LIGHT, area, 1.0, &mut crate::motion::still());
            let r = popup.menu.rect();
            assert_eq!(r.y, anchor.bottom() + 2.0);
            if view == View::Small {
                assert_eq!(r.x, anchor.x);
            } else {
                assert_eq!(r.right(), anchor.right());
            }
        }
    }
}
