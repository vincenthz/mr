//! Markdown parsing: turns CommonMark events into a flat list of display blocks.

use std::path::{Path, PathBuf};

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Color, Modifier, Style};

// ---------------------------------------------------------------------------
// Theme
// ---------------------------------------------------------------------------

pub const LINK_STYLE: Style = Style::new()
    .fg(Color::Indexed(39))
    .add_modifier(Modifier::UNDERLINED);
pub const INLINE_CODE_STYLE: Style = Style::new().fg(Color::Indexed(214)).bg(Color::Indexed(236));
pub const CODE_STYLE: Style = Style::new().fg(Color::Indexed(250));
pub const CODE_BG: Color = Color::Indexed(234);
pub const QUOTE_TEXT_STYLE: Style = Style::new().fg(Color::Indexed(245));
pub const RULE_STYLE: Style = Style::new().fg(Color::Indexed(238));

pub fn heading_style(level: HeadingLevel) -> Style {
    match level {
        HeadingLevel::H1 => Style::new().fg(Color::White).bold(),
        HeadingLevel::H2 => Style::new().fg(Color::Indexed(81)).bold(),
        HeadingLevel::H3 => Style::new().fg(Color::Indexed(114)).bold(),
        HeadingLevel::H4 => Style::new().fg(Color::Indexed(213)).bold(),
        HeadingLevel::H5 => Style::new().fg(Color::Indexed(185)).bold(),
        HeadingLevel::H6 => Style::new().fg(Color::Indexed(245)).bold(),
    }
}

// ---------------------------------------------------------------------------
// Model
// ---------------------------------------------------------------------------

/// One styled run of inline text.
#[derive(Clone)]
pub struct Seg {
    pub style: Style,
    pub text: String,
    pub link: Option<String>,
}

/// A logical (not yet wrapped) display line.
pub struct RawLine {
    pub segs: Vec<Seg>,
    /// Prefix for the first rendered line (list marker, quote bar, ...).
    pub prefix: String,
    /// Prefix for continuation lines after wrapping.
    pub cont_prefix: String,
    /// Whether to soft-wrap this line at all (code lines are not wrapped).
    pub wrap: bool,
    /// Expand to a horizontal rule at layout time (heading underlines, `---`).
    pub rule: Option<(char, Style)>,
    /// Optional full-width background (code blocks).
    pub bg: Option<Color>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Center,
    Right,
}

pub struct RawBlock {
    pub lines: Vec<RawLine>,
    /// Blank lines to insert before this block.
    pub gap_before: u16,
}

pub struct TableData {
    /// All rows; row 0 is the header row.
    pub rows: Vec<Vec<Vec<Seg>>>,
    pub aligns: Vec<Align>,
}

pub enum Block {
    Lines(RawBlock),
    Image { src: String, alt: String },
    Table(TableData),
}

pub struct Doc {
    pub blocks: Vec<Block>,
    pub title: Option<String>,
    pub base_dir: PathBuf,
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

struct ListCtx {
    ordered: bool,
    index: u64,
    /// Number of items started in this list so far.
    items: u64,
}

struct Builder {
    blocks: Vec<Block>,
    /// Inline runs of the logical line currently being built.
    segs: Vec<Seg>,
    /// Completed logical lines of the block (list item / paragraph) being built.
    item_lines: Vec<Vec<Seg>>,
    /// Stack of cumulative inline styles.
    style_stack: Vec<Style>,
    /// Stack of open link URLs.
    links: Vec<String>,
    quote_depth: usize,
    list_stack: Vec<ListCtx>,
    /// Marker (prefix / hanging indent) of the list item currently open.
    marker: Option<(String, String)>,
    heading: Option<HeadingLevel>,
    code: Option<String>,
    table: Option<TableBuild>,
    /// (src, alt) of the image currently being parsed.
    image: Option<(String, String)>,
    title: Option<String>,
}

struct TableBuild {
    aligns: Vec<Align>,
    rows: Vec<Vec<Vec<Seg>>>,
    row: Vec<Vec<Seg>>,
    cell: Vec<Seg>,
    in_head: bool,
}

impl Builder {
    fn new() -> Self {
        Self {
            blocks: Vec::new(),
            segs: Vec::new(),
            item_lines: Vec::new(),
            style_stack: vec![Style::new()],
            links: Vec::new(),
            quote_depth: 0,
            list_stack: Vec::new(),
            marker: None,
            heading: None,
            code: None,
            table: None,
            image: None,
            title: None,
        }
    }

