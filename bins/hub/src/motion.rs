//! Hub's animation names and window-local motion state. Shared owns the clocks and curves.
use crate::vm::{Hit, Layout, Pointer, View, Vm};
use hub_core::model::Row;
use mdshared_ui::anim::{
    Animator, AsId, Motion, PageEffect, PageTransition, Policy, Transient, document, fluent,
};
use mdshared_ui::chrome::{BAR_H, TITLE_H};
use mdshared_ui::gfx::{Painter, Rect};
use mdshared_ui::theme::Palette;
use std::cell::RefCell;
use std::time::Instant;

pub fn still() -> Animator {
    let mut anim = Animator::new();
    anim.set_policy(Policy::Never);
    anim.begin_frame(Instant::now());
    anim
}

/// The shared presence slot exposes the outgoing item for drawing, not for input. The form and
/// menu update their layout when drawn, even while they're leaving.
pub struct Overlay<T>(Transient<RefCell<T>>);

impl<T> Overlay<T> {
    pub fn new(pair: document::Pair) -> Self {
        Self(pair.transient())
    }

    pub fn show(&mut self, value: T) {
        self.0.show(RefCell::new(value));
    }

    pub fn hide(&mut self) {
        self.0.hide();
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }

    pub fn is_open(&self) -> bool {
        self.0.is_shown()
    }

    pub fn is_alive(&self) -> bool {
        self.0.alive().is_some()
    }

    pub fn input(&mut self) -> Option<&mut T> {
        self.0.current_mut().map(RefCell::get_mut)
    }

    pub fn draw(
        &mut self,
        anim: &mut Animator,
        id: impl AsId,
        draw: impl FnOnce(&mut T, f32, &mut Animator),
    ) -> bool {
        let Some((value, t)) = self.0.frame(anim, id) else {
            return false;
        };
        draw(&mut value.borrow_mut(), t, anim);
        t > 0.0
    }
}

pub struct WindowMotion {
    pub anim: Animator,
    pub page: PageTransition,
    pub due: Option<Instant>,
    pub painted: Option<Painted>,
}

/// Geometry and scroll of the outgoing base frame, separate from today's window or inputs.
pub struct Painted {
    pub region: Rect,
    pub scale: f32,
    pub scroll: [f32; 2],
    pub overlaid: bool,
}

impl Default for WindowMotion {
    fn default() -> Self {
        Self {
            anim: Animator::new(),
            page: PageTransition::new("hub-content"),
            due: None,
            painted: None,
        }
    }
}

impl WindowMotion {
    /// Capture from the painted page unless an overlay covers it, in which case restore its
    /// unoverlaid base once. A resize/scale change discards both old pixels and stale geometry.
    pub fn start_page(
        &mut self,
        p: &mut Painter,
        pal: &Palette,
        area: Rect,
        old: &Vm,
        new: &Vm,
    ) -> Rect {
        let region = page_region(area.w, area.h, new);
        if page_changed(old, new) {
            if let Some(frame) = &self.painted
                && frame.scale == p.scale
                && frame.region == region
                && self.anim.animates()
            {
                if frame.overlaid {
                    crate::view::draw_full(
                        p,
                        pal,
                        area.w,
                        area.h,
                        old,
                        frame.scroll[0],
                        frame.scroll[1],
                    );
                }
                self.page
                    .begin(&mut self.anim, p, region, PageEffect::Crossfade);
            } else {
                self.page.cancel(&mut self.anim);
            }
        }
        region
    }

    pub fn apply_page(&mut self, p: &mut Painter, region: Rect, layout: &mut Layout) {
        if self.page.apply(&mut self.anim, p, region) {
            // A stale picture must not accept a different action at the same coordinates.
            block_page_hits(layout, region);
        }
    }
}

/// One wake alongside the normal Hub schedule. Once consumed it must not wake again until draw.
pub fn take_due(due: &mut Option<Instant>, now: Instant) -> bool {
    if due.is_some_and(|t| t <= now) {
        *due = None;
        true
    } else {
        false
    }
}

