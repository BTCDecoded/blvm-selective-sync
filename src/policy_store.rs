//! Persistent policy store and quorum merge.

use anyhow::{Context, Result};
use blvm_node::storage::database::Database;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::config::SyncPolicyConfig;
use crate::registry_entry::{RegistryEntry, RegistryIndex};

pub const REGISTRY_ENTRIES_TREE: &str = "registry_entries";
pub const ON_CHAIN_REGISTRY_TREE: &str = "on_chain_registry";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredPolicyEntry {
    pub entry: RegistryEntry,
    pub sources: Vec<String>,
    pub fetched_at: String,
}

#[derive(Debug, Clone, Default)]
pub struct RefreshReport {
    pub urls_fetched: usize,
    pub urls_failed: Vec<(String, String)>,
    pub entries_stored: usize,
}

/// Merge fetched registries into the policy store using quorum rules.
pub fn refresh_from_fetched(
    db: &dyn Database,
    config: &SyncPolicyConfig,
    fetched: &[(String, RegistryIndex)],
    fetched_at: &str,
) -> Result<RefreshReport> {
    let mut report = RefreshReport {
        urls_fetched: fetched.len(),
        ..Default::default()
    };

    if fetched.is_empty() {
        return Ok(report);
    }

    let required = quorum_required(config.min_registry_agreement, fetched.len());
    let mut votes: HashMap<String, (RegistryEntry, Vec<String>)> = HashMap::new();

    for (url, index) in fetched {
        for entry in &index.entries {
            if entry.txid.len() != 64 || !entry.txid.chars().all(|c| c.is_ascii_hexdigit()) {
                tracing::trace!("Skipping invalid txid in registry {url}");
                continue;
            }
            let vote = votes
                .entry(entry.txid.clone())
                .or_insert_with(|| (entry.clone(), Vec::new()));
            if !vote.1.contains(url) {
                vote.1.push(url.clone());
            }
        }
    }

    let tree = db
        .open_tree(REGISTRY_ENTRIES_TREE)
        .context("Failed to open registry_entries tree")?;

    tree.clear().context("Failed to clear registry_entries")?;

    for (txid, (entry, sources)) in votes {
        if sources.len() < required {
            continue;
        }
        let stored = StoredPolicyEntry {
            entry,
            sources,
            fetched_at: fetched_at.to_string(),
        };
        let value = serde_json::to_vec(&stored).context("Failed to serialize policy entry")?;
        tree.insert(txid.as_bytes(), &value)
            .context("Failed to store policy entry")?;
        report.entries_stored += 1;
    }

    Ok(report)
}

fn quorum_required(min_agreement: f64, registry_count: usize) -> usize {
    if registry_count == 0 {
        return usize::MAX;
    }
    let min_agreement = min_agreement.clamp(0.0, 1.0);
    let required = (min_agreement * registry_count as f64).ceil() as usize;
    required.max(1)
}

/// Load all policy entries from remote quorum store and on-chain builder tree.
pub fn load_merged_entries(db: &dyn Database) -> Result<Vec<StoredPolicyEntry>> {
    let mut by_txid: HashMap<String, StoredPolicyEntry> = HashMap::new();

    if let Ok(tree) = db.open_tree(REGISTRY_ENTRIES_TREE) {
        for item in tree.iter().flatten() {
            let stored: StoredPolicyEntry =
                serde_json::from_slice(&item.1).context("Corrupt registry_entries value")?;
            by_txid.insert(stored.entry.txid.clone(), stored);
        }
    }

    if let Ok(tree) = db.open_tree(ON_CHAIN_REGISTRY_TREE) {
        for item in tree.iter().flatten() {
            let entry: RegistryEntry =
                serde_json::from_slice(&item.1).context("Corrupt on_chain_registry value")?;
            let txid = entry.txid.clone();
            by_txid
                .entry(txid)
                .and_modify(|existing| {
                    if !existing.sources.contains(&"on_chain".to_string()) {
                        existing.sources.push("on_chain".to_string());
                    }
                })
                .or_insert_with(|| StoredPolicyEntry {
                    entry,
                    sources: vec!["on_chain".to_string()],
                    fetched_at: String::new(),
                });
        }
    }

    Ok(by_txid.into_values().collect())
}