    fn cur_style(&self) -> Style {
        *self.style_stack.last().unwrap_or(&Style::new())
    }

    fn push_style(&mut self, delta: Style) {
        let cur = self.cur_style().patch(delta);
        self.style_stack.push(cur);
    }

    fn pop_style(&mut self) {
        if self.style_stack.len() > 1 {
            self.style_stack.pop();
        }
    }

    fn link(&self) -> Option<String> {
        self.links.last().cloned()
    }

    fn make_seg(&self, text: &str) -> Seg {
        let link = self.link();
        let style = if link.is_some() {
            self.cur_style().patch(LINK_STYLE)
        } else {
            self.cur_style()
        };
        Seg {
            style,
            text: text.to_string(),
            link,
        }
    }

    fn on_text(&mut self, text: &str) {
        if let Some((_, alt)) = self.image.as_mut() {
            alt.push_str(text);
            return;
        }
        if let Some(buf) = self.code.as_mut() {
            buf.push_str(text);
            return;
        }
        let seg = self.make_seg(text);
        if let Some(tbl) = self.table.as_mut() {
            tbl.cell.push(seg);
            return;
        }
        self.segs.push(seg);
    }

    /// End the current logical line (soft/hard break inside a paragraph).
    fn line_break(&mut self) {
        if !self.segs.is_empty() {
            let segs = std::mem::take(&mut self.segs);
            self.item_lines.push(segs);
        }
    }

    fn flush_block(&mut self, gap: u16) {
        self.line_break();
        if self.item_lines.is_empty() {
            return;
        }

        // Capture the doc title from the first H1.
        if self.heading == Some(HeadingLevel::H1) && self.title.is_none() {
            let t: String = self
                .item_lines
                .iter()
                .flat_map(|l| l.iter().map(|s| s.text.as_str()))
                .collect();
            self.title = Some(t);
        }

        let quote = "┃ ".repeat(self.quote_depth);
        let (first_prefix, cont_prefix) = match &self.marker {
            Some((p, c)) => (format!("{quote}{p}"), format!("{quote}{c}")),
            None => (quote.clone(), quote),
        };

        let in_quote = self.quote_depth > 0;
        let mut lines = Vec::with_capacity(self.item_lines.len());
        for (i, mut segs) in self.item_lines.drain(..).enumerate() {
            if in_quote {
                for s in &mut segs {
                    s.style = s.style.patch(QUOTE_TEXT_STYLE);
                }
            }
            lines.push(RawLine {
                segs,
                prefix: if i == 0 {
                    first_prefix.clone()
                } else {
                    cont_prefix.clone()
                },
                cont_prefix: cont_prefix.clone(),
                wrap: true,
                rule: None,
                bg: None,
            });
        }

        self.blocks.push(Block::Lines(RawBlock {
            lines,
            gap_before: gap,
        }));
    }

