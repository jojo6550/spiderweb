# Spiderweb UX/UI Redesign — Phase 3

**Date:** 2026-05-20  
**Scope:** Approach 1 — Interaction-first with visual polish  
**Status:** Approved for implementation

---

## Goals

Make Spiderweb stand out on UX and UI by delivering:
1. In-app URL navigation (currently impossible without restart)
2. Vimium-style link hints (`f` key) — most distinctive interaction feature
3. Catppuccin Mocha visual theme across all chrome surfaces
4. Content typography hierarchy (margins, heading levels, code spans)

---

## Color System — Catppuccin Mocha

| Role | Name | Hex | ratatui mapping |
|---|---|---|---|
| Content bg | Base | `#1e1e2e` | `Color::Rgb(30,30,46)` |
| Chrome bg | Crust | `#181825` | `Color::Rgb(24,24,37)` |
| Address bar bg | Surface0 | `#313244` | `Color::Rgb(49,50,68)` |
| Selected/edit bg | Surface1 | `#45475a` | `Color::Rgb(69,71,90)` |
| Body text | Text | `#cdd6f4` | `Color::Rgb(205,214,244)` |
| Muted text | Subtext0 | `#a6adc8` | `Color::Rgb(166,173,200)` |
| Disabled/dim | Overlay0 | `#6c7086` | `Color::Rgb(108,112,134)` |
| h1 / URL mode badge | Mauve | `#cba6f7` | `Color::Rgb(203,166,247)` |
| h2 / NORMAL badge | Blue | `#89b4fa` | `Color::Rgb(137,180,250)` |
| h3 / links | Sky | `#89dceb` | `Color::Rgb(137,220,235)` |
| HTTPS dot / hints / HINT badge | Green | `#a6e3a1` | `Color::Rgb(166,227,161)` |
| First hint / errors / SEARCH badge | Red | `#f38ba8` | `Color::Rgb(243,139,168)` |
| Bookmark star | Pink | `#f5c2e7` | `Color::Rgb(245,194,231)` |
| Code spans | Green (italic) | `#a6e3a1` | `Color::Rgb(166,227,161)` |

---

## InputMode — Extended Enum

File: `src/app.rs`

```rust
pub enum InputMode {
    Normal,
    Search(String),  // existing
    Url(String),     // new: URL edit buffer (pre-filled with current URL)
    Hint(String),    // new: typed hint letters so far (e.g. "A", "AB")
}
```

---

## Feature: URL Edit Mode

### Trigger
- `o` key in Normal mode → `InputMode::Url(current_tab_url.clone())`

### Address bar rendering (URL mode)
- Background: Surface1 (`#45475a`)
- Bottom border: 2px Blue (`#89b4fa`) — signals active input
- Prefix: `▸` in Blue
- Content: edit buffer + block cursor `█`
- Mode badge in status bar: `URL` in Mauve bg

### Key handling (`handle_url` in `keybinds.rs`)
| Key | Action |
|---|---|
| `Enter` | `app.navigate(buffer, tx)` → Normal |
| `Esc` | Discard buffer → Normal |
| `Char(c)` | Append to buffer |
| `Backspace` | Pop last char |
| `Ctrl+W` | Clear last word (split on `/` and space) |

### Content area during URL edit
- Render normally (do not dim — too complex, low value)

---

## Feature: Vimium-style Link Hints

### Trigger
- `f` key in Normal mode → `InputMode::Hint(String::new())`

### Hint code generation
- Codes assigned to all links visible in current viewport (scroll offset to scroll+height)
- Alphabet: `ASDFGHJKLQWERTYUIOPZXCVBNM` (home-row first for speed)
- Always 2-letter codes: first letter = group (A=first 26, B=next 26…), second = position within group
- Codes stored as `Vec<(usize, String)>` — `(link_index, code)` — computed fresh each time `f` is pressed