/// Export document for CLI / file output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportRegistryDocument {
    pub entries: Vec<RegistryEntry>,
    pub sources: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_refresh: Option<String>,
}

/// Build export view from merged policy store.
pub fn export_document(
    db: &dyn Database,
    config: &SyncPolicyConfig,
) -> Result<ExportRegistryDocument> {
    let entries: Vec<RegistryEntry> = load_merged_entries(db)?
        .into_iter()
        .map(|s| s.entry)
        .collect();
    Ok(ExportRegistryDocument {
        entries,
        sources: config.registries.clone(),
        last_refresh: config.last_refresh.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry_entry::EmbeddingType;
    use blvm_sdk::module::ModuleDb;
    use tempfile::TempDir;

    fn sample_entry(txid: &str) -> RegistryEntry {
        RegistryEntry {
            txid: txid.to_string(),
            block_height: Some(1),
            block_position: Some(0),
            embedding_type: EmbeddingType::Witness,
            merkle_sibling_path: None,
            outputs: vec![],
            inputs: vec![],
        }
    }

    #[test]
    fn quorum_requires_ceiling_fraction() {
        assert_eq!(quorum_required(0.5, 2), 1);
        assert_eq!(quorum_required(0.5, 3), 2);
        assert_eq!(quorum_required(1.0, 2), 2);
    }

    #[test]
    fn refresh_stores_entry_with_single_source_at_half_quorum() {
        let dir = tempfile::tempdir().unwrap();
        let db = ModuleDb::open(dir.path()).unwrap();
        let config = SyncPolicyConfig {
            min_registry_agreement: 0.5,
            ..Default::default()
        };
        let txid = "a".repeat(64);
        let index = crate::registry_entry::RegistryIndex {
            entries: vec![sample_entry(&txid)],
        };
        let fetched = vec![("https://a.example".to_string(), index)];
        let report =
            refresh_from_fetched(db.as_db().as_ref(), &config, &fetched, "1").expect("refresh");
        assert_eq!(report.entries_stored, 1);
        let merged = load_merged_entries(db.as_db().as_ref()).expect("load");
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].entry.txid, txid);
    }

    #[test]
    fn refresh_requires_both_registries_at_full_quorum() {
        let dir = TempDir::new().unwrap();
        let db = ModuleDb::open(dir.path()).unwrap();
        let config = SyncPolicyConfig {
            min_registry_agreement: 1.0,
            ..Default::default()
        };
        let txid = "b".repeat(64);
        let index_a = crate::registry_entry::RegistryIndex {
            entries: vec![sample_entry(&txid)],
        };
        let index_b = crate::registry_entry::RegistryIndex {
            entries: vec![sample_entry(&txid)],
        };
        let fetched = vec![
            ("https://a.example".to_string(), index_a),
            ("https://b.example".to_string(), index_b),
        ];
        let report =
            refresh_from_fetched(db.as_db().as_ref(), &config, &fetched, "1").expect("refresh");
        assert_eq!(report.entries_stored, 1);

        let empty_b = crate::registry_entry::RegistryIndex { entries: vec![] };
        let partial = vec![
            ("https://a.example".to_string(), fetched[0].1.clone()),
            ("https://b.example".to_string(), empty_b),
        ];
        let db2 = ModuleDb::open(dir.path().join("sub")).unwrap();
        let report2 =
            refresh_from_fetched(db2.as_db().as_ref(), &config, &partial, "2").expect("refresh");
        assert_eq!(report2.entries_stored, 0);
    }
}