    fn item_gap(&self) -> u16 {
        match self.list_stack.last() {
            Some(ctx) if ctx.items <= 1 && self.list_stack.len() == 1 => 1,
            Some(_) => 0,
            None => 1,
        }
    }
}

pub fn parse(markdown: &str, base_dir: &Path) -> Doc {
    let mut b = Builder::new();

    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_TASKLISTS);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_FOOTNOTES);

    let parser = Parser::new_ext(markdown, opts);

    for event in parser {
        match event {
            // ----- block structure -------------------------------------------------
            Event::Start(Tag::Paragraph) => {}
            Event::End(TagEnd::Paragraph) => {
                if self_list_empty(&b) {
                    b.flush_block(1);
                } else {
                    b.line_break();
                }
            }

            Event::Start(Tag::Heading { level, .. }) => {
                b.flush_block(1);
                b.heading = Some(level);
                b.push_style(heading_style(level));
            }
            Event::End(TagEnd::Heading(level)) => {
                b.pop_style();
                b.flush_block(1);
                // Heading underline for the top two levels.
                let rule = match level {
                    HeadingLevel::H1 => Some(('━', RULE_STYLE)),
                    HeadingLevel::H2 => Some(('─', RULE_STYLE)),
                    _ => None,
                };
                if let Some((ch, st)) = rule
                    && let Some(Block::Lines(rb)) = b.blocks.last_mut()
                {
                    rb.lines.push(RawLine {
                        segs: Vec::new(),
                        prefix: String::new(),
                        cont_prefix: String::new(),
                        wrap: false,
                        rule: Some((ch, st)),
                        bg: None,
                    });
                }
                b.heading = None;
            }

            Event::Start(Tag::BlockQuote(_)) => {
                b.flush_block(1);
                b.quote_depth += 1;
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                b.flush_block(0);
                b.quote_depth = b.quote_depth.saturating_sub(1);
            }

            Event::Start(Tag::CodeBlock(kind)) => {
                b.flush_block(1);
                let _lang = match &kind {
                    CodeBlockKind::Fenced(l) => l.to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                b.code = Some(String::new());
            }
            Event::End(TagEnd::CodeBlock) => {
                let buf = b.code.take().unwrap_or_default();
                let mut lines = Vec::new();
                let mut raw: Vec<&str> = buf.split('\n').collect();
                if raw.last().is_some_and(|s| s.is_empty()) {
                    raw.pop();
                }
                for l in raw {
                    lines.push(RawLine {
                        segs: vec![Seg {
                            style: CODE_STYLE,
                            text: l.to_string(),
                            link: None,
                        }],
                        prefix: " ".to_string(),
                        cont_prefix: " ".to_string(),
                        wrap: false,
                        rule: None,
                        bg: Some(CODE_BG),
                    });
                }
                b.blocks.push(Block::Lines(RawBlock {
                    lines,
                    gap_before: 1,
                }));
            }

            Event::Rule => {
                b.flush_block(1);
                b.blocks.push(Block::Lines(RawBlock {
                    lines: vec![RawLine {
                        segs: Vec::new(),
                        prefix: String::new(),
                        cont_prefix: String::new(),
                        wrap: false,
                        rule: Some(('─', RULE_STYLE)),
                        bg: None,
                    }],
                    gap_before: 1,
                }));
            }

            // ----- lists -----------------------------------------------------------
            Event::Start(Tag::List(start)) => {
                b.flush_block(1);
                b.list_stack.push(ListCtx {
                    ordered: start.is_some(),
                    index: start.unwrap_or(1).saturating_sub(1),
                    items: 0,
                });
            }
            Event::End(TagEnd::List(_)) => {
                b.flush_block(0);
                b.list_stack.pop();
            }
            Event::Start(Tag::Item) => {
                b.flush_block(b.item_gap());
                let gap_marker;
                let depth = b.list_stack.len();
                if let Some(ctx) = b.list_stack.last_mut() {
                    ctx.index += 1;
                    ctx.items += 1;
                    let indent = "  ".repeat(depth.saturating_sub(1));
                    if ctx.ordered {
                        let m = format!("{}. ", ctx.index);
                        gap_marker = (
                            format!("{indent}{m}"),
                            format!("{indent}{}", " ".repeat(m.chars().count())),
                        );
                    } else {
                        gap_marker = (format!("{indent}• "), format!("{indent}  "));
                    }
                } else {
                    gap_marker = ("• ".to_string(), "  ".to_string());
                }
                b.marker = Some(gap_marker);
            }
            Event::End(TagEnd::Item) => {
                let gap = b.item_gap();
                b.flush_block(gap);
                b.marker = None;
            }
            Event::TaskListMarker(checked) => {
                let (glyph, color) = if checked {
                    ("☒ ", Color::Indexed(114))
                } else {
                    ("☐ ", Color::Indexed(244))
                };
                b.segs.push(Seg {
                    style: Style::new().fg(color),
                    text: glyph.to_string(),
                    link: None,
                });
            }

            // ----- tables ----------------------------------------------------------
            Event::Start(Tag::Table(aligns)) => {
                b.flush_block(1);
                b.table = Some(TableBuild {
                    aligns: aligns
                        .iter()
                        .map(|a| match a {
                            pulldown_cmark::Alignment::Left => Align::Left,
                            pulldown_cmark::Alignment::Center => Align::Center,
                            pulldown_cmark::Alignment::Right => Align::Right,
                            pulldown_cmark::Alignment::None => Align::Left,
                        })
                        .collect(),
                    rows: Vec::new(),
                    row: Vec::new(),
                    cell: Vec::new(),
                    in_head: false,
                });
            }
            Event::Start(Tag::TableHead) => {
                if let Some(t) = b.table.as_mut() {
                    t.in_head = true;
                    t.row = Vec::new();
                }
            }
            Event::End(TagEnd::TableHead) => {
                if let Some(t) = b.table.as_mut() {
                    t.in_head = false;
                    if !t.row.is_empty() {
                        t.rows.push(std::mem::take(&mut t.row));
                    }
                }
            }
            Event::Start(Tag::TableRow) => {
                if let Some(t) = b.table.as_mut() {
                    t.row = Vec::new();
                }
            }
            Event::End(TagEnd::TableRow) => {
                if let Some(t) = b.table.as_mut()
                    && !t.in_head
                    && !t.row.is_empty()
                {
                    t.rows.push(std::mem::take(&mut t.row));
                }
            }
            Event::Start(Tag::TableCell) => {
                if let Some(t) = b.table.as_mut() {
                    t.cell = Vec::new();
                }
            }
            Event::End(TagEnd::TableCell) => {
                if let Some(t) = b.table.as_mut() {
                    let cell = std::mem::take(&mut t.cell);
                    t.row.push(cell);
                }
            }
            Event::End(TagEnd::Table) => {
                if let Some(t) = b.table.take() {
                    b.blocks.push(Block::Table(TableData {
                        rows: t.rows,
                        aligns: t.aligns,
                    }));
                }
            }

            // ----- inline ----------------------------------------------------------
            Event::Start(Tag::Emphasis) => b.push_style(Style::new().italic()),
            Event::End(TagEnd::Emphasis) => b.pop_style(),
            Event::Start(Tag::Strong) => b.push_style(Style::new().bold()),
            Event::End(TagEnd::Strong) => b.pop_style(),
            Event::Start(Tag::Strikethrough) => {
                b.push_style(Style::new().add_modifier(Modifier::CROSSED_OUT))
            }
            Event::End(TagEnd::Strikethrough) => b.pop_style(),

            Event::Start(Tag::Link { dest_url, .. }) => b.links.push(dest_url.to_string()),
            Event::End(TagEnd::Link) => {
                b.links.pop();
            }

            Event::Start(Tag::Image { dest_url, .. }) => {
                b.flush_block(1);
                b.image = Some((dest_url.to_string(), String::new()));
            }
            Event::End(TagEnd::Image) => {
                if let Some((src, alt)) = b.image.take() {
                    b.blocks.push(Block::Image { src, alt });
                }
            }

            // ----- leaf events -----------------------------------------------------
            Event::Text(text) => b.on_text(&text),
            Event::Code(text) => {
                let seg = Seg {
                    style: b.cur_style().patch(INLINE_CODE_STYLE),
                    text: text.to_string(),
                    link: None,
                };
                if let Some(t) = b.table.as_mut() {
                    t.cell.push(seg);
                } else {
                    b.segs.push(seg);
                }
            }
            Event::SoftBreak | Event::HardBreak => {
                if b.table.is_some() {
                    // No line breaks inside table cells; join with a space.
                    b.on_text(" ");
                } else {
                    b.line_break();
                }
            }
            Event::FootnoteReference(label) => {
                b.segs.push(Seg {
                    style: Style::new().fg(Color::Indexed(245)),
                    text: format!("[^{label}]"),
                    link: None,
                });
            }

            // ----- ignored ---------------------------------------------------------
            _ => {}
        }
    }

    b.flush_block(1);

    Doc {
        blocks: b.blocks,
        title: b.title,
        base_dir: base_dir.to_path_buf(),
    }
}

fn self_list_empty(b: &Builder) -> bool {
    b.list_stack.is_empty()
}
