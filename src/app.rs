//! Application state and input handling.

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use image::DynamicImage;
use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;
use ratatui_image::picker::Picker;

use crate::doc::{self, Doc};
use crate::layout::Layout;

/// How long transient status-bar messages stay visible.
pub const MESSAGE_TTL: std::time::Duration = std::time::Duration::from_secs(3);

pub struct App {
    pub file: PathBuf,
    pub doc: Doc,
    pub picker: Option<Picker>,
    pub protocol_name: String,
    pub layout: Layout,
    pub offset: usize,
    pub hover: Option<String>,
    /// Transient status message with its expiry.
    pub message: Option<(std::time::Instant, String)>,
    pub doc_area: Rect,
    pub img_cache: HashMap<String, Option<DynamicImage>>,
    pub quit: bool,
}

impl App {
    pub fn new(file: PathBuf, markdown: String, picker: Option<Picker>) -> App {
        let doc = doc::parse(
            &markdown,
            file.parent().unwrap_or(std::path::Path::new(".")),
        );
        let protocol_name = picker
            .as_ref()
            .map(|p| format!("{:?}", p.protocol_type()))
            .unwrap_or_else(|| "none".to_string());
        App {
            file: file.clone(),
            doc,
            picker,
            protocol_name,
            layout: Layout::empty(),
            offset: 0,
            hover: None,
            message: None,
            doc_area: Rect::default(),
            img_cache: HashMap::new(),
            quit: false,
        }
    }

    pub fn reload(&mut self) -> std::io::Result<()> {
        let markdown = std::fs::read_to_string(&self.file)?;
        self.doc = doc::parse(
            &markdown,
            self.file.parent().unwrap_or(std::path::Path::new(".")),
        );
        self.layout = Layout::empty(); // forces relayout on next draw
        self.offset = 0;
        self.set_message("reloaded");
        Ok(())
    }

    pub fn relayout(&mut self, width: u16) {
        let doc = &self.doc;
        let picker = self.picker.as_ref();
        let cache = &mut self.img_cache;
        self.layout = Layout::build(doc, width, picker, cache);
    }

    pub fn set_message(&mut self, msg: impl Into<String>) {
        self.message = Some((std::time::Instant::now(), msg.into()));
    }

    /// Drop the transient message once it has expired.
    pub fn expire_message(&mut self) {
        if let Some((at, _)) = &self.message
            && at.elapsed() > MESSAGE_TTL
        {
            self.message = None;
        }
    }

    pub fn viewport_height(&self) -> usize {
        self.doc_area.height as usize
    }

    pub fn scroll(&mut self, delta: i64) {
        let max = self.layout.total.saturating_sub(self.viewport_height());
        self.offset = (self.offset as i64 + delta).clamp(0, max as i64) as usize;
    }

    pub fn goto_top(&mut self) {
        self.offset = 0;
    }

    pub fn goto_bottom(&mut self) {
        let max = self.layout.total.saturating_sub(self.viewport_height());
        self.offset = max;
    }

    pub fn link_at_screen(&self, col: u16, row: u16) -> Option<String> {
        let area = self.doc_area;
        if row < area.y || row >= area.y + area.height || col < area.x || col >= area.x + area.width
        {
            return None;
        }
        let doc_row = self.offset + (row - area.y) as usize;
        let col = col - area.x;
        self.layout.link_at(doc_row, col).map(|ls| ls.url.clone())
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            if let KeyCode::Char('c') = key.code {
                self.quit = true;
            }
            return;
        }
        let half = (self.viewport_height() / 2).max(1) as i64;
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
            KeyCode::Char('j') | KeyCode::Down => self.scroll(1),
            KeyCode::Char('k') | KeyCode::Up => self.scroll(-1),
            KeyCode::Char('d') => self.scroll(half),
            KeyCode::Char('u') => self.scroll(-half),
            KeyCode::Char(' ') | KeyCode::PageDown => self.scroll(self.viewport_height() as i64),
            KeyCode::PageUp => self.scroll(-(self.viewport_height() as i64)),
            KeyCode::Char('g') | KeyCode::Home => self.goto_top(),
            KeyCode::Char('G') | KeyCode::End => self.goto_bottom(),
            KeyCode::Char('r') => {
                if let Err(e) = self.reload() {
                    self.set_message(format!("reload failed: {e}"));
                }
            }
            KeyCode::Char('o') => {
                if let Some(url) = self.hover.clone() {
                    self.open_link(&url);
                }
            }
            KeyCode::Char('y') => {
                if let Some(url) = self.hover.clone() {
                    match copy_to_clipboard(&url) {
                        Ok(()) => self.set_message(format!("copied: {url}")),
                        Err(e) => self.set_message(format!("copy failed: {e}")),
                    }
                }
            }
            _ => {}
        }
    }

    pub fn on_mouse(&mut self, m: MouseEvent) {
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if let Some(url) = self.link_at_screen(m.column, m.row) {
                    self.open_link(&url);
                }
            }
            MouseEventKind::Moved => {
                self.hover = self.link_at_screen(m.column, m.row);
            }
            MouseEventKind::ScrollDown => self.scroll(3),
            MouseEventKind::ScrollUp => self.scroll(-3),
            _ => {}
        }
    }

    fn open_link(&mut self, url: &str) {
        match open_url(url) {
            Ok(()) => self.set_message(format!("opened: {url}")),
            Err(e) => self.set_message(format!("could not open: {e}")),
        }
    }
}

fn open_url(url: &str) -> Result<(), String> {
    if !(url.starts_with("http://") || url.starts_with("https://") || url.starts_with("mailto:")) {
        return Err(format!("non-http link not opened: {url}"));
    }
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(all(unix, not(target_os = "macos")))]
    let program = "xdg-open";
    Command::new(program)
        .arg(url)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

fn copy_to_clipboard(text: &str) -> std::io::Result<()> {
    #[cfg(target_os = "macos")]
    let mut child = Command::new("pbcopy")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?;
    #[cfg(all(unix, not(target_os = "macos")))]
    let mut child = {
        // Prefer wayland, fall back to X11.
        Command::new("wl-copy")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .or_else(|_| {
                Command::new("xclip")
                    .args(["-selection", "clipboard"])
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
            })?
    };
    child.stdin.take().unwrap().write_all(text.as_bytes())?;
    Ok(())
}
