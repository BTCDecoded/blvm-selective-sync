//! CLI commands for sync-policy management.

use anyhow::{Context, Result};
use blvm_node::module::EventType;
use blvm_protocol::serialization::transaction::serialize_transaction;
use std::io::Write;

use crate::config::SyncPolicyConfig;
use crate::registry_entry::{build_registry_entry, build_registry_from_block, EmbeddingType};
use blvm_protocol::spam_filter::SpamFilterPreset;

/// Sync-policy subcommand variants.
#[derive(Debug, Clone)]
pub enum SyncPolicyCommand {
    List,
    Subscribe { url: String },
    Unsubscribe { url: String },
    Refresh,
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
    ExportRegistry { output_path: Option<String> },
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
        SyncPolicyCommand::Status => run_status(w),
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
        } => run_build_registry(w, &block_hex_or_path, preset, block_height, output_path.as_deref()),
        SyncPolicyCommand::ExportRegistry { output_path } => run_export_registry(w, output_path.as_deref()),
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
        writeln!(w, "No registries to refresh. Subscribe first: blvm sync-policy subscribe <url>")?;
        return Ok(());
    }
    // TODO: fetch from each registry URL, aggregate entries
    config.last_refresh = Some(now_iso());
    config.save()?;
    writeln!(w, "✓ Refresh triggered (registry fetch not yet implemented)")?;
    writeln!(w, "  Last refresh: {}", config.last_refresh.as_deref().unwrap_or("never"))?;
    if let Some(c) = ctx {
        publish_selective_sync_policy_applied(c, "refresh", config.registries.len());
    }
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

fn now_iso() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| format!("{}", d.as_secs()))
        .unwrap_or_else(|_| "unknown".to_string())
}

fn run_status<W: Write>(w: &mut W) -> Result<()> {
    let config = SyncPolicyConfig::load(SyncPolicyConfig::config_path())?;

    writeln!(w, "Sync policy status")?;
    writeln!(w, "==================")?;
    writeln!(w, "Registries: {}", config.registries.len())?;
    writeln!(
        w,
        "Last refresh: {}",
        config.last_refresh.as_deref().unwrap_or("never")
    )?;
    writeln!(w, "Config path: {}", SyncPolicyConfig::config_path().display())?;
    writeln!(w)?;
    writeln!(w, "Note: Full IBD integration (policy engine, witness stripping) is Phase 1.")?;
    writeln!(w, "This CLI manages registry subscriptions for when the feature is enabled.")?;

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
                node_api.get_transaction(&hash).await.map_err(|e| {
                    anyhow::anyhow!("Failed to fetch transaction from node: {}", e)
                })?
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
    let index = crate::registry_entry::RegistryIndex { entries: vec![] };
    let json = serde_json::to_string_pretty(&serde_json::json!({
        "entries": index.entries,
        "sources": config.registries,
        "last_refresh": config.last_refresh,
    }))?;
    let path = output_path.unwrap_or("registry.json");
    std::fs::write(path, &json).context("Failed to write registry file")?;
    writeln!(w, "Exported registry to {} ({} sources, {} entries)", path, config.registries.len(), index.entries.len())?;
    Ok(())
}

fn run_config_path<W: Write>(w: &mut W) -> Result<()> {
    writeln!(w, "{}", SyncPolicyConfig::config_path().display())?;
    Ok(())
}
