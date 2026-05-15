//! Sync-policy module: unified CLI via #[module] macro.

const ON_CHAIN_REGISTRY_TREE: &str = "on_chain_registry";

use blvm_protocol::spam_filter::SpamFilterPreset;
use blvm_sdk::module::prelude::*;
use blvm_sdk_macros::module;

use crate::config::SyncPolicyConfig;
use crate::registry_entry::{build_registry_from_block, infer_embedding_type, EmbeddingType};

/// Sync-policy module: CLI + event handler in one struct.
#[derive(Clone)]
pub struct SyncPolicyModule;

#[module]
impl SyncPolicyModule {
    #[command]
    fn list(&self, ctx: &InvocationContext) -> Result<String, ModuleError> {
        let (stdout, stderr, code) =
            crate::cli::run_sync_policy_capture(crate::cli::SyncPolicyCommand::List, Some(ctx))
                .map_err(|e| ModuleError::Other(e.to_string()))?;
        if code != 0 {
            return Err(ModuleError::Other(if stderr.is_empty() {
                stdout
            } else {
                stderr
            }));
        }
        Ok(stdout)
    }

    #[command]
    fn subscribe(&self, ctx: &InvocationContext, url: String) -> Result<String, ModuleError> {
        let (stdout, stderr, code) = crate::cli::run_sync_policy_capture(
            crate::cli::SyncPolicyCommand::Subscribe { url },
            Some(ctx),
        )
        .map_err(|e| ModuleError::Other(e.to_string()))?;
        if code != 0 {
            return Err(ModuleError::Other(if stderr.is_empty() {
                stdout
            } else {
                stderr
            }));
        }
        Ok(stdout)
    }

    #[command]
    fn unsubscribe(&self, ctx: &InvocationContext, url: String) -> Result<String, ModuleError> {
        let (stdout, stderr, code) = crate::cli::run_sync_policy_capture(
            crate::cli::SyncPolicyCommand::Unsubscribe { url },
            Some(ctx),
        )
        .map_err(|e| ModuleError::Other(e.to_string()))?;
        if code != 0 {
            return Err(ModuleError::Other(if stderr.is_empty() {
                stdout
            } else {
                stderr
            }));
        }
        Ok(stdout)
    }

    #[command]
    fn refresh(&self, ctx: &InvocationContext) -> Result<String, ModuleError> {
        let (stdout, stderr, code) =
            crate::cli::run_sync_policy_capture(crate::cli::SyncPolicyCommand::Refresh, Some(ctx))
                .map_err(|e| ModuleError::Other(e.to_string()))?;
        if code != 0 {
            return Err(ModuleError::Other(if stderr.is_empty() {
                stdout
            } else {
                stderr
            }));
        }
        Ok(stdout)
    }

    #[command]
    fn status(&self, _ctx: &InvocationContext) -> Result<String, ModuleError> {
        let (stdout, stderr, code) =
            crate::cli::run_sync_policy_capture(crate::cli::SyncPolicyCommand::Status, None)
                .map_err(|e| ModuleError::Other(e.to_string()))?;
        if code != 0 {
            return Err(ModuleError::Other(if stderr.is_empty() {
                stdout
            } else {
                stderr
            }));
        }
        Ok(stdout)
    }

    #[command]
    fn build_entry(
        &self,
        ctx: &InvocationContext,
        tx_hex_or_txid: String,
        embedding: Option<String>,
    ) -> Result<String, ModuleError> {
        let tx_hex = crate::cli::resolve_tx_hex(ctx, &tx_hex_or_txid)
            .map_err(|e| ModuleError::Other(e.to_string()))?;
        let embedding = match embedding.as_deref().unwrap_or("witness") {
            "auto" => {
                infer_embedding_type(&tx_hex).map_err(|e| ModuleError::Other(e.to_string()))?
            }
            "witness" => EmbeddingType::Witness,
            "op_return" => EmbeddingType::OpReturn,
            _ => {
                return Err(ModuleError::Other(
                    "embedding must be 'witness', 'op_return', or 'auto'".into(),
                ))
            }
        };
        let (stdout, stderr, code) = crate::cli::run_sync_policy_capture(
            crate::cli::SyncPolicyCommand::BuildEntry {
                tx_hex,
                embedding,
                block_height: None,
                block_position: None,
            },
            None,
        )
        .map_err(|e| ModuleError::Other(e.to_string()))?;
        if code != 0 {
            return Err(ModuleError::Other(if stderr.is_empty() {
                stdout
            } else {
                stderr
            }));
        }
        Ok(stdout)
    }

