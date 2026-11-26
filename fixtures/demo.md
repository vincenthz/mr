# mr — terminal markdown renderer

A fast little markdown viewer for your terminal, with **mouse support** and
inline *image rendering*.

## Features

- Clickable [links](https://rust-lang.org) — click or press `o` to open
- Wheel scrolling, `j`/`k`, `g`/`G`, `PageUp`/`PageDown`
- Images via kitty / sixel / iterm2 / halfblocks protocols
- Tables, task lists, code blocks, block quotes

### Task list

- [x] parse markdown
- [x] wrap and style text
- [ ] world domination

### Nested lists

1. one
2. two
   - nested bullet one
   - nested bullet two
     with a wrapped continuation line
3. three

## Code

```rust
fn main() {
    println!("hello, mr!");
}
```

Inline code looks like `cargo run -- fixtures/demo.md`.

## Table

| Feature | Status | Notes |
| ------- | :----: | ----: |
| Links | yes | click to open |
| Images | yes | kitty + sixel |
| Tables | yes | GFM |

> Blockquotes render with a colored bar and dimmed text.
> They can span multiple lines and contain **bold** or *italics*.

---

Horizontal rules work too. Remote images (like
![remote](https://example.com/logo.png)) fall back to a placeholder box.

![a generated test image](img/test.png)

A [reference link][rfc] and an autolink <https://example.com> also work.

[rfc]: https://www.rfc-editor.org/rfc/rfc7749
