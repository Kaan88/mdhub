//! Tests of the release notes page: the Markdown subset, its layout, the scrolling, the page, and
//! hostile input (random bytes, random Markdown, deep and huge text). A child module of
//! `changelog.rs`, so it reaches what the page keeps private.
use super::*;
use mdshared_text::bitmap::Bitmap;
use mdshared_ui::gfx::Gfx;
use mdshared_ui::theme::{DARK, LIGHT};

fn with_painter<R>(w: u32, h: u32, f: impl FnOnce(&mut Painter) -> R) -> R {
    let mut bmp = Bitmap::new(w, h);
    let mut gfx = Gfx::new();
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    f(&mut p)
}

fn text_of(spans: &[Span]) -> String {
    spans.iter().map(|s| s.text.as_str()).collect()
}

fn style_of(spans: &[Span], word: &str) -> Option<Style> {
    spans
        .iter()
        .find(|s| s.text.contains(word))
        .map(|s| s.style)
}

#[test]
fn headings_are_levels_one_to_three_and_a_hash_without_a_space_is_text() {
    let b = parse("# One\n## Two ##\n### Three\n#### Four\n\n#hashtag\n####### seven");
    let levels: Vec<u8> = b
        .iter()
        .filter_map(|b| match b {
            Block::Heading(l, _) => Some(*l),
            _ => None,
        })
        .collect();
    assert_eq!(levels, vec![1, 2, 3, 3]);
    match &b[1] {
        Block::Heading(2, s) => assert_eq!(text_of(s), "Two"),
        other => panic!("{other:?}"),
    }
    assert!(matches!(&b[4], Block::Para(s) if text_of(s) == "#hashtag ####### seven"));
}

#[test]
fn bullets_numbers_and_nesting_make_items_with_a_depth() {
    let b = parse("- a\n* b\n+ c\n1. one\n2) two\n  - nested\n    - deeper\n- back");
    let items: Vec<(usize, String, String)> = b
        .iter()
        .filter_map(|b| match b {
            Block::Item {
                depth,
                marker,
                spans,
            } => Some((*depth, marker.clone(), text_of(spans))),
            _ => None,
        })
        .collect();
    let bullet = "\u{2022}".to_string();
    assert_eq!(items.len(), 8);
    assert_eq!(items[0], (0, bullet.clone(), "a".into()));
    assert_eq!(items[3], (0, "1.".into(), "one".into()));
    assert_eq!(items[4], (0, "2.".into(), "two".into()));
    assert_eq!(items[5], (1, bullet.clone(), "nested".into()));
    assert_eq!(items[6], (2, bullet.clone(), "deeper".into()));
    assert_eq!(items[7], (0, bullet, "back".into()));
}

#[test]
fn a_list_item_takes_its_continuation_lines_and_a_paragraph_joins_its_lines() {
    let b = parse("- first line\n  and its second\n\nA paragraph\nof two lines.\n\nAnother.");
    assert!(
        matches!(&b[0], Block::Item { spans, .. } if text_of(spans) == "first line and its second")
    );
    assert!(matches!(&b[1], Block::Para(s) if text_of(s) == "A paragraph of two lines."));
    assert!(matches!(&b[2], Block::Para(s) if text_of(s) == "Another."));
    assert_eq!(b.len(), 3);
}

#[test]
fn bold_italic_code_links_and_escapes() {
    let s = inline("a **bold** and *it* and _it2_ and `co de` and [text](https://x.y/z) \\*lit\\*");
    assert_eq!(style_of(&s, "bold").map(|s| s.bold), Some(true));
    assert_eq!(style_of(&s, "it").map(|s| s.italic), Some(true));
    assert_eq!(style_of(&s, "it2").map(|s| s.italic), Some(true));
    assert_eq!(style_of(&s, "co de").map(|s| s.code), Some(true));
    assert_eq!(style_of(&s, "text").map(|s| s.link), Some(true));
    let all = text_of(&s);
    assert!(
        all.contains("a bold and it and it2 and co de and text *lit*"),
        "{all}"
    );
    assert!(
        !all.contains("https"),
        "a link shows its text, not its address"
    );
    let t = inline("***both*** x");
    assert_eq!(
        style_of(&t, "both").map(|s| (s.bold, s.italic)),
        Some((true, true))
    );
}

