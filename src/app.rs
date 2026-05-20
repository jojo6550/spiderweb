//! Top-level application state and event loop.

use crate::network::client::SpiderClient;
use crate::parser::html::ParsedPage;
use crate::renderer::text as text_renderer;

/// Run the browser for the given URL.
///
/// Phase 1: fetches, parses HTML, renders text to stdout.
/// TUI and image rendering wired in Phase 2.
pub async fn run(url: String) -> anyhow::Result<()> {
    tracing::info!(%url, "fetching");

    let client = SpiderClient::new()?;
    let resp = client.fetch(&url).await?;

    tracing::info!(
        status = %resp.status,
        content_type = ?resp.content_type,
        bytes = resp.body.len(),
        "response received"
    );

    if resp.is_html() {
        let page = ParsedPage::from_bytes(&resp.body);

        if let Some(title) = page.title() {
            println!("\x1b[1m{title}\x1b[0m\n");
        }

        let rendered = text_renderer::render(&page);
        println!("{rendered}");
    } else if resp.is_text() {
        println!("{}", String::from_utf8_lossy(&resp.body));
    } else {
        tracing::warn!(
            content_type = ?resp.content_type,
            "non-text response — no renderer yet"
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn placeholder() {}
}
