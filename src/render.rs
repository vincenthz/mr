//! Terminal rendering: document viewport, scrollbar, status bar.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState};

use crate::app::App;
use crate::layout::{DocLine, LaidBlock};

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let chunks = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).split(area);
    let doc_area = chunks[0];
    let status_area = chunks[1];

    if app.layout.width != doc_area.width {
        let w = doc_area.width;
        app.relayout(w);
    }
    app.doc_area = doc_area;

    let vh = doc_area.height as usize;
    let max_off = app.layout.total.saturating_sub(vh);
    app.offset = app.offset.min(max_off);

    let status = build_status(app, status_area);

    let layout = &app.layout;
    let offset = app.offset;

    let mut y: i64 = -(offset as i64);
    'outer: for (i, block) in layout.blocks.iter().enumerate() {
        let h = layout.heights[i] as i64;
        if y + h <= 0 {
            y += h;
            continue;
        }
        match block {
            LaidBlock::Text(lines) => {
                for (j, line) in lines.iter().enumerate() {
                    let sy = y + j as i64;
                    if sy < 0 {
                        continue;
                    }
                    if sy >= vh as i64 {
                        break 'outer;
                    }
                    render_line(f, line, doc_area, sy as u16);
                }
            }
            LaidBlock::Image { proto } => {
                let (iw, ih) = (proto.size().width, proto.size().height);
                if y >= 0 && y + ih as i64 <= vh as i64 {
                    let rect = Rect {
                        x: doc_area.x,
                        y: doc_area.y + y as u16,
                        width: iw.min(doc_area.width),
                        height: ih,
                    };
                    f.render_widget(ratatui_image::Image::new(proto), rect);
                }
            }
        }
        y += h;
        if y >= vh as i64 {
            break;
        }
    }

    if layout.total > vh {
        let mut state = ScrollbarState::new(layout.total)
            .viewport_content_length(vh)
            .position(offset);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            doc_area,
            &mut state,
        );
    }

    let rows = Layout::horizontal([
        Constraint::Min(0),
        Constraint::Length(status.1.width() as u16),
    ])
    .split(status_area);
    f.render_widget(Paragraph::new(status.0).style(STATUS_STYLE), rows[0]);
    if !status.1.spans.is_empty() {
        f.render_widget(Paragraph::new(status.1).style(STATUS_STYLE), rows[1]);
    }
}

fn render_line(f: &mut Frame, line: &DocLine, area: Rect, row: u16) {
    let mut p = Paragraph::new(Line::from(line.spans.clone()));
    if let Some(bg) = line.bg {
        p = p.style(Style::new().bg(bg));
    }
    let rect = Rect {
        x: area.x,
        y: area.y + row,
        width: area.width,
        height: 1,
    };
    f.render_widget(p, rect);
}

const STATUS_STYLE: Style = Style::new().bg(Color::Indexed(237)).fg(Color::Indexed(252));

fn build_status(app: &App, area: Rect) -> (Line<'static>, Line<'static>) {
    let name = app
        .doc
        .title
        .clone()
        .unwrap_or_else(|| {
            app.file
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| "stdin".into())
        })
        .trim()
        .to_string();

    let pct = if app.layout.total == 0 {
        100
    } else {
        let vh = app.viewport_height().max(1);
        ((app.offset + vh) * 100) / app.layout.total.max(1)
    };
    let left = Line::from(vec![
        Span::styled(
            " mr ",
            Style::new().bg(Color::Indexed(31)).fg(Color::White).bold(),
        ),
        Span::raw(" "),
        Span::raw(name),
        Span::raw(format!(
            "  [{}, {}]  {}%",
            app.protocol_name,
            app.layout.total,
            pct.min(100)
        )),
    ]);

    let right = if let Some((_, msg)) = &app.message {
        Line::from(Span::styled(
            msg.clone(),
            Style::new().fg(Color::Indexed(220)),
        ))
    } else if let Some(url) = &app.hover {
        Line::from(Span::styled(url.clone(), crate::doc::LINK_STYLE))
    } else {
        Line::from(vec![Span::styled(
            "scroll: j/k · links: click · y: yank · r: reload · q: quit",
            Style::new().fg(Color::Indexed(250)),
        )])
    };

    // Truncate the right side if both sides don't fit.
    let left_w = left.width();
    let right_w = right.width();
    let avail = area.width as usize;
    if left_w + right_w + 1 > avail {
        let budget = avail.saturating_sub(left_w + 1);
        let right = match right.spans.first() {
            Some(span) => Line::from(Span::styled(truncate(&span.content, budget), span.style)),
            None => right,
        };
        (left, right)
    } else {
        (left, right)
    }
}

use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

fn truncate(s: &str, max: usize) -> String {
    if s.width() <= max || max < 2 {
        return s.chars().take(max).collect();
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
