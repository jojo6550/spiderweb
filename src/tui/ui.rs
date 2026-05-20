//! ratatui layout composition: address bar, scrollable content pane, status bar.

use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::app::App;

/// Render the full TUI frame.
pub fn draw(frame: &mut Frame, app: &App) {
    let [addr_area, content_area, status_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    // ── Address bar ──────────────────────────────────────────────────────────
    let addr_style = Style::new().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD);
    let addr_text = if app.loading {
        format!(" ⟳ {}", app.url)
    } else {
        format!(" > {}", app.url)
    };
    frame.render_widget(Paragraph::new(addr_text).style(addr_style), addr_area);

    // ── Content pane ─────────────────────────────────────────────────────────
    let viewport_h = content_area.height as usize;
    let visible: Vec<Line> = app
        .lines
        .iter()
        .enumerate()
        .skip(app.scroll)
        .take(viewport_h)
        .map(|(i, text)| {
            let is_link_line = app
                .selected_link
                .map(|sel| app.link_line_idx(sel) == Some(i))
                .unwrap_or(false);
            if is_link_line {
                Line::from(Span::styled(
                    text.as_str(),
                    Style::new().bg(Color::DarkGray).fg(Color::Cyan).add_modifier(Modifier::UNDERLINED),
                ))
            } else {
                Line::raw(text.as_str())
            }
        })
        .collect();

    frame.render_widget(Paragraph::new(visible), content_area);

    // ── Status bar ────────────────────────────────────────────────────────────
    let status_style = Style::new().bg(Color::DarkGray).fg(Color::White);
    let hint = " q:quit  j/k:scroll  Tab:link  Enter:go  Backspace:back";
    let status_text = if app.status.is_empty() {
        format!("{hint}  [{}/{}]", app.scroll + 1, app.lines.len().max(1))
    } else {
        format!(" {}  {hint}", app.status)
    };
    frame.render_widget(Paragraph::new(status_text).style(status_style), status_area);
}

#[cfg(test)]
mod tests {
    #[test]
    fn placeholder() {}
}
