//! ModuleAPI for IBD block filtering.

use async_trait::async_trait;
use blvm_node::module::inter_module::api::ModuleAPI;
use blvm_node::module::ipc::protocol::EventPayload;
use blvm_node::module::traits::{EventType, ModuleError, NodeAPI};
use blvm_node::storage::database::Database;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::debug;

use crate::config::SyncPolicyConfig;
use crate::ibd_filter::{
    FilterBlockRequest, FilterBlockResponse, block_hash_for_event, filter_block,
};

pub const FILTER_BLOCK_BEFORE_STORE: &str = "filter_block_before_store";

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
        if method != FILTER_BLOCK_BEFORE_STORE {
            return Err(ModuleError::OperationError(format!(
                "Unknown method: {method}"
            )));
        }

        let req: FilterBlockRequest = bincode::deserialize(params).map_err(|e| {
            ModuleError::OperationError(format!("Invalid filter_block_before_store params: {e}"))
        })?;

        let config = self.load_config();
        if !config.ibd_filter_enabled {
            let response = FilterBlockResponse {
                block: req.block,
                witnesses: req.witnesses,
                stripped_txids: vec![],
                filtered: false,
            };
            return bincode::serialize(&response).map_err(|e| {
                ModuleError::SerializationError(format!("Failed to serialize response: {e}"))
            });
        }

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
            let payload = EventPayload::IBDBlockFiltered {
                block_hash,
                height: req.height,
                reason: "selective_sync".to_string(),
            };
            if let Err(e) = self
                .node_api
                .publish_event(EventType::IBDBlockFiltered, payload)
                .await
            {
                tracing::warn!("Failed to publish IBDBlockFiltered: {e}");
            }
        }

        Ok(bincode::serialize(&response).map_err(|e| {
            ModuleError::SerializationError(format!("Failed to serialize response: {e}"))
        })?)
    }

    fn list_methods(&self) -> Vec<String> {
        vec![FILTER_BLOCK_BEFORE_STORE.to_string()]
    }

    fn api_version(&self) -> u32 {
        1
    }
}
