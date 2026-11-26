# mr — terminal markdown renderer

A small, fast markdown viewer for your terminal with **mouse support** and
**inline image rendering**.

Published as [`mr-view`](https://crates.io/crates/mr-view) — the binary is
simply `mr`:

```
cargo install mr-view
mr README.md
```

Or run straight from a checkout:

```
cargo run -- fixtures/demo.md
```

## Features

- **CommonMark rendering** — headings (with rules), paragraphs with soft-wrap,
  nested ordered/unordered lists, task lists, block quotes, code blocks with
  background, GFM tables with column alignment, horizontal rules, footnotes
- **Clickable links** — every link's screen position is tracked, so clicking it
  opens it in your browser (`open` / `xdg-open`). Hovering shows the URL in the
  status bar; press `y` to copy it to the clipboard
- **Images** — rendered inline via the terminal's native graphics protocol:
  kitty graphics, sixel, iTerm2, or unicode halfblocks as a fallback. Images
  are scaled to the terminal width; remote/unreadable images show a placeholder
  box
- **Mouse** — wheel scrolling, link hover, click-to-open
- **Keyboard** — vim-style scrolling (`j`/`k`/`d`/`u`/`g`/`G`), page keys,
  `r` to reload the file, `q` to quit
- **Live resize** — re-wraps and re-scales images on window size changes

## Terminal support

| Capability | Terminals |
| ---------- | --------- |
| Kitty graphics | Kitty, Ghostty |
| Sixel | WezTerm, foot, XTerm, iTerm (with sixel enabled), Alacritty (sixel fork) |
| iTerm2 inline | iTerm2, WezTerm, Mintty |
| Halfblocks (any terminal) | everywhere — images are drawn with unicode half-block characters |

The protocol is auto-detected at startup by querying the terminal; the status
bar shows which one is in use.

## Usage

```
mr <file.md>
```

| Key | Action |
| --- | ------ |
| `j` `k` / arrows | scroll one line |
| `d` `u` | scroll half a page |
| space / `PageDn` `PageUp` | scroll a page |
| `g` `G` / `Home` `End` | jump to top / bottom |
| mouse wheel | scroll |
| click link | open in browser |
| `y` | copy hovered link to clipboard |
| `r` | reload the file |
| `q` / `Esc` / Ctrl-C | quit |

## Development

```
cargo test            # headless rendering tests (ratatui TestBackend)
cargo run -- fixtures/demo.md
```

The code is structured as:

- `src/doc.rs` — pulldown-cmark events → display blocks
- `src/layout.rs` — word wrapping, link-position tracking, tables, image
  placement/encoding; one coordinate system shared by rendering and hit-testing
- `src/render.rs` — ratatui drawing (viewport, scrollbar, status bar)
- `src/app.rs` — state, key/mouse handling, link opening / yanking
- `src/main.rs` — terminal lifecycle and event loop
