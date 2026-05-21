# UX/UI Redesign Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Catppuccin Mocha theme + Vimium-style link hints + inline URL editing across `app.rs`, `keybinds.rs`, `ui.rs`, `renderer/text.rs`, and `browser/tabs.rs`.

**Architecture:** Add `LineKind`/`CodeSpan` metadata to `RenderedPage` and `Tab` so `draw_content` can apply per-line styling without storing ANSI in the line strings. Two new `InputMode` variants (`Url`, `Hint`) extend the existing mode dispatch. All Catppuccin colors are `const Color::Rgb(r,g,b)` at the top of `ui.rs`.

**Tech Stack:** Rust stable, ratatui 0.28, crossterm, scraper, tokio

---

### Task 1: LineKind + CodeSpan types — extend data layer

**Files:**
- Modify: `src/renderer/text.rs`
- Modify: `src/browser/tabs.rs`
- Modify: `src/app.rs`

- [ ] **Step 1: Write failing tests**

Add to `src/renderer/text.rs` tests:
```rust
#[test]
fn line_kinds_default_normal() {
    let page = ParsedPage::parse_html("<html><body><p>Hello</p></body></html>");
    let rp = render_full(&page);
    assert!(rp.line_kinds.iter().all(|k| *k == LineKind::Normal));
}

#[test]
fn rendered_page_has_line_kinds_same_length_as_lines() {
    let page = ParsedPage::parse_html("<html><body><h1>T</h1><p>P</p></body></html>");
    let rp = render_full(&page);
    assert_eq!(rp.lines.len(), rp.line_kinds.len());
}

#[test]
fn code_spans_empty_for_plain_text() {
    let page = ParsedPage::parse_html("<html><body><p>Hello world</p></body></html>");
    let rp = render_full(&page);
    assert!(rp.code_spans.is_empty());
}
```

- [ ] **Step 2: Run — expect compile errors**

```
cargo test --test renderer_tests 2>&1 | head -30
```

Expected: `error[E0609]: no field 'line_kinds' on type 'RenderedPage'`

- [ ] **Step 3: Add LineKind and CodeSpan types to renderer/text.rs**

Add after the existing `RenderedImage` struct:
```rust
/// Per-line semantic kind for typography styling in draw_content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineKind {
    #[default]
    Normal,
    H1,
    H2,
    H3,
    H4Plus,
}

/// Byte-offset range of an inline code span within a rendered line.
#[derive(Debug, Clone)]
pub struct CodeSpan {
    pub line: usize,
    pub start: usize, // char offset in stripped line
    pub end: usize,   // char offset in stripped line (exclusive)
}
```

Replace `RenderedPage` struct:
```rust
pub struct RenderedPage {
    pub lines: Vec<String>,
    pub links: Vec<RenderedLink>,
    pub images: Vec<RenderedImage>,
    pub line_kinds: Vec<LineKind>,
    pub code_spans: Vec<CodeSpan>,
}
```

Add `code_spans` field to `Ctx`:
```rust
struct Ctx {
    buf: String,
    links: Vec<RenderedLink>,
    images: Vec<RenderedImage>,
    code_spans: Vec<CodeSpan>,
}

impl Ctx {
    fn new() -> Self {
        Self {
            buf: String::with_capacity(4096),
            links: Vec::new(),
            images: Vec::new(),
            code_spans: Vec::new(),
        }
    }
    // keep all existing methods unchanged
}
```

Update `render_full` to initialize and return new fields:
```rust
pub fn render_full(page: &ParsedPage) -> RenderedPage {
    let hidden = css::extract_hidden(page.document());
    let mut ctx = Ctx::new();
    render_element(page.document().root_element(), &mut ctx, &hidden);
    let normalized = normalize(&ctx.buf);
    let stripped = strip_ansi(&normalized);
    let mut lines: Vec<String> = stripped.lines().map(str::to_owned).collect();

    let mut links = ctx.links;
    let mut images = ctx.images;
    let mut code_spans = ctx.code_spans;
    let mut line_kinds = vec![LineKind::Normal; lines.len()];

    for (line_idx, line) in lines.iter_mut().enumerate() {
        let mut buf = String::with_capacity(line.len());
        let mut iter = line.chars().peekable();
        while let Some(c) = iter.next() {
            if c == MARKER {
                let mut kind = '?';
                let mut digits = String::new();
                if let Some(&k) = iter.peek() { kind = k; iter.next(); }
                while let Some(&d) = iter.peek() {
                    if d.is_ascii_digit() { digits.push(d); iter.next(); } else { break; }
                }
                if iter.peek() == Some(&MARKER) { iter.next(); }
                match kind {
                    'L' => { if let Ok(i) = digits.parse::<usize>() { if let Some(rl) = links.get_mut(i) { rl.line = line_idx; } } }
                    'I' => { if let Ok(i) = digits.parse::<usize>() { if let Some(ri) = images.get_mut(i) { ri.line = line_idx; } } }
                    'H' => {
                        if let Ok(level) = digits.parse::<usize>() {
                            if let Some(lk) = line_kinds.get_mut(line_idx) {
                                *lk = match level { 1 => LineKind::H1, 2 => LineKind::H2, 3 => LineKind::H3, _ => LineKind::H4Plus };
                            }
                        }
                    }
                    'C' => { if let Ok(i) = digits.parse::<usize>() { if let Some(cs) = code_spans.get_mut(i) { cs.line = line_idx; cs.start = buf.len(); } } }
                    'Z' => { if let Ok(i) = digits.parse::<usize>() { if let Some(cs) = code_spans.get_mut(i) { cs.end = buf.len(); } } }
                    _ => {}
                }
            } else {
                buf.push(c);
            }
        }
        *line = buf;
    }

    RenderedPage { lines, links, images, line_kinds, code_spans }
}
```

