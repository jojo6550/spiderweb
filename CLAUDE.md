# Spiderweb — Terminal Browser

A minimal, fast, terminal-native web browser written in Rust. Renders HTML, images (Sixel/Kitty protocol), and video in the terminal. Built for keyboard-driven, distraction-free browsing.

---

## Project Context

- **Language**: Rust (stable toolchain)
- **TUI framework**: `ratatui` + `crossterm`
- **Async runtime**: `tokio` (multi-threaded)
- **HTTP client**: `reqwest` with HTTP/2 and cookie support
- **HTML parser**: `scraper` (built on `html5ever`)
- **Image rendering**: `viuer` (Sixel / Kitty / iTerm2 auto-detection)
- **Video decoding**: `ffmpeg-next` bindings
- **CSS parser**: `cssparser`
- **Build**: `cargo` — no CMake, no external build scripts

---

## Build & Run Commands

```bash
# Build (debug)
cargo build

# Build (release — always use for perf testing)
cargo build --release

# Run
cargo run -- https://example.com

# Run tests
cargo test

# Lint
cargo clippy -- -D warnings

# Format
cargo fmt

# Check without building
cargo check
```

---

## Repository Structure

```
spiderweb/
├── CLAUDE.md
├── Cargo.toml
├── Cargo.lock
├── README.md
├── src/
│   ├── main.rs            # Entry point, CLI args, tokio runtime init
│   ├── app.rs             # Top-level App state, event loop
│   ├── browser/
│   │   ├── mod.rs
│   │   ├── tabs.rs        # Tab management, history per tab
│   │   ├── history.rs     # Navigation history (back/forward stack)
│   │   └── bookmarks.rs   # Bookmark persistence (JSON)
│   ├── network/
│   │   ├── mod.rs
│   │   ├── client.rs      # reqwest client wrapper, cookie jar
│   │   ├── dns.rs         # DNS resolver config
│   │   └── response.rs    # HTTP response types
│   ├── parser/
│   │   ├── mod.rs
│   │   ├── html.rs        # HTML parse → DOM tree
│   │   ├── css.rs         # CSS parse → style rules
│   │   └── layout.rs      # Layout engine (block/inline flow)
│   ├── renderer/
│   │   ├── mod.rs
│   │   ├── text.rs        # Text rendering, ANSI color mapping
│   │   ├── image.rs       # Image decode + Sixel/Kitty output
│   │   └── video.rs       # FFmpeg frame pipeline
│   ├── tui/
│   │   ├── mod.rs
│   │   ├── ui.rs          # ratatui layout composition
│   │   ├── widgets.rs     # Custom widgets (address bar, tab bar, status)
│   │   └── keybinds.rs    # Key event routing
│   └── config/
│       ├── mod.rs
│       └── settings.rs    # User config (~/.config/spiderweb/config.toml)
└── tests/
    ├── network_tests.rs
    ├── parser_tests.rs
    └── renderer_tests.rs
```

---

## Coding Standards

- **No `unwrap()` or `expect()` in library code** — use `?` and propagate errors via `anyhow::Result` or typed error enums
- **No blocking calls on the tokio runtime** — use `spawn_blocking` for CPU-heavy work (image decoding, layout)
- All public types and functions must have doc comments (`///`)
- Use `tracing` for logging — never `println!` in library code
- Every new module needs at least one unit test
- Keep `main.rs` thin — just CLI parsing and runtime boot
- Prefer `Arc<RwLock<T>>` over `Mutex<T>` for shared state accessed across async tasks

---

## Architecture Rules

- **Network, parsing, and rendering are separate async tasks** — communicate via `tokio::sync::mpsc` channels, never share raw state across threads
- The TUI render loop runs on the main thread at ~30fps — it must never block
- Image frames go through: `decode (rayon thread) → resize → encode Sixel/Kitty → send to render channel`
- HTTP responses are streamed — do not buffer entire response bodies into memory before parsing begins
- CSS is applied after layout, not before — the layout engine works on unstyled DOM first

---

## MVP Scope (Phase 1)

The MVP is a working terminal browser that can:

1. Accept a URL from the CLI (`spiderweb https://example.com`)
2. Fetch the page over HTTP/S with proper TLS
3. Parse HTML and render readable text to the terminal
4. Display inline images using Sixel or Kitty protocol (auto-detected)
5. Show a TUI with: address bar, scrollable content area, status bar
6. Basic keyboard navigation: scroll (j/k or arrow keys), follow links (Enter), back (Backspace), quit (q)
7. Respect `Content-Type` — handle `text/html` and `text/plain` at minimum

