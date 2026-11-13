//! IBD witness stripping for flagged transactions.
//!
//! An OP_RETURN script is committed by the txid and the header merkle root.
//! Rewriting it used to store a body that no longer matches the header.

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

pub fn should_strip_witness(config: &SyncPolicyConfig, entry: &RegistryEntry) -> bool {
    if config.witness_mode == "relaxed" {
        return false;
    }
    matches!(entry.embedding_type, EmbeddingType::Witness)
}

pub fn should_strip_op_return(_config: &SyncPolicyConfig, entry: &RegistryEntry) -> bool {
    matches!(entry.embedding_type, EmbeddingType::OpReturn)
}

pub fn should_strip_entry(config: &SyncPolicyConfig, entry: &RegistryEntry) -> bool {
    should_strip_witness(config, entry) || should_strip_op_return(config, entry)
}

/// Drop witness stacks for flagged transactions. Transaction bytes stay.
pub fn filter_block(
    db: &dyn Database,
    config: &SyncPolicyConfig,
    _height: u64,
    block: Block,
    mut witnesses: Vec<Vec<Witness>>,
) -> Result<FilterBlockResponse> {
    let canonical_txids: Vec<String> = block
        .transactions
        .iter()
        .map(|tx| hex::encode(calculate_tx_id(tx)).to_ascii_lowercase())
        .collect();

    let entries = load_merged_entries(db)?;
    let witness_txids: HashSet<String> = entries
        .iter()
        .filter(|stored| should_strip_witness(config, &stored.entry))
        .map(|stored| stored.entry.txid.to_ascii_lowercase())
        .collect();

    if witness_txids.is_empty() {
        return Ok(FilterBlockResponse {
            block,
            witnesses,
            stripped_txids: vec![],
            filtered: false,
        });
    }

    let mut stripped_txids = Vec::new();

    for (i, tx) in block.transactions.iter().enumerate() {
        let txid = &canonical_txids[i];
        if witness_txids.contains(txid) && i < witnesses.len() {
            witnesses[i] = tx.inputs.iter().map(|_| Vec::new()).collect();
            stripped_txids.push(txid.clone());
        }
    }

    Ok(FilterBlockResponse {
        block,
        witnesses,
        stripped_txids: stripped_txids.clone(),
        filtered: !stripped_txids.is_empty(),
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
    use blvm_protocol::mining::calculate_merkle_root_from_tx_ids;
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

    fn opreturn_entry(txid: &str) -> RegistryEntry {
        RegistryEntry {
            txid: txid.to_string(),
            block_height: Some(1),
            block_position: Some(1),
            embedding_type: EmbeddingType::OpReturn,
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
            outputs: vec![
                TransactionOutput {
                    value: 0,
                    script_pubkey: vec![0x6a, 0x04, b't', b'e', b's', b't'],
                },
                TransactionOutput {
                    value: 1_000,
                    script_pubkey: vec![0x51],
                },
            ]
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

    fn store_index(db: &dyn Database, config: &SyncPolicyConfig, entries: Vec<RegistryEntry>) {
        let index = RegistryIndex { entries };
        refresh_from_fetched(db, config, &[("test".into(), index)], "now").unwrap();
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
        store_index(
            db.as_db().as_ref(),
            &config,
            vec![witness_entry(&flagged_txid)],
        );

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
        store_index(
            db.as_db().as_ref(),
            &config,
            vec![witness_entry(&flagged_txid)],
        );

        let response =
            filter_block(db.as_db().as_ref(), &config, 1, block, witnesses.clone()).unwrap();
        assert!(!response.filtered);
        assert_eq!(response.witnesses, witnesses);
    }

    #[test]
    fn op_return_script_stays_because_the_txid_commits_to_it() {
        let (block, witnesses) = sample_block_with_witnesses();
        let flagged_txid = hex::encode(calculate_tx_id(&block.transactions[1]));
        let dir = tempfile::tempdir().unwrap();
        let db = ModuleDb::open(dir.path()).unwrap();
        let config = SyncPolicyConfig {
            witness_mode: "strict".to_string(),
            ..Default::default()
        };
        store_index(
            db.as_db().as_ref(),
            &config,
            vec![opreturn_entry(&flagged_txid)],
        );

        let original_script = block.transactions[1].outputs[0].script_pubkey.clone();
        let original_txid = flagged_txid.clone();
        let response =
            filter_block(db.as_db().as_ref(), &config, 1, block, witnesses.clone()).unwrap();
        assert!(!response.filtered);
        assert_eq!(
            response.block.transactions[1].outputs[0].script_pubkey,
            original_script
        );
        assert_eq!(
            hex::encode(calculate_tx_id(&response.block.transactions[1])),
            original_txid
        );
        assert_eq!(response.witnesses, witnesses);
    }

    #[test]
    fn relaxed_mode_keeps_op_return_and_witness() {
        let (block, witnesses) = sample_block_with_witnesses();
        let flagged_txid = hex::encode(calculate_tx_id(&block.transactions[1]));
        let dir = tempfile::tempdir().unwrap();
        let db = ModuleDb::open(dir.path()).unwrap();
        let config = SyncPolicyConfig {
            witness_mode: "relaxed".to_string(),
            ..Default::default()
        };
        store_index(
            db.as_db().as_ref(),
            &config,
            vec![opreturn_entry(&flagged_txid)],
        );

        let original_script = block.transactions[1].outputs[0].script_pubkey.clone();
        let response =
            filter_block(db.as_db().as_ref(), &config, 1, block, witnesses.clone()).unwrap();
        assert!(!response.filtered);
        assert_eq!(
            response.block.transactions[1].outputs[0].script_pubkey,
            original_script
        );
        assert_eq!(response.witnesses, witnesses);

        store_index(
            db.as_db().as_ref(),
            &config,
            vec![witness_entry(&flagged_txid)],
        );
        let (block2, witnesses2) = sample_block_with_witnesses();
        let response2 =
            filter_block(db.as_db().as_ref(), &config, 1, block2, witnesses2.clone()).unwrap();
        assert!(!response2.filtered);
        assert_eq!(response2.witnesses, witnesses2);
    }

    #[test]
    fn stripped_block_merkle_uses_pre_strip_txids_in_memory() {
        let (block, witnesses) = sample_block_with_witnesses();
        let flagged_txid = hex::encode(calculate_tx_id(&block.transactions[1]));
        let canonical: Vec<[u8; 32]> = block.transactions.iter().map(calculate_tx_id).collect();
        let root = calculate_merkle_root_from_tx_ids(&canonical).unwrap();

        let dir = tempfile::tempdir().unwrap();
        let db = ModuleDb::open(dir.path()).unwrap();
        let config = SyncPolicyConfig {
            witness_mode: "strict".to_string(),
            ..Default::default()
        };
        store_index(
            db.as_db().as_ref(),
            &config,
            vec![witness_entry(&flagged_txid)],
        );

        let response = filter_block(db.as_db().as_ref(), &config, 1, block, witnesses).unwrap();
        assert!(response.witnesses[1].iter().all(|stack| stack.is_empty()));
        assert_eq!(calculate_merkle_root_from_tx_ids(&canonical).unwrap(), root);
    }
}
