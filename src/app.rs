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
    parser::{html::ParsedPage, layout},
    renderer::{
        image as image_renderer,
        text::{self as text_renderer, FormField, RenderedForm, RenderedImage, RenderedLink},
    },
    tui::{keybinds, ui},
};

// ── Input mode ────────────────────────────────────────────────────────────────

/// Current keyboard input mode.
pub enum InputMode {
    Normal,
    /// Search mode — string is the live query being typed.
    Search(String),
    /// URL edit mode — string is the editable address bar buffer.
    Url(String),
    /// Hint mode — string is the typed hint letters so far.
    Hint(String),
    /// Form-field edit mode. `field_idx` is the index into `tab.fields`;
    /// `buffer` is the live value being typed.
    FieldEdit { field_idx: usize, buffer: String },
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
        line_kinds: Vec<text_renderer::LineKind>,
        code_spans: Vec<text_renderer::CodeSpan>,
        forms: Vec<RenderedForm>,
        fields: Vec<FormField>,
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
    /// Hint codes for link-hint mode: (link_index, 2-letter code).
    pub hint_codes: Vec<(usize, String)>,
    /// HTTP client shared across the session — built once at startup.
    pub client: SpiderClient,
}

impl App {
    pub fn new(
        url: String,
        settings: Settings,
        bookmarks: Bookmarks,
        client: SpiderClient,
    ) -> Self {
        Self {
            tabs: TabManager::new(url),
            bookmarks,
            settings,
            status: String::new(),
            quit: false,
            input_mode: InputMode::Normal,
            hint_codes: Vec::new(),
            client,
        }
    }

