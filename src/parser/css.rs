//! CSS extraction: find rules that hide elements (display:none, visibility:hidden).
//!
//! Phase 2 scope — handles inline `<style>` blocks with simple selectors only
//! (`.class`, `#id`, or bare `tag`). Compound selectors, combinators, attribute
//! selectors, and pseudo-classes are intentionally skipped to avoid mismatching.

use scraper::{Html, Selector};
use std::collections::HashSet;

/// Class names, IDs, and tag names that should be hidden from rendering.
#[derive(Debug, Default, Clone)]
pub struct HiddenSet {
    pub classes: HashSet<String>,
    pub ids: HashSet<String>,
    pub tags: HashSet<String>,
}

impl HiddenSet {
    /// Empty hidden set — useful for tests and explicit no-CSS mode.
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.classes.is_empty() && self.ids.is_empty() && self.tags.is_empty()
    }
}

/// Common accessibility "visually hidden" classes. Pages style these with
/// `position:absolute; clip:rect(0,0,0,0)` rather than `display:none`, so the
/// CSS extractor can't see them — hardcoded for practicality.
const ALWAYS_HIDDEN_CLASSES: &[&str] = &[
    "sr-only",
    "screen-reader-only",
    "screen-reader-text",
    "visually-hidden",
    "visuallyhidden",
    "skip-link",
    "u-hidden-visually",
    "a11y-hidden",
];

/// Extract hidden selectors from every inline `<style>` block in the document.
pub fn extract_hidden(doc: &Html) -> HiddenSet {
    let mut set = HiddenSet::default();
    for cls in ALWAYS_HIDDEN_CLASSES {
        set.classes.insert((*cls).to_owned());
    }
    let Ok(sel) = Selector::parse("style") else {
        return set;
    };
    for el in doc.select(&sel) {
        parse_stylesheet(&el.text().collect::<String>(), &mut set);
    }
    set
}

/// Parse a stylesheet string and add hidden selectors to `set`.
fn parse_stylesheet(css: &str, set: &mut HiddenSet) {
    let chars: Vec<char> = strip_comments(css).chars().collect();
    let mut i = 0;
    while i < chars.len() {
        // Skip whitespace.
        while i < chars.len() && chars[i].is_whitespace() {
            i += 1;
        }
        if i >= chars.len() {
            break;
        }

        // At-rules: skip them whole. Either `@thing ... ;` or `@thing ... { ... }`.
        if chars[i] == '@' {
            while i < chars.len() && chars[i] != ';' && chars[i] != '{' {
                i += 1;
            }
            if i < chars.len() && chars[i] == '{' {
                i = skip_balanced(&chars, i);
            } else if i < chars.len() {
                i += 1;
            }
            continue;
        }

        // Collect selector text up to `{`.
        let sel_start = i;
        while i < chars.len() && chars[i] != '{' && chars[i] != '}' {
            i += 1;
        }
        if i >= chars.len() || chars[i] != '{' {
            // Stray `}` or EOF — bail out of this rule.
            if i < chars.len() {
                i += 1;
            }
            continue;
        }
        let selector_text: String = chars[sel_start..i].iter().collect();

        // Capture block contents up to matching `}`.
        let block_start = i + 1;
        let block_end = skip_balanced(&chars, i);
        let block: String = chars[block_start..block_end.saturating_sub(1)].iter().collect();
        i = block_end;

        if has_hide_decl(&block) {
            for sel in selector_text.split(',') {
                add_simple_selector(sel.trim(), set);
            }
        }
    }
}

