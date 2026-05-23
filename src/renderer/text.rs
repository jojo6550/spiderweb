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
    "head", "math", "script", "style", "svg", "template",
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

/// Char-offset range of an inline code span within a rendered line.
#[derive(Debug, Clone)]
pub struct CodeSpan {
    pub line: usize,
    pub start: usize, // char offset in stripped line (inclusive)
    pub end: usize,   // char offset in stripped line (exclusive)
}

/// A `<form>` element parsed from the page; metadata required to submit.
#[derive(Debug, Clone)]
pub struct RenderedForm {
    /// `action` attribute. Empty = post to current URL (minus its query string).
    pub action: String,
    /// `method` attribute lowercased. `"get"` (default) or `"post"`.
    pub method: String,
}

/// A form input/field belonging to a [`RenderedForm`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldKind {
    /// Single-line text inputs (text/search/email/url/tel/number/password/no-type).
    Text,
    Checkbox,
    Radio,
    Submit,
    Textarea,
    /// `<select>`. Stores its option labels for cycling later.
    Select(Vec<String>),
    /// `<input type="hidden">` — invisible but included in the query string.
    Hidden,
}

/// A single form control. `value` is the initial/default; the runtime keeps a
/// parallel `field_values` buffer that diverges from this on edits.
#[derive(Debug, Clone)]
pub struct FormField {
    /// Index into [`RenderedPage::forms`].
    pub form_idx: usize,
    /// `name` attribute. Empty = unnamed (skipped at submit time, per HTML).
    pub name: String,
    pub kind: FieldKind,
    /// Default value. For text/textarea: the `value`/innerText. For checkbox/radio: the `value` attr.
    pub value: String,
    /// Zero-based line index in the rendered output. Hidden fields keep `0`.
    pub line: usize,
}

/// Full render result: plain-text lines plus link/image/form metadata.
pub struct RenderedPage {
    pub lines: Vec<String>,
    pub links: Vec<RenderedLink>,
    pub images: Vec<RenderedImage>,
    pub line_kinds: Vec<LineKind>,
    pub code_spans: Vec<CodeSpan>,
    pub forms: Vec<RenderedForm>,
    pub fields: Vec<FormField>,
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
    code_spans: Vec<CodeSpan>,
    forms: Vec<RenderedForm>,
    fields: Vec<FormField>,
    /// Index of the enclosing `<form>` during recursion, if any.
    current_form: Option<usize>,
}

