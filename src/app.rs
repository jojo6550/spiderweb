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
    network::client::SpiderClient,
    parser::html::ParsedPage,
    renderer::text as text_renderer,
    tui::{keybinds, ui},
};

// ── Background message ────────────────────────────────────────────────────────

/// Messages sent from background fetch tasks to the event loop.
pub enum BgMsg {
    /// Page fetched and rendered successfully.
    Loaded {
        url: String,
        lines: Vec<String>,
        links: Vec<String>,
    },
    /// Fetch or parse failed.
    Error { message: String },
}

// ── App state ─────────────────────────────────────────────────────────────────

/// Central application state — owned by the main TUI event loop.
pub struct App {
    /// URL currently displayed.
    pub url: String,
    /// Plain-text content lines for display.
    pub lines: Vec<String>,
    /// Absolute or root-relative hrefs extracted from the page.
    pub links: Vec<String>,
    /// Vertical scroll offset (line index).
    pub scroll: usize,
    /// Index into `links` of the currently highlighted link.
    pub selected_link: Option<usize>,
    /// One-line status message shown in the status bar.
    pub status: String,
    /// `true` while a fetch is in progress.
    pub loading: bool,
    /// Set to `true` to exit the event loop.
    pub quit: bool,
}

impl App {
    fn new(url: String) -> Self {
        Self {
            url,
            lines: Vec::new(),
            links: Vec::new(),
            scroll: 0,
            selected_link: None,
            status: String::new(),
            loading: true,
            quit: false,
        }
    }

    /// Apply a background message received from a fetch task.
    pub fn handle_msg(&mut self, msg: BgMsg) {
        match msg {
            BgMsg::Loaded { url, lines, links } => {
                self.url = url;
                self.lines = lines;
                self.links = links;
                self.scroll = 0;
                self.selected_link = None;
                self.loading = false;
                self.status = String::new();
            }
            BgMsg::Error { message } => {
                self.loading = false;
                self.status = format!("Error: {message}");
            }
        }
    }

    /// Scroll down by `n` lines, clamped.
    pub fn scroll_down(&mut self, n: usize) {
        self.scroll = self.scroll.saturating_add(n).min(self.lines.len().saturating_sub(1));
    }

    /// Scroll up by `n` lines.
    pub fn scroll_up(&mut self, n: usize) {
        self.scroll = self.scroll.saturating_sub(n);
    }

    /// Scroll to last line.
    pub fn scroll_to_bottom(&mut self) {
        self.scroll = self.lines.len().saturating_sub(1);
    }

    /// Advance to the next link, wrapping around.
    pub fn next_link(&mut self) {
        if self.links.is_empty() {
            return;
        }
        self.selected_link = Some(match self.selected_link {
            None => 0,
            Some(i) => (i + 1) % self.links.len(),
        });
    }

    /// Move to the previous link, wrapping around.
    pub fn prev_link(&mut self) {
        if self.links.is_empty() {
            return;
        }
        self.selected_link = Some(match self.selected_link {
            None => self.links.len().saturating_sub(1),
            Some(0) => self.links.len().saturating_sub(1),
            Some(i) => i - 1,
        });
    }

    /// Return the href of the currently selected link, if any.
    pub fn selected_link_url(&self) -> Option<String> {
        let idx = self.selected_link?;
        self.links.get(idx).cloned()
    }

    /// Return the content line index that contains link `sel`, used for highlight.
    /// Phase 1 stub — returns `None` (full link↔line mapping is Phase 2).
    pub fn link_line_idx(&self, _sel: usize) -> Option<usize> {
        None
    }

    /// Navigate to `url`: mark loading and spawn fetch task.
    pub fn navigate(&mut self, url: String, tx: &Sender<BgMsg>) {
        // Phase 1: absolute URLs only.
        if !url.starts_with("http://") && !url.starts_with("https://") {
            self.status = format!("Skipped relative URL: {url}");
            return;
        }
        self.url = url.clone();
        self.loading = true;
        self.status = String::new();
        let tx = tx.clone();
        tokio::spawn(fetch_page(url, tx));
    }
}

// ── Fetch task ────────────────────────────────────────────────────────────────

async fn fetch_page(url: String, tx: Sender<BgMsg>) {
    let msg = fetch_inner(&url).await.unwrap_or_else(|e| BgMsg::Error {
        message: e.to_string(),
    });
    let _ = tx.send(msg).await;
}

async fn fetch_inner(url: &str) -> Result<BgMsg> {
    let client = SpiderClient::new()?;
    let resp = client.fetch(url).await?;

    let (lines, links) = if resp.is_html() {
        let page = ParsedPage::from_bytes(&resp.body);
        let links: Vec<String> = page.links().into_iter().map(|l| l.href).collect();
        let ansi = text_renderer::render(&page);
        let lines = text_renderer::strip_ansi(&ansi)
            .lines()
            .map(str::to_owned)
            .collect();
        (lines, links)
    } else if resp.is_text() {
        let text = String::from_utf8_lossy(&resp.body);
        let lines = text.lines().map(str::to_owned).collect();
        (lines, Vec::new())
    } else if resp
        .content_type
        .as_deref()
        .map(|ct| ct.starts_with("image/"))
        .unwrap_or(false)
    {
        let ct = resp.content_type.as_deref().unwrap_or("image");
        let lines = vec![
            format!("[{ct} — {} bytes]", resp.body.len()),
            String::new(),
            "Image rendering in terminal not supported for direct URLs in Phase 1.".into(),
            "Phase 2 will render inline images within HTML pages.".into(),
        ];
        (lines, Vec::new())
    } else {
        return Err(anyhow::anyhow!(
            "unsupported content type: {:?}",
            resp.content_type
        ));
    };

    Ok(BgMsg::Loaded {
        url: url.to_owned(),
        lines,
        links,
    })
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
    let (tx, mut rx) = mpsc::channel::<BgMsg>(8);

    let mut app = App::new(url.clone());
    tokio::spawn(fetch_page(url, tx.clone()));

    // Panic hook: restore terminal before printing panic message.
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

    #[test]
    fn scroll_clamps() {
        let mut app = App::new("https://example.com".into());
        app.lines = vec!["a".into(), "b".into(), "c".into()];
        app.scroll_down(100);
        assert_eq!(app.scroll, 2);
        app.scroll_up(100);
        assert_eq!(app.scroll, 0);
    }

    #[test]
    fn link_cycling() {
        let mut app = App::new("https://example.com".into());
        app.links = vec!["https://a.com".into(), "https://b.com".into()];
        app.next_link();
        assert_eq!(app.selected_link, Some(0));
        app.next_link();
        assert_eq!(app.selected_link, Some(1));
        app.next_link();
        assert_eq!(app.selected_link, Some(0));
        app.prev_link();
        assert_eq!(app.selected_link, Some(1));
    }
}
