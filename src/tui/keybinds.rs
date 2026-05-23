//! Key event routing.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::mpsc::Sender;

use crate::app::{App, BgMsg, InputMode};

/// Dispatch a key event based on current input mode.
pub fn handle(key: KeyEvent, app: &mut App, tx: &Sender<BgMsg>) {
    match &app.input_mode {
        InputMode::Search(_) => handle_search(key, app),
        InputMode::Url(_) => handle_url(key, app, tx),
        InputMode::Hint(_) => handle_hint(key, app, tx),
        InputMode::FieldEdit { .. } => handle_field_edit(key, app, tx),
        InputMode::Normal => handle_normal(key, app, tx),
    }
}

/// Commit the buffer back onto the tab's `field_values`. No-op if `field_idx`
/// is out of range (e.g. page reloaded mid-edit).
fn commit_field_buffer(app: &mut App, field_idx: usize, buffer: String) {
    let tab = app.tabs.current_mut();
    if let Some(slot) = tab.field_values.get_mut(field_idx) {
        *slot = buffer;
    }
}

fn handle_field_edit(key: KeyEvent, app: &mut App, tx: &Sender<BgMsg>) {
    let (field_idx, buffer) = match &app.input_mode {
        InputMode::FieldEdit { field_idx, buffer } => (*field_idx, buffer.clone()),
        _ => return,
    };
    match (key.modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::Esc) => {
            app.input_mode = InputMode::Normal;
            app.tabs.current_mut().focused =
                Some(crate::browser::tabs::FocusItem::Field(field_idx));
        }
        (KeyModifiers::NONE, KeyCode::Enter) => {
            commit_field_buffer(app, field_idx, buffer);
            let form_idx = app
                .tabs
                .current()
                .fields
                .get(field_idx)
                .map(|f| f.form_idx);
            app.input_mode = InputMode::Normal;
            app.tabs.current_mut().focused =
                Some(crate::browser::tabs::FocusItem::Field(field_idx));
            if let Some(fi) = form_idx {
                app.submit_form(fi, tx);
            }
        }
        (KeyModifiers::NONE, KeyCode::Tab) => {
            commit_field_buffer(app, field_idx, buffer);
            let next = app.tabs.current().next_editable_field(Some(field_idx));
            match next {
                Some(n) => {
                    let new_buf = app
                        .tabs
                        .current()
                        .field_values
                        .get(n)
                        .cloned()
                        .unwrap_or_default();
                    // Keep the next field visible.
                    if let Some(line) = app.tabs.current().fields.get(n).map(|f| f.line) {
                        app.tabs.current_mut().scroll = line;
                    }
                    app.input_mode = InputMode::FieldEdit { field_idx: n, buffer: new_buf };
                }
                None => {
                    app.input_mode = InputMode::Normal;
                }
            }
        }
        (KeyModifiers::NONE, KeyCode::Backspace) => {
            let mut b = buffer;
            b.pop();
            app.input_mode = InputMode::FieldEdit { field_idx, buffer: b };
        }
        (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
            let mut b = buffer;
            b.push(c);
            app.input_mode = InputMode::FieldEdit { field_idx, buffer: b };
        }
        _ => {}
    }
}

fn handle_search(key: KeyEvent, app: &mut App) {
    let query = match &app.input_mode {
        InputMode::Search(q) => q.clone(),
        _ => return,
    };

    match key.code {
        KeyCode::Esc => {
            app.tabs.current_mut().clear_search();
            app.input_mode = InputMode::Normal;
        }
        KeyCode::Enter => {
            app.input_mode = InputMode::Normal;
        }
        KeyCode::Backspace => {
            let mut q = query;
            q.pop();
            let q_clone = q.clone();
            app.tabs.current_mut().search(&q_clone);
            app.input_mode = InputMode::Search(q);
        }
        KeyCode::Char(c)
            if key.modifiers == KeyModifiers::NONE
                || key.modifiers == KeyModifiers::SHIFT =>
        {
            let mut q = query;
            q.push(c);
            let q_clone = q.clone();
            app.tabs.current_mut().search(&q_clone);
            app.input_mode = InputMode::Search(q);
        }
        _ => {}
    }
}

