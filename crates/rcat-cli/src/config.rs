//! Configuration loading for rcat (TOML + CLI overrides).

use rcat_core::plugin::DEFAULT_PLUGIN_TIMEOUT_SECS;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// User-facing configuration (from `~/.config/rcat/config.toml` or `--config`).
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct RcatConfig {
    /// Seconds to wait for an external plugin subprocess before killing it.
    pub plugin_timeout_secs: u64,
}

impl Default for RcatConfig {
    fn default() -> Self {
        Self {
            plugin_timeout_secs: DEFAULT_PLUGIN_TIMEOUT_SECS,
        }
    }
}

impl RcatConfig {
    pub fn plugin_timeout(&self) -> Duration {
        Duration::from_secs(self.plugin_timeout_secs.max(1))
    }

    /// Load config: optional explicit path, then XDG `config.toml`, then defaults.
    pub fn load(explicit: Option<&Path>) -> Self {
        if let Some(path) = explicit {
            if let Ok(cfg) = Self::from_file(path) {
                return cfg;
            }
            tracing::warn!(path = %path.display(), "failed to load config file, using defaults");
        } else if let Some(path) = default_config_path()
            && path.exists()
            && let Ok(cfg) = Self::from_file(&path)
        {
            return cfg;
        }
        Self::default()
    }

    fn from_file(path: &Path) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let content = std::fs::read_to_string(path)?;
        Ok(toml::from_str(&content)?)
    }
}

fn default_config_path() -> Option<PathBuf> {
    dirs::config_dir().map(|d| d.join("rcat").join("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn parses_plugin_timeout() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "plugin_timeout_secs = 10").unwrap();
        let cfg = RcatConfig::from_file(f.path()).unwrap();
        assert_eq!(cfg.plugin_timeout_secs, 10);
    }

    #[test]
    fn default_timeout_matches_core() {
        assert_eq!(
            RcatConfig::default().plugin_timeout_secs,
            DEFAULT_PLUGIN_TIMEOUT_SECS
        );
    }
}