    #[command]
    fn config_path(&self, _ctx: &InvocationContext) -> Result<String, ModuleError> {
        let (stdout, stderr, code) =
            crate::cli::run_sync_policy_capture(crate::cli::SyncPolicyCommand::ConfigPath, None)
                .map_err(|e| ModuleError::Other(e.to_string()))?;
        if code != 0 {
            return Err(ModuleError::Other(if stderr.is_empty() {
                stdout
            } else {
                stderr
            }));
        }
        Ok(stdout)
    }

    #[command]
    fn export_registry(
        &self,
        _ctx: &InvocationContext,
        output_path: Option<String>,
    ) -> Result<String, ModuleError> {
        let (stdout, stderr, code) = crate::cli::run_sync_policy_capture(
            crate::cli::SyncPolicyCommand::ExportRegistry { output_path },
            None,
        )
        .map_err(|e| ModuleError::Other(e.to_string()))?;
        if code != 0 {
            return Err(ModuleError::Other(if stderr.is_empty() {
                stdout
            } else {
                stderr
            }));
        }
        Ok(stdout)
    }

    #[command]
    fn build_registry(
        &self,
        _ctx: &InvocationContext,
        block_hex_or_path: String,
        preset: Option<String>,
        block_height: Option<u64>,
        output_path: Option<String>,
    ) -> Result<String, ModuleError> {
        let preset = match preset.as_deref().unwrap_or("moderate") {
            "conservative" => SpamFilterPreset::Conservative,
            "moderate" => SpamFilterPreset::Moderate,
            "aggressive" => SpamFilterPreset::Aggressive,
            _ => {
                return Err(ModuleError::Other(
                    "preset must be 'conservative', 'moderate', or 'aggressive'".into(),
                ))
            }
        };
        let (stdout, stderr, code) = crate::cli::run_sync_policy_capture(
            crate::cli::SyncPolicyCommand::BuildRegistry {
                block_hex_or_path,
                preset,
                block_height,
                output_path,
            },
            None,
        )
        .map_err(|e| ModuleError::Other(e.to_string()))?;
        if code != 0 {
            return Err(ModuleError::Other(if stderr.is_empty() {
                stdout
            } else {
                stderr
            }));
        }
        Ok(stdout)
    }

    #[on_event(NewBlock)]
    async fn on_new_block(
        &self,
        event: &blvm_node::module::ipc::protocol::EventMessage,
        ctx: &InvocationContext,
    ) -> Result<(), ModuleError> {
        let (block_hash, height) = match &event.payload {
            blvm_node::module::ipc::protocol::EventPayload::NewBlock {
                block_hash, height, ..
            } => (block_hash, *height),
            _ => return Ok(()),
        };
        let config = SyncPolicyConfig::load(SyncPolicyConfig::config_path()).unwrap_or_default();
        if !config.on_chain_registry_builder {
            return Ok(());
        }
        let node_api = match ctx.node_api() {
            Some(api) => api,
            None => return Ok(()),
        };
        let block = match node_api.get_block_by_height(height).await {
            Ok(Some(b)) => b,
            _ => return Ok(()),
        };
        let empty_witnesses: Vec<Vec<blvm_protocol::segwit::Witness>> =
            block.transactions.iter().map(|_| Vec::new()).collect();
        let block_bytes = blvm_protocol::serialization::block::serialize_block_with_witnesses(
            &block,
            &empty_witnesses,
            false,
        );
        let block_hex = hex::encode(block_bytes);
        let preset = SpamFilterPreset::Moderate;
        let index = match build_registry_from_block(&block_hex, Some(height), preset) {
            Ok(idx) => idx,
            Err(_) => return Ok(()),
        };
        if index.entries.is_empty() {
            return Ok(());
        }
        let db = ctx.db();
        if let Ok(tree) = db.open_tree(ON_CHAIN_REGISTRY_TREE) {
            for entry in &index.entries {
                let key = format!("{}:{}", height, entry.txid);
                if let Ok(serialized) = serde_json::to_vec(entry) {
                    let _ = tree.insert(key.as_bytes(), &serialized);
                }
            }
        }
        tracing::debug!(
            "On-chain registry: height {} added {} entries",
            height,
            index.entries.len()
        );
        Ok(())
    }
}
