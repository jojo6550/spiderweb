# spiderweb

Minimal, fast, terminal-native web browser written in Rust. Renders HTML, images (Sixel/Kitty protocol), and video in the terminal. Keyboard-driven, distraction-free browsing.

## Build & Run

```bash
# Build (debug)
cargo build

# Build (release)
cargo build --release

# Run
cargo run -- https://example.com

# Tests
cargo test

# Lint
cargo clippy -- -D warnings

# Format
cargo fmt
```

## Status

Phase 1 scaffold — module skeleton and dependency set in place. Networking, parsing, and rendering are stubs pending implementation.

## License

MIT OR Apache-2.0
