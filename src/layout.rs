//! Layout: wraps the parsed document to a terminal width and measures every
//! block, so the renderer and the mouse hit-testing share one coordinate system.

use std::collections::HashMap;
use std::path::PathBuf;

use image::DynamicImage;
use ratatui::layout::Size;
use ratatui::style::{Color, Style};
use ratatui::text::Span;
use ratatui_image::Resize;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::Protocol;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use crate::doc::{Align, Block, Doc, RawLine, Seg, TableData};

/// A clickable region on a wrapped line. Columns are relative to the line start.
#[derive(Clone)]
pub struct LinkSpan {
    pub start: u16,
    pub end: u16,
    pub url: String,
}

/// A single rendered line with its link regions.
#[derive(Clone)]
pub struct DocLine {
    pub spans: Vec<Span<'static>>,
    pub links: Vec<LinkSpan>,
    pub bg: Option<Color>,
}

pub enum LaidBlock {
    Text(Vec<DocLine>),
    Image { proto: Protocol },
}

pub struct Layout {
    pub width: u16,
    pub blocks: Vec<LaidBlock>,
    pub heights: Vec<usize>,
    pub total: usize,
}

impl Layout {
    pub fn empty() -> Self {
        Self {
            width: 0,
            blocks: Vec::new(),
            heights: Vec::new(),
            total: 0,
        }
    }

    /// Build a layout for `doc` at `width`. Image cache is keyed by src string.
    pub fn build(
        doc: &Doc,
        width: u16,
        picker: Option<&Picker>,
        cache: &mut HashMap<String, Option<DynamicImage>>,
    ) -> Layout {
        let width = width.max(8);
        let mut blocks: Vec<LaidBlock> = Vec::new();
        let mut heights: Vec<usize> = Vec::new();

        let push_gap = |blocks: &mut Vec<LaidBlock>, heights: &mut Vec<usize>, n: u16| {
            if n > 0 {
                blocks.push(LaidBlock::Text(vec![
                    DocLine {
                        spans: Vec::new(),
                        links: Vec::new(),
                        bg: None
                    };
                    n as usize
                ]));
                heights.push(n as usize);
            }
        };

        for (i, block) in doc.blocks.iter().enumerate() {
            let gap = if i == 0 { 0 } else { 1 };
            match block {
                Block::Lines(rb) => {
                    push_gap(&mut blocks, &mut heights, rb.gap_before.min(gap));
                    let mut lines = Vec::new();
                    for rl in &rb.lines {
                        lines.extend(wrap_line(rl, width));
                    }
                    heights.push(lines.len());
                    blocks.push(LaidBlock::Text(lines));
                }
                Block::Table(t) => {
                    push_gap(&mut blocks, &mut heights, gap);
                    let lines = build_table(t, width);
                    heights.push(lines.len());
                    blocks.push(LaidBlock::Text(lines));
                }
                Block::Image { src, alt } => {
                    push_gap(&mut blocks, &mut heights, gap);
                    match image_block(src, alt, width, picker, cache, &doc.base_dir) {
                        Some(b @ LaidBlock::Image { .. }) => {
                            let h = match &b {
                                LaidBlock::Image { proto } => proto.size().height as usize,
                                _ => unreachable!(),
                            };
                            heights.push(h);
                            blocks.push(b);
                        }
                        _ => {
                            let lines = placeholder_lines(alt, src, width);
                            heights.push(lines.len());
                            blocks.push(LaidBlock::Text(lines));
                        }
                    }
                }
            }
        }

        let total: usize = heights.iter().sum();
        Layout {
            width,
            blocks,
            heights,
            total,
        }
    }

    /// Find the link at document row `row`, column `col`, if any.
    pub fn link_at(&self, row: usize, col: u16) -> Option<&LinkSpan> {
        let mut base = 0usize;
        for (i, block) in self.blocks.iter().enumerate() {
            let h = self.heights[i];
            if row < base + h {
                if let LaidBlock::Text(lines) = block {
                    return lines
                        .get(row - base)
                        .and_then(|l| l.links.iter().find(|ls| col >= ls.start && col < ls.end));
                }
                return None;
            }
            base += h;
        }
        None
    }
}

// ---------------------------------------------------------------------------
// Wrapping
// ---------------------------------------------------------------------------

struct Tok {
    /// (style, link, text) fragments of this token
    parts: Vec<(Style, Option<String>, String)>,
    w: usize,
    space: bool,
}

