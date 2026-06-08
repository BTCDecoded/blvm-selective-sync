//! Optional audit log append.

use anyhow::{Context, Result};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use crate::config::SyncPolicyConfig;

/// Append one line to the audit log when enabled in config.
pub fn append_line(config: &SyncPolicyConfig, line: &str) -> Result<()> {
    if !config.audit_log {
        return Ok(());
    }
    let path = audit_log_path(config);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create audit log dir {}", parent.display()))?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("Failed to open audit log {}", path.display()))?;
    writeln!(file, "{line}")?;
    Ok(())
}

fn audit_log_path(config: &SyncPolicyConfig) -> PathBuf {
    config
        .audit_log_path
        .as_ref()
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            SyncPolicyConfig::config_path()
                .parent()
                .map(|p| p.join("audit.log"))
                .unwrap_or_else(|| PathBuf::from("audit.log"))
        })
}