- [ ] **Step 4: Extend Tab in browser/tabs.rs**

Add imports at top:
```rust
use crate::renderer::text::{CodeSpan, LineKind, RenderedLink};
```

Add fields to `Tab`:
```rust
pub struct Tab {
    pub url: String,
    pub title: String,
    pub history: History,
    pub lines: Vec<String>,
    pub links: Vec<RenderedLink>,
    pub line_kinds: Vec<LineKind>,
    pub code_spans: Vec<CodeSpan>,
    pub scroll: usize,
    pub selected_link: Option<usize>,
    pub loading: bool,
    pub search_matches: Vec<usize>,
    pub search_idx: usize,
}
```

Update `Tab::new`:
```rust
pub fn new(url: String) -> Self {
    Self {
        url,
        title: String::new(),
        history: History::new(),
        lines: Vec::new(),
        links: Vec::new(),
        line_kinds: Vec::new(),
        code_spans: Vec::new(),
        scroll: 0,
        selected_link: None,
        loading: true,
        search_matches: Vec::new(),
        search_idx: 0,
    }
}
```

- [ ] **Step 5: Extend BgMsg and fetch_inner in app.rs**

Update `BgMsg::Loaded`:
```rust
pub enum BgMsg {
    Loaded {
        tab_idx: usize,
        url: String,
        title: Option<String>,
        lines: Vec<String>,
        links: Vec<RenderedLink>,
        line_kinds: Vec<renderer::text::LineKind>,
        code_spans: Vec<renderer::text::CodeSpan>,
    },
    Error { tab_idx: usize, message: String },
}
```

Add import at top of app.rs:
```rust
use crate::renderer;
```

Update `handle_msg`:
```rust
BgMsg::Loaded { tab_idx, url, title, lines, links, line_kinds, code_spans } => {
    if let Some(tab) = self.tabs.tabs.get_mut(tab_idx) {
        tab.url = url;
        tab.title = title.unwrap_or_default();
        tab.lines = lines;
        tab.links = links;
        tab.line_kinds = line_kinds;
        tab.code_spans = code_spans;
        tab.scroll = 0;
        tab.selected_link = None;
        tab.loading = false;
        tab.clear_search();
    }
}
```

Update `fetch_inner`:
```rust
async fn fetch_inner(url: &str, tab_idx: usize) -> Result<BgMsg> {
    let client = SpiderClient::new()?;
    let resp = client.fetch(url).await?;

    let (lines, links, line_kinds, code_spans, title) = if resp.is_html() {
        let page = ParsedPage::from_bytes(&resp.body);
        let title = page.title();
        let rendered = text_renderer::render_full(&page);
        (rendered.lines, rendered.links, rendered.line_kinds, rendered.code_spans, title)
    } else if resp.is_text() {
        let text = String::from_utf8_lossy(&resp.body);
        let ls: Vec<String> = text.lines().map(str::to_owned).collect();
        let lk = vec![renderer::text::LineKind::Normal; ls.len()];
        (ls, Vec::new(), lk, Vec::new(), None)
    } else {
        let ct = resp.content_type.as_deref().unwrap_or("binary");
        let ls = vec![format!("[{ct} — {} bytes — not renderable]", resp.body.len())];
        let lk = vec![renderer::text::LineKind::Normal];
        (ls, Vec::new(), lk, Vec::new(), None)
    };

    Ok(BgMsg::Loaded { tab_idx, url: url.to_owned(), title, lines, links, line_kinds, code_spans })
}
```

- [ ] **Step 6: Run tests — expect pass**

```
cargo test
```

Expected: all existing tests pass, new tests pass.

- [ ] **Step 7: Commit**

```
git add src/renderer/text.rs src/browser/tabs.rs src/app.rs
git commit -m "feat(types): LineKind + CodeSpan metadata — extend RenderedPage, Tab, BgMsg"
```

---

### Task 2: Heading renderer — markers and line kind assignment

**Files:**
- Modify: `src/renderer/text.rs`

- [ ] **Step 1: Write failing tests**

Add to `src/renderer/text.rs` tests:
```rust
#[test]
fn h1_gets_linekind_h1() {
    let page = ParsedPage::parse_html("<html><body><h1>Title</h1></body></html>");
    let rp = render_full(&page);
    let h1_idx = rp.lines.iter().position(|l| l.contains("Title")).unwrap();
    assert_eq!(rp.line_kinds[h1_idx], LineKind::H1);
}

#[test]
fn h2_gets_linekind_h2() {
    let page = ParsedPage::parse_html("<html><body><h2>Section</h2></body></html>");
    let rp = render_full(&page);
    let idx = rp.lines.iter().position(|l| l.contains("Section")).unwrap();
    assert_eq!(rp.line_kinds[idx], LineKind::H2);
}

#[test]
fn h3_gets_linekind_h3() {
    let page = ParsedPage::parse_html("<html><body><h3>Sub</h3></body></html>");
    let rp = render_full(&page);
    let idx = rp.lines.iter().position(|l| l.contains("Sub")).unwrap();
    assert_eq!(rp.line_kinds[idx], LineKind::H3);
}

#[test]
fn h4_gets_linekind_h4plus() {
    let page = ParsedPage::parse_html("<html><body><h4>Fine</h4></body></html>");
    let rp = render_full(&page);
    let idx = rp.lines.iter().position(|l| l.contains("Fine")).unwrap();
    assert_eq!(rp.line_kinds[idx], LineKind::H4Plus);
}

#[test]
fn h1_line_has_block_glyph_prefix() {
    let page = ParsedPage::parse_html("<html><body><h1>Hello</h1></body></html>");
    let rp = render_full(&page);
    let line = rp.lines.iter().find(|l| l.contains("Hello")).unwrap();
    assert!(line.starts_with("▌ "), "got: {line:?}");
}
```