    /// Home-row-first alphabet used for hint codes.
    const HINT_CHARS: &'static [char] = &[
        'A', 'S', 'D', 'F', 'G', 'H', 'J', 'K', 'L',
        'Q', 'W', 'E', 'R', 'T', 'Y', 'U', 'I', 'O', 'P',
        'Z', 'X', 'C', 'V', 'B', 'N', 'M',
    ];

    /// Enter link-hint mode, assigning a 2-letter Vimium-style code to each link.
    pub fn enter_hint_mode(&mut self) {
        let tab = self.tabs.current();
        if tab.links.is_empty() {
            self.status = "No links on page".into();
            return;
        }
        let n = Self::HINT_CHARS.len();
        self.hint_codes = tab
            .links
            .iter()
            .enumerate()
            .map(|(pos, _)| {
                let first = Self::HINT_CHARS[pos / n];
                let second = Self::HINT_CHARS[pos % n];
                (pos, format!("{first}{second}"))
            })
            .collect();
        self.input_mode = InputMode::Hint(String::new());
    }

    pub fn handle_msg(&mut self, msg: BgMsg) {
        match msg {
            BgMsg::Loaded {
                tab_idx, url, title, lines, links, line_kinds, code_spans, forms, fields,
            } => {
                if let Some(tab) = self.tabs.tabs.get_mut(tab_idx) {
                    tab.url = url;
                    tab.title = title.unwrap_or_default();
                    tab.lines = lines;
                    tab.links = links;
                    tab.line_kinds = line_kinds;
                    tab.code_spans = code_spans;
                    tab.field_values =
                        fields.iter().map(|f| f.value.clone()).collect();
                    tab.forms = forms;
                    tab.fields = fields;
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
        tokio::spawn(fetch_page(url, tab_idx, tx.clone(), self.client.clone()));
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
            tokio::spawn(fetch_page(prev, tab_idx, tx.clone(), self.client.clone()));
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
            tokio::spawn(fetch_page(next, tab_idx, tx.clone(), self.client.clone()));
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
        tokio::spawn(fetch_page(url, tab_idx, tx.clone(), self.client.clone()));
    }

    /// Submit form `form_idx` of the current tab via GET: build a query string
    /// from named (non-Submit) fields' values and navigate to `action?query`.
    /// Empty `action` resolves to the current URL stripped of any existing query.
    pub fn submit_form(&mut self, form_idx: usize, tx: &Sender<BgMsg>) {
        let tab = self.tabs.current();
        let Some(form) = tab.forms.get(form_idx) else {
            self.status = "No such form".into();
            return;
        };
        let action = form.action.clone();
        let query = tab.build_query(form_idx);
        let base = tab.url.clone();
        let target_base = if action.is_empty() {
            current_url_without_query(&base)
        } else {
            action
        };
        let href = if query.is_empty() {
            target_base
        } else {
            let sep = if target_base.contains('?') { '&' } else { '?' };
            format!("{target_base}{sep}{query}")
        };
        self.navigate(href, tx);
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

/// Return `base` with any `?query` and `#fragment` stripped. Used for form
/// submissions whose `action` attribute is empty (HTML default: post to self,
/// minus query). Falls back to `base` if parsing fails.
fn current_url_without_query(base: &str) -> String {
    match url::Url::parse(base) {
        Ok(mut u) => {
            u.set_query(None);
            u.set_fragment(None);
            u.to_string()
        }
        Err(_) => base.to_owned(),
    }
}

// ── Fetch task ────────────────────────────────────────────────────────────────

async fn fetch_page(url: String, tab_idx: usize, tx: Sender<BgMsg>, client: SpiderClient) {
    let msg = fetch_inner(&url, tab_idx, client).await.unwrap_or_else(|e| BgMsg::Error {
        tab_idx,
        message: e.to_string(),
    });
    let _ = tx.send(msg).await;
}

async fn fetch_inner(url: &str, tab_idx: usize, client: SpiderClient) -> Result<BgMsg> {
    let resp = client.fetch(url).await?;

    struct PageData {
        lines: Vec<String>,
        links: Vec<RenderedLink>,
        title: Option<String>,
        images: Vec<RenderedImage>,
        line_kinds: Vec<text_renderer::LineKind>,
        code_spans: Vec<text_renderer::CodeSpan>,
        forms: Vec<RenderedForm>,
        fields: Vec<FormField>,
    }

    let PageData {
        mut lines, mut links, title, mut images, mut line_kinds, code_spans, forms, mut fields,
    } = if resp.is_html() {
        let page = ParsedPage::from_bytes(&resp.body);
        let title = page.title();
        let r = text_renderer::render_full(&page);
        PageData {
            lines: r.lines,
            links: r.links,
            title,
            images: r.images,
            line_kinds: r.line_kinds,
            code_spans: r.code_spans,
            forms: r.forms,
            fields: r.fields,
        }
    } else if resp.is_text() {
        let text = String::from_utf8_lossy(&resp.body);
        let lines: Vec<String> = text.lines().map(str::to_owned).collect();
        let line_kinds = vec![text_renderer::LineKind::Normal; lines.len()];
        PageData {
            lines, links: Vec::new(), title: None, images: Vec::new(),
            line_kinds, code_spans: Vec::new(), forms: Vec::new(), fields: Vec::new(),
        }
    } else {
        let ct = resp.content_type.as_deref().unwrap_or("binary");
        let lines = vec![format!("[{ct} — {} bytes — not renderable]", resp.body.len())];
        PageData {
            lines, links: Vec::new(), title: None, images: Vec::new(),
            line_kinds: vec![text_renderer::LineKind::Normal], code_spans: Vec::new(),
            forms: Vec::new(), fields: Vec::new(),
        }
    };

    if !images.is_empty() {
        inline_images(&mut lines, &mut links, &images, url, &client).await;
    }

    // Word-wrap text lines to readable width; preserves image lines as-is.
    lines = layout::wrap_lines(
        lines, &mut links, &mut images, &mut fields, layout::DEFAULT_WIDTH,
    );

    // Sync line_kinds length after wrapping (new wrapped lines default to Normal).
    line_kinds.resize(lines.len(), text_renderer::LineKind::Normal);

    Ok(BgMsg::Loaded {
        tab_idx, url: url.to_owned(), title, lines, links, line_kinds, code_spans, forms, fields,
    })
}

/// Fetch images concurrently (shared client), convert to ANSI half-block lines,
/// splice into `lines` in place of each placeholder. Shifts link line numbers
/// to account for inserted rows. Per-image timeout caps total page latency.
async fn inline_images(
    lines: &mut Vec<String>,
    links: &mut [RenderedLink],
    images: &[RenderedImage],
    base_url: &str,
    client: &SpiderClient,
) {
    use futures_util::future::join_all;
    use std::time::Duration;

    const MAX_IMAGES: usize = 12;
    const MAX_CELLS_WIDE: u32 = 80;
    const MAX_CELLS_TALL: u32 = 18;
    const PER_IMAGE_TIMEOUT: Duration = Duration::from_secs(4);

    let fetches = images.iter().enumerate().take(MAX_IMAGES).map(|(idx, img)| {
        let src = img.src.clone();
        let base = base_url.to_owned();
        async move {
            let abs = resolve_url(&base, &src)?;
            let resp = tokio::time::timeout(PER_IMAGE_TIMEOUT, client.fetch(&abs))
                .await
                .ok()?
                .ok()?;
            let body = resp.body.to_vec();
            let ansi = tokio::task::spawn_blocking(move || {
                image_renderer::to_ansi_lines(&body, MAX_CELLS_WIDE, MAX_CELLS_TALL)
            })
            .await
            .ok()?
            .ok()?;
            Some((idx, ansi))
        }
    });

    let mut results: Vec<(usize, Vec<String>)> =
        join_all(fetches).await.into_iter().flatten().collect();

    // Splice from largest line index down so earlier positions remain valid.
    results.sort_by(|a, b| {
        images
            .get(b.0)
            .map(|i| i.line)
            .unwrap_or(0)
            .cmp(&images.get(a.0).map(|i| i.line).unwrap_or(0))
    });

    for (img_idx, ansi_lines) in results {
        let Some(img) = images.get(img_idx) else { continue };
        let pos = img.line;
        if pos >= lines.len() {
            continue;
        }
        lines.remove(pos);
        let n_inserted = ansi_lines.len();
        for (i, l) in ansi_lines.into_iter().enumerate() {
            lines.insert(pos + i, l);
        }
        let delta = n_inserted as isize - 1;
        for link in links.iter_mut() {
            if link.line > pos {
                link.line = ((link.line as isize) + delta).max(0) as usize;
            }
        }
    }
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
    let client = SpiderClient::new()?;

    let (tx, mut rx) = mpsc::channel::<BgMsg>(8);
    let mut app = App::new(url.clone(), settings, bookmarks, client.clone());
    tokio::spawn(fetch_page(url, 0, tx.clone(), client));

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
                    if key.kind == crossterm::event::KeyEventKind::Press {
                        keybinds::handle(key, app, tx);
                    }
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
        let client = SpiderClient::new().expect("client should build");
        App::new(
            "https://example.com".into(),
            Settings::default(),
            Bookmarks::empty(),
            client,
        )
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
        // HINT_CHARS = [A,S,D,F,...] so index 0='A', 1='S', 2='D'
        // pos=0: 0/27=0->'A', 0%27=0->'A' → "AA"
        // pos=1: 1/27=0->'A', 1%27=1->'S' → "AS"
        // pos=2: 2/27=0->'A', 2%27=2->'D' → "AD"
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
}
