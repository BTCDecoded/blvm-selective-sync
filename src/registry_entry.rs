//! Build registry entry from transaction (for registry operators).
//!
//! Used by `blvm sync-policy build-entry` and `build-registry`.

use anyhow::{Context, Result};
use blvm_protocol::block::calculate_tx_id;
use blvm_protocol::serialization::block::deserialize_block_with_witnesses;
use blvm_protocol::serialization::transaction::deserialize_transaction_with_witness;
use blvm_protocol::spam_filter::{SpamFilter, SpamFilterPreset, SpamType};
use serde::{Deserialize, Serialize};

/// OP_RETURN opcode
const OP_RETURN: u8 = 0x6a;

/// Registry entry schema (Section 4.2 of design doc).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryEntry {
    pub txid: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_height: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_position: Option<u32>,
    pub embedding_type: EmbeddingType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub merkle_sibling_path: Option<Vec<String>>,
    pub outputs: Vec<OutputStub>,
    pub inputs: Vec<InputStub>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmbeddingType {
    Witness,
    OpReturn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputStub {
    pub index: u32,
    pub value: i64,
    pub script_pubkey: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InputStub {
    pub txid: String,
    pub vout: u32,
}

/// Infer embedding type from transaction using SpamFilter (for build-entry --auto).
///
/// Maps: Ordinals/LargeWitness → Witness; BRC20/OP_RETURN>80 bytes → OpReturn.
/// Defaults to Witness when inference is ambiguous.
pub fn infer_embedding_type(tx_hex: &str) -> Result<EmbeddingType> {
    let tx_bytes = hex::decode(tx_hex.trim().trim_start_matches("0x"))
        .context("Invalid hex: expected hex-encoded transaction")?;

    let (tx, witnesses, _) =
        deserialize_transaction_with_witness(&tx_bytes).context("Failed to parse transaction")?;

    let filter = SpamFilter::with_preset(SpamFilterPreset::Moderate);
    let witnesses_slice = witnesses.as_slice();
    let result = filter.is_spam_with_witness(&tx, Some(witnesses_slice), None);

    // Witness-based: Ordinals, LargeWitness
    for t in &result.detected_types {
        if matches!(t, SpamType::Ordinals | SpamType::LargeWitness) {
            return Ok(EmbeddingType::Witness);
        }
    }

    // OP_RETURN-based: BRC20 or explicit OP_RETURN > 80 bytes
    if result
        .detected_types
        .iter()
        .any(|t| matches!(t, SpamType::BRC20))
    {
        return Ok(EmbeddingType::OpReturn);
    }
    if has_large_op_return(&tx) {
        return Ok(EmbeddingType::OpReturn);
    }

    // Default: witness (most common for inscriptions)
    Ok(EmbeddingType::Witness)
}

/// Check if any output has OP_RETURN with data > 80 bytes (Stamps/legacy pattern).
fn has_large_op_return(tx: &blvm_protocol::Transaction) -> bool {
    for out in &tx.outputs {
        let script = &out.script_pubkey;
        if script.len() > 82 && script[0] == OP_RETURN {
            // OP_RETURN (1) + push opcode (1) + length byte for OP_PUSHDATA1 (1) + 80 bytes = 83 min
            let data_len = match script.get(1) {
                Some(0x4c) => script.get(2).copied().unwrap_or(0) as usize, // OP_PUSHDATA1
                Some(0x4d) => u16::from_le_bytes([
                    script.get(2).copied().unwrap_or(0),
                    script.get(3).copied().unwrap_or(0),
                ]) as usize, // OP_PUSHDATA2
                Some(0x4e) => u32::from_le_bytes([
                    script.get(2).copied().unwrap_or(0),
                    script.get(3).copied().unwrap_or(0),
                    script.get(4).copied().unwrap_or(0),
                    script.get(5).copied().unwrap_or(0),
                ]) as usize, // OP_PUSHDATA4
                Some(n) if *n <= 75 => *n as usize,                         // direct push
                _ => 0,
            };
            if data_len > 80 {
                return true;
            }
        }
    }
    false
}

/// Build a registry entry from raw transaction hex.
///
/// For witness path: extracts txid (no witness), inputs, outputs.
/// Merkle sibling path must be provided separately (requires block context).
pub fn build_registry_entry(
    tx_hex: &str,
    embedding_type: EmbeddingType,
    block_height: Option<u64>,
    block_position: Option<u32>,
) -> Result<RegistryEntry> {
    let tx_bytes = hex::decode(tx_hex.trim().trim_start_matches("0x"))
        .context("Invalid hex: expected hex-encoded transaction")?;

    let (tx, _witnesses, _consumed) =
        deserialize_transaction_with_witness(&tx_bytes).context("Failed to parse transaction")?;

    let txid = calculate_tx_id(&tx);
    let txid_hex = hex::encode(txid);

    let outputs: Vec<OutputStub> = tx
        .outputs
        .iter()
        .enumerate()
        .map(|(i, out)| OutputStub {
            index: i as u32,
            value: out.value,
            script_pubkey: hex::encode(&out.script_pubkey),
        })
        .collect();

    let inputs: Vec<InputStub> = tx
        .inputs
        .iter()
        .map(|inp| InputStub {
            txid: hex::encode(inp.prevout.hash),
            vout: inp.prevout.index,
        })
        .collect();

    Ok(RegistryEntry {
        txid: txid_hex,
        block_height,
        block_position,
        embedding_type,
        merkle_sibling_path: None, // Caller must provide if needed
        outputs,
        inputs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use blvm_protocol::serialization::transaction::serialize_transaction;
    use blvm_protocol::types::{OutPoint, Transaction, TransactionInput, TransactionOutput};

    fn minimal_tx_hex() -> String {
        let tx = Transaction {
            version: 1,
            inputs: vec![TransactionInput {
                prevout: OutPoint {
                    hash: [1u8; 32].into(),
                    index: 0,
                },
                script_sig: vec![0x51].into(),
                sequence: 0xffffffff,
            }]
            .into(),
            outputs: vec![TransactionOutput {
                value: 5000000000,
                script_pubkey: vec![0x51].into(),
            }]
            .into(),
            lock_time: 0,
        };
        hex::encode(serialize_transaction(&tx))
    }

    #[test]
    fn test_infer_embedding_type_valid_tx() {
        let tx_hex = minimal_tx_hex();
        let result = infer_embedding_type(&tx_hex).expect("infer");
        assert!(matches!(
            result,
            EmbeddingType::Witness | EmbeddingType::OpReturn
        ));
    }

    #[test]
    fn test_infer_embedding_type_invalid_hex() {
        assert!(infer_embedding_type("nothex").is_err());
    }
}

/// Registry index output (array of entries for build-registry).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistryIndex {
    pub entries: Vec<RegistryEntry>,
}

/// Build a registry index from a block: run SpamFilter on all txs, output entries for flagged txs.
///
/// Skips coinbase. Uses SpamFilter preset for detection. For each flagged tx, infers embedding
/// type and builds RegistryEntry.
pub fn build_registry_from_block(
    block_hex: &str,
    block_height: Option<u64>,
    preset: SpamFilterPreset,
) -> Result<RegistryIndex> {
    let block_bytes = hex::decode(block_hex.trim().trim_start_matches("0x"))
        .context("Invalid hex: expected hex-encoded block")?;

    let (block, witnesses) =
        deserialize_block_with_witnesses(&block_bytes).context("Failed to parse block")?;

    let filter = SpamFilter::with_preset(preset);
    let mut entries = Vec::new();

    for (tx_index, tx) in block.transactions.iter().enumerate() {
        // Skip coinbase
        if tx.inputs.is_empty() || tx.inputs[0].prevout.hash == [0u8; 32] {
            continue;
        }

        let witnesses_for_tx = witnesses.get(tx_index).map(|w| w.as_slice());
        let result = filter.is_spam_with_witness(tx, witnesses_for_tx, None);

        if !result.is_spam {
            continue;
        }

        let embedding = if result
            .detected_types
            .iter()
            .any(|t| matches!(t, SpamType::Ordinals | SpamType::LargeWitness))
        {
            EmbeddingType::Witness
        } else if result
            .detected_types
            .iter()
            .any(|t| matches!(t, SpamType::BRC20))
            || has_large_op_return(tx)
        {
            EmbeddingType::OpReturn
        } else {
            EmbeddingType::Witness
        };

        let txid = calculate_tx_id(tx);
        let txid_hex = hex::encode(txid);

        let outputs: Vec<OutputStub> = tx
            .outputs
            .iter()
            .enumerate()
            .map(|(i, out)| OutputStub {
                index: i as u32,
                value: out.value,
                script_pubkey: hex::encode(&out.script_pubkey),
            })
            .collect();

        let inputs: Vec<InputStub> = tx
            .inputs
            .iter()
            .map(|inp| InputStub {
                txid: hex::encode(inp.prevout.hash),
                vout: inp.prevout.index,
            })
            .collect();

        entries.push(RegistryEntry {
            txid: txid_hex,
            block_height,
            block_position: Some(tx_index as u32),
            embedding_type: embedding,
            merkle_sibling_path: None,
            outputs,
            inputs,
        });
    }

    Ok(RegistryIndex { entries })
}
