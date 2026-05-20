//! Key event routing.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::mpsc::Sender;

use crate::app::{App, BgMsg, InputMode};

/// Dispatch a key event based on current input mode.
pub fn handle(key: KeyEvent, app: &mut App, tx: &Sender<BgMsg>) {
    if matches!(app.input_mode, InputMode::Search(_)) {
        handle_search(key, app);
    } else {
        handle_normal(key, app, tx);
    }
}

fn handle_search(key: KeyEvent, app: &mut App) {
    let query = match &app.input_mode {
        InputMode::Search(q) => q.clone(),
        InputMode::Normal => return,
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

        // ── Link selection ────────────────────────────────────────────────────
        (KeyModifiers::NONE, KeyCode::Tab) => {
            app.tabs.current_mut().next_link();
        }
        (KeyModifiers::SHIFT, KeyCode::BackTab) => {
            app.tabs.current_mut().prev_link();
        }

        // ── Navigation ────────────────────────────────────────────────────────
        (KeyModifiers::NONE, KeyCode::Enter) => {
            let href = app.tabs.current().selected_href().map(str::to_owned);
            if let Some(href) = href {
                app.navigate(href, tx);
            } else {
                app.status = "No link selected — press Tab to select a link".into();
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

        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{browser::bookmarks::Bookmarks, config::settings::Settings};

    fn make_app() -> App {
        crate::app::App::new(
            "https://example.com".into(),
            Settings::default(),
            Bookmarks::empty(),
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
}