/// Split styled segs into word/space tokens, merging fragments that belong to
/// the same word across style boundaries.
fn tokenize(segs: &[Seg]) -> Vec<Tok> {
    let mut toks: Vec<Tok> = Vec::new();
    let mut open: Option<Tok> = None;

    for seg in segs {
        let mut rest: &str = &seg.text;
        while !rest.is_empty() {
            let (run, is_space) = if rest.starts_with(' ') {
                let n = rest.find(|c| c != ' ').unwrap_or(rest.len());
                (&rest[..n], true)
            } else {
                let n = rest.find(' ').unwrap_or(rest.len());
                (&rest[..n], false)
            };
            if is_space {
                if let Some(t) = open.take() {
                    toks.push(t);
                }
                if let Some(last) = toks.last_mut()
                    && last.space
                {
                    last.parts.push((seg.style, None, run.to_string()));
                    last.w += run.width();
                    rest = &rest[run.len()..];
                    continue;
                }
                toks.push(Tok {
                    parts: vec![(seg.style, None, run.to_string())],
                    w: run.width(),
                    space: true,
                });
            } else {
                match open.as_mut() {
                    Some(t) => {
                        t.parts.push((seg.style, seg.link.clone(), run.to_string()));
                        t.w += run.width();
                    }
                    None => {
                        open = Some(Tok {
                            parts: vec![(seg.style, seg.link.clone(), run.to_string())],
                            w: run.width(),
                            space: false,
                        });
                    }
                }
            }
            rest = &rest[run.len()..];
        }
    }
    if let Some(t) = open.take() {
        toks.push(t);
    }
    toks
}

/// Builds one output line, tracking link column spans while appending.
struct LineBuilder {
    spans: Vec<Span<'static>>,
    links: Vec<LinkSpan>,
    col: u16,
    open: Option<(u16, String)>,
    bg: Option<Color>,
}

impl LineBuilder {
    fn new(prefix: &str, bg: Option<Color>) -> Self {
        let spans = if prefix.is_empty() {
            Vec::new()
        } else {
            vec![Span::raw(prefix.to_string())]
        };
        Self {
            col: prefix.width() as u16,
            spans,
            links: Vec::new(),
            open: None,
            bg,
        }
    }

    fn push(&mut self, style: Style, text: &str, link: Option<&str>) {
        if text.is_empty() {
            return;
        }
        let w = text.width() as u16;
        if let Some((start, url)) = &self.open
            && link != Some(url.as_str())
        {
            let start = *start;
            let url = url.clone();
            self.open = None;
            self.links.push(LinkSpan {
                start,
                end: self.col,
                url,
            });
        }
        if let Some(url) = link
            && self.open.is_none()
        {
            self.open = Some((self.col, url.to_string()));
        }
        if let Some(last) = self.spans.last_mut()
            && last.style == style
        {
            last.content.to_mut().push_str(text);
            self.col += w;
            return;
        }
        self.spans.push(Span::styled(text.to_string(), style));
        self.col += w;
    }

    fn finish(mut self) -> DocLine {
        if let Some((start, url)) = self.open.take() {
            self.links.push(LinkSpan {
                start,
                end: self.col,
                url,
            });
        }
        DocLine {
            spans: self.spans,
            links: self.links,
            bg: self.bg,
        }
    }
}

/// Greedy word wrap of tokens with a first-line prefix and a continuation
/// prefix. Returns wrapped lines with link spans recorded per line.
fn wrap_tokens(
    toks: Vec<Tok>,
    first_prefix: &str,
    cont_prefix: &str,
    width: u16,
    bg: Option<Color>,
) -> Vec<DocLine> {
    let mut out = Vec::new();
    let mut b = LineBuilder::new(first_prefix, bg);
    let mut cur_w = b.col as usize;
    let mut avail = (width as usize).saturating_sub(cur_w);
    let mut pending: Option<Tok> = None;
    let mut has_word = false;

    for tok in toks {
        if tok.space {
            pending = Some(tok);
            continue;
        }
        let pw = pending.as_ref().map(|p| p.w).unwrap_or(0);
        if has_word && cur_w + pw + tok.w > avail {
            out.push(b.finish());
            b = LineBuilder::new(cont_prefix, bg);
            cur_w = b.col as usize;
            avail = (width as usize).saturating_sub(cur_w);
            pending = None;
            has_word = false;
        }
        if let Some(p) = pending.take()
            && (has_word || p.w <= avail)
        {
            for (st, _, t) in &p.parts {
                b.push(*st, t, None);
            }
            cur_w += p.w;
        }
        for (st, l, t) in &tok.parts {
            b.push(*st, t, l.as_deref());
        }
        cur_w += tok.w;
        has_word = true;
    }
    out.push(b.finish());
    out
}

