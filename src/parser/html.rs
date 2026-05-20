//! HTML parse → DOM tree via scraper/html5ever.

use scraper::{Html, Selector};

/// Parsed HTML document.
pub struct ParsedPage {
    html: Html,
}

/// A hyperlink extracted from the document.
pub struct Link {
    pub href: String,
    pub text: String,
}

impl ParsedPage {
    /// Parse HTML from raw bytes (UTF-8, replacing invalid sequences).
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let src = String::from_utf8_lossy(bytes);
        Self {
            html: Html::parse_document(&src),
        }
    }

    /// Parse HTML from a string slice.
    pub fn parse_html(src: &str) -> Self {
        Self {
            html: Html::parse_document(src),
        }
    }

    /// Exposes the parsed document for renderers.
    pub fn document(&self) -> &Html {
        &self.html
    }

    /// Page `<title>` text, trimmed. Returns `None` if absent or empty.
    pub fn title(&self) -> Option<String> {
        let sel = Selector::parse("title").ok()?;
        self.html
            .select(&sel)
            .next()
            .map(|el| el.text().collect::<String>().trim().to_owned())
            .filter(|s| !s.is_empty())
    }

    /// All `<a href>` links in document order.
    pub fn links(&self) -> Vec<Link> {
        let Ok(sel) = Selector::parse("a[href]") else {
            return Vec::new();
        };
        self.html
            .select(&sel)
            .filter_map(|el| {
                let href = el.value().attr("href")?.to_owned();
                let text = el.text().collect::<String>().trim().to_owned();
                Some(Link { href, text })
            })
            .collect()
    }

    /// `<meta name="description">` content attribute.
    pub fn description(&self) -> Option<String> {
        let Ok(sel) = Selector::parse(r#"meta[name="description"]"#) else {
            return None;
        };
        self.html
            .select(&sel)
            .next()
            .and_then(|el| el.value().attr("content").map(str::to_owned))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<!DOCTYPE html>
<html>
<head>
  <title>Test Page</title>
  <meta name="description" content="A test page." />
</head>
<body>
  <h1>Hello</h1>
  <p>World</p>
  <a href="https://example.com">Example</a>
  <a href="/relative">Relative</a>
</body>
</html>"#;

    #[test]
    fn title_extracted() {
        let page = ParsedPage::parse_html(SAMPLE);
        assert_eq!(page.title().as_deref(), Some("Test Page"));
    }

    #[test]
    fn description_extracted() {
        let page = ParsedPage::parse_html(SAMPLE);
        assert_eq!(page.description().as_deref(), Some("A test page."));
    }

    #[test]
    fn links_extracted() {
        let page = ParsedPage::parse_html(SAMPLE);
        let links = page.links();
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].href, "https://example.com");
        assert_eq!(links[0].text, "Example");
        assert_eq!(links[1].href, "/relative");
    }

    #[test]
    fn from_bytes_utf8_lossy() {
        let bytes = b"<html><head><title>Bytes</title></head><body>ok</body></html>";
        let page = ParsedPage::from_bytes(bytes);
        assert_eq!(page.title().as_deref(), Some("Bytes"));
    }

    #[test]
    fn empty_doc_no_panic() {
        let page = ParsedPage::parse_html("");
        assert!(page.title().is_none());
        assert!(page.links().is_empty());
        assert!(page.description().is_none());
    }
}