#[test]
fn an_opener_without_a_closer_is_plain_text() {
    assert_eq!(text_of(&inline("**open")), "**open");
    assert_eq!(text_of(&inline("*open")), "*open");
    assert_eq!(text_of(&inline("`open")), "`open");
    assert_eq!(text_of(&inline("[no](close")), "[no](close");
    assert_eq!(text_of(&inline("snake_case_name")), "snake_case_name");
    assert!(inline("snake_case_name").iter().all(|s| !s.style.italic));
    assert_eq!(text_of(&inline("2 * 3 * 4")), "2 * 3 * 4");
    assert!(inline("2 * 3 * 4").iter().all(|s| !s.style.italic));
    for src in ["a_b_c", "[[[[x]]]]", "**a *b** c*", "[x] (y)"] {
        assert!(!text_of(&inline(src)).is_empty(), "{src}");
    }
}

#[test]
fn fenced_code_is_shown_as_it_is_and_an_unclosed_fence_runs_to_the_end() {
    let b = parse("text\n```rust\n# not a heading\n- not a list\n```\nafter\n~~~\nopen fence\n");
    assert!(matches!(&b[1], Block::Code(l) if l == &["# not a heading", "- not a list"]));
    assert!(matches!(&b[2], Block::Para(s) if text_of(s) == "after"));
    assert!(matches!(&b[3], Block::Code(l) if l == &["open fence"]));
}

#[test]
fn rules_and_table_rows() {
    let b = parse("---\n***\n___\n- - -\n| a | b |\n|---|---|\n| 1 | 2 |\ntext");
    assert!(b[..4].iter().all(|b| *b == Block::Rule), "{b:?}");
    assert!(matches!(&b[4], Block::Code(l) if l.len() == 3));
    assert!(matches!(&b[5], Block::Para(_)));
    // Two dashes are not a rule; they are text.
    assert!(matches!(&parse("--")[0], Block::Para(_)));
}

/// The limits hold on input made to break them.
#[test]
fn huge_and_deep_input_is_cut_not_crashed() {
    let many = "- x\n".repeat(200_000);
    assert!(parse(&many).len() <= MAX_BLOCKS);
    let long = "a".repeat(2_000_000);
    match &parse(&long)[0] {
        Block::Para(s) => assert!(text_of(s).chars().count() <= MAX_LINE),
        other => panic!("{other:?}"),
    }
    let deep: String = (0..300)
        .map(|i| format!("{}- x\n", " ".repeat(i * 2)))
        .collect();
    let blocks = parse(&deep);
    assert!(!blocks.is_empty());
    assert!(blocks.iter().all(|b| match b {
        Block::Item { depth, .. } => *depth <= MAX_DEPTH,
        _ => true,
    }));
    let nest = format!("{}x{}", "*".repeat(5000), "*".repeat(5000));
    let _ = inline(&nest);
    let brackets = "[".repeat(50_000) + &"](".repeat(50_000);
    let _ = inline(&brackets);
    let ticks = "`a".repeat(50_000);
    let _ = inline(&ticks);
    let big_code = format!("```\n{}", "line\n".repeat(100_000));
    match &parse(&big_code)[0] {
        Block::Code(l) => assert_eq!(l.len(), MAX_CODE_LINES),
        other => panic!("{other:?}"),
    }
    let bold = "**a** ".repeat(40_000);
    assert!(!inline(&bold).is_empty());
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
}

