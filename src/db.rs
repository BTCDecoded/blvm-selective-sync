//! Module database helpers.

use anyhow::{Context, Result};
use blvm_sdk::module::ModuleDb;
use std::path::{Path, PathBuf};

use crate::config::SyncPolicyConfig;

/// Open the module DB at the same data directory as [`SyncPolicyConfig`].
pub fn open_policy_db() -> Result<ModuleDb> {
    ModuleDb::open(data_dir_from_config()?)
}

/// Resolve module data directory from config path env vars.
pub fn data_dir_from_config() -> Result<PathBuf> {
    let path = SyncPolicyConfig::config_path();
    path.parent()
        .map(Path::to_path_buf)
        .filter(|p| !p.as_os_str().is_empty())
        .context("Invalid config path: no parent directory")
}
