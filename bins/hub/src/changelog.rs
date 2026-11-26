//! The release notes page: a small Markdown subset read and laid out with the shared painter
//! (`mdshared-ui`'s `Painter`, `scroll`, `rows`-style arithmetic) and nothing else. No renderer
//! crate: headings (`#` to `###`), bullet and numbered lists, paragraphs, bold, italic and inline
//! code, links (shown as their text), fenced code as a mono block, rules; a table row is a mono
//! line, everything else is plain text.
//!
//! The text is hostile: `parse` is iterative (no recursion, so nesting cannot overflow the stack),
//! every size is capped, and no path indexes or unwraps on what it reads. The release build aborts
//! on a panic.
//!
//! Layout is done once per (text, width, scale) and kept (`laid`), so a scroll frame only walks
//! the laid primitives and draws the ones in view.
use crate::ui::{self, blue, red};
use crate::vm::{Hit, Layout, NotesState, NotesVm, Pointer};
use mdshared_ui::gfx::{Face, Painter, Rect, Rgb, mix};
use mdshared_ui::scroll;
use mdshared_ui::theme::Palette;
use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::rc::Rc;

// ---- the Markdown subset ----------------------------------------------------------------------

/// The most lines read, blocks kept, characters kept of a line, lines of one code block, depth of
/// a list, characters of one item or paragraph.
pub const MAX_LINES: usize = 20_000;
pub const MAX_BLOCKS: usize = 3_000;
pub const MAX_LINE: usize = 1_500;
pub const MAX_CODE_LINES: usize = 300;
pub const MAX_DEPTH: usize = 3;
pub const MAX_TEXT: usize = 4_000;

