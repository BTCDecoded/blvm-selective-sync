//! CLI commands for sync-policy management.

use anyhow::{Context, Result};
use blvm_node::module::EventType;
use blvm_protocol::serialization::transaction::serialize_transaction;
use std::io::Write;

use crate::config::SyncPolicyConfig;
use crate::db::open_policy_db;
use crate::ops::{RefreshAndApplyResult, refresh_and_apply};
use crate::policy_store::{export_document, load_merged_entries};
use crate::registry_entry::{EmbeddingType, build_registry_entry, build_registry_from_block};
use crate::withheld::{get_block_serve_denylist_snapshot, get_tx_serve_denylist_snapshot};
use blvm_protocol::spam_filter::SpamFilterPreset;

/// Sync-policy subcommand variants.
#[derive(Debug, Clone)]
pub enum SyncPolicyCommand {
    List,
    Subscribe {
        url: String,
    },
    Unsubscribe {
        url: String,
    },
    Refresh,
    Apply,
    Status,
    BuildEntry {
        tx_hex: String,
        embedding: EmbeddingType,
        block_height: Option<u64>,
        block_position: Option<u32>,
    },
    BuildRegistry {
        block_hex_or_path: String,
        preset: SpamFilterPreset,
        block_height: Option<u64>,
        output_path: Option<String>,
    },
    ExportRegistry {
        output_path: Option<String>,
    },
    ConfigPath,
}

/// Run a sync-policy command (prints to stdout).
pub fn run_sync_policy(cmd: SyncPolicyCommand) -> Result<()> {
    run_sync_policy_impl(cmd, &mut std::io::stdout(), None)
}

/// Run a sync-policy command and capture output (for IPC module dispatch).
pub fn run_sync_policy_capture(
    cmd: SyncPolicyCommand,
    ctx: Option<&blvm_sdk::module::runner::InvocationContext>,
) -> Result<(String, String, i32)> {
    let mut out = Vec::new();
    match run_sync_policy_impl(cmd, &mut out, ctx) {
        Ok(()) => Ok((String::from_utf8(out)?, String::new(), 0)),
        Err(e) => Ok((String::new(), format!("{}\n", e), 1)),
    }
}

fn run_sync_policy_impl<W: Write>(
    cmd: SyncPolicyCommand,
    w: &mut W,
    ctx: Option<&blvm_sdk::module::runner::InvocationContext>,
) -> Result<()> {
    match cmd {
        SyncPolicyCommand::List => run_list(w),
        SyncPolicyCommand::Subscribe { url } => run_subscribe(w, &url, ctx),
        SyncPolicyCommand::Unsubscribe { url } => run_unsubscribe(w, &url, ctx),
        SyncPolicyCommand::Refresh => run_refresh(w, ctx),
        SyncPolicyCommand::Apply => run_apply(w, ctx),
        SyncPolicyCommand::Status => run_status(w, ctx),
        SyncPolicyCommand::BuildEntry {
            tx_hex,
            embedding,
            block_height,
            block_position,
        } => run_build_entry(w, &tx_hex, embedding, block_height, block_position),
        SyncPolicyCommand::BuildRegistry {
            block_hex_or_path,
            preset,
            block_height,
            output_path,
        } => run_build_registry(
            w,
            &block_hex_or_path,
            preset,
            block_height,
            output_path.as_deref(),
        ),
        SyncPolicyCommand::ExportRegistry { output_path } => {
            run_export_registry(w, output_path.as_deref())
        }
        SyncPolicyCommand::ConfigPath => run_config_path(w),
    }
}

fn run_list<W: Write>(w: &mut W) -> Result<()> {
    let config = SyncPolicyConfig::load(SyncPolicyConfig::config_path())?;
    let path = SyncPolicyConfig::config_path();

    writeln!(w, "Sync policy config: {}", path.display())?;
    writeln!(w)?;

    if config.registries.is_empty() {
        writeln!(w, "No registries subscribed.")?;
        writeln!(w, "Use: blvm sync-policy subscribe <url>")?;
        return Ok(());
    }

    writeln!(w, "Subscribed registries ({}):", config.registries.len())?;
    for (i, url) in config.registries.iter().enumerate() {
        writeln!(w, "  {}. {}", i + 1, url)?;
    }
    if let Some(ref t) = config.last_refresh {
        writeln!(w)?;
        writeln!(w, "Last refresh: {}", t)?;
    }

    Ok(())
}

fn run_subscribe<W: Write>(
    w: &mut W,
    url: &str,
    ctx: Option<&blvm_sdk::module::runner::InvocationContext>,
) -> Result<()> {
    let mut config = SyncPolicyConfig::load(SyncPolicyConfig::config_path())?;
    config.subscribe(url);
    config.save()?;
    writeln!(w, "✓ Subscribed to: {}", url.trim())?;
    writeln!(w, "  Registries: {}", config.registries.len())?;
    if let Some(c) = ctx {
        publish_selective_sync_policy_applied(c, "subscribe", config.registries.len());
    }
    Ok(())
}