/// Given index of an opening `{`, return index just past the matching `}`.
fn skip_balanced(chars: &[char], open_idx: usize) -> usize {
    let mut depth = 0;
    let mut i = open_idx;
    while i < chars.len() {
        match chars[i] {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    chars.len()
}

/// Strip `/* ... */` comments from a stylesheet.
fn strip_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut chars = css.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '/' && chars.peek() == Some(&'*') {
            chars.next();
            while let Some(c) = chars.next() {
                if c == '*' && chars.peek() == Some(&'/') {
                    chars.next();
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// `true` if `block` contains `display: none` or `visibility: hidden`.
fn has_hide_decl(block: &str) -> bool {
    for decl in block.split(';') {
        let Some((prop, val)) = decl.split_once(':') else {
            continue;
        };
        let prop = prop.trim().to_ascii_lowercase();
        let val = val.trim().to_ascii_lowercase();
        if prop == "display" && val.starts_with("none") {
            return true;
        }
        if prop == "visibility" && val.starts_with("hidden") {
            return true;
        }
    }
    false
}

/// Accept only simple selectors: `.foo`, `#bar`, or `tag`.
/// Reject compound (`.a.b`), combinators (`a b`, `a>b`), attribute selectors, pseudos.
fn add_simple_selector(sel: &str, set: &mut HiddenSet) {
    let sel = sel.trim();
    if sel.is_empty() {
        return;
    }
    if sel.contains(' ')
        || sel.contains('>')
        || sel.contains('+')
        || sel.contains('~')
        || sel.contains('[')
        || sel.contains(':')
        || sel.matches('.').count() > 1
        || sel.matches('#').count() > 1
        || (sel.contains('.') && sel.contains('#'))
    {
        return;
    }
    let is_valid_ident = |s: &str| {
        !s.is_empty()
            && s.chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    };
    if let Some(s) = sel.strip_prefix('.') {
        if is_valid_ident(s) {
            set.classes.insert(s.to_owned());
        }
    } else if let Some(s) = sel.strip_prefix('#') {
        if is_valid_ident(s) {
            set.ids.insert(s.to_owned());
        }
    } else if sel.chars().all(|c| c.is_ascii_alphabetic()) {
        set.tags.insert(sel.to_ascii_lowercase());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_class_display_none() {
        let mut set = HiddenSet::default();
        parse_stylesheet(".foo { display: none; }", &mut set);
        assert!(set.classes.contains("foo"));
    }

    #[test]
    fn extracts_id_visibility_hidden() {
        let mut set = HiddenSet::default();
        parse_stylesheet("#nav { visibility: hidden; color: red; }", &mut set);
        assert!(set.ids.contains("nav"));
    }

    #[test]
    fn handles_comments() {
        let mut set = HiddenSet::default();
        parse_stylesheet("/* hi */ .x /* mid */ { display:none; }", &mut set);
        assert!(set.classes.contains("x"));
    }

    #[test]
    fn ignores_at_rules() {
        let mut set = HiddenSet::default();
        parse_stylesheet(
            "@media (max-width: 600px) { .foo { display: none; } } .bar { display:none; }",
            &mut set,
        );
        // We skip at-rules whole, so .foo inside @media is NOT added.
        assert!(!set.classes.contains("foo"));
        assert!(set.classes.contains("bar"));
    }

    #[test]
    fn ignores_compound_selectors() {
        let mut set = HiddenSet::default();
        parse_stylesheet(".a.b { display:none } .a > .c { display:none }", &mut set);
        assert!(!set.classes.contains("a"));
        assert!(!set.classes.contains("b"));
        assert!(!set.classes.contains("c"));
    }

    #[test]
    fn comma_separated_selectors() {
        let mut set = HiddenSet::default();
        parse_stylesheet(".a, .b, #c { display:none }", &mut set);
        assert!(set.classes.contains("a"));
        assert!(set.classes.contains("b"));
        assert!(set.ids.contains("c"));
    }

    #[test]
    fn skips_non_hide_rules() {
        let mut set = HiddenSet::default();
        parse_stylesheet(".a { color: red; font-weight: bold; }", &mut set);
        assert!(!set.classes.contains("a"));
    }

    #[test]
    fn extract_hidden_includes_always_hidden() {
        let html = scraper::Html::parse_document("<html><head></head><body></body></html>");
        let set = extract_hidden(&html);
        assert!(set.classes.contains("sr-only"));
        assert!(set.classes.contains("visually-hidden"));
    }

    #[test]
    fn extract_hidden_reads_style_blocks() {
        let html = scraper::Html::parse_document(
            "<html><head><style>.junk{display:none}</style></head><body></body></html>",
        );
        let set = extract_hidden(&html);
        assert!(set.classes.contains("junk"));
    }
}
