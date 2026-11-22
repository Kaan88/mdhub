//! Tests of the drawing: layout, states, panics, the picture command.
use crate::demo;
use crate::shot;
use crate::ui;
use crate::view::{self, COMPACT_W, PANE_MIN, ROW_H, row_actions, status_of};
use crate::vm::{AppVm, Hit, Layout, View, Vm};
use hub_core::model::Row;
use mdshared_text::bitmap::Bitmap;
use mdshared_ui::gfx::{Gfx, Painter, Rect};
use mdshared_ui::theme::{DARK, LIGHT, Palette};

const VIEWS: [View; 3] = [View::List, View::Large, View::Small];

fn draw_at(gfx: &mut Gfx, pal: &Palette, vm: &Vm, w: u32, h: u32, scroll: f32) -> (Layout, Bitmap) {
    let mut bmp = Bitmap::new(w, h);
    let lay = {
        let mut p = Painter::new(&mut bmp, gfx, 1.0);
        view::draw_scrolled(&mut p, pal, w as f32, h as f32, vm, scroll)
    };
    (lay, bmp)
}

fn overlaps(a: Rect, b: Rect) -> bool {
    ui::intersect(a, b).is_some()
}

fn all_rows() -> Vec<Row> {
    vec![
        Row::NotInstalled,
        Row::UpToDate,
        Row::Update,
        Row::External,
        Row::Downloading(0.5, "5 of 10 MB".into()),
        Row::Verifying,
        Row::Waiting("it is open".into()),
        Row::Failed("no connection".into()),
        Row::Done("just now".into()),
        Row::NoInfo("no hub.txt".into()),
    ]
}

fn one(row: Row, installed: bool) -> AppVm {
    let mut vm = demo::build("main", View::List).unwrap();
    let mut a = vm.apps.remove(0);
    a.row = row;
    if !installed {
        a.installed = None;
    }
    a
}

#[test]
fn every_row_state_has_words_and_an_action() {
    for pal in [LIGHT, DARK] {
        for row in all_rows() {
            let a = one(row.clone(), true);
            assert!(
                !status_of(&pal, &a, false).line.is_empty(),
                "{row:?} has no words"
            );
            assert!(
                !status_of(&pal, &a, true).line.is_empty(),
                "{row:?} has no words (compact)"
            );
            let acts = row_actions(&a);
            assert!(!acts.is_empty(), "{row:?} has no action");
            assert!(!acts[0].label.is_empty());
        }
    }
    // Not installed and no release information: no button, but the words say why.
    let a = one(Row::NoInfo("no hub.txt".into()), false);
    assert!(row_actions(&a).is_empty());
    assert_eq!(status_of(&LIGHT, &a, false).sub, "no hub.txt");
}

#[test]
fn a_failed_row_offers_try_again_and_an_update_offers_update() {
    use crate::vm::Act;
    let a = one(Row::Failed("x".into()), true);
    assert_eq!(row_actions(&a)[0].act, Act::TryAgain);
    let a = one(Row::Update, true);
    assert_eq!(row_actions(&a)[0].act, Act::Update);
    assert_eq!(row_actions(&a)[1].act, Act::Open);
    let mut a = one(Row::Update, true);
    a.running = true;
    let acts = row_actions(&a);
    assert_eq!(acts.len(), 1, "a running app is asked to close, not opened");
    assert_eq!(acts[0].act, Act::Update);
    assert!(status_of(&LIGHT, &a, false).sub.contains("Open now"));
}

