//! DOM → terminal text with ANSI color mapping.

use scraper::{ElementRef, Node};

use crate::parser::css::{self, HiddenSet};
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

/// An image extracted from the rendered page, with its placeholder line.
#[derive(Debug, Clone)]
pub struct RenderedImage {
    pub src: String,
    pub alt: String,
    /// Zero-based index into [`RenderedPage::lines`] of the placeholder.
    pub line: usize,
}

/// Full render result: plain-text lines plus link/image metadata.
pub struct RenderedPage {
    pub lines: Vec<String>,
    pub links: Vec<RenderedLink>,
    pub images: Vec<RenderedImage>,
}

// ── Render context ────────────────────────────────────────────────────────────

// Markers placed inline during render so we can locate links and images
// reliably in the post-normalize output. Private-use area chars survive
// strip_ansi but are stripped from the final output before returning.
const MARKER: char = '\u{F000}';

struct Ctx {
    buf: String,
    links: Vec<RenderedLink>,
    images: Vec<RenderedImage>,
}

impl Ctx {
    fn new() -> Self {
        Self {
            buf: String::with_capacity(4096),
            links: Vec::new(),
            images: Vec::new(),
        }
    }

    fn push_str(&mut self, s: &str) {
        self.buf.push_str(s);
    }

    fn push_char(&mut self, ch: char) {
        self.buf.push(ch);
    }

    /// Push a raw ANSI escape sequence (no `\n` inside).
    fn push_ansi(&mut self, s: &str) {
        self.buf.push_str(s);
    }

    /// Push a link-position marker for link index `idx`.
    fn push_link_marker(&mut self, idx: usize) {
        self.buf.push(MARKER);
        self.buf.push('L');
        self.buf.push_str(&idx.to_string());
        self.buf.push(MARKER);
    }

