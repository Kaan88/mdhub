use super::*;
use crate::demo;
use crate::glue::MenuItem;
use crate::popup::Popup;
use crate::vm::Act;
use mdshared_text::bitmap::Bitmap;
use mdshared_ui::anim::{Clock, ManualClock};
use mdshared_ui::gfx::Gfx;
use mdshared_ui::theme::LIGHT;

#[test]
fn hover_and_row_fill_fade_without_polling_after_they_settle() {
    let clock = ManualClock::new();
    let mut anim = Animator::new();
    let hit = Hit::Action(0, Act::Open);
    let ptr = Pointer {
        hot: hit,
        held: Hit::None,
    };
    let row = ("mdedit", 0);
    let mut draw = |pointer, color| {
        anim.begin_frame(clock.now());
        let hover = button(&mut anim, (row, Act::Open), pointer, hit).hover;
        let fill = row_ground(&mut anim, View::List, row, color);
        (hover, fill, anim.end_frame())
    };
    let quiet = [40; 3];
    let hot = [120; 3];
    assert_eq!(draw(Pointer::default(), quiet), (0.0, quiet, None));
    let start = draw(ptr, hot);
    assert_eq!((start.0, start.1), (0.0, quiet));
    assert!(start.2.is_some());
    clock.advance_ms(40);
    let middle = draw(ptr, hot);
    assert!((0.0..1.0).contains(&middle.0) && middle.0 > 0.0);
    assert!(quiet[0] < middle.1[0] && middle.1[0] < hot[0]);
    let reverse = draw(Pointer::default(), quiet);
    assert_eq!((reverse.0, reverse.1), (middle.0, middle.1));
    for _ in 0..6 {
        clock.advance_ms(16);
        draw(Pointer::default(), quiet);
    }
    assert_eq!(draw(Pointer::default(), quiet), (0.0, quiet, None));
}

#[test]
fn status_changes_and_progress_ticks_are_distinct_and_settle() {
    let clock = ManualClock::new();
    let mut anim = Animator::new();
    let key = ("mdedit", 0);
    let mut draw = |row: &Row| {
        anim.begin_frame(clock.now());
        let alpha = status(&mut anim, key, row);
        let amount = progress(&mut anim, key, row);
        (alpha, amount, anim.end_frame())
    };
    assert_eq!(draw(&Row::Update), (1.0, 0.0, None));
    let start = draw(&Row::Downloading(0.6, "first report".into()));
    assert!(start.0 < 1.0 && start.0 > 0.0);
    assert_eq!(start.1, 0.0);
    assert!(start.2.is_some());
    for _ in 0..8 {
        clock.advance_ms(16);
        draw(&Row::Downloading(0.6, "first report".into()));
    }
    let mid = draw(&Row::Downloading(0.6, "first report".into()));
    assert!(mid.1 > 0.0 && mid.1 < 0.6);
    let retarget = draw(&Row::Downloading(0.8, "second report".into()));
    assert_eq!((retarget.0, retarget.1), (1.0, mid.1));
    for _ in 0..12 {
        clock.advance_ms(16);
        draw(&Row::Downloading(0.8, "second report".into()));
    }
    assert_eq!(
        draw(&Row::Downloading(0.8, String::new())),
        (1.0, 0.8, None)
    );
    let verifying = draw(&Row::Verifying);
    assert!(verifying.0 < 1.0 && verifying.2.is_some());
    assert_eq!(verifying.1, 0.8);
    let done = draw(&Row::Done(String::new()));
    assert!(done.0 < 1.0);
    assert_eq!(done.1, 0.0);
}

#[test]
fn a_due_frame_is_consumed_once_and_never_rearms_itself() {
    let mut anim = Animator::new();
    let clock = ManualClock::new();
    let mut overlay = Overlay::<()>::new(document::MENU);
    overlay.show(());
    anim.begin_frame(clock.now());
    overlay.draw(&mut anim, "menu", |_, _, _| {});
    let mut wake = anim.end_frame();
    let t = wake.expect("entering asks for a frame");
    assert!(!take_due(&mut wake, clock.now()));
    assert!(take_due(&mut wake, t));
    assert!(!take_due(&mut wake, t));
    assert_eq!(wake, None);
}