#[test]
fn random_bytes_and_random_markdown_never_panic() {
    let mut r = Rng(0x9e37_79b9_7f4a_7c15);
    for _ in 0..3_000 {
        let n = (r.next() % 600) as usize;
        let bytes: Vec<u8> = (0..n).map(|_| r.next() as u8).collect();
        let text = hub_core::notes::clean(&bytes);
        let _ = parse(&text);
    }
    let alphabet = [
        "*",
        "**",
        "_",
        "`",
        "```",
        "~~~",
        "[",
        "]",
        "(",
        ")",
        "#",
        "##",
        "-",
        "- ",
        "1. ",
        ">",
        "|",
        " ",
        "  ",
        "\n",
        "\n\n",
        "\\",
        "!",
        "a",
        "word",
        "x_y",
        "\u{e9}",
        "\u{1f600}",
        "\t",
        "---",
        "https://x",
    ];
    let mut docs: Vec<String> = Vec::new();
    for _ in 0..20_000 {
        let n = (r.next() % 120) as usize;
        let s: String = (0..n)
            .map(|_| alphabet[(r.next() % alphabet.len() as u64) as usize])
            .collect();
        let _ = parse(&s);
        if docs.len() < 150 {
            docs.push(s);
        }
    }
    // Layout too, for a sample, at widths from a sliver to wide.
    with_painter(300, 300, |p| {
        for d in &docs {
            for w in [0.0, 1.0, 60.0, 200.0, 900.0] {
                let l = layout(p, &parse(d), w);
                assert!(l.height.is_finite() && l.height >= 0.0);
            }
        }
        for w in [f32::NAN, f32::INFINITY, -5.0] {
            let _ = layout(p, &parse("# a\nb c d"), w);
        }
    });
}

#[test]
fn layout_wraps_to_the_column_and_every_construct_has_its_place() {
    with_painter(300, 300, |p| {
        let src = "# Title\n\nThe quick brown fox jumps over the lazy dog. The quick brown fox \
                   jumps over the lazy dog. The quick brown fox jumps over the lazy dog.\n\n\
                   - item\n  - nested item\n1. numbered\n\n```\ncode line\n```\n\n---\n\nend";
        let w = 260.0;
        let l = layout(p, &parse(src), w);
        // The paragraph took several lines, each inside the column.
        let mut ys: Vec<f32> = l
            .prims
            .iter()
            .filter_map(|p| match p {
                Prim::Text { y, face, size, .. } if *face == Face::Ui && *size == BODY => Some(*y),
                _ => None,
            })
            .collect();
        ys.dedup();
        assert!(ys.len() >= 5, "{ys:?}");
        for pr in &l.prims {
            if let Prim::Text {
                x,
                face,
                size,
                text,
                ..
            } = pr
                && *face != Face::Mono
            {
                let tw = p.measure(*face, *size, text);
                assert!(x + tw <= w + 2.0, "'{text}' runs to {}", x + tw);
            }
        }
        // The heading is the large semibold one; the nested item is indented more.
        assert!(
            l.prims.iter().any(
                |p| matches!(p, Prim::Text { face: Face::Semibold, size, .. } if *size > 20.0)
            )
        );
        let x_of = |t: &str| {
            l.prims.iter().find_map(|p| match p {
                Prim::Text { x, text, .. } if text.starts_with(t) => Some(*x),
                _ => None,
            })
        };
        assert!(x_of("nested item").unwrap() > x_of("item").unwrap());
        // Code: a block fill and the mono line; the rule; the height covers the last line.
        assert!(l.prims.iter().any(|p| matches!(
            p,
            Prim::Bg {
                fill: Fill::Block,
                ..
            }
        )));
        assert!(l.prims.iter().any(
            |p| matches!(p, Prim::Text { face: Face::Mono, text, .. } if text == "code line")
        ));
        assert!(l.prims.iter().any(|p| matches!(p, Prim::Rule { .. })));
        let last = l
            .prims
            .iter()
            .filter_map(|p| match p {
                Prim::Text { y, h, .. } => Some(y + h),
                _ => None,
            })
            .fold(0.0, f32::max);
        assert!(l.height >= last - 0.5, "{} < {last}", l.height);
    });
}

