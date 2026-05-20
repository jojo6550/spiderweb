//! Key event routing: scroll, follow link, back, quit.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::mpsc::Sender;

use crate::app::{App, BgMsg};

/// Handle one key event. Returns after mutating `app` or sending a `BgMsg`.
pub fn handle(key: KeyEvent, app: &mut App, tx: &Sender<BgMsg>) {
    match (key.modifiers, key.code) {
        // ── Quit ──────────────────────────────────────────────────────────────
        (KeyModifiers::NONE, KeyCode::Char('q'))
        | (KeyModifiers::CONTROL, KeyCode::Char('c')) => {
            app.quit = true;
        }

        // ── Scroll ────────────────────────────────────────────────────────────
        (KeyModifiers::NONE, KeyCode::Char('j')) | (KeyModifiers::NONE, KeyCode::Down) => {
            app.scroll_down(1);
        }
        (KeyModifiers::NONE, KeyCode::Char('k')) | (KeyModifiers::NONE, KeyCode::Up) => {
            app.scroll_up(1);
        }
        (KeyModifiers::NONE, KeyCode::Char('d')) | (KeyModifiers::NONE, KeyCode::PageDown) => {
            app.scroll_down(20);
        }
        (KeyModifiers::NONE, KeyCode::Char('u')) | (KeyModifiers::NONE, KeyCode::PageUp) => {
            app.scroll_up(20);
        }
        (KeyModifiers::NONE, KeyCode::Char('g')) | (KeyModifiers::NONE, KeyCode::Home) => {
            app.scroll = 0;
        }
        (KeyModifiers::SHIFT, KeyCode::Char('G')) | (KeyModifiers::NONE, KeyCode::End) => {
            app.scroll_to_bottom();
        }

        // ── Link selection ────────────────────────────────────────────────────
        (KeyModifiers::NONE, KeyCode::Tab) => {
            app.next_link();
        }
        (KeyModifiers::SHIFT, KeyCode::BackTab) => {
            app.prev_link();
        }

        // ── Navigation ────────────────────────────────────────────────────────
        (KeyModifiers::NONE, KeyCode::Enter) => {
            if let Some(url) = app.selected_link_url() {
                app.navigate(url, tx);
            } else {
                app.status = "No link selected (Tab to select)".into();
            }
        }
        (KeyModifiers::NONE, KeyCode::Backspace) => {
            app.status = "History: back — Phase 2".into();
        }

        _ => {}
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn placeholder() {}
}
