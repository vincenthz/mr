//! `mr` — a terminal markdown renderer with mouse support and inline images.
//!
//! This crate provides the internals of the `mr` viewer; the CLI entry point
//! lives in `src/main.rs`. The pipeline is split into three stages:
//!
//! 1. [`doc`] parses CommonMark (via pulldown-cmark) into display blocks,
//! 2. [`layout`] wraps those blocks for a given terminal width, measuring every
//!    line and recording where each link lands so the renderer and the mouse
//!    hit-testing share one coordinate system, and
//! 3. [`render`] draws the viewport, scrollbar and status bar with ratatui.
//!
//! [`app`] ties it together: key/mouse handling, link opening and clipboard
//! support.
//!
//! Images are rendered with the terminal's native graphics protocol (kitty,
//! sixel or iTerm2), detected at startup through [`ratatui_image::picker::Picker`],
//! with unicode halfblocks as a universal fallback.
//!
//! # Example
//!
//! ```no_run
//! use mr::app::App;
//! use mr::doc;
//! use mr::layout::Layout;
//!
//! let markdown = "# Hello\n\nSome [link](https://rust-lang.org)\n";
//! let doc = doc::parse(markdown, std::path::Path::new("."));
//! let mut cache = std::collections::HashMap::new();
//! let layout = Layout::build(&doc, 80, None, &mut cache);
//! assert!(layout.total > 0);
//! ```
//!
//! Published as `mr-view` on crates.io; the binary is simply `mr`.

pub mod app;
pub mod doc;
pub mod layout;
pub mod render;
