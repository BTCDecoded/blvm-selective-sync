//! IBD witness stripping for flagged transactions.

use anyhow::Result;
use blvm_node::storage::database::Database;
use blvm_protocol::block::calculate_tx_id;
use blvm_protocol::segwit::Witness;
use blvm_protocol::{Block, Hash};
use std::collections::HashSet;

use crate::apply_policy::block_hash_from_header;
use crate::config::SyncPolicyConfig;
use crate::policy_store::load_merged_entries;
use crate::registry_entry::{EmbeddingType, RegistryEntry};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FilterBlockRequest {
    pub height: u64,
    pub block: Block,
    pub witnesses: Vec<Vec<Witness>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FilterBlockResponse {
    pub block: Block,
    pub witnesses: Vec<Vec<Witness>>,
    pub stripped_txids: Vec<String>,
    pub filtered: bool,
}

pub fn should_strip_entry(config: &SyncPolicyConfig, entry: &RegistryEntry) -> bool {
    match config.witness_mode.as_str() {
        "relaxed" => matches!(entry.embedding_type, EmbeddingType::OpReturn),
        _ => matches!(entry.embedding_type, EmbeddingType::Witness),
    }
}

/// Strip witness stacks for flagged transactions in a block.
pub fn filter_block(
    db: &dyn Database,
    config: &SyncPolicyConfig,
    height: u64,
    block: Block,
    mut witnesses: Vec<Vec<Witness>>,
) -> Result<FilterBlockResponse> {
    let _ = height;
    let entries = load_merged_entries(db)?;
    if entries.is_empty() {
        return Ok(FilterBlockResponse {
            block,
            witnesses,
            stripped_txids: vec![],
            filtered: false,
        });
    }

    let strip_txids: HashSet<String> = entries
        .iter()
        .filter(|stored| should_strip_entry(config, &stored.entry))
        .map(|stored| stored.entry.txid.to_ascii_lowercase())
        .collect();

    if strip_txids.is_empty() {
        return Ok(FilterBlockResponse {
            block,
            witnesses,
            stripped_txids: vec![],
            filtered: false,
        });
    }

    let mut stripped_txids = Vec::new();
    for (i, tx) in block.transactions.iter().enumerate() {
        let txid = hex::encode(calculate_tx_id(tx));
        if !strip_txids.contains(&txid) {
            continue;
        }
        if i < witnesses.len() {
            witnesses[i] = tx.inputs.iter().map(|_| Vec::new()).collect();
        }
        stripped_txids.push(txid);
    }

    let filtered = !stripped_txids.is_empty();
    Ok(FilterBlockResponse {
        block,
        witnesses,
        stripped_txids,
        filtered,
    })
}

pub fn block_hash_for_event(block: &Block) -> Hash {
    block_hash_from_header(&block.header)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::policy_store::refresh_from_fetched;
    use crate::registry_entry::{RegistryEntry, RegistryIndex};
    use blvm_protocol::{
        Block, BlockHeader, OutPoint, Transaction, TransactionInput, TransactionOutput,
    };
    use blvm_sdk::module::ModuleDb;

    fn witness_entry(txid: &str) -> RegistryEntry {
        RegistryEntry {
            txid: txid.to_string(),
            block_height: Some(1),
            block_position: Some(1),
            embedding_type: EmbeddingType::Witness,
            merkle_sibling_path: None,
            outputs: vec![],
            inputs: vec![],
        }
    }

    fn sample_block_with_witnesses() -> (Block, Vec<Vec<Witness>>) {
        let coinbase = Transaction {
            version: 1,
            inputs: vec![TransactionInput {
                prevout: OutPoint {
                    hash: [0; 32],
                    index: 0xffffffff,
                },
                script_sig: vec![0x01, 0x00],
                sequence: 0xffffffff,
            }]
            .into(),
            outputs: vec![TransactionOutput {
                value: 50_0000_0000,
                script_pubkey: vec![0x51],
            }]
            .into(),
            lock_time: 0,
        };
        let flagged = Transaction {
            version: 1,
            inputs: vec![TransactionInput {
                prevout: OutPoint {
                    hash: [1; 32],
                    index: 0,
                },
                script_sig: vec![],
                sequence: 0xffffffff,
            }]
            .into(),
            outputs: vec![TransactionOutput {
                value: 1_000,
                script_pubkey: vec![0x51],
            }]
            .into(),
            lock_time: 0,
        };
        let block = Block {
            header: BlockHeader {
                version: 1,
                prev_block_hash: [0; 32],
                merkle_root: [0; 32],
                timestamp: 1,
                bits: 0x1d00ffff,
                nonce: 0,
            },
            transactions: vec![coinbase, flagged].into(),
        };
        let witnesses = vec![vec![vec![]], vec![vec![vec![0x01, 0x02, 0x03]]]];
        (block, witnesses)
    }

    #[test]
    fn strips_flagged_witness_stack() {
        let (block, witnesses) = sample_block_with_witnesses();
        let flagged_txid = hex::encode(calculate_tx_id(&block.transactions[1]));

        let dir = tempfile::tempdir().unwrap();
        let db = ModuleDb::open(dir.path()).unwrap();
        let config = SyncPolicyConfig {
            witness_mode: "strict".to_string(),
            ..Default::default()
        };
        let index = RegistryIndex {
            entries: vec![witness_entry(&flagged_txid)],
        };
        refresh_from_fetched(
            db.as_db().as_ref(),
            &config,
            &[("test".into(), index)],
            "now",
        )
        .unwrap();

        let response = filter_block(db.as_db().as_ref(), &config, 1, block, witnesses).unwrap();
        assert!(response.filtered);
        assert_eq!(response.stripped_txids, vec![flagged_txid]);
        assert!(response.witnesses[1].iter().all(|stack| stack.is_empty()));
    }

    #[test]
    fn filter_block_request_response_bincode_roundtrip() {
        let (block, witnesses) = sample_block_with_witnesses();
        let req = FilterBlockRequest {
            height: 42,
            block: block.clone(),
            witnesses: witnesses.clone(),
        };
        let bytes = bincode::serialize(&req).expect("serialize request");
        let decoded: FilterBlockRequest =
            bincode::deserialize(&bytes).expect("deserialize request");
        assert_eq!(decoded.height, 42);
        assert_eq!(decoded.block.transactions.len(), block.transactions.len());

        let resp = FilterBlockResponse {
            block,
            witnesses,
            stripped_txids: vec!["aa".repeat(64)],
            filtered: true,
        };
        let bytes = bincode::serialize(&resp).expect("serialize response");
        let decoded: FilterBlockResponse =
            bincode::deserialize(&bytes).expect("deserialize response");
        assert!(decoded.filtered);
        assert_eq!(decoded.stripped_txids.len(), 1);
    }

    #[test]
    fn relaxed_mode_skips_witness_entries() {
        let (block, witnesses) = sample_block_with_witnesses();
        let flagged_txid = hex::encode(calculate_tx_id(&block.transactions[1]));

        let dir = tempfile::tempdir().unwrap();
        let db = ModuleDb::open(dir.path()).unwrap();
        let config = SyncPolicyConfig {
            witness_mode: "relaxed".to_string(),
            ..Default::default()
        };
        let index = RegistryIndex {
            entries: vec![witness_entry(&flagged_txid)],
        };
        refresh_from_fetched(
            db.as_db().as_ref(),
            &config,
            &[("test".into(), index)],
            "now",
        )
        .unwrap();

        let response =
            filter_block(db.as_db().as_ref(), &config, 1, block, witnesses.clone()).unwrap();
        assert!(!response.filtered);
        assert_eq!(response.witnesses, witnesses);
    }
}