#[test]
fn the_row_menu_draws_start_middle_end_and_exits_without_input() {
    let (mut bmp, mut gfx) = (Bitmap::new(400, 240), Gfx::new());
    let area = Rect::new(0.0, 0.0, 400.0, 240.0);
    let anchor = Rect::new(340.0, 50.0, 30.0, 30.0);
    let clock = ManualClock::new();
    let mut anim = Animator::new();
    let mut slot = Overlay::new(document::MENU);
    let mut frame = |slot: &mut Overlay<Popup>, anim: &mut Animator| {
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        p.fill(area, LIGHT.page);
        anim.begin_frame(clock.now());
        let mut t = None;
        slot.draw(anim, "row-menu", |popup, amount, anim| {
            t = Some(amount);
            popup.draw(&mut p, &LIGHT, area, amount, anim);
        });
        (t, bmp.pixels.clone(), anim.end_frame())
    };
    let base = frame(&mut slot, &mut anim);
    slot.show(Popup::new(
        "mdedit",
        anchor,
        View::List,
        [(MenuItem::Open, "Open", true)],
    ));
    let start = frame(&mut slot, &mut anim);
    assert_eq!((start.0, &start.1), (Some(0.0), &base.1));
    assert!(start.2.is_some());
    clock.advance_ms(40);
    let middle = frame(&mut slot, &mut anim);
    assert!(middle.0.is_some_and(|t| t > 0.0 && t < 1.0));
    assert_ne!(middle.1, base.1);
    for _ in 0..8 {
        clock.advance_ms(16);
        frame(&mut slot, &mut anim);
    }
    let end = frame(&mut slot, &mut anim);
    assert_eq!((end.0, end.2), (Some(1.0), None));
    assert_ne!(end.1, middle.1);
    slot.hide();
    assert!(slot.input().is_none());
    let leaving = frame(&mut slot, &mut anim);
    assert_eq!(leaving.1, end.1);
    assert!(leaving.2.is_some());
    for _ in 0..12 {
        clock.advance_ms(16);
        frame(&mut slot, &mut anim);
    }
    assert_eq!(frame(&mut slot, &mut anim), base);
    assert!(!slot.is_alive());
    slot.show(Popup::new(
        "mdedit",
        anchor,
        View::List,
        [(MenuItem::Open, "Open", true)],
    ));
    anim.set_system_reduced(true);
    let reduced = frame(&mut slot, &mut anim);
    assert_eq!((reduced.0, reduced.1, reduced.2), (Some(1.0), end.1, None));
}

#[test]
fn list_grid_and_notes_switch_without_overlay_ghosts_or_stale_action_hits() {
    let area = Rect::new(0.0, 0.0, 860.0, 600.0);
    let list = demo::build("main", View::List).unwrap();
    let grid = demo::build("main", View::Large).unwrap();
    let notes = demo::build("notes", View::List).unwrap();
    let loading = demo::build("notes-loading", View::List).unwrap();
    for (old, new) in [
        (&list, &grid),
        (&grid, &notes),
        (&notes, &list),
        (&loading, &notes),
    ] {
        let clock = ManualClock::new();
        let mut motion = WindowMotion::default();
        let (mut bmp, mut gfx) = (Bitmap::new(860, 600), Gfx::new());
        let settled = |v| crate::shot::render(v, &LIGHT, 860, 600, 1.0);
        let range =
            |region: Rect| (region.y as usize * 860 * 4)..(region.bottom() as usize * 860 * 4);
        bmp.pixels = settled(old).pixels;
        let region = page_region(area.w, area.h, old);
        motion.painted = Some(Painted {
            region,
            scale: 1.0,
            scroll: [0.0; 2],
            overlaid: true,
        });
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        p.fill(region, [255, 0, 255]);
        motion.anim.begin_frame(clock.now());
        let r = motion.start_page(&mut p, &LIGHT, area, old, new);
        let mut layout = crate::view::draw_full(&mut p, &LIGHT, area.w, area.h, new, 0.0, 0.0);
        let original_hits = layout.hits.clone();
        motion.apply_page(&mut p, r, &mut layout);
        assert_eq!(p.bmp.pixels[range(r)], settled(old).pixels[range(r)]);
        assert!(layout.hits.len() < original_hits.len());
        assert!(layout.hits.iter().any(|(_, hit)| *hit == Hit::Close));
        assert!(motion.anim.end_frame().is_some());
        for _ in 0..18 {
            clock.advance_ms(16);
            motion.anim.begin_frame(clock.now());
            layout = crate::view::draw_full(&mut p, &LIGHT, area.w, area.h, new, 0.0, 0.0);
            motion.apply_page(&mut p, r, &mut layout);
            motion.due = motion.anim.end_frame();
        }
        assert_eq!(p.bmp.pixels, settled(new).pixels);
        assert_eq!(layout.hits, original_hits);
        assert_eq!(motion.due, None);
    }
}