    /// Push an image-position marker for image index `idx`.
    fn push_image_marker(&mut self, idx: usize) {
        self.buf.push(MARKER);
        self.buf.push('I');
        self.buf.push_str(&idx.to_string());
        self.buf.push(MARKER);
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Render `page` to a terminal-ready ANSI string.
pub fn render(page: &ParsedPage) -> String {
    let hidden = css::extract_hidden(page.document());
    let mut ctx = Ctx::new();
    render_element(page.document().root_element(), &mut ctx, &hidden);
    strip_markers(&normalize(&ctx.buf))
}

/// Strip private-use position markers from `s`.
fn strip_markers(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == MARKER {
            // Skip kind char + digits + closing marker.
            chars.next();
            while chars.peek().is_some_and(|c| c.is_ascii_digit()) {
                chars.next();
            }
            if chars.peek() == Some(&MARKER) {
                chars.next();
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Render `page` to plain-text lines with link and image position metadata.
pub fn render_full(page: &ParsedPage) -> RenderedPage {
    let hidden = css::extract_hidden(page.document());
    let mut ctx = Ctx::new();
    render_element(page.document().root_element(), &mut ctx, &hidden);
    let normalized = normalize(&ctx.buf);
    let stripped = strip_ansi(&normalized);
    let mut lines: Vec<String> = stripped.lines().map(str::to_owned).collect();

    let mut links = ctx.links;
    let mut images = ctx.images;

    // Scan each line for markers; record positions, then strip markers.
    for (line_idx, line) in lines.iter_mut().enumerate() {
        let mut buf = String::with_capacity(line.len());
        let mut iter = line.chars().peekable();
        while let Some(c) = iter.next() {
            if c == MARKER {
                let mut kind = '?';
                let mut digits = String::new();
                if let Some(&k) = iter.peek() {
                    kind = k;
                    iter.next();
                }
                while let Some(&d) = iter.peek() {
                    if d.is_ascii_digit() {
                        digits.push(d);
                        iter.next();
                    } else {
                        break;
                    }
                }
                // Closing marker
                if iter.peek() == Some(&MARKER) {
                    iter.next();
                }
                if let Ok(idx) = digits.parse::<usize>() {
                    match kind {
                        'L' => {
                            if let Some(rl) = links.get_mut(idx) {
                                rl.line = line_idx;
                            }
                        }
                        'I' => {
                            if let Some(ri) = images.get_mut(idx) {
                                ri.line = line_idx;
                            }
                        }
                        _ => {}
                    }
                }
            } else {
                buf.push(c);
            }
        }
        *line = buf;
    }

    RenderedPage { lines, links, images }
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

fn render_element(el: ElementRef<'_>, ctx: &mut Ctx, hidden: &HiddenSet) {
    let tag = el.value().name();

    if SKIP.contains(&tag) || is_hidden(el, hidden) {
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
                    if is_hidden(child_el, hidden) {
                        continue;
                    }
                    match child_el.value().name() {
                        "a" => render_link(child_el, ctx),
                        "img" => render_img(child_el, ctx),
                        "input" => render_input(child_el, ctx),
                        "button" => render_button(child_el, ctx),
                        "textarea" => render_textarea(child_el, ctx),
                        "select" => render_select(child_el, ctx),
                        _ => render_element(child_el, ctx, hidden),
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

/// `true` if the element should be skipped due to inline style, attribute, or
/// CSS rule indicating it's invisible.
fn is_hidden(el: ElementRef<'_>, hidden: &HiddenSet) -> bool {
    let val = el.value();

    if val.attr("hidden").is_some() {
        return true;
    }
    if val.attr("aria-hidden") == Some("true") {
        return true;
    }
    if let Some(style) = val.attr("style") {
        let lower = style.to_ascii_lowercase();
        if lower.contains("display:none")
            || lower.contains("display: none")
            || lower.contains("visibility:hidden")
            || lower.contains("visibility: hidden")
        {
            return true;
        }
    }
    if hidden.tags.contains(val.name()) {
        return true;
    }
    if let Some(id) = val.attr("id") {
        if hidden.ids.contains(id) {
            return true;
        }
    }
    if let Some(class_attr) = val.attr("class") {
        for cls in class_attr.split_whitespace() {
            if hidden.classes.contains(cls) {
                return true;
            }
        }
    }
    false
}

fn render_img(el: ElementRef<'_>, ctx: &mut Ctx) {
    let src = el.value().attr("src").unwrap_or("").to_owned();
    let alt = el.value().attr("alt").unwrap_or("img").to_owned();
    if src.is_empty() {
        ctx.push_ansi("\x1b[2m[");
        ctx.push_str(&alt);
        ctx.push_str("]\x1b[0m ");
        return;
    }
    let idx = ctx.images.len();
    ctx.images.push(RenderedImage { src, alt: alt.clone(), line: 0 });

    // Place image on its own line so splice can replace cleanly.
    ctx.push_char('\n');
    ctx.push_image_marker(idx);
    ctx.push_ansi("\x1b[2m[img: ");
    ctx.push_str(&alt);
    ctx.push_str("]\x1b[0m");
    ctx.push_char('\n');
}

fn render_input(el: ElementRef<'_>, ctx: &mut Ctx) {
    let val = el.value();
    let ty = val.attr("type").unwrap_or("text").to_ascii_lowercase();
    match ty.as_str() {
        "hidden" => {}
        "submit" | "button" | "reset" => {
            let label = val.attr("value").unwrap_or("Submit");
            ctx.push_ansi("\x1b[7m ");
            ctx.push_str(label);
            ctx.push_ansi(" \x1b[0m ");
        }
        "checkbox" => {
            let mark = if val.attr("checked").is_some() { "[x]" } else { "[ ]" };
            ctx.push_str(mark);
            ctx.push_char(' ');
        }
        "radio" => {
            let mark = if val.attr("checked").is_some() { "(•)" } else { "( )" };
            ctx.push_str(mark);
            ctx.push_char(' ');
        }
        _ => {
            let value = val.attr("value").unwrap_or("");
            let placeholder = val.attr("placeholder").unwrap_or("");
            let name = val.attr("name").unwrap_or("");
            let label = if !value.is_empty() {
                value.to_owned()
            } else if !placeholder.is_empty() {
                placeholder.to_owned()
            } else if !name.is_empty() {
                name.to_owned()
            } else {
                "input".to_owned()
            };
            ctx.push_ansi("\x1b[2m[");
            ctx.push_str(&label);
            ctx.push_str(" __]\x1b[0m ");
        }
    }
}

fn render_button(el: ElementRef<'_>, ctx: &mut Ctx) {
    let text: String = el.text().collect();
    let text = text.trim();
    if text.is_empty() {
        let label = el.value().attr("value").unwrap_or("Button");
        ctx.push_ansi("\x1b[7m ");
        ctx.push_str(label);
        ctx.push_ansi(" \x1b[0m ");
        return;
    }
    ctx.push_ansi("\x1b[7m ");
    ctx.push_str(text);
    ctx.push_ansi(" \x1b[0m ");
}

fn render_textarea(el: ElementRef<'_>, ctx: &mut Ctx) {
    let val = el.value();
    let initial: String = el.text().collect();
    let initial = initial.trim();
    let placeholder = val.attr("placeholder").unwrap_or("");
    let name = val.attr("name").unwrap_or("textarea");
    let preview = if !initial.is_empty() {
        initial.lines().next().unwrap_or("").to_owned()
    } else if !placeholder.is_empty() {
        placeholder.to_owned()
    } else {
        name.to_owned()
    };
    ctx.push_ansi("\x1b[2m[");
    ctx.push_str(&preview);
    ctx.push_str(" __]\x1b[0m ");
}

fn render_select(el: ElementRef<'_>, ctx: &mut Ctx) {
    let mut chosen: Option<String> = None;
    for opt in el.children().filter_map(ElementRef::wrap) {
        if opt.value().name() != "option" {
            continue;
        }
        let text: String = opt.text().collect();
        let text = text.trim().to_owned();
        if opt.value().attr("selected").is_some() {
            chosen = Some(text);
            break;
        } else if chosen.is_none() {
            chosen = Some(text);
        }
    }
    let label = chosen.unwrap_or_else(|| "▼".to_owned());
    ctx.push_ansi("\x1b[2m[");
    ctx.push_str(&label);
    ctx.push_str(" ▼]\x1b[0m ");
}

fn render_link(el: ElementRef<'_>, ctx: &mut Ctx) {
    let href = el.value().attr("href").unwrap_or("");
    let text: String = el.text().collect();
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    let idx = ctx.links.len();
    if !href.is_empty() {
        ctx.links.push(RenderedLink { href: href.to_owned(), line: 0 });
        ctx.push_link_marker(idx);
    }
    ctx.push_ansi("\x1b[4;36m");
    ctx.push_str(text);
    ctx.push_ansi("\x1b[0m ");
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
    fn link_text_shown_href_hidden() {
        let page =
            ParsedPage::parse_html(r#"<html><body><a href="/about">About</a></body></html>"#);
        let out = render(&page);
        assert!(strip_ansi(&out).contains("About"));
        assert!(!strip_ansi(&out).contains("/about"), "href must not appear inline");
        let rp = render_full(&page);
        assert_eq!(rp.links[0].href, "/about");
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
    fn input_text_renders_with_placeholder() {
        let page = ParsedPage::parse_html(
            r#"<html><body><input type="text" placeholder="Search"></body></html>"#,
        );
        let plain = strip_ansi(&render(&page));
        assert!(plain.contains("Search"), "got: {plain:?}");
    }

    #[test]
    fn input_submit_renders_value_as_button() {
        let page = ParsedPage::parse_html(
            r#"<html><body><input type="submit" value="Go!"></body></html>"#,
        );
        let plain = strip_ansi(&render(&page));
        assert!(plain.contains("Go!"));
    }

    #[test]
    fn button_text_rendered() {
        let page = ParsedPage::parse_html(
            r#"<html><body><button>Click Me</button></body></html>"#,
        );
        let plain = strip_ansi(&render(&page));
        assert!(plain.contains("Click Me"));
    }

    #[test]
    fn hidden_input_skipped() {
        let page = ParsedPage::parse_html(
            r#"<html><body><p>Hi</p><input type="hidden" name="csrf" value="secret"></body></html>"#,
        );
        let plain = strip_ansi(&render(&page));
        assert!(!plain.contains("secret"), "hidden value leaked: {plain:?}");
    }

    #[test]
    fn select_shows_selected_option() {
        let page = ParsedPage::parse_html(
            r#"<html><body><select>
                <option>One</option>
                <option selected>Two</option>
                <option>Three</option>
            </select></body></html>"#,
        );
        let plain = strip_ansi(&render(&page));
        assert!(plain.contains("Two"));
    }

    #[test]
    fn render_full_captures_image_metadata() {
        let page = ParsedPage::parse_html(
            r#"<html><body><p>Hi</p><img src="https://x.com/a.png" alt="A"/></body></html>"#,
        );
        let rp = render_full(&page);
        assert_eq!(rp.images.len(), 1);
        assert_eq!(rp.images[0].src, "https://x.com/a.png");
        assert_eq!(rp.images[0].alt, "A");
    }

    #[test]
    fn markers_stripped_from_render() {
        let page = ParsedPage::parse_html(
            r#"<html><body><a href="/x">Link</a><img src="a.png" alt="A"/></body></html>"#,
        );
        let out = render(&page);
        // Marker char should not leak into final output.
        assert!(!out.contains('\u{F000}'), "marker leaked: {out:?}");
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
    fn hidden_attribute_skipped() {
        let page = ParsedPage::parse_html(
            "<html><body><p>Shown</p><p hidden>Hidden</p></body></html>",
        );
        let plain = strip_ansi(&render(&page));
        assert!(plain.contains("Shown"));
        assert!(!plain.contains("Hidden"), "got: {plain:?}");
    }

    #[test]
    fn inline_display_none_skipped() {
        let page = ParsedPage::parse_html(
            r#"<html><body><p>Shown</p><div style="display: none">Gone</div></body></html>"#,
        );
        let plain = strip_ansi(&render(&page));
        assert!(plain.contains("Shown"));
        assert!(!plain.contains("Gone"), "got: {plain:?}");
    }

    #[test]
    fn css_style_block_hides_class() {
        let page = ParsedPage::parse_html(
            r#"<html><head><style>.junk { display:none; }</style></head>
            <body><p>Shown</p><div class="junk">Gone</div></body></html>"#,
        );
        let plain = strip_ansi(&render(&page));
        assert!(plain.contains("Shown"));
        assert!(!plain.contains("Gone"), "got: {plain:?}");
    }

    #[test]
    fn sr_only_class_hidden_by_default() {
        let page = ParsedPage::parse_html(
            r#"<html><body><span class="sr-only">screen reader</span><p>Visible</p></body></html>"#,
        );
        let plain = strip_ansi(&render(&page));
        assert!(plain.contains("Visible"));
        assert!(!plain.contains("screen reader"), "got: {plain:?}");
    }

    #[test]
    fn empty_body_no_panic() {
        let page = ParsedPage::parse_html("<html><body></body></html>");
        let out = render(&page);
        assert!(out.is_empty() || out.chars().all(|c| c.is_whitespace()));
    }
}
