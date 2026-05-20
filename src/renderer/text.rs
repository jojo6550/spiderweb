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

/// Render `page` to a terminal-ready string with ANSI escape codes.
pub fn render(page: &ParsedPage) -> String {
    let mut buf = String::with_capacity(4096);
    render_element(page.document().root_element(), &mut buf);
    normalize(&buf)
}

fn render_element(el: ElementRef<'_>, buf: &mut String) {
    let tag = el.value().name();

    if SKIP.contains(&tag) {
        return;
    }

    let is_block = BLOCK.contains(&tag);
    let heading = heading_level(tag);

    if is_block {
        buf.push('\n');
    }

    match tag {
        "h1" => buf.push_str("\x1b[1;34m"),
        t if heading_level(t).is_some() => buf.push_str("\x1b[1m"),
        "li" => buf.push_str("  • "),
        "hr" => {
            buf.push_str("────────────────────────────────────────");
            buf.push('\n');
            return;
        }
        _ => {}
    }

    for child in el.children() {
        match child.value() {
            Node::Text(text) => {
                let s = text.trim();
                if !s.is_empty() {
                    buf.push_str(s);
                    buf.push(' ');
                }
            }
            Node::Element(_) => {
                if let Some(child_el) = ElementRef::wrap(child) {
                    if child_el.value().name() == "a" {
                        render_link(child_el, buf);
                    } else {
                        render_element(child_el, buf);
                    }
                }
            }
            _ => {}
        }
    }

    if heading.is_some() || tag == "h1" {
        buf.push_str("\x1b[0m");
    }

    if is_block {
        buf.push('\n');
    }
}

fn render_link(el: ElementRef<'_>, buf: &mut String) {
    let href = el.value().attr("href").unwrap_or("");
    let text: String = el.text().collect();
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    buf.push_str("\x1b[4;36m");
    buf.push_str(text);
    buf.push_str("\x1b[0m");
    if !href.is_empty() {
        buf.push_str("\x1b[2m[");
        buf.push_str(href);
        buf.push_str("]\x1b[0m");
    }
    buf.push(' ');
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

/// Strip ANSI escape sequences from a string.
pub fn strip_ansi(s: &str) -> String {
    let mut out = String::new();
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
        let page = ParsedPage::parse_html(
            r#"<html><body><a href="/about">About</a></body></html>"#,
        );
        let out = render(&page);
        assert!(out.contains("About"));
        assert!(out.contains("/about"));
    }

    #[test]
    fn list_items_bulleted() {
        let page =
            ParsedPage::parse_html("<html><body><ul><li>One</li><li>Two</li></ul></body></html>");
        let plain = strip_ansi(&render(&page));
        assert!(plain.contains("• One"), "got: {plain:?}");
        assert!(plain.contains("• Two"));
    }

    #[test]
    fn empty_body_no_panic() {
        let page = ParsedPage::parse_html("<html><body></body></html>");
        let out = render(&page);
        assert!(out.is_empty() || out.chars().all(|c| c.is_whitespace()));
    }
}