#[test]
fn hit_rects_do_not_overlap_where_they_must_not() {
    let mut gfx = Gfx::new();
    for name in ["main", "updating", "all", "banner", "failed"] {
        for v in VIEWS {
            for (w, h) in [(1280, 800), (860, 600), (1000, 700), (999, 700)] {
                let vm = demo::build(name, v).unwrap();
                let (lay, _) = draw_at(&mut gfx, &LIGHT, &vm, w, h, 0.0);
                let ctx = format!("{name} {v:?} {w}x{h}");
                // The controls: everything but the bodies (Row) and the drag area.
                let ctrl: Vec<_> = lay
                    .hits
                    .iter()
                    .filter(|(_, h)| !matches!(h, Hit::Row(_) | Hit::Drag | Hit::None))
                    .collect();
                for (i, (a, ha)) in ctrl.iter().enumerate() {
                    for (b, hb) in &ctrl[i + 1..] {
                        assert!(
                            !overlaps(*a, *b),
                            "{ctx}: {ha:?} {a:?} overlaps {hb:?} {b:?}"
                        );
                    }
                }
                let drag = lay.hits.iter().find(|(_, h)| *h == Hit::Drag).unwrap().0;
                for (r, h) in &ctrl {
                    if matches!(h, Hit::Min | Hit::Max | Hit::Close) {
                        assert!(!overlaps(drag, *r), "{ctx}: drag overlaps {h:?}");
                    }
                }
                // A button or menu sits inside its own row or tile.
                for (r, h) in &ctrl {
                    let (Hit::Action(i, _) | Hit::Menu(i)) = h else {
                        continue;
                    };
                    let (body, _) = lay
                        .hits
                        .iter()
                        .find(|(_, hh)| *hh == Hit::Row(*i))
                        .unwrap_or_else(|| panic!("{ctx}: no body for {h:?}"));
                    assert!(
                        r.x >= body.x - 0.01
                            && r.right() <= body.right() + 0.01
                            && r.y >= body.y - 0.01
                            && r.bottom() <= body.bottom() + 0.01,
                        "{ctx}: {h:?} {r:?} leaves its body {body:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn every_visible_app_has_a_body_and_its_action_hit() {
    let mut gfx = Gfx::new();
    for name in ["main", "updating", "failed", "done", "noinfo", "first-run"] {
        for v in [View::List, View::Large] {
            let vm = demo::build(name, v).unwrap();
            let (lay, _) = draw_at(&mut gfx, &LIGHT, &vm, 1280, 800, 0.0);
            for (i, a) in vm.apps.iter().enumerate() {
                assert!(
                    lay.hits.iter().any(|(_, h)| *h == Hit::Row(i)),
                    "{name} {v:?}: no body for {}",
                    a.id
                );
                for b in row_actions(a) {
                    let at = lay.hits.iter().find(|(_, h)| *h == Hit::Action(i, b.act));
                    assert!(
                        at.is_some() || (v == View::Large && b.act != row_actions(a)[0].act),
                        "{name} {v:?}: {} lacks {:?}",
                        a.id,
                        b.act
                    );
                }
            }
        }
    }
}

#[test]
fn layout_at_finds_the_front_most_hit() {
    let mut gfx = Gfx::new();
    let vm = demo::build("main", View::List).unwrap();
    let (lay, _) = draw_at(&mut gfx, &LIGHT, &vm, 1280, 800, 0.0);
    // The first row's Update button is inside the first row's body: the button wins.
    let (r, _) = lay
        .hits
        .iter()
        .find(|(_, h)| matches!(h, Hit::Action(0, crate::vm::Act::Update)))
        .unwrap();
    assert_eq!(
        lay.at(r.cx(), r.cy()),
        Hit::Action(0, crate::vm::Act::Update)
    );
    assert_eq!(lay.at(300.0, r.cy()), Hit::Row(0));
    assert_eq!(lay.at(1270.0, 10.0), Hit::Close);
    assert_eq!(lay.at(500.0, 10.0), Hit::Drag);
}

#[test]
fn compact_layout_drops_columns_under_1000_and_the_pane_under_150() {
    assert!(view::cols(COMPACT_W - 1.0).released == 0.0);
    assert!(view::cols(COMPACT_W - 1.0).size == 0.0);
    assert!(view::cols(COMPACT_W).released > 0.0);
    assert!(view::cols(COMPACT_W).size > 0.0);
    let vm = demo::build("main", View::List).unwrap();
    // The list is 28 + 4 rows; the chrome is 40 + 44 + 26.
    let want = view::content_height(&vm, 1280.0);
    let chrome = 40.0 + 44.0 + 26.0;
    let fits = view::geometry(1280.0, chrome + want + PANE_MIN, &vm);
    assert!(fits.pane.h >= PANE_MIN, "{fits:?}");
    let tight = view::geometry(1280.0, chrome + want + PANE_MIN - 1.0, &vm);
    assert_eq!(tight.pane.h, 0.0, "{tight:?}");
    assert!((tight.list.h - (want + PANE_MIN - 1.0)).abs() < 0.01);
    // The pane and the list share the room, nothing is lost.
    assert!((fits.list.h + fits.pane.h - (fits.status_y - fits.list.y)).abs() < 0.01);
}

#[test]
fn the_compact_list_says_the_download_size_under_the_status() {
    let a = one(Row::Update, true);
    assert_eq!(status_of(&LIGHT, &a, true).sub, "35.2 MB download");
    assert_eq!(status_of(&LIGHT, &a, false).sub, "");
}

#[test]
fn a_running_app_is_drawn_with_a_dot() {
    let mut gfx = Gfx::new();
    let mut vm = demo::build("main", View::List).unwrap();
    vm.apps[1].running = false;
    let (_, off) = draw_at(&mut gfx, &LIGHT, &vm, 1280, 800, 0.0);
    vm.apps[1].running = true;
    let (_, on) = draw_at(&mut gfx, &LIGHT, &vm, 1280, 800, 0.0);
    assert_ne!(off.pixels, on.pixels);
    // A green pixel (the dot) exists in the row of mdview that is not there without it.
    let green = ui::green(&LIGHT);
    let has = |b: &Bitmap| {
        let y0 = (40 + 44 + 28 + 52) as usize;
        (y0..y0 + 52).any(|y| {
            (0..400).any(|x| {
                let o = (y * b.width as usize + x) * 4;
                b.pixels[o] == green[2]
                    && b.pixels[o + 1] == green[1]
                    && b.pixels[o + 2] == green[0]
            })
        })
    };
    assert!(has(&on) && !has(&off));
}

#[test]
fn scrolling_moves_the_hits_and_clamps() {
    let mut gfx = Gfx::new();
    let vm = demo::build("all", View::List).unwrap();
    let geo = view::geometry(860.0, 400.0, &vm);
    assert!(geo.max_scroll > 0.0);
    let (a, _) = draw_at(&mut gfx, &LIGHT, &vm, 860, 400, 0.0);
    let (b, _) = draw_at(&mut gfx, &LIGHT, &vm, 860, 400, ROW_H);
    let y = |l: &Layout, i| {
        l.hits
            .iter()
            .find(|(_, h)| *h == Hit::Row(i))
            .map(|(r, _)| r.y)
    };
    assert_eq!(y(&a, 1).unwrap() - y(&b, 1).unwrap(), ROW_H);
    assert!(y(&b, 0).is_none(), "the first row is scrolled out");
    // Hits stay inside the list.
    let (c, _) = draw_at(&mut gfx, &LIGHT, &vm, 860, 400, 1.0e9);
    for (r, h) in &c.hits {
        if matches!(h, Hit::Row(_) | Hit::Action(..) | Hit::Menu(_)) {
            assert!(r.y >= geo.list.y + 28.0 - 0.01 && r.bottom() <= geo.list.bottom() + 0.01);
        }
    }
    assert_eq!(c.list_content_h, geo.content_h);
    for s in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -5.0] {
        draw_at(&mut gfx, &LIGHT, &vm, 860, 400, s);
    }
}

#[test]
fn draw_never_panics_at_any_size() {
    let mut gfx = Gfx::new();
    let long = "w".repeat(1000);
    let spaced = "word ".repeat(200);
    let leak = |s: &str| -> &'static str { Box::leak(s.to_string().into_boxed_str()) };
    for name in ["main", "updating", "all", "banner", "empty", "first-run"] {
        for v in VIEWS {
            let mut vm = demo::build(name, v).unwrap();
            for a in &mut vm.apps {
                a.name = leak(&long);
                a.blurb = leak(&spaced);
                a.notes = spaced.clone();
                a.date = long.clone();
                a.size = long.clone();
                a.installed = Some(long.clone());
                a.latest = Some(long.clone());
                a.details = vec![
                    (long.clone(), long.clone()),
                    (spaced.clone(), spaced.clone()),
                ];
                a.glyph = "no such glyph";
                if let Row::Failed(w) | Row::Waiting(w) | Row::Done(w) | Row::NoInfo(w) = &mut a.row
                {
                    *w = long.clone();
                }
            }
            vm.checked = long.clone();
            vm.hub_version = long.clone();
            vm.filter = spaced.clone();
            vm.filter_focus = true;
            vm.busy = Some(long.clone());
            vm.sel = 99;
            if let Some(b) = vm.banner.as_mut() {
                b.text = long.clone();
                b.yes = long.clone();
                b.no = long.clone();
            }
            for pal in [LIGHT, DARK] {
                for (w, h) in [
                    (1, 1),
                    (1, 100),
                    (100, 1),
                    (7, 7),
                    (37, 61),
                    (139, 300),
                    (300, 139),
                    (639, 400),
                    (640, 400),
                    (999, 700),
                    (1000, 700),
                    (1280, 80),
                    (1600, 1200),
                ] {
                    draw_at(&mut gfx, &pal, &vm, w, h, 0.0);
                }
            }
        }
    }
    // Zero apps, normal strings, every size once, at 4000x3000.
    for name in ["empty", "all"] {
        for v in VIEWS {
            let vm = demo::build(name, v).unwrap();
            draw_at(&mut gfx, &DARK, &vm, 4000, 3000, 0.0);
        }
    }
    // A Vm that was never filled in.
    draw_at(&mut gfx, &LIGHT, &Vm::default(), 300, 200, 0.0);
    // Hostile numbers.
    let mut vm = demo::build("updating", View::List).unwrap();
    vm.apps[0].row = Row::Downloading(f32::NAN, String::new());
    vm.apps[1].row = Row::Downloading(f32::INFINITY, "x".into());
    vm.apps[2].row = Row::Downloading(-3.0, "x".into());
    for v in VIEWS {
        vm.view = v;
        draw_at(&mut gfx, &LIGHT, &vm, 860, 600, 0.0);
    }
}

#[test]
fn every_demo_builds_and_an_unknown_one_does_not() {
    for n in demo::NAMES {
        for v in VIEWS {
            assert!(demo::build(n, v).is_some(), "{n}");
        }
    }
    assert!(demo::build("nope", View::List).is_none());
}

#[test]
fn the_demo_set_covers_every_row_state() {
    let vm = demo::build("all", View::List).unwrap();
    let mut seen = std::collections::HashSet::new();
    for n in demo::NAMES {
        for a in demo::build(n, View::List).unwrap().apps {
            seen.insert(std::mem::discriminant(&a.row));
        }
    }
    assert_eq!(seen.len(), 10);
    assert_eq!(vm.apps.len(), 10);
}

#[test]
fn shot_arguments_parse_and_refuse_nonsense() {
    let s = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let a = shot::parse(&s(&[
        "--shot", "o.png", "--size", "860x600", "--scale", "2", "--dark", "--view", "small",
        "--demo", "failed",
    ]))
    .unwrap();
    assert_eq!(
        (
            a.out.as_str(),
            a.size,
            a.scale,
            a.dark,
            a.view,
            a.demo.as_str()
        ),
        ("o.png", (860, 600), 2.0, true, View::Small, "failed")
    );
    let d = shot::parse(&s(&["--shot", "o.png"])).unwrap();
    assert_eq!(
        (d.size, d.dark, d.view, d.demo.as_str()),
        ((1280, 800), false, View::List, "main")
    );
    assert!(shot::parse(&s(&["--shot"])).is_err());
    assert!(shot::parse(&s(&["--shot", "o.png", "--size", "0x5"])).is_err());
    assert!(shot::parse(&s(&["--shot", "o.png", "--size", "axb"])).is_err());
    assert!(shot::parse(&s(&["--shot", "o.png", "--scale", "nan"])).is_err());
    assert!(shot::parse(&s(&["--shot", "o.png", "--view", "tiny"])).is_err());
    assert!(shot::parse(&s(&["o.png"])).is_err());
}

#[test]
fn a_shot_is_written_as_a_png_at_the_scaled_size() {
    let dir = std::env::temp_dir().join(format!("hub-draw-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("a.png");
    let code = shot::run(&[
        "--shot".into(),
        out.to_string_lossy().into_owned(),
        "--size".into(),
        "400x300".into(),
        "--scale".into(),
        "2".into(),
        "--demo".into(),
        "updating".into(),
    ]);
    assert_eq!(code, 0);
    let bytes = std::fs::read(&out).unwrap();
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    // IHDR width and height.
    assert_eq!(u32::from_be_bytes(bytes[16..20].try_into().unwrap()), 800);
    assert_eq!(u32::from_be_bytes(bytes[20..24].try_into().unwrap()), 600);
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(
        shot::run(&[
            "--shot".into(),
            "x.png".into(),
            "--demo".into(),
            "nope".into()
        ]),
        2
    );
}

#[test]
fn catalog_glyph_names_resolve_to_icons() {
    for a in hub_core::catalog::CATALOG.iter() {
        assert_ne!(
            ui::glyph_path(a.glyph),
            mdshared_ui::glyph::DOC,
            "{} has no icon",
            a.glyph
        );
    }
    assert_eq!(ui::glyph_path("zzz"), mdshared_ui::glyph::DOC);
}

fn count(lay: &Layout, hit: Hit) -> usize {
    lay.hits.iter().filter(|(_, h)| *h == hit).count()
}

fn lay_of(vm: &Vm, w: u32, h: u32) -> Layout {
    draw_at(&mut Gfx::new(), &LIGHT, vm, w, h, 0.0).0
}

#[test]
fn the_command_bar_offers_what_the_state_allows() {
    let mut vm = demo::build("main", View::List).unwrap();
    vm.updates = 2;
    vm.busy = None;
    vm.checked = "Checked just now".into();
    let l = lay_of(&vm, 1280, 800);
    assert_eq!(count(&l, Hit::UpdateAll), 1);
    assert_eq!(count(&l, Hit::CheckNow), 1);
    assert_eq!(count(&l, Hit::CancelAll), 0);
    // Update all is only for when there is something to update.
    vm.updates = 0;
    assert_eq!(count(&lay_of(&vm, 1280, 800), Hit::UpdateAll), 0);
    // While an install runs: Cancel all, and neither of the other two.
    vm.updates = 2;
    vm.busy = Some("Installing".into());
    let l = lay_of(&vm, 1280, 800);
    assert_eq!(count(&l, Hit::CancelAll), 1);
    assert_eq!(count(&l, Hit::UpdateAll), 0);
    assert_eq!(count(&l, Hit::CheckNow), 0);
    // While a check runs there is nothing to press.
    vm.busy = None;
    vm.checked = "Checking\u{2026}".into();
    let l = lay_of(&vm, 1280, 800);
    assert_eq!(count(&l, Hit::CheckNow), 0);
    assert_eq!(count(&l, Hit::UpdateAll), 1);
}

#[test]
fn the_filter_box_is_there_from_900_and_the_view_switch_always() {
    let vm = demo::build("main", View::List).unwrap();
    assert_eq!(count(&lay_of(&vm, 900, 700), Hit::Filter), 1);
    assert_eq!(count(&lay_of(&vm, 899, 700), Hit::Filter), 0);
    for w in [640, 899, 1280] {
        let l = lay_of(&vm, w, 700);
        let sw: Vec<_> = l
            .hits
            .iter()
            .filter_map(|(r, h)| match h {
                Hit::ViewSwitch(v) => Some((*v, *r)),
                _ => None,
            })
            .collect();
        let views: Vec<_> = sw.iter().map(|(v, _)| *v).collect();
        assert_eq!(views, VIEWS, "width {w}");
        assert!(sw[0].1.x < sw[1].1.x && sw[1].1.x < sw[2].1.x);
        for (v, r) in &sw {
            assert_eq!(l.at(r.cx(), r.cy()), Hit::ViewSwitch(*v));
        }
    }
}

#[test]
fn the_offer_bar_has_yes_no_and_a_close_that_means_no() {
    let vm = demo::build("banner", View::List).unwrap();
    let l = lay_of(&vm, 1280, 800);
    let yes: Vec<_> = l
        .hits
        .iter()
        .filter(|(_, h)| *h == Hit::BannerYes)
        .collect();
    let no: Vec<_> = l.hits.iter().filter(|(_, h)| *h == Hit::BannerNo).collect();
    assert_eq!(yes.len(), 1);
    assert_eq!(no.len(), 2, "the No button and the X");
    assert!(!overlaps(yes[0].0, no[0].0) && !overlaps(yes[0].0, no[1].0));
    // All of them sit in the bar, under the command bar.
    for (r, _) in yes.iter().chain(no.iter()) {
        assert!(r.y >= 40.0 + 44.0 - 0.01 && r.bottom() <= 40.0 + 44.0 + 34.0 + 0.01);
    }
    let r = yes[0].0;
    assert_eq!(l.at(r.cx(), r.cy()), Hit::BannerYes);
    // No banner, no hits.
    let vm = demo::build("main", View::List).unwrap();
    let l = lay_of(&vm, 1280, 800);
    assert_eq!(count(&l, Hit::BannerYes) + count(&l, Hit::BannerNo), 0);
}

#[test]
fn every_row_has_a_menu_in_the_list_and_the_large_grid_but_not_the_small() {
    for (v, want) in [
        (View::List, true),
        (View::Large, true),
        (View::Small, false),
    ] {
        let vm = demo::build("main", v).unwrap();
        let l = lay_of(&vm, 1280, 800);
        for i in 0..vm.apps.len() {
            assert_eq!(count(&l, Hit::Menu(i)), usize::from(want), "{v:?} app {i}");
        }
    }
}

fn rows_y(l: &Layout) -> Vec<(usize, f32)> {
    l.hits
        .iter()
        .filter_map(|(r, h)| match h {
            Hit::Row(i) => Some((*i, r.y)),
            _ => None,
        })
        .collect()
}

#[test]
fn scrolled_to_the_end_the_last_row_or_tile_sits_at_the_bottom_of_the_list() {
    let mut gfx = Gfx::new();
    for v in VIEWS {
        let mut vm = demo::build("all", v).unwrap();
        vm.sel = 99; // no pane: the list gets the whole room
        let (w, h) = (860, 200);
        let geo = view::geometry(w as f32, h as f32, &vm);
        assert!(geo.max_scroll > 0.0, "{v:?}");
        let (far, _) = draw_at(&mut gfx, &LIGHT, &vm, w, h, 1.0e9);
        let (end, _) = draw_at(&mut gfx, &LIGHT, &vm, w, h, geo.max_scroll);
        assert_eq!(
            format!("{:?}", far.hits),
            format!("{:?}", end.hits),
            "{v:?}: more than the end is the end"
        );
        let last = vm.apps.len() - 1;
        let (body, _) = far
            .hits
            .iter()
            .find(|(_, h)| *h == Hit::Row(last))
            .unwrap_or_else(|| panic!("{v:?}: the last app is not reachable"));
        // The tile or row ends PAD (grid) or exactly (list) at the bottom edge.
        let slack = geo.list.bottom() - body.bottom();
        assert!((0.0..=12.01).contains(&slack), "{v:?}: slack {slack}");
        assert!(
            !far.hits.iter().any(|(_, h)| *h == Hit::Row(0)),
            "{v:?}: the first is scrolled away"
        );
        // Nothing hittable pokes out of the list, whatever the scroll.
        for s in [0.0, 37.0, geo.max_scroll / 2.0, 1.0e9] {
            let (l, _) = draw_at(&mut gfx, &LIGHT, &vm, w, h, s);
            for (r, hit) in &l.hits {
                if matches!(hit, Hit::Row(_) | Hit::Action(..) | Hit::Menu(_)) {
                    assert!(
                        r.y >= geo.list.y + 28.0 - 0.01 && r.bottom() <= geo.list.bottom() + 0.01,
                        "{v:?} scroll {s}: {hit:?} {r:?} leaves the list"
                    );
                }
            }
        }
        // A scroll that is not a number is no scroll; a negative one too.
        let (zero, _) = draw_at(&mut gfx, &LIGHT, &vm, w, h, 0.0);
        for s in [f32::NAN, f32::NEG_INFINITY, -5.0] {
            let (l, _) = draw_at(&mut gfx, &LIGHT, &vm, w, h, s);
            assert_eq!(rows_y(&l), rows_y(&zero), "{v:?}: scroll {s}");
        }
        let (l, _) = draw_at(&mut gfx, &LIGHT, &vm, w, h, f32::INFINITY);
        assert_eq!(rows_y(&l), rows_y(&zero), "{v:?}: infinite scroll is none");
    }
}

#[test]
fn grid_tiles_are_the_size_the_plan_says() {
    use crate::grid::geometry;
    let (c, w, h) = geometry(View::Large, 1280.0);
    assert_eq!((c, h), (7, 196.0));
    assert!(w >= 164.0);
    let (c, w, h) = geometry(View::Large, 860.0);
    assert_eq!((c, h), (4, 196.0));
    assert!(w >= 164.0);
    for width in [860.0, 1280.0] {
        let (c, w, h) = geometry(View::Small, width);
        assert!(c >= 7 && (width > 900.0 || c == 7), "small at {width}: {c}");
        assert_eq!(h, 104.0);
        assert!(w >= 108.0);
    }
    // Drawn: the first row of the grid holds exactly that many tiles.
    let vm = demo::build("all", View::Large).unwrap();
    let l = lay_of(&vm, 860, 700);
    let y0 = rows_y(&l)[0].1;
    assert_eq!(rows_y(&l).iter().filter(|(_, y)| *y == y0).count(), 4);
    let vm = demo::build("all", View::Small).unwrap();
    let l = lay_of(&vm, 860, 700);
    let y0 = rows_y(&l)[0].1;
    assert_eq!(rows_y(&l).iter().filter(|(_, y)| *y == y0).count(), 7);
}

#[test]
fn the_narrow_tier_drops_installed_and_latest_and_the_pane_needs_a_selection() {
    let c = view::cols(639.0);
    assert!(c.installed == 0.0 && c.latest == 0.0 && c.released == 0.0 && c.size == 0.0);
    let c = view::cols(640.0);
    assert!(c.installed > 0.0 && c.latest > 0.0 && c.released == 0.0);
    assert_eq!(PANE_MIN, 150.0, "the plan: a pane under 150 px is left out");
    let mut vm = demo::build("main", View::List).unwrap();
    let room = 40.0 + 44.0 + 26.0 + view::content_height(&vm, 1280.0) + 400.0;
    assert!(view::geometry(1280.0, room, &vm).pane.h >= 400.0);
    vm.sel = vm.apps.len();
    assert_eq!(view::geometry(1280.0, room, &vm).pane.h, 0.0);
    // The banner takes room from the list.
    let vm = demo::build("banner", View::List).unwrap();
    let plain = demo::build("main", View::List).unwrap();
    let a = view::geometry(1280.0, 800.0, &vm);
    let b = view::geometry(1280.0, 800.0, &plain);
    assert_eq!(a.list.y - b.list.y, 34.0);
}

#[test]
fn row_buttons_pane_tabs_and_arrows_follow_the_row() {
    use crate::vm::Act;
    let acts = |r: Row| {
        row_actions(&one(r, true))
            .iter()
            .map(|b| b.act)
            .collect::<Vec<_>>()
    };
    assert_eq!(acts(Row::External), [Act::Adopt, Act::Open]);
    for r in [
        Row::Downloading(0.1, String::new()),
        Row::Verifying,
        Row::Waiting(String::new()),
    ] {
        assert_eq!(acts(r), [Act::Cancel]);
    }
    assert_eq!(acts(Row::UpToDate), [Act::Open]);
    assert_eq!(
        row_actions(&one(Row::NotInstalled, false))[0].act,
        Act::Install
    );
    let tab = |r: Row| crate::panes::tab_label(&r);
    assert_eq!(tab(Row::Failed(String::new())), "What happened");
    assert_eq!(tab(Row::Verifying), "Progress");
    assert_eq!(tab(Row::Downloading(0.0, String::new())), "Progress");
    assert_eq!(tab(Row::Waiting(String::new())), "Progress");
    assert_eq!(tab(Row::NoInfo(String::new())), "Why");
    assert_eq!(tab(Row::Update), "What's new");
    assert_eq!(tab(Row::UpToDate), "What's new");
    for r in [Row::Update, Row::Verifying, Row::Failed(String::new())] {
        assert!(view::newer(&r), "{r:?}");
    }
    for r in [
        Row::UpToDate,
        Row::NotInstalled,
        Row::External,
        Row::Done(String::new()),
    ] {
        assert!(!view::newer(&r), "{r:?}");
    }
}

#[test]
fn cap_cuts_at_n_characters_not_bytes() {
    assert_eq!(ui::cap("abcdef", 3), "abc");
    assert_eq!(ui::cap("abc", 3), "abc");
    assert_eq!(ui::cap("\u{e9}\u{e9}\u{e9}", 2), "\u{e9}\u{e9}");
    assert_eq!(ui::cap(&"x".repeat(1000), 200).len(), 200);
    assert_eq!(ui::cap("abc", 0), "");
}

#[test]
fn the_list_wants_its_head_and_its_rows() {
    let vm = demo::build("main", View::List).unwrap();
    let n = vm.apps.len() as f32;
    assert_eq!(view::content_height(&vm, 1280.0), 28.0 + n * 52.0);
    assert_eq!(lay_of(&vm, 1280, 800).list_content_h, 28.0 + n * 52.0);
    let vm = demo::build("empty", View::List).unwrap();
    assert_eq!(view::content_height(&vm, 1280.0), 28.0 + 80.0);
}

#[test]
fn a_shot_of_more_than_64_million_pixels_is_refused() {
    let s = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let big = ["--shot", "o.png", "--size", "16384x16384", "--scale", "4"];
    assert!(shot::parse(&s(&big)).is_err());
    let swapped = ["--shot", "o.png", "--scale", "4", "--size", "16384x16384"];
    assert!(shot::parse(&s(&swapped)).is_err());
    assert!(shot::parse(&s(&["--shot", "o.png", "--size", "16384x16384"])).is_err());
    let fine = ["--shot", "o.png", "--size", "4000x3000", "--scale", "2"];
    assert!(shot::parse(&s(&fine)).is_ok());
    assert_eq!(shot::run(&s(&big)), 2);
}
