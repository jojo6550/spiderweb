//! Tab management: per-tab content, history, scroll, and link state.

use crate::browser::history::History;
use crate::renderer::text::{CodeSpan, FieldKind, FormField, LineKind, RenderedForm, RenderedLink};

/// A single focusable item on a page — either a hyperlink or an interactive
/// form field. Used as the `Tab::focused` cursor.
#[derive(Debug, Clone, PartialEq)]
pub enum FocusItem {
    Link(usize),
    Field(usize),
}

/// State for a single browser tab.
pub struct Tab {
    pub url: String,
    pub title: String,
    pub history: History,
    pub lines: Vec<String>,
    pub links: Vec<RenderedLink>,
    pub line_kinds: Vec<LineKind>,
    pub code_spans: Vec<CodeSpan>,
    pub forms: Vec<RenderedForm>,
    pub fields: Vec<FormField>,
    /// Live values for each `fields[i]`, initialised from `fields[i].value` on load.
    pub field_values: Vec<String>,
    pub scroll: usize,
    pub focused: Option<FocusItem>,
    pub loading: bool,
    pub search_matches: Vec<usize>,
    pub search_idx: usize,
}

impl Tab {
    pub fn new(url: String) -> Self {
        Self {
            url,
            title: String::new(),
            history: History::new(),
            lines: Vec::new(),
            links: Vec::new(),
            line_kinds: Vec::new(),
            code_spans: Vec::new(),
            forms: Vec::new(),
            fields: Vec::new(),
            field_values: Vec::new(),
            scroll: 0,
            focused: None,
            loading: true,
            search_matches: Vec::new(),
            search_idx: 0,
        }
    }

    /// All focusable items (links + non-hidden fields) sorted by line number.
    pub fn build_focus_order(&self) -> Vec<FocusItem> {
        let mut items: Vec<(usize, FocusItem)> = Vec::new();
        for (i, link) in self.links.iter().enumerate() {
            items.push((link.line, FocusItem::Link(i)));
        }
        for (i, field) in self.fields.iter().enumerate() {
            if matches!(field.kind, FieldKind::Hidden) {
                continue;
            }
            items.push((field.line, FocusItem::Field(i)));
        }
        items.sort_by_key(|(line, _)| *line);
        items.into_iter().map(|(_, item)| item).collect()
    }

    /// Advance focus to the next item in document order, wrapping around.
    pub fn next_focus(&mut self) {
        let order = self.build_focus_order();
        if order.is_empty() {
            return;
        }
        let next_idx = match &self.focused {
            None => 0,
            Some(current) => {
                let pos = order.iter().position(|x| x == current);
                pos.map(|p| (p + 1) % order.len()).unwrap_or(0)
            }
        };
        self.focused = Some(order[next_idx].clone());
        self.scroll_to_focused();
    }

    /// Retreat focus to the previous item in document order, wrapping around.
    pub fn prev_focus(&mut self) {
        let order = self.build_focus_order();
        if order.is_empty() {
            return;
        }
        let next_idx = match &self.focused {
            None => order.len().saturating_sub(1),
            Some(current) => {
                let pos = order.iter().position(|x| x == current);
                match pos {
                    Some(0) | None => order.len().saturating_sub(1),
                    Some(p) => p - 1,
                }
            }
        };
        self.focused = Some(order[next_idx].clone());
        self.scroll_to_focused();
    }

    fn scroll_to_focused(&mut self) {
        let line = match &self.focused {
            Some(FocusItem::Link(i)) => self.links.get(*i).map(|l| l.line),
            Some(FocusItem::Field(i)) => self.fields.get(*i).map(|f| f.line),
            None => None,
        };
        if let Some(l) = line {
            self.scroll = l;
        }
    }

    /// Returns the href of the focused link, or `None` if a field (or nothing) is focused.
    pub fn selected_href(&self) -> Option<&str> {
        match &self.focused {
            Some(FocusItem::Link(i)) => self.links.get(*i).map(|l| l.href.as_str()),
            _ => None,
        }
    }

    /// Index of the next editable text/textarea field after `from` in the same
    /// form, or — if `from` is None — the first editable field across all forms.
    /// Returns `None` if there are no editable fields.
    pub fn next_editable_field(&self, from: Option<usize>) -> Option<usize> {
        let is_editable =
            |f: &FormField| matches!(f.kind, FieldKind::Text | FieldKind::Textarea);
        let (start, form_filter): (usize, Option<usize>) = match from {
            Some(i) => (i + 1, self.fields.get(i).map(|f| f.form_idx)),
            None => (0, None),
        };
        let after = self
            .fields
            .iter()
            .enumerate()
            .skip(start)
            .find(|(_, f)| {
                is_editable(f) && form_filter.is_none_or(|fi| f.form_idx == fi)
            })
            .map(|(i, _)| i);
        if after.is_some() {
            return after;
        }
        self.fields
            .iter()
            .enumerate()
            .find(|(_, f)| {
                is_editable(f) && form_filter.is_none_or(|fi| f.form_idx == fi)
            })
            .map(|(i, _)| i)
    }