#[test]
fn inline_code_gets_a_fill_and_a_word_wider_than_the_column_is_cut() {
    with_painter(300, 300, |p| {
        let l = layout(p, &parse("use `cargo build` now"), 400.0);
        assert!(l.prims.iter().any(|p| matches!(
            p,
            Prim::Bg {
                fill: Fill::Inline,
                ..
            }
        )));
        assert!(l.prims.iter().any(
            |p| matches!(p, Prim::Text { face: Face::Mono, text, .. } if text == "cargo build")
        ));
        let url = "https://github.com/Kaan88/markdown-editor-rs/releases/download/v1.2.1/very-long-name.exe";
        let w = 120.0;
        let l = layout(p, &parse(url), w);
        let lines = l
            .prims
            .iter()
            .filter(|p| matches!(p, Prim::Text { .. }))
            .count();
        assert!(lines >= 3, "{lines}");
        for pr in &l.prims {
            if let Prim::Text {
                x,
                face,
                size,
                text,
                ..
            } = pr
            {
                assert!(x + p.measure(*face, *size, text) <= w + 3.0, "{text}");
            }
        }
        let joined: String = l
            .prims
            .iter()
            .filter_map(|p| match p {
                Prim::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(joined, url, "cutting a word loses nothing");
    });
}

#[test]
fn the_layout_is_kept_until_the_text_or_the_width_changes() {
    with_painter(300, 300, |p| {
        let a = laid(p, "# a\ntext", 300.0);
        let b = laid(p, "# a\ntext", 300.0);
        assert!(Rc::ptr_eq(&a, &b));
        let c = laid(p, "# a\ntext", 301.0);
        assert!(!Rc::ptr_eq(&a, &c));
        let d = laid(p, "# a\ntext!", 301.0);
        assert!(!Rc::ptr_eq(&c, &d));
    });
}

#[test]
fn scrolling_is_clamped_and_pages_move_by_the_view() {
    assert_eq!(scrolled(Nav::LineDown, 0.0, 1000.0, 500.0), LINE);
    assert_eq!(scrolled(Nav::LineUp, 10.0, 1000.0, 500.0), 0.0);
    assert_eq!(scrolled(Nav::PageDown, 0.0, 1000.0, 500.0), 460.0);
    assert_eq!(scrolled(Nav::PageDown, 900.0, 1000.0, 500.0), 1000.0);
    assert_eq!(scrolled(Nav::PageUp, 100.0, 1000.0, 500.0), 0.0);
    assert_eq!(scrolled(Nav::End, 0.0, 1000.0, 500.0), 1000.0);
    assert_eq!(scrolled(Nav::Home, 700.0, 1000.0, 500.0), 0.0);
    // Nothing to scroll: always the top. Odd numbers: never out of range.
    assert_eq!(scrolled(Nav::End, 5.0, 0.0, 500.0), 0.0);
    for cur in [f32::NAN, f32::INFINITY, -5.0, 1e30] {
        for max in [f32::NAN, f32::INFINITY, -1.0, 300.0] {
            let v = scrolled(Nav::PageDown, cur, max, f32::NAN);
            let top = if max.is_finite() { max.max(0.0) } else { 0.0 };
            assert!(v.is_finite() && v >= 0.0 && v <= top, "{cur} {max} {v}");
        }
    }
}

fn page(name: &str, w: u32, h: u32, scroll: f32, pal: &Palette) -> (Layout, Bitmap) {
    let vm = crate::demo::build(name, crate::vm::View::List).unwrap();
    let mut bmp = Bitmap::new(w, h);
    let mut gfx = Gfx::new();
    let lay = {
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        crate::view::draw_full(&mut p, pal, w as f32, h as f32, &vm, 0.0, scroll)
    };
    (lay, bmp)
}

#[test]
fn the_page_has_a_back_arrow_a_release_page_button_and_no_list() {
    for pal in [LIGHT, DARK] {
        for (w, h) in [(1280, 800), (860, 600)] {
            let (lay, _) = page("notes", w, h, 0.0, &pal);
            let has = |x: Hit| lay.hits.iter().any(|(_, h)| *h == x);
            assert!(has(Hit::NotesBack) && has(Hit::NotesPage) && has(Hit::Close));
            assert!(!has(Hit::NotesRetry), "no Try again unless it failed");
            assert!(!lay.hits.iter().any(|(_, h)| matches!(
                h,
                Hit::Row(_) | Hit::Action(..) | Hit::CheckNow | Hit::Notes(_)
            )));
            let back = lay
                .hits
                .iter()
                .find(|(_, h)| *h == Hit::NotesBack)
                .unwrap()
                .0;
            let page_btn = lay
                .hits
                .iter()
                .find(|(_, h)| *h == Hit::NotesPage)
                .unwrap()
                .0;
            assert!(back.x >= 0.0 && page_btn.right() <= w as f32);
            assert!(
                !crate::ui::intersect(back, page_btn).is_some(),
                "{w}x{h}: the buttons overlap"
            );
        }
    }
    let (lay, _) = page("notes-failed", 860, 600, 0.0, &LIGHT);
    assert!(lay.hits.iter().any(|(_, h)| *h == Hit::NotesRetry));
    let (lay, _) = page("notes-loading", 860, 600, 0.0, &LIGHT);
    assert!(!lay.hits.iter().any(|(_, h)| *h == Hit::NotesRetry));
    assert_eq!(lay.notes_max_scroll, 0.0, "a short page does not scroll");
}

#[test]
fn a_long_page_scrolls_and_the_scroll_is_clamped_in_the_drawing() {
    let (lay, top) = page("notes-long", 860, 600, 0.0, &LIGHT);
    assert!(lay.notes_max_scroll > 300.0 && lay.notes_view_h > 300.0);
    let (_, mid) = page("notes-long", 860, 600, 200.0, &LIGHT);
    assert!(top.pixels != mid.pixels, "scrolling moves the text");
    // Past the end is the end; before the start is the start; NaN is the start.
    let (_, end) = page("notes-long", 860, 600, lay.notes_max_scroll, &LIGHT);
    let (_, beyond) = page(
        "notes-long",
        860,
        600,
        lay.notes_max_scroll + 5000.0,
        &LIGHT,
    );
    assert!(end.pixels == beyond.pixels);
    assert!(end.pixels != top.pixels);
    let (_, before) = page("notes-long", 860, 600, -300.0, &LIGHT);
    assert!(before.pixels == top.pixels);
    let (_, nan) = page("notes-long", 860, 600, f32::NAN, &LIGHT);
    assert!(nan.pixels == top.pixels);
    // The short page at 1280x800 has nothing to scroll.
    let (small, _) = page("notes", 1280, 800, 0.0, &LIGHT);
    assert_eq!(small.notes_max_scroll, 0.0);
}

#[test]
fn the_scrollbar_is_hit_only_when_the_text_scrolls_and_the_thumb_is_in_front() {
    let (lay, _) = page("notes", 1280, 800, 0.0, &LIGHT);
    assert!(lay.notes_bar.is_none());
    assert!(
        !lay.hits
            .iter()
            .any(|(_, h)| matches!(h, Hit::NotesTrack | Hit::NotesThumb))
    );
    let (lay, _) = page("notes-long", 860, 600, 0.0, &LIGHT);
    let (track, thumb) = lay.notes_bar.expect("a long page has a bar");
    assert_eq!(lay.at(thumb.cx(), thumb.cy()), Hit::NotesThumb);
    assert_eq!(lay.at(track.cx(), track.bottom() - 2.0), Hit::NotesTrack);
    assert!(on_bar(&lay, thumb.cx(), thumb.cy()) && !on_bar(&lay, track.x - 20.0, thumb.cy()));
}

#[test]
fn dragging_the_thumb_moves_the_text_in_proportion_and_clamps() {
    let (lay, _) = page("notes-long", 860, 600, 0.0, &LIGHT);
    let (track, thumb) = lay.notes_bar.unwrap();
    let max = lay.notes_max_scroll;
    // Taken in its middle: the grab is where it was taken, below the thumb's top.
    let grab = match bar_press(&lay, thumb.cx(), thumb.cy(), 0.0) {
        Some(BarPress::Grab(g)) => g,
        other => panic!("{other:?}"),
    };
    assert!((grab - thumb.h / 2.0).abs() < 0.01);
    // Not moved: the text stays at the top.
    assert_eq!(bar_drag(&lay, grab, thumb.cy()), Some(0.0));
    // Half the free track down: half the text.
    let span = track.h - thumb.h;
    let half = bar_drag(&lay, grab, thumb.cy() + span / 2.0).unwrap();
    assert!((half - max / 2.0).abs() < 0.5, "{half} vs {}", max / 2.0);
    let quarter = bar_drag(&lay, grab, thumb.cy() + span / 4.0).unwrap();
    assert!((quarter - max / 4.0).abs() < 0.5);
    // The whole way and far past it: the end; far above: the top.
    assert!((bar_drag(&lay, grab, thumb.cy() + span).unwrap() - max).abs() < 0.5);
    assert_eq!(bar_drag(&lay, grab, 1e9), Some(max));
    assert_eq!(bar_drag(&lay, grab, -1e9), Some(0.0));
    // Nothing from outside panics: NaN and infinities are no move at all.
    assert_eq!(bar_drag(&lay, grab, f32::NAN), None);
    assert_eq!(bar_drag(&lay, f32::INFINITY, 10.0), None);
    // The drawn page at the dragged offset puts the thumb under the pointer.
    let (lay2, _) = page("notes-long", 860, 600, half, &LIGHT);
    let (_, t2) = lay2.notes_bar.unwrap();
    assert!((t2.y + grab - (thumb.cy() + span / 2.0)).abs() < 0.5);
    // A page that does not scroll has no bar to drag.
    let (short, _) = page("notes", 1280, 800, 0.0, &LIGHT);
    assert_eq!(bar_drag(&short, grab, 300.0), None);
    assert_eq!(bar_press(&short, 1270.0, 300.0, 0.0), None);
}

#[test]
fn a_click_on_the_track_pages_toward_it() {
    let (lay, _) = page("notes-long", 860, 600, 0.0, &LIGHT);
    let (track, thumb) = lay.notes_bar.unwrap();
    let (max, view) = (lay.notes_max_scroll, lay.notes_view_h);
    let pg = scrolled(Nav::PageDown, 0.0, max, view);
    assert!(pg > 0.0 && pg < max);
    // Below the thumb: a page down.
    let below = track.bottom() - 2.0;
    assert!(below > thumb.bottom());
    assert_eq!(
        bar_press(&lay, track.cx(), below, 0.0),
        Some(BarPress::Page(pg))
    );
    // Above the thumb (the text scrolled to its end): a page up.
    let (end, _) = page("notes-long", 860, 600, max, &LIGHT);
    let (track, thumb) = end.notes_bar.unwrap();
    let above = track.y + 2.0;
    assert!(above < thumb.y);
    let up = scrolled(Nav::PageUp, max, max, view);
    assert!(up < max);
    assert_eq!(
        bar_press(&end, track.cx(), above, max),
        Some(BarPress::Page(up))
    );
    // Paging clamps: from near the end a page down is the end.
    assert_eq!(
        bar_press(&lay, track.cx(), below, max - 1.0),
        Some(BarPress::Page(max))
    );
    // Off the bar: nothing.
    assert_eq!(bar_press(&lay, track.x - 30.0, below, 0.0), None);
}

#[test]
fn the_bar_is_drawn_wide_when_active() {
    for pal in [LIGHT, DARK] {
        let (lay, idle) = page("notes-long", 860, 600, 0.0, &pal);
        let (_, active) = page("notes-bar-active", 860, 600, 0.0, &pal);
        assert!(
            idle.pixels != active.pixels,
            "the active bar looks different"
        );
        // Only the bar's column differs.
        let (track, _) = lay.notes_bar.unwrap();
        let w = idle.width as usize;
        for (i, (a, b)) in idle
            .pixels
            .chunks(4)
            .zip(active.pixels.chunks(4))
            .enumerate()
        {
            if a != b {
                let x = (i % w) as f32;
                assert!(x >= track.x - 1.0, "pixel {x} outside the bar differs");
            }
        }
    }
}

#[test]
fn the_version_line_says_what_goes_to_what() {
    let mut n = crate::demo::build("notes", crate::vm::View::List)
        .unwrap()
        .notes
        .unwrap();
    assert_eq!(version_line(&n), "mdedit  1.2.0 \u{2192} 1.2.1");
    n.installed = Some("1.2.1".into());
    assert!(version_line(&n).contains("(installed)") && !version_line(&n).contains('\u{2192}'));
    n.installed = None;
    assert!(version_line(&n).contains("(not installed)"));
}

#[test]
fn hostile_notes_draw_without_a_panic() {
    let mut r = Rng(7);
    let mut vm = crate::demo::build("notes", crate::vm::View::List).unwrap();
    for _ in 0..40 {
        let n = (r.next() % 3000) as usize;
        let bytes: Vec<u8> = (0..n).map(|_| r.next() as u8).collect();
        if let Some(nv) = vm.notes.as_mut() {
            nv.state = NotesState::Loaded(hub_core::notes::clean(&bytes));
            nv.one_liner = "x".repeat((r.next() % 5000) as usize);
        }
        let mut bmp = Bitmap::new(400, 300);
        let mut gfx = Gfx::new();
        let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
        let sc = (r.next() % 500) as f32;
        let lay = crate::view::draw_full(&mut p, &LIGHT, 400.0, 300.0, &vm, 0.0, sc);
        assert!(lay.notes_max_scroll.is_finite());
    }
}

/// The biggest notes there can be (64 KiB of ordinary text) are laid out once, and drawn from the
/// kept layout after: the numbers are printed (`--nocapture`), the bounds are loose.
#[test]
fn a_64_kib_file_lays_out_once_and_scrolls_cheaply() {
    let mut src = String::new();
    let mut i = 0;
    while src.len() < 64 * 1024 - 200 {
        i += 1;
        src.push_str(&format!(
            "## Version 1.{i}\n\n- Faster scans of drive {i}, and a **cleaner** list of `what` is safe to remove.\n- A fix for the crash when the *folder* name has {i} unusual characters in it.\n\nA paragraph of plain words about this release that is long enough to wrap onto more than one line of the page.\n\n"
        ));
    }
    let mut bmp = Bitmap::new(860, 600);
    let mut gfx = Gfx::new();
    let mut p = Painter::new(&mut bmp, &mut gfx, 1.0);
    let t = std::time::Instant::now();
    let l = laid(&mut p, &src, 740.0);
    let first = t.elapsed();
    let t = std::time::Instant::now();
    for _ in 0..50 {
        let _ = laid(&mut p, &src, 740.0);
    }
    let kept = t.elapsed() / 50;
    let view = Rect::new(0.0, 0.0, 860.0, 480.0);
    let t = std::time::Instant::now();
    for k in 0..50 {
        draw_laid(&mut p, &LIGHT, &l, 60.0, -(k as f32) * 97.0, view);
    }
    let draw = t.elapsed() / 50;
    println!(
        "64 KiB notes: {} prims, height {:.0}; first layout {first:?}, kept lookup {kept:?}, one scroll frame {draw:?}",
        l.prims.len(),
        l.height
    );
    assert!(first < std::time::Duration::from_secs(5));
}
