//! The pane under the list: what the selected app's update brings, how an update is going, what
//! went wrong, or why there is nothing to install. One simple pane for now: the words on the
//! left, the details of the release on the right.
use crate::ui::{self, red};
use crate::view::PANE_TAB_H;
use crate::vm::{Hit, Layout, Vm};
use hub_core::model::Row;
use mdshared_ui::gfx::{Face, Painter, Rect, Rgb, mix};
use mdshared_ui::theme::Palette;

const PAD: f32 = 20.0;

/// The pane's one tab, by what the row is doing.
pub fn tab_label(row: &Row) -> &'static str {
    match row {
        Row::Failed(_) => "What happened",
        Row::Downloading(..) | Row::Verifying | Row::Waiting(_) => "Progress",
        Row::NoInfo(_) => "Why",
        _ => "What's new",
    }
}

pub fn draw(p: &mut Painter, pal: &Palette, r: Rect, vm: &Vm, lay: &mut Layout) {
    p.fill(r, pal.pane);
    let px = p.px().max(1.0);
    p.fill(Rect::new(r.x, r.y, r.w, px), pal.line);
    let Some(a) = vm.apps.get(vm.sel) else {
        return;
    };
    let old = p.clip(r);
    // The tab strip: one tab for now.
    let cy = r.y + PANE_TAB_H / 2.0 + 1.0;
    let label = tab_label(&a.row);
    let wd = p.measure(Face::Semibold, 13.0, label);
    p.text(Face::Semibold, 13.0, r.x + 16.0, cy, label, pal.text, 200.0);
    p.fill(
        Rect::new(r.x + 12.0, r.y + PANE_TAB_H - 2.0, wd + 8.0, 2.0),
        pal.accent,
    );
    p.fill(
        Rect::new(r.x, r.y + PANE_TAB_H, r.w, px),
        mix(pal.pane, pal.line, 0.7),
    );
    // The link to the full notes, beside the tab: only for a release Hub knows about.
    if a.latest.is_some() {
        let lx = r.x + 16.0 + wd + 28.0;
        let lw = p.text(
            Face::Ui,
            12.5,
            lx,
            cy,
            "Release notes",
            ui::blue(pal),
            200.0,
        );
        p.icon(
            mdshared_ui::glyph::RIGHT,
            12.0,
            lx + lw + 9.0,
            cy,
            ui::blue(pal),
        );
        // A link: underlined while the pointer is over it.
        if vm.pointer.over(Hit::Notes(vm.sel)) {
            p.fill(Rect::new(lx, cy + 8.0, lw, px), ui::blue(pal));
        }
        lay.hits.push((
            Rect::new(lx - 6.0, r.y + 2.0, lw + 34.0, PANE_TAB_H - 4.0),
            Hit::Notes(vm.sel),
        ));
    }
    let head = match (&a.installed, &a.latest) {
        (Some(i), Some(l)) if i != l => format!("{}  {} \u{2192} {}", a.name, i, l),
        (Some(i), _) => format!("{}  {}", a.name, i),
        (None, Some(l)) => format!("{}  {}", a.name, l),
        (None, None) => a.name.to_string(),
    };
    p.text_right(
        Face::Ui,
        12.5,
        r.right() - 16.0,
        cy,
        &ui::cap(&head, 120),
        pal.muted,
    );

    let body = Rect::new(
        r.x + PAD,
        r.y + PANE_TAB_H + 14.0,
        (r.w - 2.0 * PAD).max(0.0),
        (r.h - PANE_TAB_H - 22.0).max(0.0),
    );
    let split = (body.w * 0.56).floor();
    let left = Rect::new(body.x, body.y, (split - 24.0).max(0.0), body.h);
    let right = Rect::new(body.x + split, body.y, (body.w - split).max(0.0), body.h);
    let line = mix(pal.pane, pal.line, 0.7);
    p.fill(Rect::new(right.x - 12.0, body.y, px, body.h), line);
    let limit = r.bottom();

    // The left: a heading, a line under it, and the words.
    let (title, title_col, sub, text) = left_words(pal, a);
    let lc = p.clip(Rect::new(left.x, r.y + PANE_TAB_H + px, left.w + 8.0, r.h));
    p.text(
        Face::Semibold,
        14.0,
        left.x,
        left.y + 9.0,
        &ui::cap(&title, 200),
        title_col,
        left.w,
    );
    p.text(
        Face::Ui,
        12.0,
        left.x,
        left.y + 29.0,
        &ui::cap(&sub, 200),
        pal.muted,
        left.w,
    );
    let notes = if text.is_empty() {
        "No release notes."
    } else {
        text.as_str()
    };
    ui::para(
        p,
        Face::Ui,
        12.5,
        left.x,
        left.y + 46.0,
        left.w,
        limit,
        notes,
        if text.is_empty() { pal.faint } else { pal.text },
    );
    p.restore(lc);

    // The right: the details.
    let rc = p.clip(Rect::new(
        right.x,
        r.y + PANE_TAB_H + px,
        right.w + PAD,
        r.h,
    ));
    let mut y = right.y;
    for (k, v) in a.details.iter().take(12) {
        if y > limit {
            break;
        }
        y = ui::kv(p, pal, right.x, y, 92.0, right.w, limit, &ui::cap(k, 40), v);
    }
    p.restore(rc);
    p.restore(old);
}

/// Heading, the line under it, and the paragraph, by what the row is doing.
fn left_words(pal: &Palette, a: &crate::vm::AppVm) -> (String, Rgb, String, String) {
    let released = if a.date.is_empty() {
        String::new()
    } else {
        format!("Released {}", a.date)
    };
    let latest = a.latest.clone().unwrap_or_default();
    let name = a.name;
    match &a.row {
        Row::Failed(why) => (
            "Not updated".into(),
            red(pal),
            match &a.installed {
                Some(v) => format!("{name} is still at {v}; nothing was changed"),
                None => format!("{name} was not installed; nothing was changed"),
            },
            why.clone(),
        ),
        Row::Downloading(_, t) => (
            format!("Downloading {name} {latest}"),
            pal.text,
            t.clone(),
            a.notes.clone(),
        ),
        Row::Verifying => (
            format!("Checking {name} {latest}"),
            pal.text,
            "Comparing the download with the SHA-256 in the release".into(),
            a.notes.clone(),
        ),
        Row::Waiting(why) => (
            format!("Waiting to update {name}"),
            pal.text,
            why.clone(),
            a.notes.clone(),
        ),
        Row::Done(t) => (
            format!("{name} {latest}"),
            pal.text,
            format!("Updated {t}"),
            a.notes.clone(),
        ),
        Row::NoInfo(why) => (
            "No release information".into(),
            pal.text,
            format!("Hub cannot tell what is newest for {name}"),
            why.clone(),
        ),
        _ => (
            if latest.is_empty() {
                name.to_string()
            } else {
                format!("{name} {latest}")
            },
            pal.text,
            released,
            a.notes.clone(),
        ),
    }
}
