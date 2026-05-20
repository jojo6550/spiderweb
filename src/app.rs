//! Top-level application state and event loop.

use std::io::Stdout;

use anyhow::{Context, Result};
use crossterm::{
    event::{Event, EventStream},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use futures_util::StreamExt;
use ratatui::{Terminal, backend::CrosstermBackend};
use tokio::sync::mpsc::{self, Sender};

use crate::{
    browser::{bookmarks::Bookmarks, tabs::TabManager},
    config::settings::Settings,
    network::client::SpiderClient,
    parser::html::ParsedPage,
    renderer::text::{self as text_renderer, RenderedLink},
    tui::{keybinds, ui},
};

// ── Input mode ────────────────────────────────────────────────────────────────

/// Current keyboard input mode.
pub enum InputMode {
    Normal,
    /// Search mode — string is the live query being typed.
    Search(String),
}

// ── Background message ────────────────────────────────────────────────────────

/// Messages sent from background fetch tasks to the event loop.
pub enum BgMsg {
    Loaded {
        tab_idx: usize,
        url: String,
        title: Option<String>,
        lines: Vec<String>,
        links: Vec<RenderedLink>,
    },
    Error {
        tab_idx: usize,
        message: String,
    },
}

// ── App state ─────────────────────────────────────────────────────────────────

/// Central application state — owned by the main TUI event loop.
pub struct App {
    pub tabs: TabManager,
    pub bookmarks: Bookmarks,
    pub settings: Settings,
    pub status: String,
    pub quit: bool,
    pub input_mode: InputMode,
}

impl App {
    pub fn new(url: String, settings: Settings, bookmarks: Bookmarks) -> Self {
        Self {
            tabs: TabManager::new(url),
            bookmarks,
            settings,
            status: String::new(),
            quit: false,
            input_mode: InputMode::Normal,
        }
    }

    pub fn handle_msg(&mut self, msg: BgMsg) {
        match msg {
            BgMsg::Loaded { tab_idx, url, title, lines, links } => {
                if let Some(tab) = self.tabs.tabs.get_mut(tab_idx) {
                    tab.url = url;
                    tab.title = title.unwrap_or_default();
                    tab.lines = lines;
                    tab.links = links;
                    tab.scroll = 0;
                    tab.selected_link = None;
                    tab.loading = false;
                    tab.clear_search();
                }
            }
            BgMsg::Error { tab_idx, message } => {
                if let Some(tab) = self.tabs.tabs.get_mut(tab_idx) {
                    tab.loading = false;
                    tab.lines = vec![
                        format!("Error: {message}"),
                        String::new(),
                        "Press Backspace to go back.".into(),
                    ];
                    tab.links.clear();
                }
                self.status = format!("Error: {message}");
            }
        }
    }

    /// Navigate the current tab to `href`, resolving relative URLs against the tab's current URL.
    pub fn navigate(&mut self, href: String, tx: &Sender<BgMsg>) {
        let base = self.tabs.current().url.clone();
        let Some(url) = resolve_url(&base, &href) else {
            self.status = format!("Skipped: {href}");
            return;
        };
        let tab = self.tabs.current_mut();
        tab.history.push(tab.url.clone());
        tab.url = url.clone();
        tab.loading = true;
        tab.selected_link = None;
        tab.clear_search();
        self.status.clear();
        let tab_idx = self.tabs.active;
        tokio::spawn(fetch_page(url, tab_idx, tx.clone()));
    }

    pub fn go_back(&mut self, tx: &Sender<BgMsg>) {
        let tab_idx = self.tabs.active;
        let tab = self.tabs.current_mut();
        let current = tab.url.clone();
        if let Some(prev) = tab.history.go_back(&current) {
            tab.url = prev.clone();
            tab.loading = true;
            tab.selected_link = None;
            tab.clear_search();
            self.status.clear();
            tokio::spawn(fetch_page(prev, tab_idx, tx.clone()));
        } else {
            self.status = "No history".into();
        }
    }

    pub fn go_forward(&mut self, tx: &Sender<BgMsg>) {
        let tab_idx = self.tabs.active;
        let tab = self.tabs.current_mut();
        let current = tab.url.clone();
        if let Some(next) = tab.history.go_forward(&current) {
            tab.url = next.clone();
            tab.loading = true;
            tab.selected_link = None;
            tab.clear_search();
            self.status.clear();
            tokio::spawn(fetch_page(next, tab_idx, tx.clone()));
        } else {
            self.status = "No forward history".into();
        }
    }

    /// Open a new tab navigating to `href` (resolved relative to the current tab's URL).
    pub fn open_new_tab(&mut self, href: String, tx: &Sender<BgMsg>) {
        let base = self.tabs.current().url.clone();
        let Some(url) = resolve_url(&base, &href) else {
            self.status = format!("Skipped: {href}");
            return;
        };
        let tab_idx = self.tabs.open_new(url.clone());
        tokio::spawn(fetch_page(url, tab_idx, tx.clone()));
    }

    /// Toggle bookmark for the current tab's URL; persists immediately.
    pub fn toggle_bookmark(&mut self) {
        let tab = self.tabs.current();
        let url = tab.url.clone();
        let title = tab.title.clone();
        if self.bookmarks.contains(&url) {
            self.bookmarks.remove(&url);
            self.status = "Bookmark removed".into();
        } else {
            self.bookmarks.add(url, title);
            self.status = "Bookmarked!".into();
        }
        if let Err(e) = self.bookmarks.save() {
            self.status = format!("Save failed: {e}");
        }
    }

    /// Show up to 5 bookmarks in the status bar.
    pub fn list_bookmarks(&mut self) {
        if self.bookmarks.entries.is_empty() {
            self.status = "No bookmarks (press b to bookmark this page)".into();
            return;
        }
        let list: Vec<String> = self
            .bookmarks
            .entries
            .iter()
            .take(5)
            .enumerate()
            .map(|(i, b)| format!("[{}] {}", i + 1, b.url))
            .collect();
        let extra = if self.bookmarks.entries.len() > 5 {
            format!("  (+{} more)", self.bookmarks.entries.len() - 5)
        } else {
            String::new()
        };
        self.status = format!("{}{extra}", list.join("  "));
    }
}

// ── URL resolver ──────────────────────────────────────────────────────────────

fn resolve_url(base: &str, href: &str) -> Option<String> {
    if href.starts_with("http://") || href.starts_with("https://") {
        return Some(href.to_owned());
    }
    if href.is_empty() || href.starts_with("mailto:") || href.starts_with("javascript:") {
        return None;
    }
    url::Url::parse(base).ok()?.join(href).ok().map(|u| u.to_string())
}

// ── Fetch task ────────────────────────────────────────────────────────────────

async fn fetch_page(url: String, tab_idx: usize, tx: Sender<BgMsg>) {
    let msg = fetch_inner(&url, tab_idx).await.unwrap_or_else(|e| BgMsg::Error {
        tab_idx,
        message: e.to_string(),
    });
    let _ = tx.send(msg).await;
}

async fn fetch_inner(url: &str, tab_idx: usize) -> Result<BgMsg> {
    let client = SpiderClient::new()?;
    let resp = client.fetch(url).await?;

    let (lines, links, title) = if resp.is_html() {
        let page = ParsedPage::from_bytes(&resp.body);
        let title = page.title();
        let rendered = text_renderer::render_full(&page);
        (rendered.lines, rendered.links, title)
    } else if resp.is_text() {
        let text = String::from_utf8_lossy(&resp.body);
        let lines = text.lines().map(str::to_owned).collect();
        (lines, Vec::new(), None)
    } else {
        let ct = resp.content_type.as_deref().unwrap_or("binary");
        let lines =
            vec![format!("[{ct} — {} bytes — not renderable]", resp.body.len())];
        (lines, Vec::new(), None)
    };

    Ok(BgMsg::Loaded { tab_idx, url: url.to_owned(), title, lines, links })
}

// ── Terminal helpers ──────────────────────────────────────────────────────────

fn setup_terminal() -> Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode().context("enable raw mode")?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen).context("enter alternate screen")?;
    Terminal::new(CrosstermBackend::new(stdout)).context("create terminal")
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> Result<()> {
    disable_raw_mode().context("disable raw mode")?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen).context("leave alternate screen")?;
    terminal.show_cursor().context("show cursor")
}