fn wrap_line(rl: &RawLine, width: u16) -> Vec<DocLine> {
    if let Some((ch, st)) = rl.rule {
        let n = (width as usize).saturating_sub(rl.prefix.width()).max(3);
        return vec![DocLine {
            spans: vec![Span::styled(ch.to_string().repeat(n), st)],
            links: Vec::new(),
            bg: None,
        }];
    }
    if rl.segs.is_empty() {
        return vec![DocLine {
            spans: if rl.prefix.is_empty() {
                Vec::new()
            } else {
                vec![Span::raw(rl.prefix.clone())]
            },
            links: Vec::new(),
            bg: rl.bg,
        }];
    }
    if !rl.wrap {
        let mut b = LineBuilder::new(&rl.prefix, rl.bg);
        for seg in &rl.segs {
            b.push(seg.style, &seg.text, seg.link.as_deref());
        }
        return vec![b.finish()];
    }
    let toks = tokenize(&rl.segs);
    wrap_tokens(toks, &rl.prefix, &rl.cont_prefix, width, rl.bg)
}

/// Wrap bare segs (table cells) with no prefix; link columns start at 0.
fn wrap_segs(segs: &[Seg], width: u16) -> Vec<DocLine> {
    if segs.is_empty() {
        return vec![DocLine {
            spans: Vec::new(),
            links: Vec::new(),
            bg: None,
        }];
    }
    let toks = tokenize(segs);
    wrap_tokens(toks, "", "", width.max(4), None)
}

// ---------------------------------------------------------------------------
// Tables
// ---------------------------------------------------------------------------

fn segs_width(segs: &[Seg]) -> usize {
    segs.iter().map(|s| s.text.width()).sum()
}