- [ ] **Step 2: Run — expect failures**

```
cargo test h1_gets_linekind 2>&1 | tail -10
```

Expected: `FAILED` — `line_kinds[idx] == Normal` not `H1`

- [ ] **Step 3: Add heading marker helpers to Ctx**

Add to `impl Ctx` in `src/renderer/text.rs`:
```rust
fn push_heading_marker(&mut self, level: usize) {
    self.buf.push(MARKER);
    self.buf.push('H');
    self.buf.push_str(&level.to_string());
    self.buf.push(MARKER);
}
```

- [ ] **Step 4: Update render_element heading handling**

Replace the existing heading match arms and reset code:

Find and replace:
```rust
match tag {
    "h1" => ctx.push_ansi("\x1b[1;34m"),
    t if heading_level(t).is_some() => ctx.push_ansi("\x1b[1m"),
    "li" => ctx.push_str("  • "),
```
With:
```rust
match tag {
    t if heading_level(t).is_some() => {
        let level = heading_level(t).unwrap();
        ctx.push_heading_marker(level);
        match level {
            1 => ctx.push_str("▌ "),
            2 => ctx.push_str("  "),
            3 => ctx.push_str("    "),
            _ => ctx.push_str("      "),
        }
    }
    "li" => ctx.push_str("  • "),
```

Find and replace the heading reset:
```rust
    if heading.is_some() || tag == "h1" {
        ctx.push_ansi("\x1b[0m");
    }
```
With:
```rust
    // heading styling applied in draw_content via LineKind — no ANSI reset needed
```