// ── Entry point ───────────────────────────────────────────────────────────────

/// Run the browser starting at `url`.
pub async fn run(url: String) -> Result<()> {
    let settings = Settings::load().unwrap_or_default();
    let bookmarks = Bookmarks::load().unwrap_or_else(|_| Bookmarks::empty());

    let (tx, mut rx) = mpsc::channel::<BgMsg>(8);
    let mut app = App::new(url.clone(), settings, bookmarks);
    tokio::spawn(fetch_page(url, 0, tx.clone()));

    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(std::io::stdout(), LeaveAlternateScreen);
        original_hook(info);
    }));

    let mut terminal = setup_terminal()?;
    let result = event_loop(&mut terminal, &mut app, &mut rx, &tx).await;
    restore_terminal(&mut terminal)?;
    result
}

async fn event_loop(
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
    app: &mut App,
    rx: &mut mpsc::Receiver<BgMsg>,
    tx: &Sender<BgMsg>,
) -> Result<()> {
    let mut events = EventStream::new();
    let mut render_tick = tokio::time::interval(std::time::Duration::from_millis(33));

    loop {
        tokio::select! {
            _ = render_tick.tick() => {
                terminal.draw(|f| ui::draw(f, app))?;
            }
            Some(msg) = rx.recv() => {
                app.handle_msg(msg);
            }
            Some(Ok(event)) = events.next() => {
                if let Event::Key(key) = event {
                    keybinds::handle(key, app, tx);
                }
            }
        }

        if app.quit {
            break;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_app() -> App {
        App::new("https://example.com".into(), Settings::default(), Bookmarks::empty())
    }

    #[test]
    fn scroll_clamps() {
        let mut app = make_app();
        let tab = app.tabs.current_mut();
        tab.lines = vec!["a".into(), "b".into(), "c".into()];
        tab.loading = false;
        tab.scroll_down(100);
        assert_eq!(tab.scroll, 2);
        tab.scroll_up(100);
        assert_eq!(tab.scroll, 0);
    }

    #[test]
    fn resolve_relative() {
        let r = resolve_url("https://example.com/foo/bar", "/about");
        assert_eq!(r.as_deref(), Some("https://example.com/about"));
    }

    #[test]
    fn resolve_absolute_passthrough() {
        let r = resolve_url("https://example.com", "https://other.com/page");
        assert_eq!(r.as_deref(), Some("https://other.com/page"));
    }

    #[test]
    fn resolve_mailto_returns_none() {
        assert!(resolve_url("https://example.com", "mailto:x@y.com").is_none());
    }

    #[test]
    fn resolve_javascript_returns_none() {
        assert!(resolve_url("https://example.com", "javascript:void(0)").is_none());
    }

    #[test]
    fn bookmark_toggle() {
        let mut app = make_app();
        app.tabs.current_mut().loading = false;
        app.toggle_bookmark();
        assert!(app.bookmarks.contains("https://example.com"));
        app.toggle_bookmark();
        assert!(!app.bookmarks.contains("https://example.com"));
    }
}
