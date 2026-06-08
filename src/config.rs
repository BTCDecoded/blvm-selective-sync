//! Sync policy configuration storage.
//!
//! Stores registry URLs in config.toml. Used by list, subscribe, unsubscribe, refresh, status.

use anyhow::{Context, Result};
use blvm_sdk_macros::config;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Config file name
const CONFIG_FILENAME: &str = "config.toml";

fn default_min_registry_agreement() -> f64 {
    0.5
}
fn default_registry_refresh_interval() -> u64 {
    3600
}

/// Sync policy config (registry list only; full config in node config.toml).
/// Node config `[modules.selective-sync]` overrides when present.
/// Env override: `MODULE_CONFIG_REGISTRIES`, `MODULE_CONFIG_LAST_REFRESH`.
#[config(name = "selective-sync")]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SyncPolicyConfig {
    #[serde(default)]
    #[config_env]
    pub registries: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[config_env]
    pub last_refresh: Option<String>,
    /// Min fraction of registries that must agree for an entry (0.0–1.0).
    #[serde(default = "default_min_registry_agreement")]
    pub min_registry_agreement: f64,
    /// Seconds between registry URL refresh.
    #[serde(default = "default_registry_refresh_interval")]
    pub registry_refresh_interval: u64,
    /// Witness mode: "strict" | "relaxed".
    #[serde(default)]
    pub witness_mode: String,
    /// Enable audit log.
    #[serde(default)]
    pub audit_log: bool,
    /// Path for audit log file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audit_log_path: Option<String>,
    /// When true, subscribe to NewBlock and build local registry from each block.
    #[serde(default)]
    pub on_chain_registry_builder: bool,
    /// When true, strip flagged witness data during IBD block persistence.
    #[serde(default)]
    pub ibd_filter_enabled: bool,
}

impl SyncPolicyConfig {
    /// Path to config file. Uses BLVM_DATA_DIR, DATA_DIR (module data-dir), or ~/.local/share/blvm/.
    pub fn config_path() -> PathBuf {
        if let Ok(dir) = std::env::var("BLVM_DATA_DIR") {
            return PathBuf::from(dir).join(CONFIG_FILENAME);
        }
        if let Ok(dir) = std::env::var("DATA_DIR") {
            return PathBuf::from(dir).join(CONFIG_FILENAME);
        }
        dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("blvm")
            .join(CONFIG_FILENAME)
    }

    /// Save config to disk.
    pub fn save(&self) -> Result<()> {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("Failed to create {}", parent.display()))?;
        }
        let data =
            toml::to_string_pretty(self).with_context(|| format!("Failed to serialize config"))?;
        std::fs::write(&path, data).with_context(|| format!("Failed to write {}", path.display()))
    }

    /// Add registry URL (deduplicated).
    pub fn subscribe(&mut self, url: &str) {
        let url = url.trim().to_string();
        if !url.is_empty() && !self.registries.contains(&url) {
            self.registries.push(url);
        }
    }

    /// Remove registry URL.
    pub fn unsubscribe(&mut self, url: &str) {
        self.registries.retain(|r| r != url.trim());
    }
}
