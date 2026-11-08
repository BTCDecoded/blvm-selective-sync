//! ModuleAPI for IBD block filtering, rehydrate, and download policy.

use async_trait::async_trait;
use blvm_node::module::inter_module::api::ModuleAPI;
use blvm_node::module::ipc::protocol::EventPayload;
use blvm_node::module::traits::{EventType, ModuleError, NodeAPI};
use blvm_node::storage::database::Database;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::debug;

use crate::config::SyncPolicyConfig;
use crate::download_policy::{
    DownloadPolicyRequest, DownloadPolicyResponse, decide_download_policy,
};
use crate::ibd_filter::{
    FilterBlockRequest, FilterBlockResponse, block_hash_for_event, filter_block,
};
use crate::rehydrate::{
    CanonicalTxidsRequest, LookupBlockRequest, RehydrateRequest, canonical_txids_for_block,
    lookup_block_for_txids, rehydrate_block,
};

pub const FILTER_BLOCK_BEFORE_STORE: &str = "filter_block_before_store";
pub const REHYDRATE_BLOCK_FOR_CONSENSUS: &str = "rehydrate_block_for_consensus";
pub const GET_CANONICAL_TXIDS: &str = "get_canonical_txids";
pub const LOOKUP_BLOCK_FOR_TXIDS: &str = "lookup_block_for_txids";
pub const FILTER_BLOCK_DOWNLOAD_POLICY: &str = "filter_block_download_policy";

pub struct SelectiveSyncModuleAPI {
    db: Arc<dyn Database>,
    node_api: Arc<dyn NodeAPI>,
    data_dir: PathBuf,
}

impl SelectiveSyncModuleAPI {
    pub fn new(db: Arc<dyn Database>, node_api: Arc<dyn NodeAPI>, data_dir: PathBuf) -> Self {
        Self {
            db,
            node_api,
            data_dir,
        }
    }

    fn load_config(&self) -> SyncPolicyConfig {
        SyncPolicyConfig::load(self.data_dir.join("config.toml")).unwrap_or_default()
    }
}

#[async_trait]
impl ModuleAPI for SelectiveSyncModuleAPI {
    async fn handle_request(
        &self,
        method: &str,
        params: &[u8],
        caller_module_id: &str,
    ) -> Result<Vec<u8>, ModuleError> {
        match method {
            FILTER_BLOCK_BEFORE_STORE => self.handle_filter(params, caller_module_id).await,
            REHYDRATE_BLOCK_FOR_CONSENSUS => self.handle_rehydrate(params),
            GET_CANONICAL_TXIDS => self.handle_canonical_txids(params),
            LOOKUP_BLOCK_FOR_TXIDS => self.handle_lookup(params).await,
            FILTER_BLOCK_DOWNLOAD_POLICY => self.handle_download_policy(params),
            _ => Err(ModuleError::OperationError(format!(
                "Unknown method: {method}"
            ))),
        }
    }

    fn list_methods(&self) -> Vec<String> {
        vec![
            FILTER_BLOCK_BEFORE_STORE.to_string(),
            REHYDRATE_BLOCK_FOR_CONSENSUS.to_string(),
            GET_CANONICAL_TXIDS.to_string(),
            LOOKUP_BLOCK_FOR_TXIDS.to_string(),
            FILTER_BLOCK_DOWNLOAD_POLICY.to_string(),
        ]
    }

    fn api_version(&self) -> u32 {
        1
    }
}

