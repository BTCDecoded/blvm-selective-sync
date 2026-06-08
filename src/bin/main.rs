//! Selective-sync module binary.
//!
//! Runtime-loadable module for blvm-node. Registers "sync-policy" CLI and handles
//! invocations (list, subscribe, unsubscribe, refresh, apply, status, build-entry, config-path).

use anyhow::Result;
use blvm_node::module::ipc::protocol::{
    InvocationMessage, InvocationResultMessage, InvocationResultPayload, InvocationType,
};
use blvm_sdk::module::runner::{run_module_with_setup_and_api, InvocationContext};
use blvm_sdk::module::{ModuleBootstrap, ModuleDb};
use blvm_selective_sync::module_api::SelectiveSyncModuleAPI;
use blvm_selective_sync::{ops, SyncPolicyConfig, SyncPolicyModule};
use std::sync::Arc;
use std::time::Duration;
use tracing::warn;

const MODULE_NAME: &str = "selective-sync";

#[tokio::main]
async fn main() -> Result<()> {
    let bootstrap = ModuleBootstrap::init_module(MODULE_NAME);
    let db = ModuleDb::open(&bootstrap.data_dir)?;

    let setup = |node_api: Arc<dyn blvm_node::module::traits::NodeAPI>,
                 db: Arc<dyn blvm_node::storage::database::Database>,
                 data_dir: &std::path::Path| {
        let data_dir = data_dir.to_path_buf();
        async move {
            let config = SyncPolicyConfig::load(data_dir.join("config.toml")).unwrap_or_default();
            if !config.registries.is_empty() && config.registry_refresh_interval > 0 {
                let interval_secs = config.registry_refresh_interval;
                let refresh_data_dir = data_dir.clone();
                let refresh_db = Arc::clone(&db);
                let refresh_node_api = Arc::clone(&node_api);
                tokio::spawn(async move {
                    let mut interval =
                        tokio::time::interval(Duration::from_secs(interval_secs.max(60)));
                    loop {
                        interval.tick().await;
                        let mut cfg = SyncPolicyConfig::load(refresh_data_dir.join("config.toml"))
                            .unwrap_or_default();
                        if cfg.registries.is_empty() {
                            continue;
                        }
                        match ops::refresh_and_apply(
                            refresh_db.as_ref(),
                            &mut cfg,
                            Some(refresh_node_api.as_ref()),
                        )
                        .await
                        {
                            Ok(result) => tracing::info!(
                                "Periodic refresh: stored={} tx_deny={:?}",
                                result.refresh.entries_stored,
                                result.apply.as_ref().map(|a| a.tx_count)
                            ),
                            Err(e) => tracing::warn!("Periodic refresh failed: {e}"),
                        }
                    }
                });
            }

            let module_api: Arc<dyn blvm_node::module::inter_module::api::ModuleAPI> =
                Arc::new(SelectiveSyncModuleAPI::new(
                    Arc::clone(&db),
                    Arc::clone(&node_api),
                    data_dir.clone(),
                ));
            let module = SyncPolicyModule;
            Ok((module.clone(), module, module_api))
        }
    };

    let dispatch = |invocation: InvocationMessage,
                    ctx: InvocationContext,
                    module: &SyncPolicyModule,
                    cli: &SyncPolicyModule| {
        let (success, payload, error) = match &invocation.invocation_type {
            InvocationType::Cli { subcommand, args } => {
                let args: Vec<String> = args.clone();
                match cli.dispatch_cli(&ctx, subcommand, &args) {
                    Ok(stdout) => (
                        true,
                        Some(InvocationResultPayload::Cli {
                            stdout,
                            stderr: String::new(),
                            exit_code: 0,
                        }),
                        None,
                    ),
                    Err(e) => (false, None, Some(e.to_string())),
                }
            }
            InvocationType::Rpc { method, params } => {
                let db_ref = ctx.db();
                match module.dispatch_rpc(method, params, db_ref) {
                    Ok(v) => (true, Some(InvocationResultPayload::Rpc(v)), None),
                    Err(e) => (false, None, Some(e.to_string())),
                }
            }
            InvocationType::ModuleApi { .. } => (
                false,
                None,
                Some("ModuleApi dispatch should be handled by runner".to_string()),
            ),
        };
        InvocationResultMessage {
            correlation_id: invocation.correlation_id,
            success,
            payload,
            error,
        }
    };

    let on_event = |e, m: &SyncPolicyModule, ctx: &InvocationContext| {
        let m = m.clone();
        let ctx = ctx.clone();
        async move { m.dispatch_event(e, &ctx).await }
    };

    run_module_with_setup_and_api(
        bootstrap.socket_path.clone(),
        &bootstrap.module_id,
        MODULE_NAME,
        env!("CARGO_PKG_VERSION"),
        SyncPolicyModule::cli_spec(),
        &[],
        SyncPolicyModule::event_types(),
        dispatch,
        on_event,
        setup,
        db.as_db(),
        bootstrap.data_dir.as_path(),
    )
    .await
    .map_err(|e| anyhow::anyhow!("{e}"))?;

    warn!("Event receiver closed, module shutting down");
    Ok(())
}
