//! Bookmark persistence — saved to `~/.config/spiderweb/bookmarks.json`.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A single saved bookmark.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bookmark {
    pub url: String,
    pub title: String,
}

/// Bookmark store — loaded from and saved to disk.
pub struct Bookmarks {
    path: PathBuf,
    pub entries: Vec<Bookmark>,
}

impl Bookmarks {
    /// Empty store at the default path — used as a fallback when `load` fails.
    pub fn empty() -> Self {
        let path = dirs::config_dir()
            .unwrap_or_default()
            .join("spiderweb")
            .join("bookmarks.json");
        Self { path, entries: Vec::new() }
    }

    /// Load bookmarks from `~/.config/spiderweb/bookmarks.json`.
    /// Returns an empty store (not an error) if the file doesn't exist.
    pub fn load() -> Result<Self> {
        let path = bookmark_path()?;
        let entries = if path.exists() {
            let raw = std::fs::read_to_string(&path)
                .with_context(|| format!("read {}", path.display()))?;
            serde_json::from_str(&raw)
                .with_context(|| format!("parse {}", path.display()))?
        } else {
            Vec::new()
        };
        Ok(Self { path, entries })
    }

    /// Save the current bookmark list to disk.
    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create dir {}", parent.display()))?;
        }
        let json = serde_json::to_string_pretty(&self.entries)?;
        std::fs::write(&self.path, json)
            .with_context(|| format!("write {}", self.path.display()))
    }

    /// Add a bookmark, deduplicating by URL.
    pub fn add(&mut self, url: String, title: String) {
        if !self.entries.iter().any(|b| b.url == url) {
            self.entries.push(Bookmark { url, title });
        }
    }

    /// Remove bookmark by URL. Returns `true` if found and removed.
    pub fn remove(&mut self, url: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|b| b.url != url);
        self.entries.len() < before
    }

    pub fn contains(&self, url: &str) -> bool {
        self.entries.iter().any(|b| b.url == url)
    }
}

fn bookmark_path() -> Result<PathBuf> {
    let config = dirs::config_dir()
        .context("could not determine config directory")?;
    Ok(config.join("spiderweb").join("bookmarks.json"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_deduplicates() {
        let mut b = Bookmarks { path: PathBuf::new(), entries: Vec::new() };
        b.add("https://a.com".into(), "A".into());
        b.add("https://a.com".into(), "A again".into());
        assert_eq!(b.entries.len(), 1);
    }

    #[test]
    fn remove_by_url() {
        let mut b = Bookmarks { path: PathBuf::new(), entries: Vec::new() };
        b.add("https://a.com".into(), "A".into());
        assert!(b.remove("https://a.com"));
        assert!(b.entries.is_empty());
        assert!(!b.remove("https://a.com"));
    }
}
