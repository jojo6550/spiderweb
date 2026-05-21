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

/// Convert image bytes to ANSI half-block lines for inline display in any
/// terminal that supports 24-bit color.
///
/// Each cell uses `▀` (U+2580, upper half block) with foreground = top pixel
/// and background = bottom pixel, so one terminal cell encodes two vertical
/// image pixels.
///
/// `max_cells_wide` and `max_cells_tall` are upper bounds in terminal cells.
/// The image is downscaled preserving aspect ratio. CPU-heavy — wrap in
/// `spawn_blocking`.
pub fn to_ansi_lines(bytes: &[u8], max_cells_wide: u32, max_cells_tall: u32) -> Result<Vec<String>> {
    use image::imageops::FilterType;

    let img = decode(bytes)?;
    // Terminal cells are ~2× taller than wide; 1 cell = 2 image pixels vertical.
    let max_pix_w = max_cells_wide.max(1);
    let max_pix_h = (max_cells_tall.max(1)) * 2;
    let resized = img.resize(max_pix_w, max_pix_h, FilterType::Triangle);
    let rgba = resized.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());

    let mut lines = Vec::with_capacity((h / 2 + 1) as usize);
    let mut y = 0;
    while y < h {
        let mut line = String::with_capacity(w as usize * 24);
        for x in 0..w {
            let top = *rgba.get_pixel(x, y);
            let bot = if y + 1 < h { *rgba.get_pixel(x, y + 1) } else { top };

            // Both fully transparent → blank cell.
            if top[3] < 16 && bot[3] < 16 {
                line.push(' ');
                continue;
            }
            line.push_str(&format!(
                "\x1b[38;2;{};{};{};48;2;{};{};{}m▀",
                top[0], top[1], top[2], bot[0], bot[1], bot[2]
            ));
        }
        line.push_str("\x1b[0m");
        lines.push(line);
        y += 2;
    }
    Ok(lines)
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
    fn to_ansi_lines_emits_color_cells() {
        use image::{DynamicImage, ImageBuffer, Rgb};
        let img = DynamicImage::ImageRgb8(ImageBuffer::from_pixel(4, 4, Rgb([255u8, 0, 0])));
        let mut buf = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();
        let lines = to_ansi_lines(&buf, 4, 4).unwrap();
        assert!(!lines.is_empty());
        assert!(lines[0].contains("\x1b["));
        assert!(lines[0].contains('▀'));
        // Ends with reset.
        assert!(lines[0].ends_with("\x1b[0m"));
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
