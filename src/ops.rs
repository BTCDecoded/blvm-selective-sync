//! Shared refresh / apply operations for CLI and background tasks.

use anyhow::Result;
use blvm_node::module::traits::NodeAPI;
use blvm_node::storage::database::Database;

use crate::apply_policy::{apply_policy, ApplyReport};
use crate::audit;
use crate::config::SyncPolicyConfig;
use crate::policy_store::{refresh_from_fetched, RefreshReport};
use crate::registry_fetch::fetch_registry_index;

#[derive(Debug, Clone)]
pub struct RefreshAndApplyResult {
    pub refresh: RefreshReport,
    pub apply: Option<ApplyReport>,
}

fn now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| format!("{}", d.as_secs()))
        .unwrap_or_else(|_| "unknown".to_string())
}

/// Fetch all subscribed registries, merge into policy store, update config timestamp.
pub async fn refresh_policy(
    db: &dyn Database,
    config: &mut SyncPolicyConfig,
) -> Result<RefreshReport> {
    if config.registries.is_empty() {
        anyhow::bail!("No registries subscribed");
    }

    let mut fetched = Vec::new();
    let mut failed = Vec::new();

    for url in &config.registries {
        match fetch_registry_index(url).await {
            Ok(index) => fetched.push((url.clone(), index)),
            Err(e) => {
                tracing::warn!("Registry fetch failed for {url}: {e}");
                failed.push((url.clone(), e.to_string()));
            }
        }
    }

    let fetched_at = now_iso();
    let report = refresh_from_fetched(db, config, &fetched, &fetched_at)?;
    let mut report = report;
    report.urls_failed = failed;

    config.last_refresh = Some(fetched_at);
    config.save()?;

    let _ = audit::append_line(
        config,
        &format!(
            "refresh fetched={} failed={} stored={}",
            report.urls_fetched,
            report.urls_failed.len(),
            report.entries_stored
        ),
    );

    Ok(report)
}

/// Refresh then optionally apply to node denylists.
pub async fn refresh_and_apply(
    db: &dyn Database,
    config: &mut SyncPolicyConfig,
    node_api: Option<&dyn NodeAPI>,
) -> Result<RefreshAndApplyResult> {
    let refresh = refresh_policy(db, config).await?;
    let apply = if let Some(api) = node_api {
        Some(apply_policy(api, db, config).await?)
    } else {
        None
    };
    Ok(RefreshAndApplyResult { refresh, apply })
}
