//! Apply merged policy to node P2P serve denylists.

use anyhow::{Context, Result};
use blvm_node::module::traits::NodeAPI;
use blvm_node::storage::database::Database;
use blvm_protocol::{BlockHeader, Hash};

use crate::audit;
use crate::config::SyncPolicyConfig;
use crate::policy_store::load_merged_entries;
use crate::registry_entry::EmbeddingType;
use crate::withheld::{merge_block_serve_denylist, merge_tx_serve_denylist};

#[derive(Debug, Clone, Default)]
pub struct ApplyReport {
    pub tx_count: usize,
    pub block_count: usize,
}

/// Push merged policy entries to the node serve denylists.
pub async fn apply_policy(
    node_api: &dyn NodeAPI,
    db: &dyn Database,
    config: &SyncPolicyConfig,
) -> Result<ApplyReport> {
    let entries = load_merged_entries(db)?;
    let mut tx_hashes = Vec::new();
    let mut block_hashes = Vec::new();

    for stored in &entries {
        if let Ok(hash) = txid_to_hash(&stored.entry.txid) {
            tx_hashes.push(hash);
        }
        if should_deny_block(config, &stored.entry) {
            if let Some(height) = stored.entry.block_height {
                if let Ok(Some(block)) = node_api.get_block_by_height(height).await {
                    let hash = block_hash_from_header(&block.header);
                    if !block_hashes.contains(&hash) {
                        block_hashes.push(hash);
                    }
                }
            }
        }
    }

    if !tx_hashes.is_empty() {
        merge_tx_serve_denylist(node_api, &tx_hashes)
            .await
            .map_err(|e| anyhow::anyhow!("merge_tx_serve_denylist: {e}"))?;
    }
    if !block_hashes.is_empty() {
        merge_block_serve_denylist(node_api, &block_hashes)
            .await
            .map_err(|e| anyhow::anyhow!("merge_block_serve_denylist: {e}"))?;
    }

    let report = ApplyReport {
        tx_count: tx_hashes.len(),
        block_count: block_hashes.len(),
    };

    let _ = audit::append_line(
        config,
        &format!(
            "apply_policy tx={} blocks={} entries={}",
            report.tx_count,
            report.block_count,
            entries.len()
        ),
    );

    Ok(report)
}

fn should_deny_block(
    config: &SyncPolicyConfig,
    entry: &crate::registry_entry::RegistryEntry,
) -> bool {
    match config.witness_mode.as_str() {
        "relaxed" => matches!(entry.embedding_type, EmbeddingType::OpReturn),
        _ => true,
    }
}

fn txid_to_hash(txid_hex: &str) -> Result<Hash> {
    let bytes = hex::decode(txid_hex.trim()).context("Invalid txid hex")?;
    if bytes.len() != 32 {
        anyhow::bail!("txid must be 32 bytes");
    }
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&bytes);
    Ok(Hash::from(hash))
}

pub(crate) fn block_hash_from_header(header: &BlockHeader) -> Hash {
    use blvm_node::storage::hashing::double_sha256;

    let mut bytes = Vec::with_capacity(80);
    bytes.extend_from_slice(&header.version.to_le_bytes());
    bytes.extend_from_slice(&header.prev_block_hash);
    bytes.extend_from_slice(&header.merkle_root);
    bytes.extend_from_slice(&header.timestamp.to_le_bytes());
    bytes.extend_from_slice(&header.bits.to_le_bytes());
    bytes.extend_from_slice(&header.nonce.to_le_bytes());
    Hash::from(double_sha256(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry_entry::{EmbeddingType, RegistryEntry};

    #[test]
    fn relaxed_mode_skips_witness_block_deny() {
        let config = SyncPolicyConfig {
            witness_mode: "relaxed".to_string(),
            ..Default::default()
        };
        let witness = RegistryEntry {
            txid: "c".repeat(64),
            block_height: Some(1),
            block_position: None,
            embedding_type: EmbeddingType::Witness,
            merkle_sibling_path: None,
            outputs: vec![],
            inputs: vec![],
        };
        assert!(!should_deny_block(&config, &witness));
        let op = RegistryEntry {
            embedding_type: EmbeddingType::OpReturn,
            ..witness
        };
        assert!(should_deny_block(&config, &op));
    }
}
