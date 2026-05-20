//! Top-level application state and event loop.

/// Run the browser for the given URL.
///
/// Phase 1 scaffold — networking, parsing, and rendering not yet wired.
pub async fn run(url: String) -> anyhow::Result<()> {
    tracing::info!(%url, "spiderweb scaffold boot");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn run_returns_ok() {
        assert!(run("https://example.com".into()).await.is_ok());
    }
}
