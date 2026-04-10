//! Push block serve denylist entries to the node via [`NodeAPI::merge_block_serve_denylist`].
//!
//! Policy for *which* blocks to deny stays in this module (and IBD integration); the node only
//! stores the merged set and applies it when answering `getdata`.

use blvm_node::module::traits::{ModuleError, NodeAPI};
use blvm_protocol::Hash;

/// Merge block hashes into the node's full-block serve denylist.
pub async fn merge_block_serve_denylist(
    node_api: &dyn NodeAPI,
    block_hashes: &[Hash],
) -> Result<(), ModuleError> {
    node_api.merge_block_serve_denylist(block_hashes).await
}