fn spans_width(spans: &[Span<'static>]) -> usize {
    spans.iter().map(|s| s.content.width()).sum()
}

fn build_table(t: &TableData, width: u16) -> Vec<DocLine> {
    let ncols = t
        .aligns
        .len()
        .max(t.rows.iter().map(|r| r.len()).max().unwrap_or(0));
    if ncols == 0 || t.rows.is_empty() {
        return Vec::new();
    }

    // Natural column widths.
    let mut w: Vec<usize> = vec![1; ncols];
    for row in &t.rows {
        for (i, cell) in row.iter().enumerate() {
            if i < ncols {
                w[i] = w[i].max(segs_width(cell));
            }
        }
    }

    // Shrink columns if the table would be too wide.
    let borders = 4 + 3 * (ncols - 1); // "│ " + " │ " + ... + " │"
    let natural: usize = w.iter().sum::<usize>() + borders;
    if natural > width as usize {
        let avail = (width as usize).saturating_sub(borders).max(ncols * 3);
        let per = (avail / ncols).max(3);
        for wi in w.iter_mut() {
            *wi = (*wi).min(per);
        }
    }

    // Wrap all cells.
    let mut cell_lines: Vec<Vec<Vec<DocLine>>> = Vec::with_capacity(t.rows.len());
    #[allow(clippy::needless_range_loop)]
    for (r, row) in t.rows.iter().enumerate() {
        let mut cols = Vec::with_capacity(ncols);
        for i in 0..ncols {
            let mut cell = row.get(i).cloned().unwrap_or_default();
            if r == 0 {
                for s in &mut cell {
                    s.style = s.style.patch(Style::new().bold());
                }
            }
            cols.push(wrap_segs(&cell, w[i] as u16));
        }
        cell_lines.push(cols);
    }

    let border_style = Style::new().fg(Color::Indexed(241));
    let mut out: Vec<DocLine> = Vec::new();

    for (r, cols) in cell_lines.iter().enumerate() {
        let row_h = cols.iter().map(|c| c.len()).max().unwrap_or(1);
        for j in 0..row_h {
            let mut spans: Vec<Span<'static>> = vec![Span::styled("│ ", border_style)];
            let mut links: Vec<LinkSpan> = Vec::new();
            let mut col_x: u16 = 2;

            for i in 0..ncols {
                let empty = DocLine {
                    spans: Vec::new(),
                    links: Vec::new(),
                    bg: None,
                };
                let cell = cols[i].get(j).unwrap_or(&empty);
                let cw = spans_width(&cell.spans);
                let total = w[i];
                let (lp, rp) = match t.aligns.get(i).copied().unwrap_or(Align::Left) {
                    Align::Left => (0, total - cw.min(total)),
                    Align::Right => (total - cw.min(total), 0),
                    Align::Center => {
                        let rem = total - cw.min(total);
                        (rem / 2, rem - rem / 2)
                    }
                };

                if lp > 0 {
                    spans.push(Span::raw(" ".repeat(lp)));
                }
                for s in &cell.spans {
                    spans.push(s.clone());
                }
                for ls in &cell.links {
                    links.push(LinkSpan {
                        start: ls.start + col_x + lp as u16,
                        end: ls.end + col_x + lp as u16,
                        url: ls.url.clone(),
                    });
                }
                if rp > 0 {
                    spans.push(Span::raw(" ".repeat(rp)));
                }
                col_x += total as u16 + if i + 1 < ncols { 3 } else { 0 };
                if i + 1 < ncols {
                    spans.push(Span::styled(" │ ", border_style));
                }
            }
            spans.push(Span::styled(" │", border_style));
            out.push(DocLine {
                spans,
                links,
                bg: None,
            });
        }

        // Separator rule right after the header row.
        if r == 0 {
            let mut s = String::from("├─");
            #[allow(clippy::needless_range_loop)]
            for i in 0..ncols {
                s.push_str(&"─".repeat(w[i]));
                s.push_str(if i + 1 < ncols { "─┼─" } else { "─┤" });
            }
            out.push(DocLine {
                spans: vec![Span::styled(s, border_style)],
                links: Vec::new(),
                bg: None,
            });
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Images
// ---------------------------------------------------------------------------

fn image_block(
    src: &str,
    _alt: &str,
    width: u16,
    picker: Option<&Picker>,
    cache: &mut HashMap<String, Option<DynamicImage>>,
    base_dir: &std::path::Path,
) -> Option<LaidBlock> {
    let picker = picker?;
    if src.starts_with("http://") || src.starts_with("https://") {
        return None; // remote images are not fetched
    }
    let img = load_image(src, base_dir, cache)?;
    let font = picker.font_size();
    let avail = Size::new(width, 40);
    let size = Resize::Fit(None).size_for(&img, font, avail);
    Some(LaidBlock::Image {
        proto: picker.new_protocol(img, size, Resize::Fit(None)).ok()?,
    })
}

fn load_image(
    src: &str,
    base_dir: &std::path::Path,
    cache: &mut HashMap<String, Option<DynamicImage>>,
) -> Option<DynamicImage> {
    if let Some(cached) = cache.get(src) {
        return cached.clone();
    }
    let mut p = PathBuf::from(src);
    if p.is_relative() {
        p = base_dir.join(p);
    }
    let img = image::ImageReader::open(&p)
        .ok()
        .and_then(|r| r.decode().ok());
    cache.insert(src.to_string(), img.clone());
    img
}

/// Fallback display for images that could not be rendered as graphics.
fn placeholder_lines(alt: &str, src: &str, width: u16) -> Vec<DocLine> {
    let border = Style::new().fg(Color::Indexed(239));
    let text_style = Style::new().fg(Color::Indexed(245));
    let label = if alt.is_empty() { "image" } else { alt };
    let src_disp = truncate(src, 56);
    let inner = label
        .width()
        .max(src_disp.width())
        .min(width.saturating_sub(4) as usize)
        .max(6);

    let mk = |text: String, style: Style| DocLine {
        spans: vec![Span::styled(text, style)],
        links: Vec::new(),
        bg: None,
    };

    let top_fill = inner.saturating_sub(label.width() + 2);
    vec![
        mk(format!("╭─ {label} {}╮", "─".repeat(top_fill)), border),
        mk(
            format!(
                "│ {}{} │",
                src_disp,
                " ".repeat(inner.saturating_sub(src_disp.width()))
            ),
            text_style,
        ),
        mk(format!("╰{}╯", "─".repeat(inner + 2)), border),
    ]
}

fn truncate(s: &str, max: usize) -> String {
    if s.width() <= max {
        return s.to_string();
    }
    let mut out = String::new();
    let mut w = 0;
    for c in s.chars() {
        let cw = c.width().unwrap_or(0);
        if w + cw > max.saturating_sub(1) {
            break;
        }
        out.push(c);
        w += cw;
    }
    out.push('…');
    out
}