fn run_unsubscribe<W: Write>(
    w: &mut W,
    url: &str,
    ctx: Option<&blvm_sdk::module::runner::InvocationContext>,
) -> Result<()> {
    let mut config = SyncPolicyConfig::load(SyncPolicyConfig::config_path())?;
    let before = config.registries.len();
    config.unsubscribe(url);
    config.save()?;
    let after = config.registries.len();
    if after < before {
        writeln!(w, "✓ Unsubscribed from: {}", url.trim())?;
    } else {
        writeln!(w, "Registry not found: {}", url.trim())?;
    }
    if let Some(c) = ctx {
        publish_selective_sync_policy_applied(c, "unsubscribe", config.registries.len());
    }
    Ok(())
}

fn run_refresh<W: Write>(
    w: &mut W,
    ctx: Option<&blvm_sdk::module::runner::InvocationContext>,
) -> Result<()> {
    let mut config = SyncPolicyConfig::load(SyncPolicyConfig::config_path())?;
    if config.registries.is_empty() {
        writeln!(
            w,
            "No registries to refresh. Subscribe first: blvm sync-policy subscribe <url>"
        )?;
        return Ok(());
    }

    let db = open_policy_db()?;
    let node_api = ctx.and_then(|c| c.node_api());
    let result: RefreshAndApplyResult = tokio::runtime::Handle::current().block_on(async {
        refresh_and_apply(db.as_db().as_ref(), &mut config, node_api.as_deref()).await
    })?;

    writeln!(w, "✓ Refresh complete")?;
    writeln!(
        w,
        "  Fetched: {} registries ({} failed)",
        result.refresh.urls_fetched,
        result.refresh.urls_failed.len()
    )?;
    writeln!(
        w,
        "  Policy entries stored: {}",
        result.refresh.entries_stored
    )?;
    if !result.refresh.urls_failed.is_empty() {
        for (url, err) in &result.refresh.urls_failed {
            writeln!(w, "  ! {url}: {err}")?;
        }
    }
    writeln!(
        w,
        "  Last refresh: {}",
        config.last_refresh.as_deref().unwrap_or("never")
    )?;
    if let Some(apply) = &result.apply {
        writeln!(
            w,
            "  Applied: {} tx denylist, {} block denylist",
            apply.tx_count, apply.block_count
        )?;
    }
    if let Some(c) = ctx {
        publish_selective_sync_policy_applied(c, "refresh", config.registries.len());
    }
    Ok(())
}

fn run_apply<W: Write>(
    w: &mut W,
    ctx: Option<&blvm_sdk::module::runner::InvocationContext>,
) -> Result<()> {
    let config = SyncPolicyConfig::load(SyncPolicyConfig::config_path())?;
    let Some(c) = ctx else {
        writeln!(w, "apply requires a running module with node API access")?;
        return Ok(());
    };
    let Some(node_api) = c.node_api() else {
        writeln!(w, "apply requires node API access")?;
        return Ok(());
    };

    let db = open_policy_db()?;
    let report = tokio::runtime::Handle::current().block_on(async {
        crate::apply_policy::apply_policy(node_api.as_ref(), db.as_db().as_ref(), &config).await
    })?;

    writeln!(w, "✓ Policy applied to node serve denylists")?;
    writeln!(w, "  Tx denylist entries merged: {}", report.tx_count)?;
    writeln!(w, "  Block denylist entries merged: {}", report.block_count)?;
    publish_selective_sync_policy_applied(c, "apply", config.registries.len());
    Ok(())
}

fn publish_selective_sync_policy_applied(
    ctx: &blvm_sdk::module::runner::InvocationContext,
    policy_source: &str,
    registry_count: usize,
) {
    if let Some(node_api) = ctx.node_api() {
        let payload = blvm_node::module::ipc::protocol::EventPayload::SelectiveSyncPolicyApplied {
            policy_source: policy_source.to_string(),
            registry_count,
        };
        let _ = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current()
                .block_on(node_api.publish_event(EventType::SelectiveSyncPolicyApplied, payload))
        });
    }
}