Remove the `heading` variable (it's no longer used for ANSI):
```rust
    let heading = heading_level(tag);
```
→ This is still used in the reset check above; replace the whole old block:

The current logic around headings in `render_element` is:
```rust
let is_block = BLOCK.contains(&tag);
let heading = heading_level(tag);

if is_block {
    ctx.push_char('\n');
}

match tag {
    "h1" => ctx.push_ansi("\x1b[1;34m"),
    t if heading_level(t).is_some() => ctx.push_ansi("\x1b[1m"),
    ...
}

// children loop

if heading.is_some() || tag == "h1" {
    ctx.push_ansi("\x1b[0m");
}

if is_block {
    ctx.push_char('\n');
}
```

Replace with:
```rust
let is_block = BLOCK.contains(&tag);

if is_block {
    ctx.push_char('\n');
}

match tag {
    t if heading_level(t).is_some() => {
        let level = heading_level(t).unwrap();
        ctx.push_heading_marker(level);
        match level {
            1 => ctx.push_str("▌ "),
            2 => ctx.push_str("  "),
            3 => ctx.push_str("    "),
            _ => ctx.push_str("      "),
        }
    }
    "li" => ctx.push_str("  • "),
    "hr" => {
        ctx.push_str("────────────────────────────────────────");
        ctx.push_char('\n');
        return;
    }
    _ => {}
}

for child in el.children() {
    // ... (unchanged)
}

// No ANSI reset needed — heading style via LineKind metadata

if is_block {
    ctx.push_char('\n');
}
```

- [ ] **Step 5: Run tests**

```
cargo test
```

Expected: all heading tests pass, existing tests still pass.

- [ ] **Step 6: Commit**

```
git add src/renderer/text.rs
git commit -m "feat(renderer): heading LineKind markers + h1 block glyph prefix"
```

---

### Task 3: Code span renderer

**Files:**
- Modify: `src/renderer/text.rs`

- [ ] **Step 1: Write failing tests**

```rust
#[test]
fn code_span_recorded_with_offsets() {
    let page = ParsedPage::parse_html(
        "<html><body><p>Use <code>rustup</code> to install</p></body></html>"
    );
    let rp = render_full(&page);
    assert_eq!(rp.code_spans.len(), 1, "expected 1 code span");
    let cs = &rp.code_spans[0];
    let line = &rp.lines[cs.line];
    let slice: String = line.chars().skip(cs.start).take(cs.end - cs.start).collect();
    assert_eq!(slice, "rustup", "got slice: {slice:?} from line: {line:?}");
}

#[test]
fn multiple_code_spans_recorded() {
    let page = ParsedPage::parse_html(
        "<html><body><p><code>foo</code> and <code>bar</code></p></body></html>"
    );
    let rp = render_full(&page);
    assert_eq!(rp.code_spans.len(), 2);
}
```

- [ ] **Step 2: Run — expect failures**

```
cargo test code_span_recorded 2>&1 | tail -10
```

Expected: `FAILED` — `code_spans.len() == 0`

- [ ] **Step 3: Add code marker helpers to Ctx**

Add to `impl Ctx`:
```rust
fn push_code_start_marker(&mut self, idx: usize) {
    self.buf.push(MARKER);
    self.buf.push('C');
    self.buf.push_str(&idx.to_string());
    self.buf.push(MARKER);
}

fn push_code_end_marker(&mut self, idx: usize) {
    self.buf.push(MARKER);
    self.buf.push('Z');
    self.buf.push_str(&idx.to_string());
    self.buf.push(MARKER);
}
```

- [ ] **Step 4: Add render_code_span function**

Add after `render_link`:
```rust
fn render_code_span(el: ElementRef<'_>, ctx: &mut Ctx) {
    let text: String = el.text().collect();
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    let idx = ctx.code_spans.len();
    ctx.code_spans.push(CodeSpan { line: 0, start: 0, end: 0 });
    ctx.push_code_start_marker(idx);
    ctx.push_str(text);
    ctx.push_code_end_marker(idx);
}
```

- [ ] **Step 5: Wire render_code_span into render_element**

In the `render_element` function, add early return for code tags before the block element logic:

Add at the top of `render_element`, after the `SKIP` check:
```rust
if matches!(tag, "code" | "kbd" | "tt") {
    render_code_span(el, ctx);
    return;
}
```

- [ ] **Step 6: Run tests**

```
cargo test
```

Expected: all code span tests pass, all prior tests still pass.

- [ ] **Step 7: Commit**

```
git add src/renderer/text.rs
git commit -m "feat(renderer): inline code span markers with char-offset tracking"
```

---

### Task 5: Catppuccin constants + chrome redesign

**Files:**
- Modify: `src/tui/ui.rs`

- [ ] **Step 1: Replace all color constants and update draw function**

Replace the entire `src/tui/ui.rs` file content:

```rust
//! ratatui layout composition: tab bar, address bar, content pane, search bar, status bar.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::app::{App, InputMode};
use crate::renderer::text::{CodeSpan, LineKind};

// ── Catppuccin Mocha palette ──────────────────────────────────────────────────
const C_BASE: Color = Color::Rgb(30, 30, 46);
const C_CRUST: Color = Color::Rgb(24, 24, 37);
const C_SURFACE0: Color = Color::Rgb(49, 50, 68);
const C_SURFACE1: Color = Color::Rgb(69, 71, 90);
const C_TEXT: Color = Color::Rgb(205, 214, 244);
const C_SUBTEXT: Color = Color::Rgb(166, 173, 200);
const C_OVERLAY: Color = Color::Rgb(108, 112, 134);
const C_MAUVE: Color = Color::Rgb(203, 166, 247);
const C_BLUE: Color = Color::Rgb(137, 180, 250);
const C_SKY: Color = Color::Rgb(137, 220, 235);
const C_GREEN: Color = Color::Rgb(166, 227, 161);
const C_RED: Color = Color::Rgb(243, 139, 168);
const C_PINK: Color = Color::Rgb(245, 194, 231);

/// Render the full TUI frame.
pub fn draw(frame: &mut Frame, app: &App) {
    let tab = app.tabs.current();
    let in_search = matches!(app.input_mode, InputMode::Search(_));

    // Tab bar always visible (1) + address bar (1) + content (fill) + search? + status (1)
    let mut constraints = vec![
        Constraint::Length(1), // tab bar
        Constraint::Length(1), // address bar
        Constraint::Fill(1),   // content
    ];
    if in_search {
        constraints.push(Constraint::Length(1));
    }
    constraints.push(Constraint::Length(1)); // status bar

    let areas = Layout::vertical(constraints).split(frame.area());

    draw_tab_bar(frame, app, areas[0]);
    draw_address_bar(frame, app, areas[1]);

    let content_area = areas[2];
    draw_content(frame, app, content_area);

    let mut status_idx = 3;
    if in_search {
        let query = if let InputMode::Search(ref q) = app.input_mode { q.as_str() } else { "" };
        let match_info = if !query.is_empty() && tab.search_matches.is_empty() {
            " (no matches)".to_owned()
        } else if !tab.search_matches.is_empty() {
            format!(" ({}/{})", tab.search_idx + 1, tab.search_matches.len())
        } else {
            String::new()
        };
        frame.render_widget(
            Paragraph::new(format!("/{query}{match_info}"))
                .style(Style::new().bg(C_SURFACE0).fg(C_TEXT)),
            areas[3],
        );
        status_idx = 4;
    }

    draw_status_bar(frame, app, areas[status_idx]);
}

fn draw_tab_bar(frame: &mut Frame, app: &App, area: Rect) {
    let spans: Vec<Span> = app
        .tabs
        .tabs
        .iter()
        .enumerate()
        .map(|(i, tab)| {
            let title = if tab.title.is_empty() {
                tab.url.split('/').nth(2).unwrap_or("tab").to_owned()
            } else {
                tab.title.chars().take(20).collect()
            };
            let loading = if tab.loading { " ⟳" } else { "" };
            let label = format!(" {}: {title}{loading} ", i + 1);
            let style = if i == app.tabs.active {
                Style::new()
                    .bg(C_BASE)
                    .fg(C_TEXT)
                    .add_modifier(Modifier::BOLD)
                    .add_modifier(Modifier::UNDERLINED)
            } else {
                Style::new().bg(C_CRUST).fg(C_OVERLAY)
            };
            Span::styled(label, style)
        })
        .collect();

    let bg_fill = Span::styled(
        " ".repeat(frame.area().width as usize),
        Style::new().bg(C_CRUST),
    );
    let mut all_spans = spans;
    all_spans.push(bg_fill);
    frame.render_widget(
        Paragraph::new(Line::from(all_spans)).style(Style::new().bg(C_CRUST)),
        area,
    );
}

fn draw_address_bar(frame: &mut Frame, app: &App, area: Rect) {
    let tab = app.tabs.current();

    if let InputMode::Url(ref buf) = app.input_mode {
        let line = Line::from(vec![
            Span::styled(" ▸ ", Style::new().bg(C_SURFACE1).fg(C_BLUE).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{buf}█"), Style::new().bg(C_SURFACE1).fg(C_TEXT)),
        ]);
        frame.render_widget(
            Paragraph::new(line).style(Style::new().bg(C_SURFACE1)),
            area,
        );
        return;
    }

    let dot_color = if tab.url.starts_with("https://") { C_GREEN } else { C_RED };
    let (bm_char, bm_color) = if app.bookmarks.contains(&tab.url) {
        ("★", C_PINK)
    } else {
        ("☆", C_OVERLAY)
    };
    let loading_suffix = if tab.loading { " ⟳" } else { "" };

    let line = Line::from(vec![
        Span::styled(" ● ", Style::new().bg(C_SURFACE0).fg(dot_color)),
        Span::styled(
            format!("{}{loading_suffix}", tab.url),
            Style::new().bg(C_SURFACE0).fg(C_TEXT),
        ),
        Span::styled(format!(" {bm_char} "), Style::new().bg(C_SURFACE0).fg(bm_color)),
    ]);
    frame.render_widget(
        Paragraph::new(line).style(Style::new().bg(C_SURFACE0)),
        area,
    );
}

fn draw_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let tab = app.tabs.current();
    let (mode_label, mode_color) = match &app.input_mode {
        InputMode::Normal => ("NORMAL", C_BLUE),
        InputMode::Search(_) => ("SEARCH", C_RED),
        InputMode::Url(_) => ("URL", C_MAUVE),
        InputMode::Hint(_) => ("HINT", C_GREEN),
    };
    let hints = match &app.input_mode {
        InputMode::Normal => " o:open  f:hints  /:search  b:bmark  j/k:scroll  t:tab  x:close",
        InputMode::Search(_) => " Esc:cancel  Enter:done  n/N:next/prev",
        InputMode::Url(_) => " Enter:go  Esc:cancel  Ctrl+W:clear-word  Backspace:del",
        InputMode::Hint(_) => " type letters to follow  ·  Shift+letters:new tab  ·  Esc:cancel",
    };
    let scroll_info = format!(" {}/{} ", tab.scroll + 1, tab.lines.len().max(1));

    let status_msg = if !app.status.is_empty() {
        format!(" {} ", app.status)
    } else {
        hints.to_owned()
    };

    let line = Line::from(vec![
        Span::styled(
            format!(" {mode_label} "),
            Style::new().bg(mode_color).fg(C_CRUST).add_modifier(Modifier::BOLD),
        ),
        Span::styled(status_msg, Style::new().bg(C_CRUST).fg(C_OVERLAY)),
        Span::styled(scroll_info, Style::new().bg(C_CRUST).fg(C_SUBTEXT)),
    ]);
    frame.render_widget(
        Paragraph::new(line).style(Style::new().bg(C_CRUST)),
        area,
    );
}

fn draw_content(frame: &mut Frame, app: &App, area: Rect) {
    let tab = app.tabs.current();

    if tab.lines.is_empty() && !tab.loading {
        let msg = if app.status.is_empty() { "No content" } else { app.status.as_str() };
        frame.render_widget(
            Paragraph::new(msg).style(Style::new().fg(C_RED).bg(C_BASE)),
            area,
        );
        return;
    }

    let viewport_h = area.height as usize;
    let in_hint_mode = matches!(app.input_mode, InputMode::Hint(_));
    let typed_hint = if let InputMode::Hint(ref s) = app.input_mode { s.as_str() } else { "" };

    let visible: Vec<Line> = tab
        .lines
        .iter()
        .enumerate()
        .skip(tab.scroll)
        .take(viewport_h)
        .map(|(i, text)| {
            let is_selected_link = !in_hint_mode
                && tab.selected_link.and_then(|sel| tab.links.get(sel)).map(|rl| rl.line == i).unwrap_or(false);
            let is_current_match = !tab.search_matches.is_empty() && tab.search_matches[tab.search_idx] == i;
            let is_other_match = !is_current_match && tab.search_matches.contains(&i);

            // Hint badge for this line
            let hint_badge: Option<(String, bool)> = if in_hint_mode {
                app.hint_codes.iter().find(|(link_idx, _)| {
                    tab.links.get(*link_idx).map(|l| l.line == i).unwrap_or(false)
                }).map(|(_, code)| {
                    let matches = code.starts_with(typed_hint);
                    (code.clone(), matches)
                })
            } else {
                None
            };

            let kind = tab.line_kinds.get(i).copied().unwrap_or_default();
            let base_style = match kind {
                LineKind::H1 => Style::new().fg(C_MAUVE).add_modifier(Modifier::BOLD),
                LineKind::H2 => Style::new().fg(C_BLUE).add_modifier(Modifier::BOLD),
                LineKind::H3 => Style::new().fg(C_SKY),
                LineKind::H4Plus => Style::new().fg(C_SUBTEXT).add_modifier(Modifier::ITALIC),
                LineKind::Normal => Style::new().fg(C_TEXT),
            };

            let mut spans: Vec<Span> = vec![Span::raw("  ")]; // 2-char left margin

            if is_selected_link {
                spans.push(Span::styled(
                    text.as_str(),
                    Style::new().bg(C_SURFACE1).fg(C_SKY).add_modifier(Modifier::UNDERLINED),
                ));
            } else if is_current_match {
                spans.push(Span::styled(text.as_str(), Style::new().bg(C_RED).fg(C_CRUST)));
            } else if is_other_match {
                spans.push(Span::styled(text.as_str(), Style::new().bg(C_SURFACE1).fg(C_RED)));
            } else {
                let line_code_spans: Vec<&CodeSpan> =
                    tab.code_spans.iter().filter(|cs| cs.line == i).collect();
                if line_code_spans.is_empty() {
                    spans.push(Span::styled(text.as_str(), base_style));
                } else {
                    spans.extend(build_code_spans(text, &line_code_spans, base_style));
                }
            }

            // Append hint badge
            if let Some((code, matches)) = hint_badge {
                let badge_style = if typed_hint.is_empty() {
                    Style::new().bg(C_GREEN).fg(C_CRUST).add_modifier(Modifier::BOLD)
                } else if matches {
                    Style::new().bg(C_RED).fg(C_CRUST).add_modifier(Modifier::BOLD)
                } else {
                    Style::new().bg(C_SURFACE1).fg(C_OVERLAY)
                };
                spans.push(Span::styled(format!(" {code} "), badge_style));
            }

            Line::from(spans)
        })
        .collect();

    frame.render_widget(
        Paragraph::new(visible).style(Style::new().bg(C_BASE)),
        area,
    );
}

/// Build ratatui spans for a line with inline code spans highlighted in green italic.
fn build_code_spans<'a>(text: &'a str, spans: &[&CodeSpan], base: Style) -> Vec<Span<'a>> {
    let code_style = Style::new().fg(C_GREEN).add_modifier(Modifier::ITALIC);
    let chars: Vec<char> = text.chars().collect();
    let mut result = Vec::new();
    let mut pos = 0usize;
    let mut sorted: Vec<&&CodeSpan> = spans.iter().collect();
    sorted.sort_by_key(|cs| cs.start);
    for cs in sorted {
        let start = cs.start.min(chars.len());
        let end = cs.end.min(chars.len());
        if start > pos {
            result.push(Span::styled(chars[pos..start].iter().collect::<String>(), base));
        }
        if end > start {
            result.push(Span::styled(chars[start..end].iter().collect::<String>(), code_style));
        }
        pos = end;
    }
    if pos < chars.len() {
        result.push(Span::styled(chars[pos..].iter().collect::<String>(), base));
    }
    result
}

#[cfg(test)]
mod tests {
    #[test]
    fn placeholder() {}
}
```

- [ ] **Step 2: Run — fix any compile errors**

```
cargo check 2>&1 | head -40
```

Fix errors, then:
```
cargo test
```

Expected: all tests pass.

- [ ] **Step 3: Commit**

```
git add src/tui/ui.rs
git commit -m "feat(ui): Catppuccin Mocha theme — tab bar always-on, mode badge, Vimium hint overlay"
```

---

### Task 4: InputMode::Url + handle_url keybinds

**Files:**
- Modify: `src/app.rs`
- Modify: `src/tui/keybinds.rs`

- [ ] **Step 1: Write failing tests**

Add to `src/app.rs` tests:
```rust
#[test]
fn o_key_enters_url_mode_with_current_url() {
    let mut app = make_app();
    app.tabs.current_mut().loading = false;
    let (tx, _rx) = tokio::sync::mpsc::channel(1);
    let key = crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('o'),
        crossterm::event::KeyModifiers::NONE,
    );
    crate::tui::keybinds::handle(key, &mut app, &tx);
    assert!(matches!(app.input_mode, InputMode::Url(ref s) if s == "https://example.com"));
}
```

Add to `src/tui/keybinds.rs` tests:
```rust
#[test]
fn url_mode_char_appends() {
    let mut app = make_app();
    app.input_mode = InputMode::Url("https://".into());
    let (tx, _rx) = tokio::sync::mpsc::channel(1);
    let key = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE);
    handle(key, &mut app, &tx);
    assert!(matches!(&app.input_mode, InputMode::Url(s) if s == "https://x"));
}

#[test]
fn url_mode_backspace_pops() {
    let mut app = make_app();
    app.input_mode = InputMode::Url("https://abc".into());
    let (tx, _rx) = tokio::sync::mpsc::channel(1);
    let key = KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE);
    handle(key, &mut app, &tx);
    assert!(matches!(&app.input_mode, InputMode::Url(s) if s == "https://ab"));
}

#[test]
fn url_mode_esc_returns_normal() {
    let mut app = make_app();
    app.input_mode = InputMode::Url("https://foo.com".into());
    let (tx, _rx) = tokio::sync::mpsc::channel(1);
    let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
    handle(key, &mut app, &tx);
    assert!(matches!(app.input_mode, InputMode::Normal));
}

#[test]
fn url_mode_ctrl_w_clears_last_segment() {
    let mut app = make_app();
    app.input_mode = InputMode::Url("https://example.com/foo/bar".into());
    let (tx, _rx) = tokio::sync::mpsc::channel(1);
    let key = KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL);
    handle(key, &mut app, &tx);
    assert!(matches!(&app.input_mode, InputMode::Url(s) if s == "https://example.com/foo/"));
}
```

- [ ] **Step 2: Run — expect failures**

```
cargo test url_mode 2>&1 | tail -15
```

Expected: compile errors or FAILED

- [ ] **Step 3: Extend InputMode and App in app.rs**

In `src/app.rs`, update `InputMode`:
```rust
pub enum InputMode {
    Normal,
    Search(String),
    Url(String),  // edit buffer, pre-filled with current URL
    Hint(String), // typed hint letters so far
}
```

Add `hint_codes` field to `App`:
```rust
pub struct App {
    pub tabs: TabManager,
    pub bookmarks: Bookmarks,
    pub settings: Settings,
    pub status: String,
    pub quit: bool,
    pub input_mode: InputMode,
    pub hint_codes: Vec<(usize, String)>, // (link_index, 2-letter code)
}
```

Update `App::new`:
```rust
pub fn new(url: String, settings: Settings, bookmarks: Bookmarks) -> Self {
    Self {
        tabs: TabManager::new(url),
        bookmarks,
        settings,
        status: String::new(),
        quit: false,
        input_mode: InputMode::Normal,
        hint_codes: Vec::new(),
    }
}
```

- [ ] **Step 4: Add handle_url and clear_last_word to keybinds.rs**

Update `handle` dispatch at top of `keybinds.rs`:
```rust
pub fn handle(key: KeyEvent, app: &mut App, tx: &Sender<BgMsg>) {
    match &app.input_mode {
        InputMode::Search(_) => handle_search(key, app),
        InputMode::Url(_) => handle_url(key, app, tx),
        InputMode::Hint(_) => handle_hint(key, app, tx),
        InputMode::Normal => handle_normal(key, app, tx),
    }
}
```

Add `handle_url` function:
```rust
fn handle_url(key: KeyEvent, app: &mut App, tx: &Sender<BgMsg>) {
    let buf = match &app.input_mode {
        InputMode::Url(b) => b.clone(),
        _ => return,
    };
    match (key.modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::Esc) => {
            app.input_mode = InputMode::Normal;
        }
        (KeyModifiers::NONE, KeyCode::Enter) => {
            let url = buf.clone();
            app.input_mode = InputMode::Normal;
            if !url.is_empty() {
                app.navigate(url, tx);
            }
        }
        (KeyModifiers::NONE, KeyCode::Backspace) => {
            let mut b = buf;
            b.pop();
            app.input_mode = InputMode::Url(b);
        }
        (KeyModifiers::CONTROL, KeyCode::Char('w')) => {
            app.input_mode = InputMode::Url(clear_last_segment(buf));
        }
        (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
            let mut b = buf;
            b.push(c);
            app.input_mode = InputMode::Url(b);
        }
        _ => {}
    }
}

fn clear_last_segment(mut s: String) -> String {
    // Pop trailing slash/space, then pop to previous slash
    while s.ends_with('/') || s.ends_with(' ') {
        s.pop();
    }
    while !s.is_empty() && !s.ends_with('/') && !s.ends_with(' ') {
        s.pop();
    }
    s
}
```

Add `handle_hint` stub (full impl in Task 6):
```rust
fn handle_hint(key: KeyEvent, app: &mut App, tx: &Sender<BgMsg>) {
    if let (KeyModifiers::NONE, KeyCode::Esc) = (key.modifiers, key.code) {
        app.hint_codes.clear();
        app.input_mode = InputMode::Normal;
    }
}
```

Wire `o` key in `handle_normal`:
```rust
(KeyModifiers::NONE, KeyCode::Char('o')) => {
    let url = app.tabs.current().url.clone();
    app.input_mode = InputMode::Url(url);
}
```

Add `f` key in `handle_normal` (full impl in Task 6):
```rust
(KeyModifiers::NONE, KeyCode::Char('f')) => {
    app.enter_hint_mode();
}
```

Add `enter_hint_mode` stub to `App` in `app.rs`:
```rust
pub fn enter_hint_mode(&mut self) {
    let tab = self.tabs.current();
    if tab.links.is_empty() {
        self.status = "No links on page".into();
        return;
    }
    // Full impl in Task 6
    self.input_mode = InputMode::Hint(String::new());
}
```

- [ ] **Step 5: Run tests**

```
cargo test
```

Expected: all URL mode tests pass.

- [ ] **Step 6: Commit**

```
git add src/app.rs src/tui/keybinds.rs
git commit -m "feat(url-mode): InputMode::Url — o key, inline address bar editing, Ctrl+W"
```

---

### Task 6: Vimium hint generation + full handle_hint

> Must run after Task 4 (InputMode variants defined) and Task 5 (ui.rs compiled).

**Files:**
- Modify: `src/app.rs`
- Modify: `src/tui/keybinds.rs`

- [ ] **Step 1: Write failing tests**

Add to `src/app.rs` tests:
```rust
#[test]
fn enter_hint_mode_assigns_codes_for_all_links() {
    let mut app = make_app();
    let tab = app.tabs.current_mut();
    tab.links = vec![
        crate::renderer::text::RenderedLink { href: "https://a.com".into(), line: 0 },
        crate::renderer::text::RenderedLink { href: "https://b.com".into(), line: 1 },
        crate::renderer::text::RenderedLink { href: "https://c.com".into(), line: 2 },
    ];
    tab.loading = false;
    app.enter_hint_mode();
    assert!(matches!(app.input_mode, InputMode::Hint(_)));
    assert_eq!(app.hint_codes.len(), 3);
    // HINT_CHARS = [A,S,D,F,G,...]; codes: AA, AS, AD
    assert_eq!(app.hint_codes[0], (0, "AA".to_string()));
    assert_eq!(app.hint_codes[1], (1, "AS".to_string()));
    assert_eq!(app.hint_codes[2], (2, "AD".to_string()));
}

#[test]
fn enter_hint_mode_with_no_links_stays_normal() {
    let mut app = make_app();
    app.tabs.current_mut().loading = false;
    app.enter_hint_mode();
    assert!(matches!(app.input_mode, InputMode::Normal));
    assert!(!app.status.is_empty());
}
```

Add to `src/tui/keybinds.rs` tests:
```rust
#[test]
fn hint_mode_esc_returns_normal_and_clears_codes() {
    let mut app = make_app();
    app.input_mode = InputMode::Hint("A".into());
    app.hint_codes = vec![(0, "AA".into())];
    let (tx, _rx) = tokio::sync::mpsc::channel(1);
    let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
    handle(key, &mut app, &tx);
    assert!(matches!(app.input_mode, InputMode::Normal));
    assert!(app.hint_codes.is_empty());
}

#[test]
fn hint_mode_first_char_updates_typed() {
    let mut app = make_app();
    app.input_mode = InputMode::Hint(String::new());
    app.hint_codes = vec![(0, "AA".into()), (1, "AS".into())];
    let (tx, _rx) = tokio::sync::mpsc::channel(1);
    let key = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
    handle(key, &mut app, &tx);
    assert!(matches!(&app.input_mode, InputMode::Hint(s) if s == "A"));
}
```

- [ ] **Step 2: Run — expect failures**

```
cargo test enter_hint_mode 2>&1 | tail -15
```

Expected: `FAILED` — codes don't match expected

- [ ] **Step 3: Implement enter_hint_mode and generate_hint_codes in app.rs**

Replace the stub `enter_hint_mode` with:
```rust
const HINT_CHARS: &[char] = &[
    'A', 'S', 'D', 'F', 'G', 'H', 'J', 'K', 'L',
    'Q', 'W', 'E', 'R', 'T', 'Y', 'U', 'I', 'O', 'P',
    'Z', 'X', 'C', 'V', 'B', 'N', 'M',
];

pub fn enter_hint_mode(&mut self) {
    let tab = self.tabs.current();
    if tab.links.is_empty() {
        self.status = "No links on page".into();
        return;
    }
    let n = HINT_CHARS.len();
    self.hint_codes = tab
        .links
        .iter()
        .enumerate()
        .map(|(pos, _)| {
            let first = HINT_CHARS[pos / n];
            let second = HINT_CHARS[pos % n];
            (pos, format!("{first}{second}"))
        })
        .collect();
    self.input_mode = InputMode::Hint(String::new());
}
```

- [ ] **Step 4: Implement full handle_hint in keybinds.rs**

Replace the stub `handle_hint` with:
```rust
fn handle_hint(key: KeyEvent, app: &mut App, tx: &Sender<BgMsg>) {
    let typed = match &app.input_mode {
        InputMode::Hint(s) => s.clone(),
        _ => return,
    };
    match key.code {
        KeyCode::Esc => {
            app.hint_codes.clear();
            app.input_mode = InputMode::Normal;
        }
        KeyCode::Backspace => {
            let mut t = typed;
            t.pop();
            app.input_mode = InputMode::Hint(t);
        }
        KeyCode::Char(c) => {
            let open_new_tab = key.modifiers.contains(KeyModifiers::SHIFT);
            let upper = c.to_ascii_uppercase();
            let mut new_typed = typed;
            new_typed.push(upper);

            if new_typed.len() >= 2 {
                let matched = app
                    .hint_codes
                    .iter()
                    .find(|(_, code)| *code == new_typed)
                    .map(|(link_idx, _)| *link_idx);
                app.hint_codes.clear();
                app.input_mode = InputMode::Normal;
                if let Some(link_idx) = matched {
                    let href = app.tabs.current().links.get(link_idx).map(|l| l.href.clone());
                    if let Some(href) = href {
                        if open_new_tab {
                            app.open_new_tab(href, tx);
                        } else {
                            app.navigate(href, tx);
                        }
                    }
                } else {
                    app.status = format!("No hint '{new_typed}'");
                }
            } else {
                app.input_mode = InputMode::Hint(new_typed);
            }
        }
        _ => {}
    }
}
```

- [ ] **Step 5: Run tests**

```
cargo test
```

Expected: all hint tests pass.

- [ ] **Step 6: Commit**

```
git add src/app.rs src/tui/keybinds.rs
git commit -m "feat(hints): Vimium-style 2-letter link hints — f key, AA/AS/AD codes, Shift=new tab"
```

---

### Task 7: Final verification

- [ ] **Step 1: Full test suite**

```
cargo test
```

Expected: all tests pass, 0 failures.

- [ ] **Step 2: Clippy**

```
cargo clippy -- -D warnings
```

Fix any warnings before proceeding.

- [ ] **Step 3: Build release**

```
cargo build --release
```

Expected: clean build.

- [ ] **Step 4: Smoke test**

```
cargo run -- https://example.com
```

Verify:
- Tab bar visible with 1 tab
- Address bar shows green ● for HTTPS
- Mode badge shows `NORMAL` in blue
- Press `o` → address bar changes to URL edit mode with purple `URL` badge
- Type characters, Backspace, Ctrl+W work
- Press `Esc` → returns to Normal
- Press `f` → `HINT` badge appears green; links get `AA`/`AS`/`AD` badges
- Type hint letters → navigates to link
- Press `j`/`k` → content scrolls with 2-char left margin

- [ ] **Step 5: Final commit if any fixes made**

```
git add -p
git commit -m "fix(ui): smoke test corrections"
```