fn handle_url(key: KeyEvent, app: &mut App, tx: &Sender<BgMsg>) {
    let buf = match &app.input_mode {
        InputMode::Url(b) => b.clone(),
        _ => return,
    };
    match (key.modifiers, key.code) {
        (KeyModifiers::NONE, KeyCode::Esc) => {
            app.input_mode = InputMode::Normal;
        }
        (KeyModifiers::NONE, KeyCode::Enter) => {
            let url = crate::app::normalize_url_input(&buf);
            app.input_mode = InputMode::Normal;
            if !url.is_empty() {
                app.navigate(url, tx);
            }
        }
        (KeyModifiers::NONE, KeyCode::Backspace) => {
            let mut b = buf;
            b.pop();
            app.input_mode = InputMode::Url(b);
        }
        (KeyModifiers::CONTROL, KeyCode::Char('w')) => {
            app.input_mode = InputMode::Url(clear_last_segment(buf));
        }
        (KeyModifiers::NONE | KeyModifiers::SHIFT, KeyCode::Char(c)) => {
            let mut b = buf;
            b.push(c);
            app.input_mode = InputMode::Url(b);
        }
        _ => {}
    }
}

/// Pop trailing slash/space, then pop back to the previous slash/space.
fn clear_last_segment(mut s: String) -> String {
    while s.ends_with('/') || s.ends_with(' ') {
        s.pop();
    }
    while !s.is_empty() && !s.ends_with('/') && !s.ends_with(' ') {
        s.pop();
    }
    s
}

/// Full Vimium-style hint mode: accumulates typed chars, matches 2-char codes, navigates.
fn handle_hint(key: KeyEvent, app: &mut App, tx: &Sender<BgMsg>) {
    let typed = match &app.input_mode {
        InputMode::Hint(s) => s.clone(),
        _ => return,
    };
    match key.code {
        KeyCode::Esc => {
            app.hint_codes.clear();
            app.input_mode = InputMode::Normal;
        }
        KeyCode::Backspace => {
            let mut t = typed;
            t.pop();
            app.input_mode = InputMode::Hint(t);
        }
        KeyCode::Char(c) => {
            let open_new_tab = key.modifiers.contains(KeyModifiers::SHIFT);
            let upper = c.to_ascii_uppercase();
            let mut new_typed = typed;
            new_typed.push(upper);

            if new_typed.len() >= 2 {
                let matched = app
                    .hint_codes
                    .iter()
                    .find(|(_, code)| *code == new_typed)
                    .map(|(link_idx, _)| *link_idx);
                app.hint_codes.clear();
                app.input_mode = InputMode::Normal;
                if let Some(link_idx) = matched {
                    let href = app
                        .tabs
                        .current()
                        .links
                        .get(link_idx)
                        .map(|l| l.href.clone());
                    if let Some(href) = href {
                        if open_new_tab {
                            app.open_new_tab(href, tx);
                        } else {
                            app.navigate(href, tx);
                        }
                    }
                } else {
                    app.status = format!("No hint '{new_typed}'");
                }
            } else {
                app.input_mode = InputMode::Hint(new_typed);
            }
        }
        _ => {}
    }
}