fn run_status<W: Write>(
    w: &mut W,
    ctx: Option<&blvm_sdk::module::runner::InvocationContext>,
) -> Result<()> {
    let config = SyncPolicyConfig::load(SyncPolicyConfig::config_path())?;
    let db = open_policy_db()?;
    let merged = load_merged_entries(db.as_db().as_ref())?;

    writeln!(w, "Sync policy status")?;
    writeln!(w, "==================")?;
    writeln!(w, "Registries: {}", config.registries.len())?;
    writeln!(
        w,
        "Last refresh: {}",
        config.last_refresh.as_deref().unwrap_or("never")
    )?;
    writeln!(w, "Policy entries (merged): {}", merged.len())?;
    writeln!(w, "Witness mode: {}", config.witness_mode)?;
    writeln!(w, "IBD filter enabled: {}", config.ibd_filter_enabled)?;
    writeln!(
        w,
        "On-chain registry builder: {}",
        config.on_chain_registry_builder
    )?;
    writeln!(
        w,
        "Registry refresh interval: {}s",
        config.registry_refresh_interval
    )?;
    writeln!(
        w,
        "Config path: {}",
        SyncPolicyConfig::config_path().display()
    )?;

    if let Some(c) = ctx {
        if let Some(node_api) = c.node_api() {
            let (tx_snap, block_snap) = tokio::runtime::Handle::current().block_on(async {
                let tx = get_tx_serve_denylist_snapshot(node_api.as_ref()).await.ok();
                let block = get_block_serve_denylist_snapshot(node_api.as_ref())
                    .await
                    .ok();
                (tx, block)
            });
            if let Some(tx) = tx_snap {
                writeln!(w)?;
                writeln!(
                    w,
                    "Tx serve denylist: {} total{}",
                    tx.total_count,
                    if tx.truncated {
                        " (snapshot truncated)"
                    } else {
                        ""
                    }
                )?;
            }
            if let Some(block) = block_snap {
                writeln!(
                    w,
                    "Block serve denylist: {} total{}",
                    block.total_count,
                    if block.truncated {
                        " (snapshot truncated)"
                    } else {
                        ""
                    }
                )?;
            }
        }
    }

    writeln!(w)?;
    writeln!(
        w,
        "IBD witness filtering: filter_block_before_store ModuleAPI (enabled={})",
        config.ibd_filter_enabled
    )?;

    Ok(())
}

/// Resolve tx_hex_or_txid: if it looks like a txid (64 hex chars) and ctx has node_api, fetch from node.
pub fn resolve_tx_hex(
    ctx: &blvm_sdk::module::runner::InvocationContext,
    tx_hex_or_txid: &str,
) -> Result<String> {
    let s = tx_hex_or_txid.trim().trim_start_matches("0x");
    // txid = 32 bytes = 64 hex chars
    if s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit()) {
        if let Some(node_api) = ctx.node_api() {
            let hash_bytes = hex::decode(s).context("Invalid txid hex")?;
            let mut hash = [0u8; 32];
            if hash_bytes.len() != 32 {
                anyhow::bail!("txid must be 32 bytes (64 hex chars)");
            }
            hash.copy_from_slice(&hash_bytes);
            let hash = blvm_node::Hash::from(hash);
            let tx = tokio::runtime::Handle::current().block_on(async move {
                node_api
                    .get_transaction(&hash)
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to fetch transaction from node: {}", e))?
                    .ok_or_else(|| anyhow::anyhow!("Transaction {} not found in node", s))
            })?;
            let serialized = serialize_transaction(&tx);
            return Ok(hex::encode(serialized));
        }
    }
    Ok(tx_hex_or_txid.to_string())
}

fn run_build_entry<W: Write>(
    w: &mut W,
    tx_hex: &str,
    embedding: EmbeddingType,
    block_height: Option<u64>,
    block_position: Option<u32>,
) -> Result<()> {
    let entry = build_registry_entry(tx_hex, embedding, block_height, block_position)?;
    let json = serde_json::to_string_pretty(&entry)?;
    writeln!(w, "{}", json)?;
    Ok(())
}

fn run_build_registry<W: Write>(
    w: &mut W,
    block_hex_or_path: &str,
    preset: SpamFilterPreset,
    block_height: Option<u64>,
    output_path: Option<&str>,
) -> Result<()> {
    let block_hex = if std::path::Path::new(block_hex_or_path).exists() {
        std::fs::read_to_string(block_hex_or_path)
            .context("Failed to read block file")?
            .trim()
            .replace('\n', "")
            .replace(' ', "")
    } else {
        block_hex_or_path
            .trim()
            .replace('\n', "")
            .replace(' ', "")
            .to_string()
    };

    let index = build_registry_from_block(&block_hex, block_height, preset)?;
    let json = serde_json::to_string_pretty(&index)?;

    if let Some(path) = output_path {
        std::fs::write(path, &json).context("Failed to write output file")?;
        writeln!(w, "Wrote {} entries to {}", index.entries.len(), path)?;
    } else {
        writeln!(w, "{}", json)?;
    }
    Ok(())
}

fn run_export_registry<W: Write>(w: &mut W, output_path: Option<&str>) -> Result<()> {
    let config = SyncPolicyConfig::load(SyncPolicyConfig::config_path())?;
    let db = open_policy_db()?;
    let doc = export_document(db.as_db().as_ref(), &config)?;
    let json = serde_json::to_string_pretty(&doc)?;
    let path = output_path.unwrap_or("registry.json");
    std::fs::write(path, &json).context("Failed to write registry file")?;
    writeln!(
        w,
        "Exported registry to {} ({} sources, {} entries)",
        path,
        config.registries.len(),
        doc.entries.len()
    )?;
    Ok(())
}

fn run_config_path<W: Write>(w: &mut W) -> Result<()> {
    writeln!(w, "{}", SyncPolicyConfig::config_path().display())?;
    Ok(())
}
