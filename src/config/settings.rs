//! User config schema — persisted to `~/.config/spiderweb/config.toml`.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Top-level user configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// URL opened on startup when none is supplied via CLI.
    pub home_page: String,
    /// Color theme ("dark" or "light").
    pub theme: String,
    /// Request timeout in seconds.
    pub timeout_secs: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            home_page: "https://example.com".into(),
            theme: "dark".into(),
            timeout_secs: 30,
        }
    }
}

impl Settings {
    /// Load from `~/.config/spiderweb/config.toml`, or return defaults.
    pub fn load() -> Result<Self> {
        let path = config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("read {}", path.display()))?;
        toml::from_str(&raw).with_context(|| format!("parse {}", path.display()))
    }

    /// Write current settings to `~/.config/spiderweb/config.toml`.
    pub fn save(&self) -> Result<()> {
        let path = config_path()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create dir {}", parent.display()))?;
        }
        let toml = toml::to_string_pretty(self).context("serialize settings")?;
        std::fs::write(&path, toml)
            .with_context(|| format!("write {}", path.display()))
    }
}

fn config_path() -> Result<PathBuf> {
    let dir = dirs::config_dir().context("could not determine config directory")?;
    Ok(dir.join("spiderweb").join("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_valid() {
        let s = Settings::default();
        assert!(!s.home_page.is_empty());
        assert!(s.timeout_secs > 0);
    }

    #[test]
    fn round_trip_toml() {
        let s = Settings::default();
        let serialized = toml::to_string_pretty(&s).unwrap();
        let parsed: Settings = toml::from_str(&serialized).unwrap();
        assert_eq!(parsed.home_page, s.home_page);
    }
}