#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Style {
    pub bold: bool,
    pub italic: bool,
    pub code: bool,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Span {
    pub text: String,
    pub style: Style,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Block {
    /// Level 1 to 3.
    Heading(u8, Vec<Span>),
    Para(Vec<Span>),
    Item {
        depth: usize,
        marker: String,
        spans: Vec<Span>,
    },
    /// A fenced block, or the rows of a table: shown as they are, in the mono face.
    Code(Vec<String>),
    Rule,
}

fn cap_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

/// A line with tabs as four spaces and control characters as spaces, at most `MAX_LINE` chars.
fn tidy(line: &str) -> String {
    let mut out = String::new();
    for c in line.chars().take(MAX_LINE) {
        match c {
            '\t' => out.push_str("    "),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

fn is_rule(t: &str) -> bool {
    let mut it = t.chars().filter(|c| *c != ' ');
    let Some(first) = it.next().filter(|c| matches!(c, '-' | '*' | '_')) else {
        return false;
    };
    let mut n = 1;
    for c in it {
        if c != first {
            return false;
        }
        n += 1;
    }
    n >= 3
}

/// `- x`, `* x`, `+ x`, `12. x`, `3) x`: the marker to show and the text after it.
fn list_marker(t: &str) -> Option<(String, &str)> {
    let mut chars = t.chars();
    let first = chars.next()?;
    if matches!(first, '-' | '*' | '+') {
        let rest = chars.as_str();
        return rest.strip_prefix(' ').map(|r| ("\u{2022}".to_string(), r));
    }
    let digits = t.chars().take_while(char::is_ascii_digit).count();
    if digits == 0 || digits > 9 {
        return None;
    }
    let after = &t[digits..];
    let mut a = after.chars();
    let d = a.next()?;
    if !matches!(d, '.' | ')') {
        return None;
    }
    a.as_str()
        .strip_prefix(' ')
        .map(|r| (format!("{}.", &t[..digits]), r))
}

/// The blocks of `src`. Never panics, whatever `src` is.
pub fn parse(src: &str) -> Vec<Block> {
    let mut blocks: Vec<Block> = Vec::new();
    let mut para: Vec<String> = Vec::new();
    let mut item: Option<(usize, String, String)> = None;
    let mut indents: Vec<usize> = Vec::new();
    let mut fence: Option<(char, Vec<String>)> = None;
    let mut table: Vec<String> = Vec::new();

    fn flush(
        blocks: &mut Vec<Block>,
        para: &mut Vec<String>,
        item: &mut Option<(usize, String, String)>,
        table: &mut Vec<String>,
    ) {
        if !para.is_empty() {
            let text = cap_chars(&para.join(" "), MAX_TEXT);
            blocks.push(Block::Para(inline(&text)));
            para.clear();
        }
        if let Some((depth, marker, text)) = item.take() {
            blocks.push(Block::Item {
                depth,
                marker,
                spans: inline(&cap_chars(&text, MAX_TEXT)),
            });
        }
        if !table.is_empty() {
            blocks.push(Block::Code(std::mem::take(table)));
        }
    }

    for raw in src.lines().take(MAX_LINES) {
        if blocks.len() >= MAX_BLOCKS {
            break;
        }
        let line = tidy(raw);
        if let Some((ch, code)) = fence.as_mut() {
            let t = line.trim();
            if t.len() >= 3 && t.chars().all(|c| c == *ch) {
                if let Some((_, lines)) = fence.take() {
                    blocks.push(Block::Code(lines));
                }
            } else if code.len() < MAX_CODE_LINES {
                code.push(line);
            }
            continue;
        }
        let t = line.trim();
        let indent = line.len() - line.trim_start().len();
        if t.is_empty() {
            flush(&mut blocks, &mut para, &mut item, &mut table);
            continue;
        }
        if t.starts_with("```") || t.starts_with("~~~") {
            flush(&mut blocks, &mut para, &mut item, &mut table);
            indents.clear();
            fence = Some((t.chars().next().unwrap_or('`'), Vec::new()));
            continue;
        }
        if t.starts_with('#') {
            let n = t.chars().take_while(|c| *c == '#').count();
            let rest = &t[n..];
            if n <= 6 && (rest.is_empty() || rest.starts_with(' ')) {
                flush(&mut blocks, &mut para, &mut item, &mut table);
                indents.clear();
                let text = rest.trim().trim_end_matches('#').trim();
                blocks.push(Block::Heading(n.min(3) as u8, inline(text)));
                continue;
            }
        }
        if is_rule(t) {
            flush(&mut blocks, &mut para, &mut item, &mut table);
            indents.clear();
            blocks.push(Block::Rule);
            continue;
        }
        if t.starts_with('|') {
            // The rows so far stay; only a paragraph or an item before them is finished.
            let rows = std::mem::take(&mut table);
            flush(&mut blocks, &mut para, &mut item, &mut table);
            table = rows;
            if table.len() < MAX_CODE_LINES {
                table.push(t.to_string());
            }
            continue;
        }
        if let Some((marker, rest)) = list_marker(t) {
            flush(&mut blocks, &mut para, &mut item, &mut table);
            while indents.last().is_some_and(|top| *top > indent) {
                indents.pop();
            }
            if indents.last() != Some(&indent) && indents.len() <= MAX_DEPTH {
                indents.push(indent);
            }
            let depth = indents.len().saturating_sub(1).min(MAX_DEPTH);
            item = Some((depth, marker, rest.trim().to_string()));
            continue;
        }
        // A plain line: the continuation of an item, or a line of a paragraph.
        match item.as_mut() {
            Some((_, _, text)) => {
                if text.chars().count() < MAX_TEXT {
                    text.push(' ');
                    text.push_str(t);
                }
            }
            None => {
                if !table.is_empty() {
                    flush(&mut blocks, &mut para, &mut item, &mut table);
                }
                indents.clear();
                para.push(t.to_string());
                if para.len() > 200 {
                    flush(&mut blocks, &mut para, &mut item, &mut table);
                }
            }
        }
    }
    if let Some((_, lines)) = fence.take() {
        blocks.push(Block::Code(lines));
    }
    flush(&mut blocks, &mut para, &mut item, &mut table);
    blocks.truncate(MAX_BLOCKS);
    blocks
}

fn is_punct(c: char) -> bool {
    c.is_ascii_punctuation()
}

/// Is there a closing run of exactly `n` `ch`s after `from` that could close an emphasis?
fn has_closer(cs: &[char], from: usize, ch: char, n: usize) -> bool {
    let mut i = from;
    while i < cs.len() {
        if cs[i] == ch {
            let mut j = i;
            while j < cs.len() && cs[j] == ch {
                j += 1;
            }
            let prev_ok = i > 0 && !cs[i - 1].is_whitespace();
            let next_ok = j >= cs.len() || ch == '*' || !cs[j].is_alphanumeric();
            if j - i == n && prev_ok && next_ok {
                return true;
            }
            i = j;
        } else {
            i += 1;
        }
    }
    false
}

/// The inline spans of one run of text: `**bold**`, `*italic*` / `_italic_`, `` `code` ``,
/// `[text](url)` (shown as `text`), `\*` escapes. An opener without its closer is plain text.
pub fn inline(s: &str) -> Vec<Span> {
    let cs: Vec<char> = s.chars().take(MAX_TEXT).collect();
    let mut out: Vec<Span> = Vec::new();
    let mut buf = String::new();
    let (mut bold, mut italic) = (false, false);
    let mut i = 0;
    let emit = |out: &mut Vec<Span>, buf: &mut String, style: Style| {
        if buf.is_empty() {
            return;
        }
        let text = std::mem::take(buf);
        match out.last_mut() {
            Some(l) if l.style == style => l.text.push_str(&text),
            _ => out.push(Span { text, style }),
        }
    };
    while i < cs.len() {
        let c = cs[i];
        let cur = Style {
            bold,
            italic,
            ..Style::default()
        };
        match c {
            '\\' if cs.get(i + 1).is_some_and(|n| is_punct(*n)) => {
                buf.push(cs[i + 1]);
                i += 2;
            }
            '`' => {
                let mut n = 0;
                while cs.get(i + n) == Some(&'`') {
                    n += 1;
                }
                // The closing run: the next run of exactly `n` backticks.
                let mut j = i + n;
                let mut found = None;
                while j < cs.len() {
                    if cs[j] == '`' {
                        let mut m = 0;
                        while cs.get(j + m) == Some(&'`') {
                            m += 1;
                        }
                        if m == n {
                            found = Some(j);
                            break;
                        }
                        j += m;
                    } else {
                        j += 1;
                    }
                }
                match found {
                    Some(j) => {
                        emit(&mut out, &mut buf, cur);
                        let mut code: String = cs[i + n..j].iter().collect();
                        if code.len() > 2 && code.starts_with(' ') && code.ends_with(' ') {
                            code = code[1..code.len() - 1].to_string();
                        }
                        if !code.is_empty() {
                            let style = Style {
                                code: true,
                                ..Style::default()
                            };
                            match out.last_mut() {
                                Some(l) if l.style == style => l.text.push_str(&code),
                                _ => out.push(Span { text: code, style }),
                            }
                        }
                        i = j + n;
                    }
                    None => {
                        for _ in 0..n {
                            buf.push('`');
                        }
                        i += n;
                    }
                }
            }
            '*' | '_' => {
                let mut n = 0;
                while cs.get(i + n) == Some(&c) {
                    n += 1;
                }
                let before = if i == 0 { None } else { cs.get(i - 1).copied() };
                let after = cs.get(i + n).copied();
                // Which toggles this run is: 1 italic, 2 bold, 3 both.
                let on = match n {
                    1 => italic,
                    2 => bold,
                    3 => bold && italic,
                    _ => false,
                };
                let can_open = after.is_some_and(|a| !a.is_whitespace())
                    && (c == '*' || before.is_none_or(|b| !b.is_alphanumeric()));
                let can_close = before.is_some_and(|b| !b.is_whitespace())
                    && (c == '*' || after.is_none_or(|a| !a.is_alphanumeric()));
                let act = (1..=3).contains(&n)
                    && if on {
                        can_close
                    } else {
                        can_open && has_closer(&cs, i + n, c, n)
                    };
                if act {
                    emit(&mut out, &mut buf, cur);
                    match n {
                        1 => italic = !italic,
                        2 => bold = !bold,
                        _ => {
                            let to = !(bold && italic);
                            bold = to;
                            italic = to;
                        }
                    }
                } else {
                    for _ in 0..n {
                        buf.push(c);
                    }
                }
                i += n;
            }
            '[' | '!' => {
                // `[text](url)` and `![alt](url)`: keep the label and its surrounding style,
                // but not the URL or a link look. Nothing in the notes body is clickable.
                let start = if c == '!' && cs.get(i + 1) == Some(&'[') {
                    i + 1
                } else if c == '[' {
                    i
                } else {
                    buf.push(c);
                    i += 1;
                    continue;
                };
                let close = cs[start + 1..]
                    .iter()
                    .position(|x| *x == ']' || *x == '[')
                    .map(|p| start + 1 + p)
                    .filter(|p| cs[*p] == ']');
                let end = close.and_then(|p| {
                    if cs.get(p + 1) != Some(&'(') {
                        return None;
                    }
                    cs[p + 2..]
                        .iter()
                        .position(|x| *x == ')' || *x == '(')
                        .map(|q| p + 2 + q)
                        .filter(|q| cs[*q] == ')')
                });
                match (close, end) {
                    (Some(p), Some(q)) if p > start + 1 && p - start <= 300 => {
                        buf.extend(cs[start + 1..p].iter());
                        i = q + 1;
                    }
                    _ => {
                        buf.push(c);
                        i += 1;
                    }
                }
            }
            c if c.is_control() => {
                buf.push(' ');
                i += 1;
            }
            c => {
                buf.push(c);
                i += 1;
            }
        }
    }
    emit(
        &mut out,
        &mut buf,
        Style {
            bold,
            italic,
            ..Style::default()
        },
    );
    out
}

// ---- layout -----------------------------------------------------------------------------------

/// What colour a piece of text takes (resolved at draw time, so a theme change needs no layout).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tone {
    Text,
    Muted,
    /// Italic: the text colour pulled towards the muted one (the shared fonts have no italic).
    Soft,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fill {
    /// Behind an inline code span.
    Inline,
    /// Behind a code block.
    Block,
}

#[derive(Clone, PartialEq, Debug)]
pub enum Prim {
    Text {
        x: f32,
        /// The line's top, and its height: the text is centred in it.
        y: f32,
        h: f32,
        face: Face,
        size: f32,
        tone: Tone,
        text: String,
        /// The most width the text may take (it is elided past it).
        max: f32,
    },
    Bg {
        rect: Rect,
        fill: Fill,
    },
    Rule {
        y: f32,
    },
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct Laid {
    pub height: f32,
    /// The column's width (a rule spans it).
    pub width: f32,
    pub prims: Vec<Prim>,
}

const BODY: f32 = 13.5;
const BODY_LH: f32 = 21.0;
const CODE: f32 = 12.5;
const CODE_LH: f32 = 19.0;
const PARA_GAP: f32 = 10.0;
const ITEM_GAP: f32 = 4.0;
const INDENT: f32 = 20.0;

fn heading_metrics(level: u8) -> (f32, f32, f32, f32) {
    // size, line height, space above, space below
    match level {
        1 => (21.0, 30.0, 20.0, 8.0),
        2 => (17.0, 26.0, 18.0, 6.0),
        _ => (14.5, 22.0, 14.0, 4.0),
    }
}

/// Text widths already measured: a word is measured once per face and size.
#[derive(Default)]
struct Widths(HashMap<(u8, u32, String), f32>);

impl Widths {
    fn of(&mut self, p: &mut Painter, face: Face, size: f32, text: &str) -> f32 {
        let f = match face {
            Face::Ui => 0,
            Face::Semibold => 1,
            Face::Mono => 2,
        };
        let key = (f, size.to_bits(), text.to_string());
        if let Some(w) = self.0.get(&key) {
            return *w;
        }
        let w = p.measure(face, size, text);
        if self.0.len() > 50_000 {
            self.0.clear();
        }
        self.0.insert(key, w);
        w
    }
}

struct Piece {
    text: String,
    style: Style,
    face: Face,
    size: f32,
    w: f32,
}

struct Placed {
    piece: Piece,
    /// A space came before it on the line.
    gap: bool,
}

fn tone_of(style: Style) -> Tone {
    if !style.code && style.italic {
        Tone::Soft
    } else {
        Tone::Text
    }
}

/// Face and size of a span's style in text of `base` face and `size`.
fn look(style: Style, base: Face, size: f32) -> (Face, f32) {
    if style.code {
        (Face::Mono, size * 0.93)
    } else {
        (if style.bold { Face::Semibold } else { base }, size)
    }
}

/// Flows `spans` into lines `w` wide starting at `(x0, *y)`; `*y` moves to the bottom.
#[allow(clippy::too_many_arguments)]
fn flow(
    p: &mut Painter,
    wd: &mut Widths,
    spans: &[Span],
    x0: f32,
    w: f32,
    base: Face,
    size: f32,
    lh: f32,
    y: &mut f32,
    out: &mut Vec<Prim>,
) {
    let space = (wd.of(p, base, size, "n n") - wd.of(p, base, size, "nn")).max(2.0);
    // Tokens: runs of non-space characters, each made of pieces of one style.
    let mut tokens: Vec<(Vec<Piece>, bool)> = Vec::new();
    let mut cur: Vec<Piece> = Vec::new();
    let mut gap = false;
    for s in spans {
        let (face, sz) = look(s.style, base, size);
        let mut seg = String::new();
        let mut seg_ws = false;
        let mut chunks: Vec<(bool, String)> = Vec::new();
        for c in s.text.chars() {
            let ws = c.is_whitespace();
            if !seg.is_empty() && ws != seg_ws {
                chunks.push((seg_ws, std::mem::take(&mut seg)));
            }
            seg_ws = ws;
            seg.push(c);
        }
        if !seg.is_empty() {
            chunks.push((seg_ws, seg));
        }
        for (ws, text) in chunks {
            if ws {
                if !cur.is_empty() {
                    tokens.push((std::mem::take(&mut cur), gap));
                }
                gap = true;
            } else {
                let w = wd.of(p, face, sz, &text);
                cur.push(Piece {
                    text,
                    style: s.style,
                    face,
                    size: sz,
                    w,
                });
            }
        }
    }
    if !cur.is_empty() {
        tokens.push((cur, gap));
    }

    let mut line: Vec<Placed> = Vec::new();
    let mut x = 0.0f32;
    let end_line = |p_line: &mut Vec<Placed>, y: &mut f32, out: &mut Vec<Prim>| {
        emit_line(p_line, x0, *y, lh, space, out);
        p_line.clear();
        *y += lh;
    };
    for (pieces, gap) in tokens {
        let tw: f32 = pieces.iter().map(|p| p.w).sum();
        let mut sp = if x > 0.0 && gap { space } else { 0.0 };
        if x > 0.0 && x + sp + tw > w {
            end_line(&mut line, y, out);
            x = 0.0;
            sp = 0.0;
        }
        if tw <= w {
            let mut at = x + sp;
            let mut gap_here = sp > 0.0;
            for pc in pieces {
                let pw = pc.w;
                line.push(Placed {
                    piece: pc,
                    gap: gap_here,
                });
                gap_here = false;
                at += pw;
            }
            x = at;
        } else {
            // A word wider than the column (an address): cut by characters.
            let mut gap_here = sp > 0.0;
            x += sp;
            for pc in pieces {
                let mut part = String::new();
                let mut pw = 0.0;
                for ch in pc.text.chars() {
                    let cw = wd.of(p, pc.face, pc.size, &ch.to_string());
                    if x + pw + cw > w && (!part.is_empty() || x > 0.0) {
                        if !part.is_empty() {
                            line.push(Placed {
                                piece: Piece {
                                    text: std::mem::take(&mut part),
                                    style: pc.style,
                                    face: pc.face,
                                    size: pc.size,
                                    w: pw,
                                },
                                gap: gap_here,
                            });
                        }
                        end_line(&mut line, y, out);
                        x = 0.0;
                        pw = 0.0;
                        gap_here = false;
                    }
                    part.push(ch);
                    pw += cw;
                }
                if !part.is_empty() {
                    x += pw;
                    line.push(Placed {
                        piece: Piece {
                            text: part,
                            style: pc.style,
                            face: pc.face,
                            size: pc.size,
                            w: pw,
                        },
                        gap: gap_here,
                    });
                }
                gap_here = false;
            }
        }
    }
    if !line.is_empty() {
        end_line(&mut line, y, out);
    }
}

/// One finished line: neighbours of one style become one run, code spans get their fill.
fn emit_line(line: &[Placed], x0: f32, y: f32, lh: f32, space: f32, out: &mut Vec<Prim>) {
    struct Run {
        x: f32,
        w: f32,
        text: String,
        style: Style,
        face: Face,
        size: f32,
    }
    let mut runs: Vec<Run> = Vec::new();
    let mut x = 0.0f32;
    for pl in line {
        // The gap before a piece is the width the flow gave it: a space.
        let gap_w = if pl.gap { space } else { 0.0 };
        let merge = runs.last().is_some_and(|r| {
            r.style == pl.piece.style && r.face == pl.piece.face && r.size == pl.piece.size
        });
        if merge {
            if let Some(r) = runs.last_mut() {
                if pl.gap {
                    r.text.push(' ');
                }
                r.text.push_str(&pl.piece.text);
                r.w += gap_w + pl.piece.w;
            }
        } else {
            runs.push(Run {
                x: x + gap_w,
                w: pl.piece.w,
                text: pl.piece.text.clone(),
                style: pl.piece.style,
                face: pl.piece.face,
                size: pl.piece.size,
            });
        }
        x += gap_w + pl.piece.w;
    }
    for r in runs {
        if r.style.code {
            out.push(Prim::Bg {
                rect: Rect::new(x0 + r.x - 3.0, y + 2.0, r.w + 6.0, lh - 4.0),
                fill: Fill::Inline,
            });
        }
        out.push(Prim::Text {
            x: x0 + r.x,
            y,
            h: lh,
            face: r.face,
            size: r.size,
            tone: tone_of(r.style),
            text: r.text,
            max: f32::INFINITY,
        });
    }
}

/// Lays `blocks` out in a column `width` wide.
pub fn layout(p: &mut Painter, blocks: &[Block], width: f32) -> Laid {
    let width = if width.is_finite() {
        width.max(60.0)
    } else {
        60.0
    };
    let mut wd = Widths::default();
    let mut out: Vec<Prim> = Vec::new();
    let mut y = 0.0f32;
    let mut prev_item = false;
    for (n, b) in blocks.iter().enumerate() {
        let is_item = matches!(b, Block::Item { .. });
        // A list ends with a little more room than its items have between them.
        if prev_item && !is_item && !matches!(b, Block::Heading(..)) {
            y += PARA_GAP - ITEM_GAP;
        }
        match b {
            Block::Heading(level, spans) => {
                let (size, lh, above, below) = heading_metrics(*level);
                if n > 0 {
                    y += above;
                }
                flow(
                    p,
                    &mut wd,
                    spans,
                    0.0,
                    width,
                    Face::Semibold,
                    size,
                    lh,
                    &mut y,
                    &mut out,
                );
                y += below;
            }
            Block::Para(spans) => {
                flow(
                    p,
                    &mut wd,
                    spans,
                    0.0,
                    width,
                    Face::Ui,
                    BODY,
                    BODY_LH,
                    &mut y,
                    &mut out,
                );
                y += PARA_GAP;
            }
            Block::Item {
                depth,
                marker,
                spans,
            } => {
                let x = *depth as f32 * INDENT;
                let tx = x + INDENT;
                out.push(Prim::Text {
                    x: x + 2.0,
                    y,
                    h: BODY_LH,
                    face: Face::Ui,
                    size: BODY,
                    tone: Tone::Muted,
                    text: marker.clone(),
                    max: INDENT,
                });
                flow(
                    p,
                    &mut wd,
                    spans,
                    tx,
                    (width - tx).max(40.0),
                    Face::Ui,
                    BODY,
                    BODY_LH,
                    &mut y,
                    &mut out,
                );
                y += ITEM_GAP;
            }
            Block::Code(lines) => {
                let h = lines.len().max(1) as f32 * CODE_LH + 12.0;
                out.push(Prim::Bg {
                    rect: Rect::new(0.0, y, width, h),
                    fill: Fill::Block,
                });
                let mut ly = y + 6.0;
                for l in lines {
                    if !l.trim().is_empty() {
                        out.push(Prim::Text {
                            x: 10.0,
                            y: ly,
                            h: CODE_LH,
                            face: Face::Mono,
                            size: CODE,
                            tone: Tone::Text,
                            text: l.clone(),
                            max: (width - 20.0).max(1.0),
                        });
                    }
                    ly += CODE_LH;
                }
                y += h + PARA_GAP;
            }
            Block::Rule => {
                out.push(Prim::Rule { y: y + 8.0 });
                y += 16.0 + PARA_GAP;
            }
        }
        prev_item = is_item;
    }
    if prev_item {
        y += PARA_GAP - ITEM_GAP;
    }
    Laid {
        height: (y - if blocks.is_empty() { 0.0 } else { PARA_GAP }).max(0.0),
        width,
        prims: out,
    }
}

type LaidKey = (u64, usize, u32, u32);

thread_local! {
    static CACHE: RefCell<Option<(LaidKey, Rc<Laid>)>> = const { RefCell::new(None) };
}

/// The layout of `src` at `width`, made again only when the text, the width or the scale changes.
pub fn laid(p: &mut Painter, src: &str, width: f32) -> Rc<Laid> {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    src.hash(&mut h);
    let key = (h.finish(), src.len(), width.to_bits(), p.px().to_bits());
    if let Some(l) = CACHE.with(|c| {
        c.borrow()
            .as_ref()
            .filter(|(k, _)| *k == key)
            .map(|(_, l)| l.clone())
    }) {
        return l;
    }
    let l = Rc::new(layout(p, &parse(src), width));
    CACHE.with(|c| *c.borrow_mut() = Some((key, l.clone())));
    l
}

fn tone_color(pal: &Palette, t: Tone) -> Rgb {
    match t {
        Tone::Text => pal.text,
        Tone::Muted => pal.muted,
        Tone::Soft => mix(pal.text, pal.muted, 0.45),
    }
}

/// Draws the primitives that fall inside `view`, the column's top-left being `(x, y)`.
fn draw_laid(p: &mut Painter, pal: &Palette, l: &Laid, x: f32, y: f32, view: Rect) {
    let px = p.px().max(1.0);
    for prim in &l.prims {
        match prim {
            Prim::Text {
                x: tx,
                y: ty,
                h,
                face,
                size,
                tone,
                text,
                max,
            } => {
                let top = y + ty;
                if top > view.bottom() || top + h < view.y {
                    continue;
                }
                p.text(
                    *face,
                    *size,
                    x + tx,
                    top + h / 2.0,
                    text,
                    tone_color(pal, *tone),
                    *max,
                );
            }
            Prim::Bg { rect, fill } => {
                let r = Rect::new(x + rect.x, y + rect.y, rect.w, rect.h);
                if r.y > view.bottom() || r.bottom() < view.y {
                    continue;
                }
                let c = mix(pal.page, pal.text, if pal.dark { 0.10 } else { 0.065 });
                match fill {
                    Fill::Inline => p.rbox(r, 4.0, c),
                    Fill::Block => p.rbox(r, 6.0, c),
                }
            }
            Prim::Rule { y: ry } => {
                let top = y + ry;
                if top > view.bottom() || top < view.y {
                    continue;
                }
                p.fill(Rect::new(x, top, l.width, px), pal.line);
            }
        }
    }
}

// ---- the page ---------------------------------------------------------------------------------

pub const HEADER_H: f32 = 52.0;
pub const FOOTER_H: f32 = 42.0;
/// The column the notes are set in, at most this wide.
pub const COLUMN_MAX: f32 = 740.0;
const SIDE: f32 = 28.0;
const TOP: f32 = 20.0;
const BOTTOM: f32 = 28.0;

pub const BACK_ICON: &str = "M13 8H3.2M7.6 3.4L3 8l4.6 4.6";

/// The page's three parts for a content area `r`: header, scrolling body, footer.
pub fn parts(r: Rect) -> (Rect, Rect, Rect) {
    let head = Rect::new(r.x, r.y, r.w, HEADER_H.min(r.h));
    let foot_h = FOOTER_H.min((r.h - head.h).max(0.0));
    let foot = Rect::new(r.x, r.bottom() - foot_h, r.w, foot_h);
    let body = Rect::new(r.x, head.bottom(), r.w, (foot.y - head.bottom()).max(0.0));
    (head, body, foot)
}

/// The words under the header when the full notes are not what is shown.
fn state_words(n: &NotesVm) -> Option<(&'static str, String)> {
    match &n.state {
        NotesState::Loaded(_) => None,
        NotesState::Loading => Some(("", "Loading the full release notes\u{2026}".into())),
        NotesState::Failed(why) => Some(("Could not load the full notes", why.clone())),
        NotesState::NoFile => Some((
            "",
            if n.one_liner.trim().is_empty() {
                "This release has no notes.".to_string()
            } else {
                "This release has no full notes file: the line above is all it says.".to_string()
            },
        )),
    }
}

/// "mdedit 1.2.0 \u{2192} 1.2.1", or "mdedit 1.2.1" when they are the same or nothing is installed.
pub fn version_line(n: &NotesVm) -> String {
    match &n.installed {
        Some(i) if *i != n.version => format!("{}  {} \u{2192} {}", n.name, i, n.version),
        Some(i) => format!("{}  {} (installed)", n.name, i),
        None => format!("{}  {} (not installed)", n.name, n.version),
    }
}

/// The notes page in the content area `r`. `scroll` is clamped here; the Layout gets how far the
/// text can scroll, and the page's hits.
#[allow(clippy::too_many_arguments)]
pub fn draw_page(
    p: &mut Painter,
    pal: &Palette,
    r: Rect,
    n: &NotesVm,
    scroll: f32,
    ptr: &Pointer,
    lay: &mut Layout,
    anim: &mut mdshared_ui::anim::Animator,
) {
    let look = |anim: &mut _, hit| crate::motion::button(anim, hit, *ptr, hit);
    let px = p.px().max(1.0);
    p.fill(r, pal.page);
    let (head, body, foot) = parts(r);

    // The header: back arrow, title, the version.
    p.fill(head, pal.pane);
    p.fill(Rect::new(head.x, head.bottom() - px, head.w, px), pal.line);
    let back = Rect::new(head.x + 10.0, head.y + (head.h - 32.0) / 2.0, 32.0, 32.0);
    ui::quiet_fill(p, pal, back, 6.0, look(anim, Hit::NotesBack));
    p.icon(BACK_ICON, 15.0, back.cx(), back.cy(), pal.text);
    lay.hits.push((back, Hit::NotesBack));
    let tx = back.right() + 12.0;
    let old = p.clip(head);
    let title = format!("Release notes of {}", n.name);
    let tw = p.text(
        Face::Semibold,
        15.0,
        tx,
        head.cy(),
        &ui::cap(&title, 80),
        pal.text,
        (head.w - tx - 120.0).max(0.0),
    );
    p.text(
        Face::Mono,
        13.0,
        tx + tw + 14.0,
        head.cy(),
        &ui::cap(&n.version, 40),
        blue(pal),
        (head.w - tx - tw - 28.0).max(0.0),
    );
    p.restore(old);

    // The footer: the installed version line, and the button for the GitHub release.
    p.fill(foot, pal.bar);
    p.fill(Rect::new(foot.x, foot.y, foot.w, px), pal.line);
    let fy = foot.cy();
    let old = p.clip(foot);
    let page = ui::sbtn_r(
        p,
        pal,
        foot.right() - 14.0,
        fy,
        "View on GitHub",
        false,
        look(anim, Hit::NotesPage),
    );
    lay.hits.push((page, Hit::NotesPage));
    let mut line = version_line(n);
    if !n.date.is_empty() {
        line.push_str(&format!("   \u{b7}   Released {}", ui::cap(&n.date, 20)));
    }
    p.text(
        Face::Ui,
        12.5,
        foot.x + 20.0,
        fy,
        &ui::cap(&line, 160),
        pal.muted,
        (page.x - foot.x - 36.0).max(0.0),
    );
    p.restore(old);

    // The body.
    let colw = (body.w - 2.0 * SIDE).clamp(60.0, COLUMN_MAX);
    let cx = body.x + ((body.w - colw) / 2.0).max(0.0);
    let one = ui::cap(&n.one_liner, MAX_TEXT);
    let doc_src: &str = match &n.state {
        NotesState::Loaded(t) => t.as_str(),
        _ => one.as_str(),
    };
    let doc = laid(p, doc_src, colw);
    let words = state_words(n);
    // The words are laid out here, under the text: a heading, then wrapped lines.
    let mut extra: Vec<(Face, f32, Rgb, String, f32)> = Vec::new();
    let mut extra_h = 0.0;
    let gap = if doc.height > 0.0 { 18.0 } else { 0.0 };
    if let Some((head_w, text)) = &words {
        extra_h += gap;
        if !head_w.is_empty() {
            extra.push((
                Face::Semibold,
                13.5,
                red(pal),
                (*head_w).to_string(),
                extra_h,
            ));
            extra_h += 24.0;
        }
        let col = if matches!(n.state, NotesState::Failed(_)) {
            pal.text
        } else {
            pal.muted
        };
        for l in ui::wrap(p, Face::Ui, 13.0, &ui::cap(text, 600), colw) {
            extra.push((Face::Ui, 13.0, col, l, extra_h));
            extra_h += 20.0;
        }
        if matches!(n.state, NotesState::Failed(_)) {
            extra_h += 12.0 + 28.0;
        }
    }
    let content = TOP + doc.height + extra_h + BOTTOM;
    let max = (content - body.h).max(0.0);
    let scroll = if scroll.is_finite() {
        scroll.clamp(0.0, max)
    } else {
        0.0
    };
    lay.notes_max_scroll = max;
    lay.notes_view_h = body.h;
    let old = p.clip(body);
    let top = body.y + TOP - scroll;
    draw_laid(p, pal, &doc, cx, top, body);
    let ex_top = top + doc.height;
    for (face, size, col, text, dy) in &extra {
        let yy = ex_top + dy;
        if yy > body.bottom() || yy + 24.0 < body.y {
            continue;
        }
        p.text(*face, *size, cx, yy + 10.0, text, *col, colw);
    }
    if matches!(n.state, NotesState::Failed(_)) {
        let by = ex_top + extra_h - 28.0 + 14.0;
        let b = ui::sbtn(
            p,
            pal,
            cx,
            by,
            "Try again",
            true,
            look(anim, Hit::NotesRetry),
        );
        if let Some(v) = ui::intersect(b, body) {
            lay.hits.push((v, Hit::NotesRetry));
        }
    }
    p.restore(old);
    // Keep the painted thumb and its hit outside the window's resize band, even at rest.
    let bar_area = Rect {
        w: (body.w - mdshared_ui::window::EDGE).max(0.0),
        ..body
    };
    if let Some((track, thumb)) = scroll::scrollbar(bar_area, content, body.h, scroll) {
        scroll::draw_scrollbar_animated(
            p,
            pal,
            anim,
            "notes-scrollbar",
            track,
            thumb,
            n.bar_active,
        );
        // The thumb after the track: it is the front-most of the two.
        lay.hits.push((track, Hit::NotesTrack));
        lay.hits.push((thumb, Hit::NotesThumb));
        lay.notes_bar = Some((track, thumb));
    }
}

// ---- moving the text --------------------------------------------------------------------------

/// How a key moves the text.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Nav {
    LineUp,
    LineDown,
    PageUp,
    PageDown,
    Home,
    End,
}

/// One key's line, in logical px.
pub const LINE: f32 = 42.0;

/// The scroll offset after `nav`, clamped to `0..=max` (a page is the view less a little overlap).
pub fn scrolled(nav: Nav, cur: f32, max: f32, view: f32) -> f32 {
    let max = if max.is_finite() { max.max(0.0) } else { 0.0 };
    let cur = if cur.is_finite() { cur } else { 0.0 };
    let page = (view - 40.0).max(LINE);
    let next = match nav {
        Nav::LineUp => cur - LINE,
        Nav::LineDown => cur + LINE,
        Nav::PageUp => cur - page,
        Nav::PageDown => cur + page,
        Nav::Home => 0.0,
        Nav::End => max,
    };
    next.clamp(0.0, max)
}

// ---- the scrollbar under the pointer -----------------------------------------------------------

/// What a press on the notes page's scrollbar does.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum BarPress {
    /// The thumb was taken this far below its top: the drag keeps it there under the pointer.
    Grab(f32),
    /// The track was clicked off the thumb: the text is now at this offset, a page toward it.
    Page(f32),
}

/// A press at (`x`, `y`) on the page last drawn into `lay`, the text at `cur`: what it does to the
/// scrollbar, or None when it is not on it (or the text cannot scroll).
pub fn bar_press(lay: &Layout, x: f32, y: f32, cur: f32) -> Option<BarPress> {
    let (_, thumb) = lay.notes_bar?;
    if !y.is_finite() {
        return None;
    }
    match lay.at(x, y) {
        Hit::NotesThumb => Some(BarPress::Grab(y - thumb.y)),
        Hit::NotesTrack => {
            let nav = if y < thumb.y {
                Nav::PageUp
            } else if y >= thumb.bottom() {
                Nav::PageDown
            } else {
                return None;
            };
            Some(BarPress::Page(scrolled(
                nav,
                cur,
                lay.notes_max_scroll,
                lay.notes_view_h,
            )))
        }
        _ => None,
    }
}

/// The text's offset while the thumb, taken `grab` below its top, is under a pointer at `y`:
/// proportional to where the thumb is on its track, clamped to `0..=max`. None when there is no bar
/// (the page closed or stopped scrolling) or the numbers are not finite.
pub fn bar_drag(lay: &Layout, grab: f32, y: f32) -> Option<f32> {
    let (track, thumb) = lay.notes_bar?;
    let to = y - grab;
    let (max, view) = (lay.notes_max_scroll, lay.notes_view_h);
    if !to.is_finite() || !max.is_finite() || !view.is_finite() {
        return None;
    }
    let max = max.max(0.0);
    let at = scroll::offset_for_thumb(track, thumb.h, to, max + view, view);
    Some(if at.is_finite() {
        at.clamp(0.0, max)
    } else {
        0.0
    })
}

/// Is the resolved pointer target on the scrollbar (track or thumb)?
pub fn bar_hit(hit: Hit) -> bool {
    matches!(hit, Hit::NotesTrack | Hit::NotesThumb)
}

#[cfg(test)]
#[path = "changelog_tests.rs"]
mod tests;
