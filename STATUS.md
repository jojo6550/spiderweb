# Spiderweb — Progress & Status

Current state of the terminal browser. Update this file at the end of each working session so the next Claude can pick up cold.

**Last updated:** 2026-05-20
**Branch:** main
**Build:** clean — 68 tests pass, zero clippy warnings (`cargo clippy --all-targets -- -D warnings`)
**Verified:** runs against `https://example.com` end-to-end (heading + body + word-wrap + link + tab bar + search mode confirmed via captured VT output on Windows)

---

## Phase 1 — MVP — DONE

Working terminal browser:
- CLI `spiderweb <url>`, tokio multi-thread runtime, clap arg parsing
- async HTTP via reqwest (HTTP/2, rustls TLS, cookie jar, gzip/brotli, 30s timeout)
- HTML parsing via `scraper`/html5ever, title/links/description extraction
- DOM → ANSI text renderer (h1-h6 styled, `<a>` cyan underline, `<li>` bulleted, `<hr>` ruled)
- ratatui TUI: address bar + scrollable content + status bar at ~30 fps via `tokio::select!` + `EventStream`
- keybinds: `j/k/d/u/g/G` scroll, `Tab/Shift+Tab` link cycle, `Enter` follow, `q/Ctrl+C` quit
- Image decode (`image` crate, PNG/JPEG/GIF/WebP) + Sixel/Kitty/iTerm2/Block protocol detection via `viuer`
- Error screen for failed fetches, mockito-based network tests
- Panic hook restores terminal on crash

## Phase 2 — Real Browser Feel — DONE (except form submission)

All of these landed across commits `a1a3578` … `c46664f`:

| Feature | Where | Notes |
|---|---|---|
| Tab management | `browser/tabs.rs`, `tui/keybinds.rs` | `t` new, `x` close, `1-9` switch, tab bar renders when >1 tab |
| Back/forward history | `browser/history.rs` | Per-tab stacks. `Backspace` back, `Alt+→`/`Ctrl+→` forward |
| Bookmarks | `browser/bookmarks.rs` | Persist `~/.config/spiderweb/bookmarks.json`, `b` toggle, `B` list in status |
| Settings | `config/settings.rs` | `~/.config/spiderweb/config.toml`, home_page/theme/timeout |
| Page search | `tabs.rs::search` + `keybinds.rs` + `ui.rs` | `/` open, type live, `n`/`N` cycle, `Esc` cancel. Yellow highlight current, dim others |
| Relative URLs | `app.rs::resolve_url` | `url` crate joins against current tab's URL. Skips `mailto:`/`javascript:` |
| Link line highlighting | `renderer/text.rs` markers | `\u{F000}L<n>` / `\u{F000}I<n>` private-use markers tracked through normalize then stripped |
| CSS hiding | `parser/css.rs` | Inline `<style>` parser for simple selectors (`.foo`, `#bar`, `tag`) with `display:none` / `visibility:hidden`. Also catches `hidden` attr, `aria-hidden=true`, inline `style="display:none"`, builtin a11y classes (`sr-only`, `visually-hidden`, etc) |
| Inline images | `renderer/image.rs::to_ansi_lines` + `app.rs::inline_images` | Truecolor half-block `▀` cells (2 vertical pixels per cell). Concurrent fetch via `futures::join_all`, shared `SpiderClient`, 4 s per-image timeout, max 12 images, 80×18 cells each. Splices into rendered output at marker positions; shifts link line numbers |
| Word-wrap layout | `parser/layout.rs::wrap_lines` | ANSI-aware visible-width counting. Wraps text at word boundaries to `DEFAULT_WIDTH = 100`. Lines containing ANSI escapes (images) pass through. Re-maps link/image line indices |
| Form widgets (display only) | `renderer/text.rs::render_input/button/textarea/select` | `<input>` shows placeholder/value/name; `type=submit/button` rendered as inverse-video pill; `type=checkbox`/`radio` as `[x]`/`(•)`; `type=hidden` skipped (no CSRF leak); `<select>` picks `[selected]` option |
| Renderer cleanup | `renderer/text.rs::render_link` | `[href]` no longer dumped inline next to every link — recorded only in `RenderedLink` for navigation |

### What's NOT done in Phase 2

- **Form submission** (`<form action="..." method="GET">`) — visual rendering works, no input editing mode, no URL building from inputs. Deferred to Phase 3 alongside streaming render.

---

## Known Issues / Limitations