**MVP explicitly excludes**: JavaScript execution, video, CSS layout engine (use basic block flow only), tabs, bookmarks.

---

## Roadmap

### Phase 1 — MVP (current)
- [ ] Project scaffold, Cargo.toml, module structure
- [ ] `network/client.rs` — async HTTP client with TLS, redirects, cookies
- [ ] `parser/html.rs` — DOM tree from raw HTML bytes
- [ ] `renderer/text.rs` — DOM → terminal text with basic ANSI styling
- [ ] `tui/ui.rs` — address bar + scrollable content pane + status bar
- [ ] `tui/keybinds.rs` — scroll, follow link, back, quit
- [ ] `renderer/image.rs` — detect Sixel/Kitty support, decode + output inline images
- [ ] CLI: `spiderweb <url>` entry point
- [ ] Basic error screen (404, connection refused, timeout)

### Phase 2 — Real Browser Feel
- [ ] Tab support (open link in new tab, switch tabs with number keys)
- [ ] Navigation history (back/forward stacks per tab)
- [ ] Bookmarks (save with `b`, list with `B`, persist to `~/.config/spiderweb/bookmarks.json`)
- [ ] `parser/css.rs` — color, font-weight, display:none, basic box model
- [ ] `parser/layout.rs` — block and inline flow layout
- [ ] Form rendering (text inputs, buttons — GET forms only)
- [ ] Link following from rendered page (highlight links, Enter to navigate)
- [ ] Search on page (`/` to open, `n`/`N` to cycle)
- [ ] Config file (`~/.config/spiderweb/config.toml`) — home page, keybind overrides, color theme

### Phase 3 — Media & Performance
- [ ] `renderer/video.rs` — FFmpeg frame pipeline, Sixel/Kitty output at target fps
- [ ] Streaming HTML render — begin painting before full page load
- [ ] Connection pooling and DNS caching
- [ ] Parallel asset fetching (images load concurrently with text render)
- [ ] SIMD Sixel encoder (replace `viuer` with custom implementation for performance)
- [ ] GIF animation support
- [ ] HTTP cache (ETag, Cache-Control, disk-backed)

### Phase 4 — Advanced
- [ ] JavaScript via embedded QuickJS (no DOM manipulation — eval only)
- [ ] HTTPS certificate pinning and security indicators
- [ ] Proxy support (SOCKS5, HTTP CONNECT)
- [ ] Download manager (save page, save image)
- [ ] `--dump` mode — fetch and print plain text to stdout (scriptable)
- [ ] Mouse support (click links, scroll wheel)
- [ ] Plugin/extension API via Lua or WASM

---

## Key Files to Know

| File | Purpose |
|---|---|
| `src/app.rs` | Central App state — owns tabs, config, channel handles |
| `src/network/client.rs` | All outbound HTTP — modify here for proxy, auth, headers |
| `src/renderer/image.rs` | Sixel/Kitty detection and output — performance-critical |
| `src/tui/keybinds.rs` | All keyboard shortcuts defined here |
| `src/config/settings.rs` | User-facing configuration schema |

---

## Terminal Protocol Notes

- Detect Sixel support: check `$TERM` and send `\x1b[c` (DA1) — look for `4` in response params
- Detect Kitty support: check `$TERM` == `xterm-kitty` or `$KITTY_WINDOW_ID`
- Fallback order: **Kitty → Sixel → iTerm2 → Unicode block characters**
- `viuer` handles detection automatically in Phase 1 — replace with custom encoder in Phase 3
- Never write image data to stderr — always stdout, and flush immediately after each frame

---

## Testing Strategy

- Unit test every parser function with real HTML/CSS snippets
- Network tests use `mockito` to mock HTTP responses — never hit real URLs in tests
- Renderer tests compare Sixel output byte-for-byte against golden files
- Run `cargo clippy -- -D warnings` before every commit — zero warnings policy
- Integration test: `cargo run -- https://example.com` must not panic on any well-formed HTML

---

## When Claude Code Is Implementing

- Always implement the full error path — no `todo!()` or `unimplemented!()` left in committed code
- After implementing a module, run `cargo check` and `cargo clippy` and fix all warnings before moving on
- When adding a dependency to `Cargo.toml`, pin the minor version (e.g. `"0.28"` not `"*"`)
- Prefer small, focused commits — one module or feature per session
- If a function exceeds ~80 lines, split it — Rust functions should be short and composable
