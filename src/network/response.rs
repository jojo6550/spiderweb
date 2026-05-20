//! HTTP response types.

use bytes::Bytes;
use reqwest::StatusCode;

/// Completed HTTP response with buffered body.
pub struct HttpResponse {
    pub status: StatusCode,
    /// Raw `Content-Type` header value, if present.
    pub content_type: Option<String>,
    pub body: Bytes,
}

impl HttpResponse {
    /// Returns `true` if `Content-Type` starts with `text/`.
    pub fn is_text(&self) -> bool {
        self.content_type
            .as_deref()
            .map(|ct| ct.starts_with("text/"))
            .unwrap_or(false)
    }

    /// Returns `true` if `Content-Type` is `text/html`.
    pub fn is_html(&self) -> bool {
        self.content_type
            .as_deref()
            .map(|ct| ct.starts_with("text/html"))
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use reqwest::StatusCode;

    fn make(ct: Option<&str>) -> HttpResponse {
        HttpResponse {
            status: StatusCode::OK,
            content_type: ct.map(str::to_owned),
            body: Bytes::new(),
        }
    }

    #[test]
    fn is_text_html() {
        assert!(make(Some("text/html; charset=utf-8")).is_text());
        assert!(make(Some("text/html; charset=utf-8")).is_html());
    }

    #[test]
    fn is_text_plain() {
        assert!(make(Some("text/plain")).is_text());
        assert!(!make(Some("text/plain")).is_html());
    }

    #[test]
    fn not_text() {
        assert!(!make(Some("application/json")).is_text());
        assert!(!make(None).is_text());
    }
}