1. **Heavy pages (Wikipedia, news sites) stall on load.** Big HTML (500 KB+) → slow `scraper` parse + `extract_hidden` scan + render with thousands of elements. Confirmed on `https://en.wikipedia.org/wiki/Rust_(programming_language)` — 8s wait, no render. Fix path: Phase 3 streaming HTML render (paint as bytes arrive).
2. **`SpiderClient::new()` called per page-load.** Should be created once at startup and threaded through `App`. Currently `fetch_inner` builds a fresh client each navigation (rustls config, connection pool churn). Image-fetch loop now shares the page-load client, but the top-level page fetch doesn't reuse across navigations.
3. **No JavaScript.** YouTube, Twitter, Reddit, Gmail, modern SPAs render as empty shells. Phase 4 work.
4. **Link-line accuracy.** Markers placed before link text; if text spans wrapped lines, only first wrapped line is highlighted. Good enough for now.
5. **CSS extraction is selector-strict.** Compound (`.a.b`), descendant (`.a .b`), attribute (`[hidden]`), pseudo (`:hover`), and `@media` rules are intentionally skipped. Many real pages still leak hidden content because their `display:none` is inside `@media` or compound selectors.
6. **No address-bar editing.** Can't type a URL into the bar mid-session — only navigate via CLI arg or link follow. Phase 3 should add it.
7. **Image splice complexity.** Bottom-up insertion to keep positions stable. If `inline_images` is interrupted mid-splice (shouldn't happen — synchronous after `join_all`), state could desync.
8. **Windows file-lock during cargo test.** Occasionally `cargo test` fails with `os error 5: Access is denied` removing `spiderweb.exe` because a prior instance still holds the file. Workaround: `cargo test --lib`.

---

## Phase 3 — Media & Performance — NOT STARTED

Priority order, biggest user-visible win first:

1. **Form submission** (close out Phase 2 leftover) — input editing mode (`InputMode::FormField`), per-tab form state keyed by `name`, build GET query string on submit-button Enter, navigate to `action` URL + `?k=v`. ~2-4 hours.
2. **Streaming HTML render** — begin painting before full response body arrives. `reqwest` already streams; need to chunk-parse and incrementally feed renderer. Fixes Wikipedia stall. Bigger lift.
3. **Connection pooling + DNS cache** — `SpiderClient` lives in `App` instead of being built per fetch. `dns` module currently empty — add resolver config + caching.
4. **`renderer/video.rs`** — FFmpeg frame pipeline, Sixel/Kitty output at target fps. `ffmpeg-next` crate. CLAUDE.md spec.
5. **GIF animation** — `image` crate decodes GIF frames; loop with timing.
6. **SIMD Sixel encoder** — replace `viuer` with custom impl. Currently only used for direct image URLs (not inline rendering, which uses half-block).
7. **HTTP cache** — ETag, Cache-Control, disk-backed under `~/.config/spiderweb/cache/`.
8. **Parallel asset fetching done partially** — image fetches already use `join_all` with shared client. CSS/JS/font assets would extend the same pattern.

---

## Phase 4 — Advanced — NOT STARTED

- **JavaScript via QuickJS** — embed `rquickjs` or `boa`, no DOM mutation, eval-only. This is what unblocks YouTube/Twitter/Reddit.
- HTTPS certificate pinning + security indicator in address bar
- SOCKS5 / HTTP CONNECT proxy
- Download manager (save page, save image)
- `--dump` mode — fetch + print plain text to stdout, scriptable
- Mouse support (click links, scroll wheel) — crossterm has `EnableMouseCapture`
- Plugin API via Lua or WASM

---

## Key Files Index

```
src/
├── main.rs              # CLI parse + tokio boot, ~22 lines
├── app.rs               # App state, BgMsg channel, event loop, fetch_inner, inline_images, resolve_url
├── lib.rs               # Module roots
├── browser/
│   ├── tabs.rs          # Tab + TabManager + per-tab search state
│   ├── history.rs       # Back/forward stacks per tab
│   └── bookmarks.rs     # JSON persistence at ~/.config/spiderweb/bookmarks.json
├── network/
│   ├── client.rs        # SpiderClient (reqwest wrapper, now Clone)
│   ├── dns.rs           # empty placeholder — Phase 3
│   └── response.rs      # HttpResponse + is_html/is_text helpers
├── parser/
│   ├── html.rs          # ParsedPage::from_bytes/parse_html, title/links/description
│   ├── css.rs           # extract_hidden — display:none rule extraction + built-in a11y classes
│   └── layout.rs        # wrap_lines + visible_width — ANSI-aware word-wrap
├── renderer/
│   ├── text.rs          # DOM → ANSI string, RenderedPage{lines,links,images}, marker-based line tracking, form widgets
│   ├── image.rs         # decode + to_ansi_lines (half-block) + viuer Sixel/Kitty path
│   └── video.rs         # empty placeholder — Phase 3
├── tui/
│   ├── ui.rs            # ratatui layout: tab bar / address / content / search / status
│   ├── keybinds.rs      # Normal mode + Search input mode
│   └── widgets.rs       # empty placeholder
└── config/
    └── settings.rs      # ~/.config/spiderweb/config.toml: home_page, theme, timeout_secs
```

---

## Dev Workflow Reminders

- **Lint everything:** `cargo clippy --all-targets -- -D warnings` (zero-warnings policy from CLAUDE.md)
- **Test:** `cargo test` (or `cargo test --lib` if a stray `spiderweb.exe` is locked on Windows)
- **Release build:** `cargo build --release` — `lto = "thin"`, `codegen-units = 1`, `strip = "symbols"`
- **Run:** `cargo run --release -- https://example.com`
- **Windows TUI verification trick:** no tmux. Use PowerShell `Start-Process -RedirectStandardOutput` to capture raw VT escapes, sleep, kill, then grep the captured file for expected strings. See chat history for exact recipe.
- **Coding standards (from CLAUDE.md):** no `unwrap()` / `expect()` in lib code, no blocking calls on tokio runtime, every module has at least one unit test, functions ≤80 lines, deps pinned to minor version.

---

## Commit History — Phase 2

```
c46664f perf(images): share HTTP client + 4s per-image timeout, cap at 12
bc76a9c feat(layout,forms): word-wrap + form element rendering — Phase 2 complete
0521e09 feat(images): inline image rendering via truecolor half-block chars
2a7018b feat(css): display:none / visibility:hidden filtering
88ea1ee fix(renderer): hide inline hrefs from rendered output
a1a3578 feat(phase2): tabs, history, bookmarks, search, relative URL resolution
```
