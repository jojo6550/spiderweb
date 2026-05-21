//! reqwest client wrapper with cookie jar, TLS, redirects.

use anyhow::{Context, Result};
use reqwest::{Client, header};
use std::time::Duration;

use crate::network::response::HttpResponse;

const USER_AGENT: &str = concat!("spiderweb/", env!("CARGO_PKG_VERSION"), " (terminal browser)");

/// Async HTTP client for spiderweb.
#[derive(Clone)]
pub struct SpiderClient {
    inner: Client,
}

impl SpiderClient {
    /// Build a new client with sane defaults: cookie store, TLS, 30 s timeout.
    pub fn new() -> Result<Self> {
        let inner = Client::builder()
            .cookie_store(true)
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .user_agent(USER_AGENT)
            .build()
            .context("failed to build HTTP client")?;
        Ok(Self { inner })
    }

    /// Fetch `url`, follow redirects, return a buffered [`HttpResponse`].
    pub async fn fetch(&self, url: &str) -> Result<HttpResponse> {
        let resp = self
            .inner
            .get(url)
            .send()
            .await
            .with_context(|| format!("request to {url} failed"))?;

        let status = resp.status();
        let content_type = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);

        tracing::debug!(%url, %status, ?content_type, "response headers");

        let body = resp
            .bytes()
            .await
            .context("failed to read response body")?;

        Ok(HttpResponse {
            status,
            content_type,
            body,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;

    #[tokio::test]
    async fn fetch_ok_html() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/")
            .with_status(200)
            .with_header("content-type", "text/html; charset=utf-8")
            .with_body("<html><body>hello</body></html>")
            .create_async()
            .await;

        let client = SpiderClient::new().unwrap();
        let resp = client.fetch(&server.url()).await.unwrap();

        assert_eq!(resp.status.as_u16(), 200);
        assert!(resp.is_html());
        assert_eq!(&resp.body[..], b"<html><body>hello</body></html>");
        mock.assert_async().await;
    }

    #[tokio::test]
    async fn fetch_404() {
        let mut server = Server::new_async().await;
        let mock = server
            .mock("GET", "/missing")
            .with_status(404)
            .with_body("not found")
            .create_async()
            .await;

        let client = SpiderClient::new().unwrap();
        let resp = client.fetch(&format!("{}/missing", server.url())).await.unwrap();

        assert_eq!(resp.status.as_u16(), 404);
        mock.assert_async().await;
    }
}
