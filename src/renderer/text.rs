//! DOM → terminal text with ANSI color mapping.

use scraper::{ElementRef, Node};

use crate::parser::html::ParsedPage;

const BLOCK: &[&str] = &[
    "address", "article", "aside", "blockquote", "dd", "details", "dialog",
    "div", "dl", "dt", "fieldset", "figcaption", "figure", "footer", "form",
    "h1", "h2", "h3", "h4", "h5", "h6", "header", "hgroup", "hr", "li",
    "main", "menu", "nav", "ol", "p", "pre", "section", "summary", "table",
    "tbody", "td", "th", "thead", "tr", "ul",
];

const SKIP: &[&str] = &[
    "head", "math", "noscript", "script", "style", "svg", "template",
];

// ── Public output types ───────────────────────────────────────────────────────

/// A link extracted from the rendered page with its display line number.
#[derive(Debug, Clone)]
pub struct RenderedLink {
    pub href: String,
    /// Zero-based index into [`RenderedPage::lines`].
    pub line: usize,
}

/// Full render result: plain-text lines plus link metadata.
pub struct RenderedPage {
    pub lines: Vec<String>,
    pub links: Vec<RenderedLink>,
}

// ── Render context ────────────────────────────────────────────────────────────

struct Ctx {
    buf: String,
    links: Vec<RenderedLink>,
    /// Current line index (counts `\n` written to `buf`).
    line: usize,
}

impl Ctx {
    fn new() -> Self {
        Self { buf: String::with_capacity(4096), links: Vec::new(), line: 0 }
    }

    fn push_str(&mut self, s: &str) {
        for ch in s.chars() {
            if ch == '\n' {
                self.line += 1;
            }
            self.buf.push(ch);
        }
    }

    fn push_char(&mut self, ch: char) {
        if ch == '\n' {
            self.line += 1;
        }
        self.buf.push(ch);
    }

