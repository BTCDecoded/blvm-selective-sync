//! Sibling-path merkle proofs for registry entries (feeds v3 selective download).

use anyhow::{Context, Result};
use blvm_node::storage::hashing::double_sha256;
use blvm_protocol::Hash;

/// Build the sibling hashes from `leaf_index` to the merkle root.
pub fn sibling_path(txids: &[Hash], leaf_index: usize) -> Result<Vec<String>> {
    if txids.is_empty() {
        anyhow::bail!("Cannot build merkle path for empty txid list");
    }
    if leaf_index >= txids.len() {
        anyhow::bail!("leaf_index {leaf_index} out of range ({})", txids.len());
    }

    let mut level = txids.to_vec();
    let mut idx = leaf_index;
    let mut path = Vec::new();

    while level.len() > 1 {
        if level.len() % 2 == 1 {
            level.push(*level.last().expect("non-empty"));
        }
        let sibling = if idx % 2 == 0 {
            level[idx + 1]
        } else {
            level[idx - 1]
        };
        path.push(hex::encode(sibling));

        let mut next = Vec::with_capacity(level.len() / 2);
        for pair in level.chunks(2) {
            next.push(parent_hash(&pair[0], &pair[1]));
        }
        idx /= 2;
        level = next;
    }
    Ok(path)
}

/// Verify `txid` at `leaf_index` against `merkle_root` using hex sibling hashes.
pub fn verify_sibling_path(
    txid: &Hash,
    leaf_index: usize,
    path: &[String],
    merkle_root: &Hash,
) -> Result<bool> {
    let mut hash = *txid;
    let mut idx = leaf_index;
    for sibling_hex in path {
        let sibling_bytes = hex::decode(sibling_hex.trim()).context("Invalid sibling hex")?;
        if sibling_bytes.len() != 32 {
            anyhow::bail!("Sibling hash must be 32 bytes");
        }
        let mut sibling = [0u8; 32];
        sibling.copy_from_slice(&sibling_bytes);
        hash = if idx % 2 == 0 {
            parent_hash(&hash, &sibling)
        } else {
            parent_hash(&sibling, &hash)
        };
        idx /= 2;
    }
    Ok(&hash == merkle_root)
}

fn parent_hash(left: &Hash, right: &Hash) -> Hash {
    let mut combined = [0u8; 64];
    combined[..32].copy_from_slice(left);
    combined[32..].copy_from_slice(right);
    double_sha256(&combined)
}

#[cfg(test)]
mod tests {
    use super::*;
    use blvm_protocol::mining::calculate_merkle_root_from_tx_ids;

    #[test]
    fn sibling_path_verifies_against_root() {
        let txids: Vec<Hash> = (0u8..5).map(|i| [i; 32]).collect();
        let root = calculate_merkle_root_from_tx_ids(&txids).expect("root");
        for i in 0..txids.len() {
            let path = sibling_path(&txids, i).expect("path");
            assert!(
                verify_sibling_path(&txids[i], i, &path, &root).expect("verify"),
                "path failed at index {i}"
            );
        }
    }

    #[test]
    fn wrong_leaf_fails() {
        let txids: Vec<Hash> = (0u8..3).map(|i| [i; 32]).collect();
        let root = calculate_merkle_root_from_tx_ids(&txids).expect("root");
        let path = sibling_path(&txids, 0).expect("path");
        assert!(!verify_sibling_path(&txids[1], 0, &path, &root).expect("verify"));
    }
}