/// Both view families share this opaque region. The command bar, notes back button and offer
/// remain live outside it; shared's PageTransition doesn't key its blends for frosted chrome.
pub fn page_region(w: f32, h: f32, vm: &Vm) -> Rect {
    let y = TITLE_H
        + BAR_H
        + if vm.banner.is_some() {
            crate::view::BANNER_H
        } else {
            0.0
        };
    let end = h - crate::view::STATUS_H;
    Rect::new(0.0, y, w, (end - y).max(0.0))
}

pub fn page_changed(old: &Vm, new: &Vm) -> bool {
    match (&old.notes, &new.notes) {
        (None, None) => old.view != new.view,
        (Some(a), Some(b)) => {
            a.id != b.id
                || a.version != b.version
                || std::mem::discriminant(&a.state) != std::mem::discriminant(&b.state)
        }
        _ => true,
    }
}

pub fn block_page_hits(layout: &mut Layout, region: Rect) {
    layout
        .hits
        .retain(|(rect, _)| crate::ui::intersect(*rect, region).is_none());
    layout.notes_bar = None;
}

#[derive(Clone, Copy)]
pub struct ButtonLook {
    pub hover: f32,
    pub pressed: bool,
}

pub fn button(anim: &mut Animator, id: impl AsId, pointer: Pointer, hit: Hit) -> ButtonLook {
    ButtonLook {
        hover: anim.toggle(id, pointer.over(hit), document::HOVER),
        pressed: pointer.down(hit),
    }
}

pub type AppKey = (&'static str, usize);

/// Production IDs are unique. The "all" demo repeats sample apps in different states.
pub fn app_key(vm: &Vm, i: usize) -> AppKey {
    let id = vm.apps[i].id;
    (id, vm.apps[..i].iter().filter(|a| a.id == id).count())
}

/// Distinguish rows by app ID, not their filtered position. A percent tick isn't a new status.
/// Actions, words and hit targets change immediately; only the new status's emphasis fades in.
pub fn status(anim: &mut Animator, id: AppKey, row: &Row) -> f32 {
    let tag = match row {
        Row::UpToDate => 0.0_f32,
        Row::Update => 1.0,
        Row::NotInstalled => 2.0,
        Row::External => 3.0,
        Row::Downloading(..) => 4.0,
        Row::Verifying => 5.0,
        Row::Waiting(_) => 6.0,
        Row::Failed(_) => 7.0,
        Row::Done(_) => 8.0,
        Row::NoInfo(_) => 9.0,
    };
    let key = ("status", id);
    let opacity = ("status-opacity", id);
    if anim.get::<f32>(key).is_some_and(|previous| previous != tag) {
        // Keep the new label readable from its first frame.
        anim.set(opacity, 0.65_f32);
    }
    anim.value(key, tag, Motion::SNAP);
    anim.value(opacity, 1.0_f32, document::HOVER)
}

/// Keep the bar's track even when invisible so an install starts from zero, not its first report.
pub fn progress(anim: &mut Animator, id: AppKey, row: &Row) -> f32 {
    let key = ("progress", id);
    let target = match row {
        Row::Downloading(f, _) if f.is_finite() => f.clamp(0.0, 1.0),
        Row::Verifying => 1.0,
        _ => {
            if anim.get::<f32>(key) != Some(0.0) || !anim.settled(key) {
                anim.set(key, 0.0_f32);
            }
            return anim.value(key, 0.0_f32, Motion::SNAP);
        }
    };
    anim.value(key, target, fluent::MOVE)
}

pub fn row_ground(anim: &mut Animator, view: View, id: AppKey, base: [u8; 3]) -> [u8; 3] {
    anim.value(("ground", view, id), base, document::HOVER)
}

#[cfg(test)]
#[path = "motion_tests.rs"]
mod tests;