fn handle_normal(key: KeyEvent, app: &mut App, tx: &Sender<BgMsg>) {
    match (key.modifiers, key.code) {
        // ── Quit ──────────────────────────────────────────────────────────────
        (KeyModifiers::NONE, KeyCode::Char('q'))
        | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
            app.quit = true;
        }

        // ── Scroll ────────────────────────────────────────────────────────────
        (KeyModifiers::NONE, KeyCode::Char('j')) | (KeyModifiers::NONE, KeyCode::Down) => {
            app.tabs.current_mut().scroll_down(1);
        }
        (KeyModifiers::NONE, KeyCode::Char('k')) | (KeyModifiers::NONE, KeyCode::Up) => {
            app.tabs.current_mut().scroll_up(1);
        }
        (KeyModifiers::NONE, KeyCode::Char('d')) | (KeyModifiers::NONE, KeyCode::PageDown) => {
            app.tabs.current_mut().scroll_down(20);
        }
        (KeyModifiers::NONE, KeyCode::Char('u')) | (KeyModifiers::NONE, KeyCode::PageUp) => {
            app.tabs.current_mut().scroll_up(20);
        }
        (KeyModifiers::NONE, KeyCode::Char('g')) | (KeyModifiers::NONE, KeyCode::Home) => {
            app.tabs.current_mut().scroll = 0;
        }
        (KeyModifiers::SHIFT, KeyCode::Char('G')) | (KeyModifiers::NONE, KeyCode::End) => {
            app.tabs.current_mut().scroll_to_bottom();
        }

        // ── Focus navigation ──────────────────────────────────────────────────
        (KeyModifiers::NONE, KeyCode::Tab) => {
            app.tabs.current_mut().next_focus();
        }
        (KeyModifiers::SHIFT, KeyCode::BackTab) => {
            app.tabs.current_mut().prev_focus();
        }

        // ── Activate focused item ─────────────────────────────────────────────
        (KeyModifiers::NONE, KeyCode::Enter) => {
            use crate::browser::tabs::FocusItem;
            use crate::renderer::text::FieldKind;
            let focused = app.tabs.current().focused.clone();
            match focused {
                None => {
                    app.status = "Nothing focused — press Tab to select".into();
                }
                Some(FocusItem::Link(_)) => {
                    let href = app.tabs.current().selected_href().map(str::to_owned);
                    if let Some(href) = href {
                        app.navigate(href, tx);
                    }
                }
                Some(FocusItem::Field(field_idx)) => {
                    let kind = app.tabs.current().fields
                        .get(field_idx)
                        .map(|f| f.kind.clone());
                    match kind {
                        Some(FieldKind::Submit) => {
                            let form_idx = app.tabs.current().fields
                                .get(field_idx)
                                .map(|f| f.form_idx);
                            if let Some(fi) = form_idx {
                                app.submit_form(fi, tx);
                            }
                        }
                        Some(FieldKind::Text | FieldKind::Textarea) => {
                            let buffer = app.tabs.current().field_values
                                .get(field_idx)
                                .cloned()
                                .unwrap_or_default();
                            if let Some(line) = app.tabs.current().fields.get(field_idx).map(|f| f.line) {
                                app.tabs.current_mut().scroll = line;
                            }
                            app.input_mode = InputMode::FieldEdit { field_idx, buffer };
                        }
                        Some(FieldKind::Checkbox) => {
                            let toggle_val = app.tabs.current().fields
                                .get(field_idx)
                                .map(|f| f.value.clone())
                                .unwrap_or_default();
                            if let Some(slot) = app.tabs.current_mut().field_values.get_mut(field_idx) {
                                *slot = if slot.is_empty() { toggle_val } else { String::new() };
                            }
                        }
                        Some(FieldKind::Radio) => {
                            let (radio_val, radio_name, radio_form) = app.tabs.current().fields
                                .get(field_idx)
                                .map(|f| (f.value.clone(), f.name.clone(), f.form_idx))
                                .unwrap_or_else(|| (String::new(), String::new(), 0));
                            let siblings: Vec<usize> = app.tabs.current().fields
                                .iter()
                                .enumerate()
                                .filter(|(i, f)| {
                                    f.form_idx == radio_form
                                        && f.name == radio_name
                                        && matches!(f.kind, FieldKind::Radio)
                                        && *i != field_idx
                                })
                                .map(|(i, _)| i)
                                .collect();
                            if let Some(slot) = app.tabs.current_mut().field_values.get_mut(field_idx) {
                                *slot = radio_val;
                            }
                            for i in siblings {
                                if let Some(slot) = app.tabs.current_mut().field_values.get_mut(i) {
                                    *slot = String::new();
                                }
                            }
                        }
                        Some(FieldKind::Select(opts)) => {
                            let current = app.tabs.current().field_values
                                .get(field_idx)
                                .cloned()
                                .unwrap_or_default();
                            let cur_idx = opts.iter().position(|o| o == &current).unwrap_or(0);
                            let next_idx = (cur_idx + 1) % opts.len().max(1);
                            if let Some(slot) = app.tabs.current_mut().field_values.get_mut(field_idx) {
                                *slot = opts.get(next_idx).cloned().unwrap_or_default();
                            }
                        }
                        Some(FieldKind::Hidden) | None => {}
                    }
                }
            }
        }
        (KeyModifiers::NONE, KeyCode::Backspace) => {
            app.go_back(tx);
        }
        (KeyModifiers::ALT, KeyCode::Right) | (KeyModifiers::CONTROL, KeyCode::Right) => {
            app.go_forward(tx);
        }

        // ── Tabs ──────────────────────────────────────────────────────────────
        (KeyModifiers::NONE, KeyCode::Char('t')) => {
            let href = app
                .tabs
                .current()
                .selected_href()
                .map(str::to_owned)
                .unwrap_or_else(|| app.settings.home_page.clone());
            app.open_new_tab(href, tx);
        }
        (KeyModifiers::NONE, KeyCode::Char('x')) => {
            app.tabs.close_current();
        }
        (KeyModifiers::NONE, KeyCode::Char('1')) => app.tabs.switch_to(0),
        (KeyModifiers::NONE, KeyCode::Char('2')) => app.tabs.switch_to(1),
        (KeyModifiers::NONE, KeyCode::Char('3')) => app.tabs.switch_to(2),
        (KeyModifiers::NONE, KeyCode::Char('4')) => app.tabs.switch_to(3),
        (KeyModifiers::NONE, KeyCode::Char('5')) => app.tabs.switch_to(4),
        (KeyModifiers::NONE, KeyCode::Char('6')) => app.tabs.switch_to(5),
        (KeyModifiers::NONE, KeyCode::Char('7')) => app.tabs.switch_to(6),
        (KeyModifiers::NONE, KeyCode::Char('8')) => app.tabs.switch_to(7),
        (KeyModifiers::NONE, KeyCode::Char('9')) => app.tabs.switch_to(8),

        // ── Bookmarks ─────────────────────────────────────────────────────────
        (KeyModifiers::NONE, KeyCode::Char('b')) => {
            app.toggle_bookmark();
        }
        (KeyModifiers::SHIFT, KeyCode::Char('B')) => {
            app.list_bookmarks();
        }

        // ── Search ────────────────────────────────────────────────────────────
        (KeyModifiers::NONE, KeyCode::Char('/')) => {
            app.input_mode = InputMode::Search(String::new());
            app.tabs.current_mut().clear_search();
        }
        (KeyModifiers::NONE, KeyCode::Char('n')) => {
            app.tabs.current_mut().search_next();
        }
        (KeyModifiers::SHIFT, KeyCode::Char('N')) => {
            app.tabs.current_mut().search_prev();
        }

        // ── URL edit ──────────────────────────────────────────────────────────
        (KeyModifiers::NONE, KeyCode::Char('o')) => {
            let url = app.tabs.current().url.clone();
            app.input_mode = InputMode::Url(url);
        }

        // ── Link hints ────────────────────────────────────────────────────────
        (KeyModifiers::NONE, KeyCode::Char('f')) => {
            app.enter_hint_mode();
        }

        // ── Forms ─────────────────────────────────────────────────────────────
        (KeyModifiers::NONE, KeyCode::Char('i')) => {
            let next = app.tabs.current().next_editable_field(None);
            match next {
                Some(idx) => {
                    let buffer = app
                        .tabs
                        .current()
                        .field_values
                        .get(idx)
                        .cloned()
                        .unwrap_or_default();
                    if let Some(line) = app.tabs.current().fields.get(idx).map(|f| f.line) {
                        app.tabs.current_mut().scroll = line;
                    }
                    app.input_mode = InputMode::FieldEdit { field_idx: idx, buffer };
                }
                None => {
                    app.status = "No editable form field on this page".into();
                }
            }
        }

        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        browser::bookmarks::Bookmarks, config::settings::Settings,
        network::client::SpiderClient,
    };

    fn make_app() -> App {
        let client = SpiderClient::new().expect("client");
        crate::app::App::new(
            "https://example.com".into(),
            Settings::default(),
            Bookmarks::empty(),
            client,
        )
    }

    #[test]
    fn search_mode_enter_exits() {
        let mut app = make_app();
        app.input_mode = InputMode::Search("hello".into());
        let key =
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        handle_search(key, &mut app);
        assert!(matches!(app.input_mode, InputMode::Normal));
    }

    #[test]
    fn search_mode_esc_clears() {
        let mut app = make_app();
        app.input_mode = InputMode::Search("hello".into());
        let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        handle_search(key, &mut app);
        assert!(matches!(app.input_mode, InputMode::Normal));
        assert!(app.tabs.current().search_matches.is_empty());
    }

    #[test]
    fn url_mode_char_appends() {
        let mut app = make_app();
        app.input_mode = InputMode::Url("https://".into());
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let key = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE);
        handle(key, &mut app, &tx);
        assert!(matches!(&app.input_mode, InputMode::Url(s) if s == "https://x"));
    }

    #[test]
    fn url_mode_backspace_pops() {
        let mut app = make_app();
        app.input_mode = InputMode::Url("https://abc".into());
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let key = KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE);
        handle(key, &mut app, &tx);
        assert!(matches!(&app.input_mode, InputMode::Url(s) if s == "https://ab"));
    }

    #[test]
    fn url_mode_esc_returns_normal() {
        let mut app = make_app();
        app.input_mode = InputMode::Url("https://foo.com".into());
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        handle(key, &mut app, &tx);
        assert!(matches!(app.input_mode, InputMode::Normal));
    }

    #[test]
    fn hint_mode_esc_returns_normal_and_clears_codes() {
        let mut app = make_app();
        app.input_mode = InputMode::Hint("A".into());
        app.hint_codes = vec![(0, "AA".into())];
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        handle(key, &mut app, &tx);
        assert!(matches!(app.input_mode, InputMode::Normal));
        assert!(app.hint_codes.is_empty());
    }

    #[test]
    fn hint_mode_first_char_updates_typed() {
        let mut app = make_app();
        app.input_mode = InputMode::Hint(String::new());
        app.hint_codes = vec![(0, "AA".into()), (1, "AS".into())];
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let key = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE);
        handle(key, &mut app, &tx);
        assert!(matches!(&app.input_mode, InputMode::Hint(s) if s == "A"));
    }

    #[test]
    fn url_mode_ctrl_w_clears_last_segment() {
        let mut app = make_app();
        app.input_mode = InputMode::Url("https://example.com/foo/bar".into());
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let key = KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL);
        handle(key, &mut app, &tx);
        assert!(matches!(&app.input_mode, InputMode::Url(s) if s == "https://example.com/foo/"));
    }

    fn app_with_search_form() -> crate::app::App {
        use crate::renderer::text::{FieldKind, FormField, RenderedForm};
        let mut app = make_app();
        let tab = app.tabs.current_mut();
        tab.loading = false;
        tab.forms = vec![RenderedForm { action: "/search".into(), method: "get".into() }];
        tab.fields = vec![FormField {
            form_idx: 0,
            name: "q".into(),
            kind: FieldKind::Text,
            value: String::new(),
            line: 3,
        }];
        tab.field_values = vec![String::new()];
        app
    }

    #[test]
    fn i_key_enters_field_edit_on_text_input() {
        let mut app = app_with_search_form();
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let key = KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE);
        handle(key, &mut app, &tx);
        assert!(matches!(
            &app.input_mode,
            InputMode::FieldEdit { field_idx: 0, buffer } if buffer.is_empty()
        ));
    }

    #[test]
    fn i_key_with_no_form_sets_status() {
        let mut app = make_app();
        app.tabs.current_mut().loading = false;
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let key = KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE);
        handle(key, &mut app, &tx);
        assert!(matches!(app.input_mode, InputMode::Normal));
        assert!(!app.status.is_empty());
    }

    #[test]
    fn field_edit_typing_appends_to_buffer() {
        let mut app = app_with_search_form();
        app.input_mode = InputMode::FieldEdit { field_idx: 0, buffer: String::new() };
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        for c in "hi".chars() {
            let key = KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
            handle(key, &mut app, &tx);
        }
        assert!(matches!(
            &app.input_mode,
            InputMode::FieldEdit { buffer, .. } if buffer == "hi"
        ));
    }

    #[test]
    fn field_edit_esc_discards_buffer() {
        let mut app = app_with_search_form();
        app.input_mode = InputMode::FieldEdit { field_idx: 0, buffer: "draft".into() };
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let key = KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE);
        handle(key, &mut app, &tx);
        assert!(matches!(app.input_mode, InputMode::Normal));
        assert_eq!(app.tabs.current().field_values[0], "");
    }

    #[test]
    fn field_edit_backspace_pops() {
        let mut app = app_with_search_form();
        app.input_mode = InputMode::FieldEdit { field_idx: 0, buffer: "abc".into() };
        let (tx, _rx) = tokio::sync::mpsc::channel(1);
        let key = KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE);
        handle(key, &mut app, &tx);
        assert!(matches!(
            &app.input_mode,
            InputMode::FieldEdit { buffer, .. } if buffer == "ab"
        ));
    }
}