impl Ctx {
    fn new() -> Self {
        Self {
            buf: String::with_capacity(4096),
            links: Vec::new(),
            images: Vec::new(),
            code_spans: Vec::new(),
            forms: Vec::new(),
            fields: Vec::new(),
            current_form: None,
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

    /// Push a heading-level marker so `render_full` can tag the line with the
    /// correct [`LineKind`] variant after normalization.
    fn push_heading_marker(&mut self, level: usize) {
        self.buf.push(MARKER);
        self.buf.push('H');
        self.buf.push_str(&level.to_string());
        self.buf.push(MARKER);
    }

    /// Push a code-span start marker for span index `idx`.
    fn push_code_start_marker(&mut self, idx: usize) {
        self.buf.push(MARKER);
        self.buf.push('C');
        self.buf.push_str(&idx.to_string());
        self.buf.push(MARKER);
    }

    /// Push a code-span end marker for span index `idx`.
    fn push_code_end_marker(&mut self, idx: usize) {
        self.buf.push(MARKER);
        self.buf.push('Z');
        self.buf.push_str(&idx.to_string());
        self.buf.push(MARKER);
    }

    /// Push a form-field position marker for field index `idx`.
    fn push_field_marker(&mut self, idx: usize) {
        self.buf.push(MARKER);
        self.buf.push('F');
        self.buf.push_str(&idx.to_string());
        self.buf.push(MARKER);
    }

    /// Register a [`FormField`] under the current form (if any). Returns the
    /// index of the new field, or `None` if not inside a `<form>`. Hidden
    /// fields are registered but emit no marker (no visible position).
    fn register_field(&mut self, name: String, kind: FieldKind, value: String) -> Option<usize> {
        let form_idx = self.current_form?;
        let idx = self.fields.len();
        self.fields.push(FormField {
            form_idx,
            name,
            kind,
            value,
            line: 0,
        });
        Some(idx)
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
    let mut code_spans = ctx.code_spans;
    let mut fields = ctx.fields;
    let forms = ctx.forms;
    let mut line_kinds = vec![LineKind::Normal; lines.len()];

    // Scan each line for markers; record positions, then strip markers.
    for (line_idx, line) in lines.iter_mut().enumerate() {
        let mut buf = String::with_capacity(line.len());
        let mut char_count: usize = 0;
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
                match kind {
                    'L' => {
                        if let Ok(idx) = digits.parse::<usize>() {
                            if let Some(rl) = links.get_mut(idx) {
                                rl.line = line_idx;
                            }
                        }
                    }
                    'I' => {
                        if let Ok(idx) = digits.parse::<usize>() {
                            if let Some(ri) = images.get_mut(idx) {
                                ri.line = line_idx;
                            }
                        }
                    }
                    'H' => {
                        if let Ok(level) = digits.parse::<usize>() {
                            if let Some(lk) = line_kinds.get_mut(line_idx) {
                                *lk = match level {
                                    1 => LineKind::H1,
                                    2 => LineKind::H2,
                                    3 => LineKind::H3,
                                    _ => LineKind::H4Plus,
                                };
                            }
                        }
                    }
                    'C' => {
                        if let Ok(i) = digits.parse::<usize>() {
                            if let Some(cs) = code_spans.get_mut(i) {
                                cs.line = line_idx;
                                cs.start = char_count;
                            }
                        }
                    }
                    'Z' => {
                        if let Ok(i) = digits.parse::<usize>() {
                            if let Some(cs) = code_spans.get_mut(i) {
                                if cs.line == line_idx {
                                    cs.end = char_count;
                                }
                            }
                        }
                    }
                    'F' => {
                        if let Ok(idx) = digits.parse::<usize>() {
                            if let Some(f) = fields.get_mut(idx) {
                                f.line = line_idx;
                            }
                        }
                    }
                    _ => {}
                }
            } else {
                buf.push(c);
                char_count += 1;
            }
        }
        *line = buf;
    }

    RenderedPage { lines, links, images, line_kinds, code_spans, forms, fields }
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

    // Inline code elements — always handled as inline, never as block.
    if matches!(tag, "code" | "kbd" | "tt") {
        render_code_span(el, ctx);
        return;
    }

    let is_block = BLOCK.contains(&tag);

    // Open a new form scope, register it, and gather any hidden inputs that
    // sit inside (so they make it into the submit query string). Nested forms
    // are illegal HTML; we save+restore the outer scope just in case.
    let saved_form = if tag == "form" {
        let form_idx = ctx.forms.len();
        let val = el.value();
        ctx.forms.push(RenderedForm {
            action: val.attr("action").unwrap_or("").to_owned(),
            method: val.attr("method").unwrap_or("get").to_ascii_lowercase(),
        });
        let prev = ctx.current_form;
        ctx.current_form = Some(form_idx);
        register_hidden_fields(el, ctx);
        Some(prev)
    } else {
        None
    };

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

    if is_block {
        ctx.push_char('\n');
    }

    if let Some(prev) = saved_form {
        ctx.current_form = prev;
    }
}

/// Register hidden inputs nested inside `form_el` (typed `input type="hidden"`).
/// Hidden fields don't emit a marker — they have no visible line — but they
/// must travel with the submit query string.
fn register_hidden_fields(form_el: ElementRef<'_>, ctx: &mut Ctx) {
    for desc in form_el.descendants().filter_map(ElementRef::wrap) {
        if desc.value().name() != "input" {
            continue;
        }
        let ty = desc.value().attr("type").unwrap_or("").to_ascii_lowercase();
        if ty != "hidden" {
            continue;
        }
        let name = desc.value().attr("name").unwrap_or("").to_owned();
        let value = desc.value().attr("value").unwrap_or("").to_owned();
        let _ = ctx.register_field(name, FieldKind::Hidden, value);
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
    let name = val.attr("name").unwrap_or("").to_owned();
    let value = val.attr("value").unwrap_or("").to_owned();
    match ty.as_str() {
        "hidden" => {
            // Already registered by `register_hidden_fields` during the
            // enclosing `<form>` descent. Emit nothing visible.
        }
        "submit" | "button" | "reset" => {
            let field_idx = ctx.register_field(name, FieldKind::Submit, value.clone());
            let label = val.attr("value").unwrap_or("Submit");
            if let Some(idx) = field_idx {
                ctx.push_field_marker(idx);
            }
            ctx.push_str("[ ");
            ctx.push_str(label);
            ctx.push_str(" ] ");
        }
        "checkbox" => {
            let initial = if val.attr("checked").is_some() {
                value.clone()
            } else {
                String::new()
            };
            let field_idx = ctx.register_field(name, FieldKind::Checkbox, initial);
            let mark = if val.attr("checked").is_some() { "[x]" } else { "[ ]" };
            if let Some(idx) = field_idx {
                ctx.push_field_marker(idx);
            }
            ctx.push_str(mark);
            ctx.push_char(' ');
        }
        "radio" => {
            let initial = if val.attr("checked").is_some() {
                value.clone()
            } else {
                String::new()
            };
            let field_idx = ctx.register_field(name, FieldKind::Radio, initial);
            let mark = if val.attr("checked").is_some() { "(•)" } else { "( )" };
            if let Some(idx) = field_idx {
                ctx.push_field_marker(idx);
            }
            ctx.push_str(mark);
            ctx.push_char(' ');
        }
        _ => {
            let placeholder = val.attr("placeholder").unwrap_or("");
            let label = if !value.is_empty() {
                value.clone()
            } else if !placeholder.is_empty() {
                placeholder.to_owned()
            } else if !name.is_empty() {
                name.clone()
            } else {
                "input".to_owned()
            };
            let field_idx = ctx.register_field(name, FieldKind::Text, value);
            if let Some(idx) = field_idx {
                ctx.push_field_marker(idx);
            }
            let truncated: String = label.chars().take(18).collect();
            let pad = "_".repeat(18usize.saturating_sub(truncated.chars().count()));
            ctx.push_char('[');
            ctx.push_str(&truncated);
            ctx.push_str(&pad);
            ctx.push_str("] ");
        }
    }
}

fn render_button(el: ElementRef<'_>, ctx: &mut Ctx) {
    let val = el.value();
    let name = val.attr("name").unwrap_or("").to_owned();
    let value = val.attr("value").unwrap_or("").to_owned();
    // Default `<button>` is type=submit per HTML. type="button" gets the same
    // Submit kind for now — its keypress is ignored at submit time anyway
    // since we cycle to the actual submit field.
    let field_idx = ctx.register_field(name, FieldKind::Submit, value);
    let text: String = el.text().collect();
    let text_trim = text.trim();
    let label = if text_trim.is_empty() {
        val.attr("value").unwrap_or("Button")
    } else {
        text_trim
    };
    if let Some(idx) = field_idx {
        ctx.push_field_marker(idx);
    }
    ctx.push_str("[ ");
    ctx.push_str(label);
    ctx.push_str(" ] ");
}

fn render_textarea(el: ElementRef<'_>, ctx: &mut Ctx) {
    let val = el.value();
    let initial: String = el.text().collect();
    let initial_trim = initial.trim().to_owned();
    let placeholder = val.attr("placeholder").unwrap_or("");
    let name = val.attr("name").unwrap_or("textarea").to_owned();
    let preview = if !initial_trim.is_empty() {
        initial_trim.lines().next().unwrap_or("").to_owned()
    } else if !placeholder.is_empty() {
        placeholder.to_owned()
    } else {
        name.clone()
    };
    let field_idx = ctx.register_field(name, FieldKind::Textarea, initial_trim);
    if let Some(idx) = field_idx {
        ctx.push_field_marker(idx);
    }
    let truncated: String = preview.chars().take(24).collect();
    let pad = "_".repeat(24usize.saturating_sub(truncated.chars().count()));
    ctx.push_char('[');
    ctx.push_str(&truncated);
    ctx.push_str(&pad);
    ctx.push_str("] ");
}

fn render_select(el: ElementRef<'_>, ctx: &mut Ctx) {
    let mut chosen: Option<String> = None;
    let mut options: Vec<String> = Vec::new();
    let mut selected_value = String::new();
    for opt in el.children().filter_map(ElementRef::wrap) {
        if opt.value().name() != "option" {
            continue;
        }
        let text: String = opt.text().collect();
        let text = text.trim().to_owned();
        let opt_val = opt
            .value()
            .attr("value")
            .map(str::to_owned)
            .unwrap_or_else(|| text.clone());
        options.push(text.clone());
        if opt.value().attr("selected").is_some() {
            chosen = Some(text);
            selected_value = opt_val;
        } else if chosen.is_none() {
            chosen = Some(text.clone());
            selected_value = opt_val;
        }
    }
    let name = el.value().attr("name").unwrap_or("").to_owned();
    let label = chosen.unwrap_or_else(|| "▼".to_owned());
    let field_idx = ctx.register_field(name, FieldKind::Select(options), selected_value);
    if let Some(idx) = field_idx {
        ctx.push_field_marker(idx);
    }
    ctx.push_str("[ ");
    ctx.push_str(&label);
    ctx.push_str(" \u{25be} ] ");
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

/// Render an inline `<code>`, `<kbd>`, or `<tt>` element, recording its char
/// offsets so that `render_full` can later tag it as a [`CodeSpan`].
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

    #[test]
    fn form_with_text_input_registered() {
        let page = ParsedPage::parse_html(
            r#"<html><body><form action="/search" method="get">
                <input name="q" type="text" placeholder="Search">
            </form></body></html>"#,
        );
        let rp = render_full(&page);
        assert_eq!(rp.forms.len(), 1);
        assert_eq!(rp.forms[0].action, "/search");
        assert_eq!(rp.forms[0].method, "get");
        assert_eq!(rp.fields.len(), 1);
        assert_eq!(rp.fields[0].name, "q");
        assert_eq!(rp.fields[0].kind, FieldKind::Text);
        assert_eq!(rp.fields[0].form_idx, 0);
    }

    #[test]
    fn hidden_input_inside_form_kept_as_field() {
        let page = ParsedPage::parse_html(
            r#"<html><body><form action="/x">
                <input type="hidden" name="csrf" value="tok123">
                <input name="q" type="text">
            </form></body></html>"#,
        );
        let rp = render_full(&page);
        // Order: register_hidden_fields runs first, then descent registers visible field.
        let csrf = rp.fields.iter().find(|f| f.name == "csrf").expect("csrf field");
        assert_eq!(csrf.kind, FieldKind::Hidden);
        assert_eq!(csrf.value, "tok123");
        // Visible render still strips the hidden value.
        let plain = strip_ansi(&render(&page));
        assert!(!plain.contains("tok123"));
    }

    #[test]
    fn submit_button_recorded_as_submit_field() {
        let page = ParsedPage::parse_html(
            r#"<html><body><form><input type="submit" value="Go"></form></body></html>"#,
        );
        let rp = render_full(&page);
        let submit = rp.fields.iter().find(|f| f.kind == FieldKind::Submit).expect("submit field");
        assert_eq!(submit.value, "Go");
    }

    #[test]
    fn fields_outside_form_not_registered() {
        let page = ParsedPage::parse_html(
            r#"<html><body><input name="x" type="text"></body></html>"#,
        );
        let rp = render_full(&page);
        assert!(rp.fields.is_empty(), "fields outside <form> must not register");
        assert!(rp.forms.is_empty());
    }
}
