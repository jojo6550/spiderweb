//! Tab management: per-tab content, history, scroll, and link state.

use crate::browser::history::History;
use crate::renderer::text::RenderedLink;

/// State for a single browser tab.
pub struct Tab {
    pub url: String,
    pub title: String,
    pub history: History,
    pub lines: Vec<String>,
    pub links: Vec<RenderedLink>,
    pub scroll: usize,
    pub selected_link: Option<usize>,
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
            scroll: 0,
            selected_link: None,
            loading: true,
            search_matches: Vec::new(),
            search_idx: 0,
        }
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

    pub fn next_link(&mut self) {
        if self.links.is_empty() {
            return;
        }
        self.selected_link = Some(match self.selected_link {
            None => 0,
            Some(i) => (i + 1) % self.links.len(),
        });
        self.scroll_to_link();
    }

    pub fn prev_link(&mut self) {
        if self.links.is_empty() {
            return;
        }
        self.selected_link = Some(match self.selected_link {
            None | Some(0) => self.links.len().saturating_sub(1),
            Some(i) => i - 1,
        });
        self.scroll_to_link();
    }

    /// Scroll so the selected link is visible.
    fn scroll_to_link(&mut self) {
        if let Some(idx) = self.selected_link {
            if let Some(link) = self.links.get(idx) {
                self.scroll = link.line;
            }
        }
    }

    pub fn selected_href(&self) -> Option<&str> {
        let idx = self.selected_link?;
        self.links.get(idx).map(|l| l.href.as_str())
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
    fn link_cycle_wraps() {
        let mut tab = Tab::new("https://x.com".into());
        tab.links = vec![
            RenderedLink { href: "https://a.com".into(), line: 0 },
            RenderedLink { href: "https://b.com".into(), line: 2 },
        ];
        tab.next_link();
        assert_eq!(tab.selected_link, Some(0));
        tab.prev_link();
        assert_eq!(tab.selected_link, Some(1));
    }
}