    /// Build a `k=v&k=v` query string for `form_idx`. Includes every named,
    /// non-Submit field (Hidden values + edited text/textarea values + the
    /// `value` of checked checkboxes and the chosen radio/select option).
    /// Submit buttons are excluded — they're the trigger, not data.
    pub fn build_query(&self, form_idx: usize) -> String {
        let mut s = url::form_urlencoded::Serializer::new(String::new());
        for (i, field) in self.fields.iter().enumerate() {
            if field.form_idx != form_idx {
                continue;
            }
            if field.name.is_empty() {
                continue;
            }
            match &field.kind {
                FieldKind::Submit => continue,
                FieldKind::Checkbox | FieldKind::Radio => {
                    let v = self.field_values.get(i).cloned().unwrap_or_default();
                    if v.is_empty() {
                        continue;
                    }
                    s.append_pair(&field.name, &v);
                }
                _ => {
                    let v = self.field_values.get(i).cloned().unwrap_or_default();
                    s.append_pair(&field.name, &v);
                }
            }
        }
        s.finish()
    }

    /// Case-insensitive search; scrolls to first match.
    pub fn search(&mut self, query: &str) {
        if query.is_empty() {
            self.clear_search();
            return;
        }
        let q = query.to_lowercase();
        self.search_matches = self
            .lines
            .iter()
            .enumerate()
            .filter(|(_, l)| l.to_lowercase().contains(&q))
            .map(|(i, _)| i)
            .collect();
        self.search_idx = 0;
        if let Some(&first) = self.search_matches.first() {
            self.scroll = first;
        }
    }

    pub fn search_next(&mut self) {
        if self.search_matches.is_empty() {
            return;
        }
        self.search_idx = (self.search_idx + 1) % self.search_matches.len();
        self.scroll = self.search_matches[self.search_idx];
    }

    pub fn search_prev(&mut self) {
        if self.search_matches.is_empty() {
            return;
        }
        self.search_idx = self
            .search_idx
            .checked_sub(1)
            .unwrap_or(self.search_matches.len() - 1);
        self.scroll = self.search_matches[self.search_idx];
    }

    pub fn clear_search(&mut self) {
        self.search_matches.clear();
        self.search_idx = 0;
    }

    pub fn scroll_down(&mut self, n: usize) {
        self.scroll = self
            .scroll
            .saturating_add(n)
            .min(self.lines.len().saturating_sub(1));
    }

    pub fn scroll_up(&mut self, n: usize) {
        self.scroll = self.scroll.saturating_sub(n);
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll = self.lines.len().saturating_sub(1);
    }
}

/// Manages all open tabs.
pub struct TabManager {
    pub tabs: Vec<Tab>,
    pub active: usize,
}

impl TabManager {
    pub fn new(initial_url: String) -> Self {
        Self { tabs: vec![Tab::new(initial_url)], active: 0 }
    }

    pub fn current(&self) -> &Tab {
        &self.tabs[self.active]
    }

