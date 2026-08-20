//! v3 download policy: skip witness fetch only when merkle paths verify.

use anyhow::Result;
use blvm_node::storage::database::Database;
use blvm_protocol::Hash;
use serde::{Deserialize, Serialize};

use crate::config::SyncPolicyConfig;
use crate::merkle_proof::verify_sibling_path;
use crate::policy_store::load_merged_entries;
use crate::registry_entry::EmbeddingType;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DownloadPolicyRequest {
    pub height: u64,
    pub block_hash: Hash,
    pub merkle_root: Hash,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DownloadPolicyResponse {
    pub skip_witness: bool,
    pub verified_paths: u32,
    pub bytes_saved_hint: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DownloadPolicyStats {
    pub skip_witness_blocks: u64,
    pub verified_paths: u64,
}

const STATS_TREE: &str = "download_policy_stats";
const STATS_KEY: &[u8] = b"stats";

pub fn decide_download_policy(
    db: &dyn Database,
    config: &SyncPolicyConfig,
    height: u64,
    merkle_root: &Hash,
) -> Result<DownloadPolicyResponse> {
    if !config.trust_registry_stubs {
        return Ok(DownloadPolicyResponse {
            skip_witness: false,
            verified_paths: 0,
            bytes_saved_hint: 0,
        });
    }

    let entries = load_merged_entries(db)?;
    let mut at_height: Vec<_> = entries
        .into_iter()
        .filter(|s| {
            s.entry.block_height == Some(height)
                && matches!(s.entry.embedding_type, EmbeddingType::Witness)
        })
        .collect();

    if at_height.is_empty() {
        return Ok(DownloadPolicyResponse {
            skip_witness: false,
            verified_paths: 0,
            bytes_saved_hint: 0,
        });
    }

    let mut verified = 0u32;
    let mut all_ok = true;
    for stored in &at_height {
        let Some(path) = stored.entry.merkle_sibling_path.as_ref() else {
            all_ok = false;
            continue;
        };
        let Some(pos) = stored.entry.block_position else {
            all_ok = false;
            continue;
        };
        let Ok(txid_bytes) = hex::decode(stored.entry.txid.trim()) else {
            all_ok = false;
            continue;
        };
        if txid_bytes.len() != 32 {
            all_ok = false;
            continue;
        }
        let mut txid = [0u8; 32];
        txid.copy_from_slice(&txid_bytes);
        match verify_sibling_path(&txid, pos as usize, path, merkle_root) {
            Ok(true) => verified += 1,
            _ => all_ok = false,
        }
    }

    let skip_witness = all_ok && verified > 0 && verified as usize == at_height.len();
    if skip_witness {
        increment_stats(db, verified)?;
    }

    let _ = &mut at_height;
    Ok(DownloadPolicyResponse {
        skip_witness,
        verified_paths: verified,
        bytes_saved_hint: if skip_witness { 1 } else { 0 },
    })
}

fn increment_stats(db: &dyn Database, verified: u32) -> Result<()> {
    let tree = db.open_tree(STATS_TREE)?;
    let mut stats = tree
        .get(STATS_KEY)?
        .and_then(|v| serde_json::from_slice::<DownloadPolicyStats>(&v).ok())
        .unwrap_or_default();
    stats.skip_witness_blocks += 1;
    stats.verified_paths += u64::from(verified);
    tree.insert(STATS_KEY, &serde_json::to_vec(&stats)?)?;
    Ok(())
}

pub fn load_stats(db: &dyn Database) -> DownloadPolicyStats {
    db.open_tree(STATS_TREE)
        .ok()
        .and_then(|tree| tree.get(STATS_KEY).ok().flatten())
        .and_then(|v| serde_json::from_slice(&v).ok())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::merkle_proof::sibling_path;
    use crate::policy_store::refresh_from_fetched;
    use crate::registry_entry::{EmbeddingType, RegistryEntry, RegistryIndex};
    use blvm_protocol::mining::calculate_merkle_root_from_tx_ids;
    use blvm_sdk::module::ModuleDb;

    #[test]
    fn skip_off_without_trust_flag() {
        let dir = tempfile::tempdir().unwrap();
        let db = ModuleDb::open(dir.path()).unwrap();
        let config = SyncPolicyConfig::default();
        let resp = decide_download_policy(db.as_db().as_ref(), &config, 1, &[0u8; 32]).unwrap();
        assert!(!resp.skip_witness);
    }

    #[test]
    fn skip_when_all_witness_paths_verify() {
        let txids: Vec<Hash> = (0u8..3).map(|i| [i + 1; 32]).collect();
        let root = calculate_merkle_root_from_tx_ids(&txids).unwrap();
        let path = sibling_path(&txids, 1).unwrap();

        let dir = tempfile::tempdir().unwrap();
        let db = ModuleDb::open(dir.path()).unwrap();
        let config = SyncPolicyConfig {
            trust_registry_stubs: true,
            min_registry_agreement: 0.5,
            ..Default::default()
        };
        let entry = RegistryEntry {
            txid: hex::encode(txids[1]),
            block_height: Some(10),
            block_position: Some(1),
            embedding_type: EmbeddingType::Witness,
            merkle_sibling_path: Some(path),
            outputs: vec![],
            inputs: vec![],
        };
        refresh_from_fetched(
            db.as_db().as_ref(),
            &config,
            &[(
                "t".into(),
                RegistryIndex {
                    entries: vec![entry],
                },
            )],
            "now",
        )
        .unwrap();

        let resp = decide_download_policy(db.as_db().as_ref(), &config, 10, &root).unwrap();
        assert!(resp.skip_witness);
        assert_eq!(resp.verified_paths, 1);
        assert_eq!(load_stats(db.as_db().as_ref()).skip_witness_blocks, 1);
    }
}
