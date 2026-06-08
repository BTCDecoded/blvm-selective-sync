//! Push serve denylist entries to the node via [`NodeAPI`].

use blvm_node::module::traits::{
    BlockServeDenylistSnapshot, ModuleError, NodeAPI, TxServeDenylistSnapshot,
};
use blvm_protocol::Hash;

/// Merge block hashes into the node's full-block serve denylist.
pub async fn merge_block_serve_denylist(
    node_api: &dyn NodeAPI,
    block_hashes: &[Hash],
) -> Result<(), ModuleError> {
    if block_hashes.is_empty() {
        return Ok(());
    }
    node_api.merge_block_serve_denylist(block_hashes).await
}

/// Merge txids into the node's full-transaction serve denylist.
pub async fn merge_tx_serve_denylist(
    node_api: &dyn NodeAPI,
    tx_hashes: &[Hash],
) -> Result<(), ModuleError> {
    if tx_hashes.is_empty() {
        return Ok(());
    }
    node_api.merge_tx_serve_denylist(tx_hashes).await
}

/// Bounded snapshot of block serve denylist.
pub async fn get_block_serve_denylist_snapshot(
    node_api: &dyn NodeAPI,
) -> Result<BlockServeDenylistSnapshot, ModuleError> {
    node_api.get_block_serve_denylist_snapshot().await
}

/// Bounded snapshot of tx serve denylist.
pub async fn get_tx_serve_denylist_snapshot(
    node_api: &dyn NodeAPI,
) -> Result<TxServeDenylistSnapshot, ModuleError> {
    node_api.get_tx_serve_denylist_snapshot().await
}