#[test]
fn repeated_demo_apps_do_not_share_animation_names() {
    let vm = demo::build("all", View::List).unwrap();
    let keys: std::collections::HashSet<_> = (0..vm.apps.len()).map(|i| app_key(&vm, i)).collect();
    assert_eq!(keys.len(), vm.apps.len());
}

#[test]
fn the_offer_stays_in_its_own_strip_above_release_notes() {
    let vm = demo::build("notes-offer", View::List).unwrap();
    let (mut bmp, mut gfx) = (Bitmap::new(860, 600), Gfx::new());
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    let layout = crate::view::draw(&mut p, &LIGHT, 860.0, 600.0, &vm);
    let hit_rect = |wanted| {
        layout
            .hits
            .iter()
            .find(|(_, hit)| *hit == wanted)
            .unwrap()
            .0
    };
    let yes = hit_rect(Hit::BannerYes);
    let back = hit_rect(Hit::NotesBack);
    assert!(yes.y >= TITLE_H && yes.bottom() <= TITLE_H + crate::view::BANNER_H);
    assert!(back.y >= yes.bottom());
    assert_eq!(layout.at(yes.cx(), yes.cy()), Hit::BannerYes);
}

#[test]
fn the_view_paints_caption_hover_and_scrollbar_motion_at_their_actual_bounds() {
    let (mut bmp, mut gfx) = (Bitmap::new(860, 600), Gfx::new());
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    for target in [Hit::Close, Hit::NotesThumb] {
        let mut vm = demo::build(
            if target == Hit::Close {
                "main"
            } else {
                "notes-long"
            },
            View::List,
        )
        .unwrap();
        let clock = ManualClock::new();
        let mut anim = Animator::new();
        anim.begin_frame(clock.now());
        let lay = crate::view::draw_motion(&mut p, &LIGHT, 860.0, 600.0, &vm, [0.0; 2], &mut anim);
        assert_eq!(anim.end_frame(), None);
        let rect = lay.hits.iter().find(|(_, hit)| *hit == target).unwrap().0;
        let (x0, y0, x1, y1) = p.dev_rect(rect);
        let crop = |p: &Painter| {
            (y0..y1)
                .flat_map(|y| {
                    let offset = y as usize * 860 * 4;
                    p.bmp.pixels[offset + x0 as usize * 4..offset + x1 as usize * 4]
                        .iter()
                        .copied()
                })
                .collect::<Vec<_>>()
        };
        let before = crop(&p);
        if let Some(notes) = &mut vm.notes {
            notes.bar_active = true;
        } else {
            vm.pointer.hot = target;
        }
        let mut draw = || {
            anim.begin_frame(clock.now());
            crate::view::draw_motion(&mut p, &LIGHT, 860.0, 600.0, &vm, [0.0; 2], &mut anim);
            (crop(&p), anim.end_frame())
        };
        let entering = draw();
        assert!(
            entering.0 == before && entering.1.is_some(),
            "{target:?} starts unlit"
        );
        clock.advance_ms(40);
        let mid = draw();
        assert!(mid.0 != before && mid.1.is_some(), "{target:?} fades");
        for _ in 0..6 {
            clock.advance_ms(16);
            draw();
        }
        let settled = draw();
        assert!(
            settled.0 != mid.0 && settled.1.is_none(),
            "{target:?} arrives"
        );
    }
}
