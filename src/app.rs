//! Top-level application state and event loop.

use crate::network::client::SpiderClient;

/// Run the browser for the given URL.
///
/// Phase 1: fetches URL and prints raw text to stdout.
/// TUI rendering is wired in Phase 2.
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

    if resp.is_text() {
        let text = String::from_utf8_lossy(&resp.body);
        println!("{text}");
    } else {
        tracing::warn!(content_type = ?resp.content_type, "non-text response — no renderer yet");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn placeholder() {}
}
