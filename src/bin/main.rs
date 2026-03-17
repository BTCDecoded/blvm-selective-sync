//! Selective-sync module binary.
//!
//! Runtime-loadable module for blvm-node. Registers "sync-policy" CLI and handles
//! invocations (list, subscribe, unsubscribe, refresh, status, build-entry, config-path).
//!
//! When spawned by the node: reads MODULE_ID, SOCKET_PATH, DATA_DIR from env.
//! For manual testing: selective-sync --module-id <id> --socket-path <path> --data-dir <dir>
//!
//! When loaded, `blvm sync-policy list` (etc.) invokes the handler via IPC.

use blvm_sdk::module::{ModuleBootstrap, ModuleDb};
use blvm_selective_sync::SyncPolicyModule;

const MODULE_NAME: &str = "selective-sync";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bootstrap = ModuleBootstrap::init_module(MODULE_NAME);
    let db = ModuleDb::open(&bootstrap.data_dir)?;
    let module = SyncPolicyModule;

    blvm_sdk::run_module! {
        bootstrap: &bootstrap,
        module_name: MODULE_NAME,
        module: module.clone(),
        module_type: SyncPolicyModule,
        db: db.as_db(),
    }?;

    Ok(())
}
