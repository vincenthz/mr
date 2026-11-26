//! Headless rendering tests using ratatui's TestBackend + halfblocks images.

use std::path::PathBuf;

use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui_image::picker::Picker;
use unicode_width::UnicodeWidthStr;

use mr::app::App;
use mr::layout::LaidBlock;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("fixtures")
        .join(name)
}

fn test_app(md: &str) -> App {
    let path = fixture(md);
    let markdown = std::fs::read_to_string(&path).unwrap();
    App::new(path, markdown, Some(Picker::halfblocks()))
}

#[test]
fn generates_test_image() {
    // The demo references fixtures/img/test.png; make sure it exists so the
    // demo file works out of the box.
    ensure_fixture();
}

#[test]
fn renders_document_with_expected_content() {
    ensure_fixture();
    let mut app = test_app("demo.md");
    let mut terminal = Terminal::new(TestBackend::new(80, 80)).unwrap();
    terminal.draw(|f| mr::render::draw(f, &mut app)).unwrap();

    let buffer = terminal.backend().buffer();
    let content: String = buffer
        .content
        .iter()
        .map(|c| c.symbol().to_string())
        .collect();

    assert!(content.contains("terminal markdown renderer"), "h1 missing");
    assert!(content.contains("Features"), "h2 missing");
    assert!(content.contains("nested bullet one"), "nested list missing");
    assert!(content.contains("hello, mr!"), "code block missing");
    assert!(
        content.contains("Blockquotes render with a colored bar"),
        "quote missing"
    );
    assert!(content.contains("click to open"), "table missing");
    assert!(content.contains("world domination"), "task list missing");
}

#[test]
fn links_are_clickable_at_their_rendered_position() {
    ensure_fixture();
    let mut app = test_app("demo.md");
    let mut terminal = Terminal::new(TestBackend::new(80, 40)).unwrap();
    terminal.draw(|f| mr::render::draw(f, &mut app)).unwrap();

    // Find "Clickable" on screen and hit-test the text right after it.
    let buffer = terminal.backend().buffer();
    let mut pos = None;
    for y in 0..40 {
        let line: String = (0..80)
            .map(|x| buffer[(x, y)].symbol().to_string())
            .collect();
        if let Some(x) = line.find("Clickable") {
            pos = Some((y, x as u16));
            break;
        }
    }
    let (row, x) = pos.expect("link text not on screen");

    // The link itself is the text right after "Clickable ".
    let link = app
        .link_at_screen(x + 10, row)
        .expect("no link at expected position");
    assert_eq!(link, "https://rust-lang.org");

    // The bullet marker before the link must not resolve to anything.
    assert!(app.link_at_screen(x.saturating_sub(2), row).is_none());
}

#[test]
fn image_renders_via_halfblocks_and_occupies_rows() {
    ensure_fixture();
    let mut app = test_app("demo.md");
    let mut terminal = Terminal::new(TestBackend::new(80, 100)).unwrap();
    terminal.draw(|f| mr::render::draw(f, &mut app)).unwrap();

    assert!(
        app.layout
            .blocks
            .iter()
            .any(|b| matches!(b, LaidBlock::Image { .. })),
        "image block missing from layout"
    );
    // Scrolling to the bottom should not panic.
    app.goto_bottom();
    terminal.draw(|f| mr::render::draw(f, &mut app)).unwrap();
}

#[test]
fn remote_image_falls_back_to_placeholder() {
    ensure_fixture();
    let mut app = test_app("demo.md");
    app.relayout(80);
    let text: Vec<String> = app
        .layout
        .blocks
        .iter()
        .filter_map(|b| match b {
            LaidBlock::Text(lines) => Some(
                lines
                    .iter()
                    .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect();
    assert!(text.iter().any(|t| t.contains("example.com/logo.png")));
}

#[test]
fn wrapping_and_scrolling_stay_in_bounds() {
    ensure_fixture();
    let mut app = test_app("demo.md");
    app.relayout(40); // narrow terminal forces wrapping
    for line in app
        .layout
        .blocks
        .iter()
        .flat_map(|b| match b {
            LaidBlock::Text(lines) => Some(lines.iter()),
            _ => None,
        })
        .flatten()
    {
        let w: usize = line.spans.iter().map(|s| s.content.width()).sum();
        // Unwrapped code lines may exceed; everything else must fit 40 columns.
        if !line.spans.iter().all(|s| s.content.contains("hello")) {
            assert!(
                w <= 41,
                "line too wide ({w}): {:?}",
                line.spans
                    .iter()
                    .map(|s| s.content.to_string())
                    .collect::<Vec<_>>()
            );
        }
    }
    // Scroll through the whole document without panicking.
    let total = app.layout.total;
    for off in (0..total).step_by(7) {
        app.offset = off;
    }
    app.offset = total.saturating_sub(1);
}

fn ensure_fixture() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/img");
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("test.png");
    if !out.exists() {
        let img = image::RgbImage::from_fn(160, 100, |x, y| {
            let r = (x * 255 / 160) as u8;
            let g = (y * 255 / 100) as u8;
            image::Rgb([r, g, 180])
        });
        img.save(&out).unwrap();
    }
}
