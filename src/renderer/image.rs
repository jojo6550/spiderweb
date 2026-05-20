//! Image decode and Sixel/Kitty output via viuer.

use anyhow::{Context, Result};
use image::DynamicImage;

/// Terminal graphics protocol supported by the current terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// Kitty graphics protocol — best quality, full color.
    Kitty,
    /// DEC Sixel — widely supported.
    Sixel,
    /// iTerm2 inline image protocol.
    ITerm2,
    /// Fallback: Unicode half-block characters via viuer.
    Block,
}

/// Detect which graphics protocol the running terminal supports.
///
/// Uses env-var checks only — avoids blocking DA1 terminal query.
/// Fallback order: Kitty → Sixel → iTerm2 → Block.
pub fn detect_protocol() -> Protocol {
    if std::env::var_os("KITTY_WINDOW_ID").is_some()
        || std::env::var("TERM").as_deref() == Ok("xterm-kitty")
    {
        return Protocol::Kitty;
    }

    if std::env::var("TERM_PROGRAM").as_deref() == Ok("iTerm.app") {
        return Protocol::ITerm2;
    }

    let term = std::env::var("TERM").unwrap_or_default();
    if term.contains("sixel")
        || matches!(term.as_str(), "mlterm" | "yaft-256color")
        || std::env::var_os("TERM_SIXEL").is_some()
    {
        return Protocol::Sixel;
    }

    Protocol::Block
}

/// Decode image bytes into a [`DynamicImage`].
///
/// CPU-heavy — caller should run this via `tokio::task::spawn_blocking`.
pub fn decode(bytes: &[u8]) -> Result<DynamicImage> {
    use image::ImageReader;
    use std::io::Cursor;

    ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .context("image format detection failed")?
        .decode()
        .context("image decode failed")
}

/// Render a [`DynamicImage`] to stdout using viuer.
///
/// `max_width` / `max_height` are in terminal cells.
/// Flushes stdout immediately after output.
///
/// CPU-heavy — caller should run this via `tokio::task::spawn_blocking`.
pub fn render_to_stdout(img: &DynamicImage, max_width: u32, max_height: u32) -> Result<()> {
    use std::io::Write;

    let conf = viuer::Config {
        width: Some(max_width),
        height: Some(max_height),
        absolute_offset: false,
        ..Default::default()
    };

    viuer::print(img, &conf).map_err(|e| anyhow::anyhow!("viuer render: {e}"))?;

    // Always flush immediately — never buffer image data.
    std::io::stdout().flush().context("flush stdout after image")?;

    Ok(())
}

/// Async convenience: decode + render bytes in a blocking thread.
pub async fn render_bytes(bytes: Vec<u8>, max_width: u32, max_height: u32) -> Result<()> {
    tokio::task::spawn_blocking(move || {
        let img = decode(&bytes)?;
        render_to_stdout(&img, max_width, max_height)
    })
    .await
    .context("image task panicked")??;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_protocol_returns_a_variant() {
        // Just ensure it doesn't panic and returns something.
        let _p = detect_protocol();
    }

    #[test]
    fn decode_invalid_bytes_errors() {
        let result = decode(b"not an image");
        assert!(result.is_err());
    }

    #[test]
    fn decode_round_trip_png() {
        use image::{DynamicImage, ImageBuffer, Rgb};
        // Encode a 1x1 red pixel to PNG in memory, then decode it back.
        let img = DynamicImage::ImageRgb8(ImageBuffer::from_pixel(1, 1, Rgb([255u8, 0, 0])));
        let mut buf = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();
        let result = decode(&buf);
        assert!(result.is_ok(), "round-trip decode failed: {result:?}");
    }
}