### Rendering (overlay in `draw_content`)
- After rendering content lines normally, iterate visible links
- For each link with a hint code, append badge to its rendered line:
  - First link (or partial match target): Red bg `#f38ba8`, dark text
  - All others: Green bg `#a6e3a1`, dark text
  - Badge format: ` AB ` (space-padded, 10px font feel via bold)
- Partial match (1 letter typed): dim non-matching hints (Overlay0 color), highlight matching group

### Key handling (`handle_hint` in `keybinds.rs`)
| Key | Action |
|---|---|
| `Char(c)` uppercase | Append to hint buffer, check for match |
| `Char(c)` lowercase | Uppercase it, same |
| `Backspace` | Pop last char |
| `Esc` | Clear hints → Normal |
| 2-char complete match (no Shift) | `app.navigate(href, tx)` → Normal |
| 2-char complete match (Shift held) | `app.open_new_tab(href, tx)` → Normal |

### Hint state in App
Add to `App`:
```rust
pub hint_codes: Vec<(usize, String)>, // (link_index, hint_code)
```
Populated when entering Hint mode, cleared on exit.

---

## Visual: Chrome Layout

### Tab bar
- Always visible (remove `show_tabs` condition)
- Active tab: Base bg, Text color, 2px Blue bottom border
- Inactive tab: Crust bg, Overlay0 color
- Loading tab: title suffixed with ` ⟳`
- Right edge: dim hint `t:new  x:close`

### Address bar (Normal mode)
- Background: Surface0 (`#313244`)
- HTTPS prefix dot: Green `●`, HTTP: Red `●`
- Bookmark star: Pink `★` if bookmarked, Overlay0 `☆` if not
- URL text: Text color

### Status bar
- Background: Crust (`#181825`)
- Left: mode badge (pill shape via bold bg):
  - NORMAL: Blue bg, Crust text
  - URL: Mauve bg, Crust text
  - SEARCH: Red bg, Crust text
  - HINT: Green bg, Crust text
- Center: context-aware key hints (change per mode)
- Right: scroll position `line/total` in Overlay0

---

## Visual: Content Typography

### Left margin
- All content rendered with 2-space left padding (add to `render_element` block prefix)

### Heading hierarchy
| Tag | Color | Weight | Prefix |
|---|---|---|---|
| `h1` | Mauve `#cba6f7` | Bold | `▌ ` glyph |
| `h2` | Blue `#89b4fa` | Bold | 2-space indent |
| `h3` | Sky `#89dceb` | Normal | 4-space indent |
| `h4–h6` | Subtext0 `#a6adc8` | Italic | 6-space indent |

### Inline elements
- `<code>`, `<kbd>`, `<tt>` → Green italic (`\x1b[3;32m` → use Rgb)
- `<strong>`, `<b>` → Bold (`\x1b[1m`, existing)
- `<em>`, `<i>` → Italic (`\x1b[3m`)

### Links
- Sky + underline (existing `\x1b[4;36m` → replace with Catppuccin Sky)

### Search matches
- Current match: Red bg (`#f38ba8`), Crust text
- Other matches: Surface1 bg (`#45475a`), Red text (`#f38ba8`)

---

## Files Changed

| File | Changes |
|---|---|
| `src/app.rs` | Add `Url(String)`, `Hint(String)` to `InputMode`; add `hint_codes` to `App`; add `enter_hint_mode` method |
| `src/tui/keybinds.rs` | Add `handle_url`, `handle_hint`; wire `o`→Url, `f`→Hint in `handle_normal` |
| `src/tui/ui.rs` | Catppuccin colors everywhere; tab bar always-on; mode badge; hint overlay in `draw_content`; URL edit address bar render |
| `src/renderer/text.rs` | Heading h1/h2/h3/h4 ANSI colors; 2-space left margin; `<code>` green italic; link Sky color |

**Not changed:** `network/`, `parser/html.rs`, `parser/css.rs`, `browser/`, `config/`

---

## Out of Scope (Phase 4)

- History autocomplete dropdown in URL bar
- Bookmark overlay panel
- Mouse support
- Theme config in `config.toml` (hardcode Catppuccin for now)