    pub fn current_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active]
    }

    /// Open a new tab and switch to it.
    pub fn open_new(&mut self, url: String) -> usize {
        self.tabs.push(Tab::new(url));
        self.active = self.tabs.len() - 1;
        self.active
    }

    /// Switch to tab by 0-based index. No-op if out of range.
    pub fn switch_to(&mut self, idx: usize) {
        if idx < self.tabs.len() {
            self.active = idx;
        }
    }

    /// Close the active tab. Keeps at least one tab open.
    pub fn close_current(&mut self) {
        if self.tabs.len() <= 1 {
            return;
        }
        self.tabs.remove(self.active);
        self.active = self.active.min(self.tabs.len() - 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_and_switch() {
        let mut tm = TabManager::new("https://a.com".into());
        tm.open_new("https://b.com".into());
        assert_eq!(tm.active, 1);
        tm.switch_to(0);
        assert_eq!(tm.current().url, "https://a.com");
    }

    #[test]
    fn close_keeps_minimum_one() {
        let mut tm = TabManager::new("https://a.com".into());
        tm.close_current();
        assert_eq!(tm.tabs.len(), 1);
    }

    #[test]
    fn focus_cycle_wraps_links() {
        let mut tab = Tab::new("https://x.com".into());
        tab.links = vec![
            RenderedLink { href: "https://a.com".into(), line: 0 },
            RenderedLink { href: "https://b.com".into(), line: 2 },
        ];
        tab.next_focus();
        assert_eq!(tab.focused, Some(FocusItem::Link(0)));
        tab.prev_focus();
        assert_eq!(tab.focused, Some(FocusItem::Link(1)));
    }

    #[test]
    fn focus_order_excludes_hidden() {
        let mut tab = Tab::new("https://x.com".into());
        tab.fields = vec![
            FormField { form_idx: 0, name: "h".into(), kind: FieldKind::Hidden, value: "".into(), line: 0 },
            FormField { form_idx: 0, name: "q".into(), kind: FieldKind::Text, value: "".into(), line: 1 },
        ];
        tab.field_values = vec!["".into(), "".into()];
        let order = tab.build_focus_order();
        assert_eq!(order, vec![FocusItem::Field(1)]);
    }

    #[test]
    fn focus_order_sorted_by_line() {
        let mut tab = Tab::new("https://x.com".into());
        tab.links = vec![RenderedLink { href: "https://a.com".into(), line: 5 }];
        tab.fields = vec![
            FormField { form_idx: 0, name: "q".into(), kind: FieldKind::Text, value: "".into(), line: 2 },
            FormField { form_idx: 0, name: "btn".into(), kind: FieldKind::Submit, value: "Go".into(), line: 8 },
        ];
        tab.field_values = vec!["".into(), "Go".into()];
        let order = tab.build_focus_order();
        assert_eq!(order, vec![
            FocusItem::Field(0),
            FocusItem::Link(0),
            FocusItem::Field(1),
        ]);
    }

    #[test]
    fn focus_cycle_wraps_mixed() {
        let mut tab = Tab::new("https://x.com".into());
        tab.links = vec![RenderedLink { href: "https://a.com".into(), line: 5 }];
        tab.fields = vec![
            FormField { form_idx: 0, name: "q".into(), kind: FieldKind::Text, value: "".into(), line: 2 },
        ];
        tab.field_values = vec!["".into()];
        // order: Field(0) at line 2, Link(0) at line 5
        tab.next_focus();
        assert_eq!(tab.focused, Some(FocusItem::Field(0)));
        tab.next_focus();
        assert_eq!(tab.focused, Some(FocusItem::Link(0)));
        tab.next_focus(); // wrap
        assert_eq!(tab.focused, Some(FocusItem::Field(0)));
    }

    #[test]
    fn prev_focus_from_none_goes_to_last() {
        let mut tab = Tab::new("https://x.com".into());
        tab.links = vec![
            RenderedLink { href: "https://a.com".into(), line: 0 },
            RenderedLink { href: "https://b.com".into(), line: 1 },
        ];
        tab.prev_focus();
        assert_eq!(tab.focused, Some(FocusItem::Link(1)));
    }

    #[test]
    fn selected_href_none_when_field_focused() {
        let mut tab = Tab::new("https://x.com".into());
        tab.fields = vec![
            FormField { form_idx: 0, name: "q".into(), kind: FieldKind::Text, value: "".into(), line: 0 },
        ];
        tab.field_values = vec!["".into()];
        tab.focused = Some(FocusItem::Field(0));
        assert_eq!(tab.selected_href(), None);
    }

    #[test]
    fn selected_href_returns_href_when_link_focused() {
        let mut tab = Tab::new("https://x.com".into());
        tab.links = vec![RenderedLink { href: "https://a.com".into(), line: 0 }];
        tab.focused = Some(FocusItem::Link(0));
        assert_eq!(tab.selected_href(), Some("https://a.com"));
    }

    fn tab_with_search_form() -> Tab {
        let mut tab = Tab::new("https://example.com/path?old=1".into());
        tab.forms = vec![RenderedForm { action: "/search".into(), method: "get".into() }];
        tab.fields = vec![
            FormField {
                form_idx: 0, name: "csrf".into(), kind: FieldKind::Hidden,
                value: "tok".into(), line: 0,
            },
            FormField {
                form_idx: 0, name: "q".into(), kind: FieldKind::Text,
                value: String::new(), line: 5,
            },
            FormField {
                form_idx: 0, name: "btn".into(), kind: FieldKind::Submit,
                value: "Go".into(), line: 6,
            },
        ];
        tab.field_values = tab.fields.iter().map(|f| f.value.clone()).collect();
        tab
    }

    #[test]
    fn build_query_skips_submit_and_unnamed() {
        let mut tab = tab_with_search_form();
        tab.field_values[1] = "hello world".into();
        let q = tab.build_query(0);
        assert_eq!(q, "csrf=tok&q=hello+world");
    }

    #[test]
    fn build_query_urlencodes_special_chars() {
        let mut tab = tab_with_search_form();
        tab.field_values[1] = "a&b=c d".into();
        let q = tab.build_query(0);
        assert!(q.contains("q=a%26b%3Dc+d"), "got: {q}");
    }

    #[test]
    fn build_query_includes_empty_named_text() {
        let tab = tab_with_search_form();
        let q = tab.build_query(0);
        assert!(q.contains("q="), "got: {q}");
    }

    #[test]
    fn next_editable_field_finds_first_text() {
        let tab = tab_with_search_form();
        assert_eq!(tab.next_editable_field(None), Some(1));
    }

    #[test]
    fn next_editable_field_wraps_within_form() {
        let tab = tab_with_search_form();
        assert_eq!(tab.next_editable_field(Some(1)), Some(1));
    }

    #[test]
    fn next_editable_field_none_when_no_text_inputs() {
        let mut tab = tab_with_search_form();
        tab.fields[1].kind = FieldKind::Checkbox;
        assert_eq!(tab.next_editable_field(None), None);
    }
}