impl SelectiveSyncModuleAPI {
    async fn handle_filter(
        &self,
        params: &[u8],
        caller_module_id: &str,
    ) -> Result<Vec<u8>, ModuleError> {
        let req: FilterBlockRequest = bincode::deserialize(params).map_err(|e| {
            ModuleError::OperationError(format!("Invalid filter_block_before_store params: {e}"))
        })?;

        let config = self.load_config();
        debug!(
            "filter_block_before_store height={} from {}",
            req.height, caller_module_id
        );

        let response = match filter_block(
            self.db.as_ref(),
            &config,
            req.height,
            req.block.clone(),
            req.witnesses.clone(),
        ) {
            Ok(response) => response,
            Err(e) => {
                if config.witness_mode == "relaxed" {
                    FilterBlockResponse {
                        block: req.block,
                        witnesses: req.witnesses,
                        stripped_txids: vec![],
                        filtered: false,
                    }
                } else {
                    return Err(ModuleError::OperationError(format!(
                        "filter_block_before_store failed: {e}"
                    )));
                }
            }
        };

        if response.filtered {
            let block_hash = block_hash_for_event(&response.block);
            let payload = EventPayload::module_event_json(
                "IBDBlockFiltered",
                &serde_json::json!({
                    "block_hash": hex::encode(block_hash),
                    "height": req.height,
                    "reason": "selective_sync",
                }),
            );
            if let Err(e) = self
                .node_api
                .publish_event(EventType::ModuleEvent, payload)
                .await
            {
                tracing::warn!("Failed to publish IBDBlockFiltered: {e}");
            }
            let _ = crate::audit::append_line(
                &config,
                &format!(
                    "strip height={} txs={} witness_mode={}",
                    req.height,
                    response.stripped_txids.len(),
                    config.witness_mode,
                ),
            );
        }

        bincode::serialize(&response).map_err(|e| {
            ModuleError::SerializationError(format!("Failed to serialize response: {e}"))
        })
    }

    fn handle_rehydrate(&self, params: &[u8]) -> Result<Vec<u8>, ModuleError> {
        let req: RehydrateRequest = bincode::deserialize(params).map_err(|e| {
            ModuleError::OperationError(format!(
                "Invalid rehydrate_block_for_consensus params: {e}"
            ))
        })?;
        let response = rehydrate_block(req.block, req.witnesses);
        bincode::serialize(&response).map_err(|e| {
            ModuleError::SerializationError(format!("Failed to serialize response: {e}"))
        })
    }

    fn handle_canonical_txids(&self, params: &[u8]) -> Result<Vec<u8>, ModuleError> {
        let _req: CanonicalTxidsRequest = bincode::deserialize(params).map_err(|e| {
            ModuleError::OperationError(format!("Invalid get_canonical_txids params: {e}"))
        })?;
        bincode::serialize(&canonical_txids_for_block()).map_err(|e| {
            ModuleError::SerializationError(format!("Failed to serialize response: {e}"))
        })
    }

    async fn handle_lookup(&self, params: &[u8]) -> Result<Vec<u8>, ModuleError> {
        let req: LookupBlockRequest = bincode::deserialize(params).map_err(|e| {
            ModuleError::OperationError(format!("Invalid lookup_block_for_txids params: {e}"))
        })?;
        let response = lookup_block_for_txids(self.node_api.as_ref(), self.db.as_ref(), &req.txids)
            .await
            .map_err(|e| {
                ModuleError::OperationError(format!("lookup_block_for_txids failed: {e}"))
            })?;
        bincode::serialize(&response).map_err(|e| {
            ModuleError::SerializationError(format!("Failed to serialize response: {e}"))
        })
    }

    fn handle_download_policy(&self, params: &[u8]) -> Result<Vec<u8>, ModuleError> {
        let req: DownloadPolicyRequest = bincode::deserialize(params).map_err(|e| {
            ModuleError::OperationError(format!("Invalid filter_block_download_policy params: {e}"))
        })?;
        let config = self.load_config();
        let response: DownloadPolicyResponse =
            decide_download_policy(self.db.as_ref(), &config, req.height, &req.merkle_root)
                .unwrap_or(DownloadPolicyResponse {
                    skip_witness: false,
                    verified_paths: 0,
                    bytes_saved_hint: 0,
                });
        bincode::serialize(&response).map_err(|e| {
            ModuleError::SerializationError(format!("Failed to serialize response: {e}"))
        })
    }
}