    /// Push a raw ANSI escape sequence (no `\n` inside — line count unaffected).
    fn push_ansi(&mut self, s: &str) {
        self.buf.push_str(s);
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Render `page` to a terminal-ready ANSI string.
pub fn render(page: &ParsedPage) -> String {
    let mut ctx = Ctx::new();
    render_element(page.document().root_element(), &mut ctx);
    normalize(&ctx.buf)
}

/// Render `page` to plain-text lines with link position metadata.
pub fn render_full(page: &ParsedPage) -> RenderedPage {
    let mut ctx = Ctx::new();
    render_element(page.document().root_element(), &mut ctx);
    let normalized = normalize(&ctx.buf);
    let lines: Vec<String> = strip_ansi(&normalized).lines().map(str::to_owned).collect();

    // Re-map link line numbers through normalize's newline collapsing.
    // Simple approach: search plain lines for link text / href.
    let links = ctx
        .links
        .into_iter()
        .map(|mut rl| {
            // Find the actual line in the normalized output.
            rl.line = lines
                .iter()
                .position(|l| !l.trim().is_empty() && l.contains(&rl.href))
                .unwrap_or(rl.line.min(lines.len().saturating_sub(1)));
            rl
        })
        .collect();

    RenderedPage { lines, links }
}

/// Strip ANSI escape sequences from a string.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut esc = false;
    for ch in s.chars() {
        match ch {
            '\x1b' => esc = true,
            'm' if esc => esc = false,
            _ if esc => {}
            _ => out.push(ch),
        }
    }
    out
}

// ── Internal render functions ─────────────────────────────────────────────────

fn render_element(el: ElementRef<'_>, ctx: &mut Ctx) {
    let tag = el.value().name();

    if SKIP.contains(&tag) {
        return;
    }

    let is_block = BLOCK.contains(&tag);
    let heading = heading_level(tag);

    if is_block {
        ctx.push_char('\n');
    }

    match tag {
        "h1" => ctx.push_ansi("\x1b[1;34m"),
        t if heading_level(t).is_some() => ctx.push_ansi("\x1b[1m"),
        "li" => ctx.push_str("  • "),
        "hr" => {
            ctx.push_str("────────────────────────────────────────");
            ctx.push_char('\n');
            return;
        }
        _ => {}
    }

    for child in el.children() {
        match child.value() {
            Node::Text(text) => {
                let s = text.trim();
                if !s.is_empty() {
                    ctx.push_str(s);
                    ctx.push_char(' ');
                }
            }
            Node::Element(_) => {
                if let Some(child_el) = ElementRef::wrap(child) {
                    match child_el.value().name() {
                        "a" => render_link(child_el, ctx),
                        "img" => render_img(child_el, ctx),
                        _ => render_element(child_el, ctx),
                    }
                }
            }
            _ => {}
        }
    }

    if heading.is_some() || tag == "h1" {
        ctx.push_ansi("\x1b[0m");
    }

    if is_block {
        ctx.push_char('\n');
    }
}

fn render_img(el: ElementRef<'_>, ctx: &mut Ctx) {
    let src = el.value().attr("src").unwrap_or("");
    let alt = el.value().attr("alt").unwrap_or("img");
    ctx.push_ansi("\x1b[2m[");
    ctx.push_str(alt);
    if !src.is_empty() {
        ctx.push_str(": ");
        ctx.push_str(src);
    }
    ctx.push_str("]\x1b[0m ");
}

fn render_link(el: ElementRef<'_>, ctx: &mut Ctx) {
    let href = el.value().attr("href").unwrap_or("");
    let text: String = el.text().collect();
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    let line = ctx.line;
    ctx.push_ansi("\x1b[4;36m");
    ctx.push_str(text);
    ctx.push_ansi("\x1b[0m");
    if !href.is_empty() {
        ctx.push_ansi("\x1b[2m[");
        ctx.push_str(href);
        ctx.push_str("]\x1b[0m");
        ctx.links.push(RenderedLink { href: href.to_owned(), line });
    }
    ctx.push_char(' ');
}

fn heading_level(tag: &str) -> Option<usize> {
    match tag {
        "h1" => Some(1),
        "h2" => Some(2),
        "h3" => Some(3),
        "h4" => Some(4),
        "h5" => Some(5),
        "h6" => Some(6),
        _ => None,
    }
}

/// Collapse 3+ consecutive newlines to 2, trim leading/trailing whitespace.
fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut nl: u8 = 0;
    for ch in s.chars() {
        if ch == '\n' {
            nl = nl.saturating_add(1);
            if nl <= 2 {
                out.push(ch);
            }
        } else {
            nl = 0;
            out.push(ch);
        }
    }
    out.trim().to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parser::html::ParsedPage;

    #[test]
    fn heading_in_output() {
        let page = ParsedPage::parse_html("<html><body><h1>Title</h1></body></html>");
        let plain = strip_ansi(&render(&page));
        assert!(plain.contains("Title"), "got: {plain:?}");
    }

    #[test]
    fn script_stripped() {
        let page = ParsedPage::parse_html(
            "<html><body><p>Hello</p><script>var x=1;</script></body></html>",
        );
        let plain = strip_ansi(&render(&page));
        assert!(!plain.contains("var x"), "script leaked: {plain:?}");
        assert!(plain.contains("Hello"));
    }

    #[test]
    fn link_includes_href() {
        let page =
            ParsedPage::parse_html(r#"<html><body><a href="/about">About</a></body></html>"#);
        let out = render(&page);
        assert!(out.contains("About"));
        assert!(out.contains("/about"));
    }

    #[test]
    fn list_items_bulleted() {
        let page = ParsedPage::parse_html(
            "<html><body><ul><li>One</li><li>Two</li></ul></body></html>",
        );
        let plain = strip_ansi(&render(&page));
        assert!(plain.contains("• One"), "got: {plain:?}");
        assert!(plain.contains("• Two"));
    }

    #[test]
    fn render_full_captures_link_line() {
        let page = ParsedPage::parse_html(
            "<html><body><p>Intro</p><a href=\"https://a.com\">A</a></body></html>",
        );
        let rp = render_full(&page);
        assert!(!rp.links.is_empty());
        assert_eq!(rp.links[0].href, "https://a.com");
    }

    #[test]
    fn empty_body_no_panic() {
        let page = ParsedPage::parse_html("<html><body></body></html>");
        let out = render(&page);
        assert!(out.is_empty() || out.chars().all(|c| c.is_whitespace()));
    }
}
