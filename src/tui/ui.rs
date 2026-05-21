//! ratatui layout composition: tab bar, address bar, content pane, status bar.

use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
};

use crate::app::{App, InputMode};
use crate::renderer::text::{CodeSpan, LineKind};

// ── Catppuccin Mocha palette ──────────────────────────────────────────────────
const C_BASE: Color = Color::Rgb(30, 30, 46);
const C_CRUST: Color = Color::Rgb(24, 24, 37);
const C_SURFACE0: Color = Color::Rgb(49, 50, 68);
const C_SURFACE1: Color = Color::Rgb(69, 71, 90);
const C_TEXT: Color = Color::Rgb(205, 214, 244);
const C_SUBTEXT: Color = Color::Rgb(166, 173, 200);
const C_OVERLAY: Color = Color::Rgb(108, 112, 134);
const C_MAUVE: Color = Color::Rgb(203, 166, 247);
const C_BLUE: Color = Color::Rgb(137, 180, 250);
const C_SKY: Color = Color::Rgb(137, 220, 235);
const C_GREEN: Color = Color::Rgb(166, 227, 161);
const C_RED: Color = Color::Rgb(243, 139, 168);
const C_PINK: Color = Color::Rgb(245, 194, 231);

/// Render the full TUI frame.
pub fn draw(frame: &mut Frame, app: &App) {
    let tab = app.tabs.current();
    let in_search = matches!(app.input_mode, InputMode::Search(_));
    let in_field_edit = matches!(app.input_mode, InputMode::FieldEdit { .. });

    // Tab bar (1) + address bar (1) + content (fill) + bottom-input? + status (1)
    let mut constraints = vec![
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Fill(1),
    ];
    if in_search || in_field_edit {
        constraints.push(Constraint::Length(1));
    }
    constraints.push(Constraint::Length(1));

    let areas = Layout::vertical(constraints).split(frame.area());

    draw_tab_bar(frame, app, areas[0]);
    draw_address_bar(frame, app, areas[1]);

    let content_area = areas[2];
    draw_content(frame, app, content_area);

    let mut status_idx = 3;
    if in_search {
        let query = if let InputMode::Search(ref q) = app.input_mode { q.as_str() } else { "" };
        let match_info = if !query.is_empty() && tab.search_matches.is_empty() {
            " (no matches)".to_owned()
        } else if !tab.search_matches.is_empty() {
            format!(" ({}/{})", tab.search_idx + 1, tab.search_matches.len())
        } else {
            String::new()
        };
        frame.render_widget(
            Paragraph::new(format!("/{query}{match_info}"))
                .style(Style::new().bg(C_SURFACE0).fg(C_TEXT)),
            areas[3],
        );
        status_idx = 4;
    } else if in_field_edit {
        if let InputMode::FieldEdit { field_idx, buffer } = &app.input_mode {
            let name = tab
                .fields
                .get(*field_idx)
                .map(|f| f.name.as_str())
                .unwrap_or("input");
            let label = if name.is_empty() { "input" } else { name };
            let line = Line::from(vec![
                Span::styled(
                    format!(" ✎ {label}: "),
                    Style::new().bg(C_SURFACE1).fg(C_MAUVE).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!("{buffer}█"),
                    Style::new().bg(C_SURFACE1).fg(C_TEXT),
                ),
            ]);
            frame.render_widget(
                Paragraph::new(line).style(Style::new().bg(C_SURFACE1)),
                areas[3],
            );
        }
        status_idx = 4;
    }

    draw_status_bar(frame, app, areas[status_idx]);
}

fn draw_tab_bar(frame: &mut Frame, app: &App, area: Rect) {
    let spans: Vec<Span> = app
        .tabs
        .tabs
        .iter()
        .enumerate()
        .map(|(i, tab)| {
            let title = if tab.title.is_empty() {
                tab.url.split('/').nth(2).unwrap_or("tab").to_owned()
            } else {
                tab.title.chars().take(20).collect()
            };
            let loading = if tab.loading { " ⟳" } else { "" };
            let label = format!(" {}: {title}{loading} ", i + 1);
            let style = if i == app.tabs.active {
                Style::new()
                    .bg(C_BASE)
                    .fg(C_TEXT)
                    .add_modifier(Modifier::BOLD)
                    .add_modifier(Modifier::UNDERLINED)
            } else {
                Style::new().bg(C_CRUST).fg(C_OVERLAY)
            };
            Span::styled(label, style)
        })
        .collect();

    let mut all_spans = spans;
    all_spans.push(Span::styled(
        " ".repeat(frame.area().width as usize),
        Style::new().bg(C_CRUST),
    ));
    frame.render_widget(
        Paragraph::new(Line::from(all_spans)).style(Style::new().bg(C_CRUST)),
        area,
    );
}

