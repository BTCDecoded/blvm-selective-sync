//! Consensus helpers for stripped blocks. Payloads are never stored or restored here.

use anyhow::Result;
use blvm_node::module::traits::NodeAPI;
use blvm_node::storage::database::Database;
use blvm_protocol::segwit::Witness;
use blvm_protocol::{Block, Hash};

use crate::apply_policy::block_hash_from_header;
use crate::policy_store::load_merged_entries;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RehydrateRequest {
    pub height: u64,
    pub block_hash: Hash,
    pub block: Block,
    pub witnesses: Vec<Vec<Witness>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RehydrateResponse {
    pub block: Block,
    pub witnesses: Vec<Vec<Witness>>,
    pub canonical_txids: Vec<String>,
    pub found: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CanonicalTxidsRequest {
    pub height: u64,
    pub block_hash: Hash,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CanonicalTxidsResponse {
    pub canonical_txids: Vec<String>,
    pub found: bool,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LookupBlockRequest {
    pub txids: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LookupBlockResponse {
    pub block_hash: Option<Hash>,
    pub height: Option<u64>,
    pub found: bool,
}

/// Module does not keep stripped payloads. `found` is always false.
pub fn rehydrate_block(block: Block, witnesses: Vec<Vec<Witness>>) -> RehydrateResponse {
    RehydrateResponse {
        block,
        witnesses,
        canonical_txids: vec![],
        found: false,
    }
}

pub fn canonical_txids_for_block() -> CanonicalTxidsResponse {
    CanonicalTxidsResponse {
        canonical_txids: vec![],
        found: false,
    }
}

/// Map a policy-flagged txid to its block via registry height + existing node lookup.
pub async fn lookup_block_for_txids(
    node_api: &dyn NodeAPI,
    db: &dyn Database,
    txids: &[String],
) -> Result<LookupBlockResponse> {
    let entries = load_merged_entries(db)?;
    for txid in txids {
        let needle = txid.trim().to_ascii_lowercase();
        let Some(height) = entries
            .iter()
            .find(|stored| stored.entry.txid.to_ascii_lowercase() == needle)
            .and_then(|stored| stored.entry.block_height)
        else {
            continue;
        };
        let Some(block) = node_api
            .get_block_by_height(height)
            .await
            .map_err(|e| anyhow::anyhow!("get_block_by_height: {e}"))?
        else {
            continue;
        };
        return Ok(LookupBlockResponse {
            block_hash: Some(block_hash_from_header(&block.header)),
            height: Some(height),
            found: true,
        });
    }
    Ok(LookupBlockResponse {
        block_hash: None,
        height: None,
        found: false,
    })
}
