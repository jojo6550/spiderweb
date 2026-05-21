//! Layout: block/inline flow plus word-wrap for terminal width.
//!
//! Phase 2 scope: takes rendered lines + link/image positions and wraps long
//! lines at word boundaries while preserving ANSI escape sequences and
//! adjusting link/image line indices to the new wrapped layout.

use crate::renderer::text::{RenderedImage, RenderedLink};

/// Default terminal width when none is supplied.
pub const DEFAULT_WIDTH: usize = 100;

/// Visible (printable) width of `s` — skips ANSI escape sequences.
pub fn visible_width(s: &str) -> usize {
    let mut count = 0usize;
    let mut esc = false;
    for c in s.chars() {
        match c {
            '\x1b' => esc = true,
            'm' if esc => esc = false,
            _ if esc => {}
            _ => count += 1,
        }
    }
    count
}

/// Wrap `lines` at `max_width` and update link/image line indices to match.
///
/// Lines that already fit pass through unchanged. Lines wider than `max_width`
/// are broken at the nearest preceding space; lines with no space are hard-broken
/// at the width boundary. Lines that contain ANSI escape sequences (typically
/// image lines) are left intact — they're already sized by the image renderer.
pub fn wrap_lines(
    lines: Vec<String>,
    links: &mut [RenderedLink],
    images: &mut [RenderedImage],
    max_width: usize,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut index_map: Vec<usize> = Vec::with_capacity(lines.len());

    for line in lines {
        index_map.push(out.len());
        if line.contains('\x1b') || visible_width(&line) <= max_width {
            out.push(line);
        } else {
            wrap_one(&line, max_width, &mut out);
        }
    }

    for link in links.iter_mut() {
        if let Some(&new_idx) = index_map.get(link.line) {
            link.line = new_idx;
        }
    }
    for img in images.iter_mut() {
        if let Some(&new_idx) = index_map.get(img.line) {
            img.line = new_idx;
        }
    }

    out
}

/// Split `line` into pieces of at most `max_width` visible chars, breaking on
/// spaces where possible. ANSI-free input assumed.
fn wrap_one(line: &str, max_width: usize, out: &mut Vec<String>) {
    let chars: Vec<char> = line.chars().collect();
    let mut start = 0;
    while start < chars.len() {
        let remaining = chars.len() - start;
        if remaining <= max_width {
            out.push(chars[start..].iter().collect());
            return;
        }
        // Find last space within the window.
        let window_end = start + max_width;
        let break_at = (start..window_end)
            .rev()
            .find(|&i| chars[i] == ' ')
            .unwrap_or(window_end); // hard-break if no space
        // Skip the breaking space itself in the next chunk.
        let chunk_end = break_at;
        let next_start = if chars.get(break_at) == Some(&' ') {
            break_at + 1
        } else {
            break_at
        };
        let chunk: String = chars[start..chunk_end].iter().collect();
        if !chunk.is_empty() {
            out.push(chunk);
        }
        start = next_start;
        if start == chunk_end && chars.get(start) != Some(&' ') {
            // Hard-break with no progress — force advance.
            start = chunk_end + 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_width_skips_ansi() {
        let s = "\x1b[1;34mHello\x1b[0m world";
        assert_eq!(visible_width(s), "Hello world".chars().count());
    }

    #[test]
    fn short_line_unchanged() {
        let lines = vec!["short".to_owned()];
        let mut links = vec![];
        let mut images = vec![];
        let out = wrap_lines(lines, &mut links, &mut images, 80);
        assert_eq!(out, vec!["short"]);
    }

    #[test]
    fn long_line_wraps_at_space() {
        let lines = vec!["one two three four".to_owned()];
        let mut links = vec![];
        let mut images = vec![];
        let out = wrap_lines(lines, &mut links, &mut images, 8);
        // "one two " (7 chars + space=8) → "one two", then "three", then "four"
        assert!(out.len() >= 2);
        for line in &out {
            assert!(visible_width(line) <= 8, "line too long: {line:?}");
        }
    }

    #[test]
    fn ansi_lines_passthrough() {
        let s = "\x1b[31m".to_owned() + &"x".repeat(120) + "\x1b[0m";
        let lines = vec![s.clone()];
        let mut links = vec![];
        let mut images = vec![];
        let out = wrap_lines(lines, &mut links, &mut images, 40);
        assert_eq!(out.len(), 1, "ANSI lines must not be wrapped");
        assert_eq!(out[0], s);
    }

    #[test]
    fn link_line_remapped_after_wrap() {
        let lines = vec![
            "short header".to_owned(),
            "this is a much longer line that will be wrapped into multiple shorter pieces"
                .to_owned(),
            "footer link line".to_owned(),
        ];
        let mut links = vec![RenderedLink {
            href: "https://x.com".into(),
            line: 2, // originally pointing at "footer link line"
        }];
        let mut images = vec![];
        let out = wrap_lines(lines, &mut links, &mut images, 20);
        // Line 1 wrapped into multiple — link should now be at first wrapped index of line 2.
        let footer_idx = out
            .iter()
            .position(|l| l.contains("footer"))
            .expect("footer should still be present");
        assert_eq!(links[0].line, footer_idx);
    }

    #[test]
    fn no_space_hard_break() {
        let lines = vec!["a".repeat(20)];
        let mut links = vec![];
        let mut images = vec![];
        let out = wrap_lines(lines, &mut links, &mut images, 5);
        assert!(out.len() >= 4);
        for line in &out {
            assert!(visible_width(line) <= 5);
        }
    }
}