fn draw_address_bar(frame: &mut Frame, app: &App, area: Rect) {
    let tab = app.tabs.current();

    if let InputMode::Url(ref buf) = app.input_mode {
        let line = Line::from(vec![
            Span::styled(
                " ▸ ",
                Style::new().bg(C_SURFACE1).fg(C_BLUE).add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!("{buf}█"), Style::new().bg(C_SURFACE1).fg(C_TEXT)),
        ]);
        frame.render_widget(
            Paragraph::new(line).style(Style::new().bg(C_SURFACE1)),
            area,
        );
        return;
    }

    let dot_color = if tab.url.starts_with("https://") { C_GREEN } else { C_RED };
    let (bm_char, bm_color) = if app.bookmarks.contains(&tab.url) {
        ("★", C_PINK)
    } else {
        ("☆", C_OVERLAY)
    };
    let loading_suffix = if tab.loading { " ⟳" } else { "" };

    let line = Line::from(vec![
        Span::styled(" ● ", Style::new().bg(C_SURFACE0).fg(dot_color)),
        Span::styled(
            format!("{}{loading_suffix}", tab.url),
            Style::new().bg(C_SURFACE0).fg(C_TEXT),
        ),
        Span::styled(format!(" {bm_char} "), Style::new().bg(C_SURFACE0).fg(bm_color)),
    ]);
    frame.render_widget(
        Paragraph::new(line).style(Style::new().bg(C_SURFACE0)),
        area,
    );
}

fn draw_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let tab = app.tabs.current();
    let (mode_label, mode_color) = match &app.input_mode {
        InputMode::Normal => ("NORMAL", C_BLUE),
        InputMode::Search(_) => ("SEARCH", C_RED),
        InputMode::Url(_) => ("URL", C_MAUVE),
        InputMode::Hint(_) => ("HINT", C_GREEN),
        InputMode::FieldEdit { .. } => ("INPUT", C_PINK),
    };
    let hints = match &app.input_mode {
        InputMode::Normal => " o:open  f:hints  i:input  /:search  b:bmark  j/k:scroll  t:tab",
        InputMode::Search(_) => " Esc:cancel  Enter:done  n/N:next/prev",
        InputMode::Url(_) => " Enter:go  Esc:cancel  Ctrl+W:clear-word  Backspace:del",
        InputMode::Hint(_) => " type letters to follow  ·  Shift+letters:new tab  ·  Esc:cancel",
        InputMode::FieldEdit { .. } => " Enter:submit  Tab:next-field  Esc:cancel",
    };
    let scroll_info = format!(" {}/{} ", tab.scroll + 1, tab.lines.len().max(1));

    let status_msg = if !app.status.is_empty() {
        format!(" {} ", app.status)
    } else {
        hints.to_owned()
    };

    let line = Line::from(vec![
        Span::styled(
            format!(" {mode_label} "),
            Style::new().bg(mode_color).fg(C_CRUST).add_modifier(Modifier::BOLD),
        ),
        Span::styled(status_msg, Style::new().bg(C_CRUST).fg(C_OVERLAY)),
        Span::styled(scroll_info, Style::new().bg(C_CRUST).fg(C_SUBTEXT)),
    ]);
    frame.render_widget(
        Paragraph::new(line).style(Style::new().bg(C_CRUST)),
        area,
    );
}

