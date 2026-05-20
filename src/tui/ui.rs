//! ratatui layout composition: tab bar, address bar, content pane, search bar, status bar.

use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::app::{App, InputMode};

/// Render the full TUI frame.
pub fn draw(frame: &mut Frame, app: &App) {
    let tab = app.tabs.current();
    let show_tabs = app.tabs.tabs.len() > 1;
    let in_search = matches!(app.input_mode, InputMode::Search(_));

    let mut constraints = Vec::new();
    if show_tabs {
        constraints.push(Constraint::Length(1));
    }
    constraints.push(Constraint::Length(1)); // address bar
    constraints.push(Constraint::Fill(1));   // content
    if in_search {
        constraints.push(Constraint::Length(1)); // search bar
    }
    constraints.push(Constraint::Length(1)); // status bar

    let areas = Layout::vertical(constraints).split(frame.area());
    let mut idx = 0usize;

    if show_tabs {
        draw_tab_bar(frame, app, areas[idx]);
        idx += 1;
    }

    // ── Address bar ──────────────────────────────────────────────────────────
    let addr_style = Style::new().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD);
    let bm_star = if app.bookmarks.contains(&tab.url) { "★ " } else { "" };
    let prefix = if tab.loading { " ⟳ " } else { " > " };
    frame.render_widget(
        Paragraph::new(format!("{prefix}{bm_star}{}", tab.url)).style(addr_style),
        areas[idx],
    );
    idx += 1;

    // ── Content pane ─────────────────────────────────────────────────────────
    let content_area = areas[idx];
    idx += 1;
    draw_content(frame, app, content_area);

    // ── Search bar ────────────────────────────────────────────────────────────
    if in_search {
        let query =
            if let InputMode::Search(ref q) = app.input_mode { q.as_str() } else { "" };
        let match_info = if !query.is_empty() && tab.search_matches.is_empty() {
            " (no matches)".to_owned()
        } else if !tab.search_matches.is_empty() {
            format!(" ({}/{})", tab.search_idx + 1, tab.search_matches.len())
        } else {
            String::new()
        };
        frame.render_widget(
            Paragraph::new(format!("/{query}{match_info}"))
                .style(Style::new().bg(Color::DarkGray).fg(Color::White)),
            areas[idx],
        );
        idx += 1;
    }

    // ── Status bar ────────────────────────────────────────────────────────────
    let hint = if in_search {
        " Esc:cancel  Enter:done  n/N:next/prev".to_owned()
    } else {
        " q:quit  j/k:scroll  Tab:link  Enter:go  Backspace:back  t:tab  b:bookmark  /:search"
            .to_owned()
    };
    let line_info = format!("[{}/{}]", tab.scroll + 1, tab.lines.len().max(1));
    let status_text = if app.status.is_empty() {
        format!("{hint}  {line_info}")
    } else {
        format!(" {}  {hint}", app.status)
    };
    frame.render_widget(
        Paragraph::new(status_text).style(Style::new().bg(Color::DarkGray).fg(Color::White)),
        areas[idx],
    );
}

fn draw_content(frame: &mut Frame, app: &App, area: Rect) {
    let tab = app.tabs.current();

    if tab.lines.is_empty() && !tab.loading {
        let msg =
            if app.status.is_empty() { "No content".to_owned() } else { app.status.clone() };
        frame.render_widget(
            Paragraph::new(msg)
                .alignment(Alignment::Center)
                .style(Style::new().fg(Color::Red)),
            area,
        );
        return;
    }

    let viewport_h = area.height as usize;
    let visible: Vec<Line> = tab
        .lines
        .iter()
        .enumerate()
        .skip(tab.scroll)
        .take(viewport_h)
        .map(|(i, text)| {
            let is_link = tab
                .selected_link
                .and_then(|sel| tab.links.get(sel))
                .map(|rl| rl.line == i)
                .unwrap_or(false);
            let is_current_match =
                !tab.search_matches.is_empty() && tab.search_matches[tab.search_idx] == i;
            let is_other_match =
                !is_current_match && tab.search_matches.contains(&i);

            if is_link {
                Line::from(Span::styled(
                    text.as_str(),
                    Style::new()
                        .bg(Color::DarkGray)
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::UNDERLINED),
                ))
            } else if is_current_match {
                Line::from(Span::styled(
                    text.as_str(),
                    Style::new().bg(Color::Yellow).fg(Color::Black),
                ))
            } else if is_other_match {
                Line::from(Span::styled(
                    text.as_str(),
                    Style::new().bg(Color::DarkGray).fg(Color::Yellow),
                ))
            } else {
                Line::raw(text.as_str())
            }
        })
        .collect();

    frame.render_widget(Paragraph::new(visible), area);
}

fn draw_tab_bar(frame: &mut Frame, app: &App, area: Rect) {
    let spans: Vec<Span> = app
        .tabs
        .tabs
        .iter()
        .enumerate()
        .map(|(i, tab)| {
            let title = if tab.title.is_empty() {
                // Fall back to hostname
                tab.url.split('/').nth(2).unwrap_or("tab").to_owned()
            } else {
                tab.title.chars().take(20).collect()
            };
            let loading = if tab.loading { "⟳" } else { "" };
            let label = format!(" {}: {title}{loading} ", i + 1);
            let style = if i == app.tabs.active {
                Style::new().bg(Color::Blue).fg(Color::White).add_modifier(Modifier::BOLD)
            } else {
                Style::new().bg(Color::DarkGray).fg(Color::Gray)
            };
            Span::styled(label, style)
        })
        .collect();

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

#[cfg(test)]
mod tests {
    #[test]
    fn placeholder() {}
}