fn draw_content(frame: &mut Frame, app: &App, area: Rect) {
    let tab = app.tabs.current();

    if tab.lines.is_empty() && !tab.loading {
        let msg = if app.status.is_empty() { "No content" } else { app.status.as_str() };
        frame.render_widget(
            Paragraph::new(msg).style(Style::new().fg(C_RED).bg(C_BASE)),
            area,
        );
        return;
    }

    let viewport_h = area.height as usize;
    let in_hint_mode = matches!(app.input_mode, InputMode::Hint(_));
    let typed_hint = if let InputMode::Hint(ref s) = app.input_mode { s.as_str() } else { "" };

    let visible: Vec<Line> = tab
        .lines
        .iter()
        .enumerate()
        .skip(tab.scroll)
        .take(viewport_h)
        .map(|(i, text)| {
            let is_selected_link = !in_hint_mode
                && tab
                    .selected_link
                    .and_then(|sel| tab.links.get(sel))
                    .map(|rl| rl.line == i)
                    .unwrap_or(false);
            let is_current_match = !tab.search_matches.is_empty()
                && tab.search_matches[tab.search_idx] == i;
            let is_other_match = !is_current_match && tab.search_matches.contains(&i);

            let hint_badge: Option<(String, bool)> = if in_hint_mode {
                app.hint_codes
                    .iter()
                    .find(|(link_idx, _)| {
                        tab.links.get(*link_idx).map(|l| l.line == i).unwrap_or(false)
                    })
                    .map(|(_, code)| {
                        let matches = code.starts_with(typed_hint);
                        (code.clone(), matches)
                    })
            } else {
                None
            };

            let kind = tab.line_kinds.get(i).copied().unwrap_or_default();
            let base_style = match kind {
                LineKind::H1 => Style::new().fg(C_MAUVE).add_modifier(Modifier::BOLD),
                LineKind::H2 => Style::new().fg(C_BLUE).add_modifier(Modifier::BOLD),
                LineKind::H3 => Style::new().fg(C_SKY),
                LineKind::H4Plus => Style::new().fg(C_SUBTEXT).add_modifier(Modifier::ITALIC),
                LineKind::Normal => Style::new().fg(C_TEXT),
            };

            let mut spans: Vec<Span> = vec![Span::raw("  ")]; // 2-char left margin

            if is_selected_link {
                spans.push(Span::styled(
                    text.as_str(),
                    Style::new()
                        .bg(C_SURFACE1)
                        .fg(C_SKY)
                        .add_modifier(Modifier::UNDERLINED),
                ));
            } else if is_current_match {
                spans.push(Span::styled(text.as_str(), Style::new().bg(C_RED).fg(C_CRUST)));
            } else if is_other_match {
                spans.push(Span::styled(text.as_str(), Style::new().bg(C_SURFACE1).fg(C_RED)));
            } else {
                let line_code_spans: Vec<&CodeSpan> =
                    tab.code_spans.iter().filter(|cs| cs.line == i).collect();
                if line_code_spans.is_empty() {
                    spans.push(Span::styled(text.as_str(), base_style));
                } else {
                    spans.extend(build_code_spans(text, &line_code_spans, base_style));
                }
            }

            if let Some((code, matches)) = hint_badge {
                let badge_style = if typed_hint.is_empty() {
                    Style::new().bg(C_GREEN).fg(C_CRUST).add_modifier(Modifier::BOLD)
                } else if matches {
                    Style::new().bg(C_RED).fg(C_CRUST).add_modifier(Modifier::BOLD)
                } else {
                    Style::new().bg(C_SURFACE1).fg(C_OVERLAY)
                };
                spans.push(Span::styled(format!(" {code} "), badge_style));
            }

            Line::from(spans)
        })
        .collect();

    frame.render_widget(
        Paragraph::new(visible).style(Style::new().bg(C_BASE)),
        area,
    );
}

/// Build ratatui spans for a line that contains inline code spans (green italic).
fn build_code_spans<'a>(text: &'a str, spans: &[&CodeSpan], base: Style) -> Vec<Span<'a>> {
    let code_style = Style::new().fg(C_GREEN).add_modifier(Modifier::ITALIC);
    let chars: Vec<char> = text.chars().collect();
    let mut result = Vec::new();
    let mut pos = 0usize;
    let mut sorted: Vec<&&CodeSpan> = spans.iter().collect();
    sorted.sort_by_key(|cs| cs.start);
    for cs in sorted {
        let start = cs.start.min(chars.len());
        let end = cs.end.min(chars.len());
        if start > pos {
            result.push(Span::styled(chars[pos..start].iter().collect::<String>(), base));
        }
        if end > start {
            result.push(Span::styled(
                chars[start..end].iter().collect::<String>(),
                code_style,
            ));
        }
        pos = end;
    }
    if pos < chars.len() {
        result.push(Span::styled(chars[pos..].iter().collect::<String>(), base));
    }
    result
}

#[cfg(test)]
mod tests {
    #[test]
    fn placeholder() {}
}
