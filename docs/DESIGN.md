# Selective-sync module design

## Overview

Operators maintain a **policy registry** of flagged transactions (witness or OP_RETURN embeddings). The module:

1. Fetches and quorum-merges remote registries into `ModuleDb` (`registry_entries` tree).
2. Optionally merges an on-chain-built registry (`on_chain_registry` tree).
3. Applies **serve policy** to the node (denylist hashes on `getdata`).
4. Optionally strips **witness data** during IBD persistence via ModuleAPI.

Policy logic stays in this module; the node provides generic integration hooks only.

## Configuration (`config.toml`)

| Key | Default | Purpose |
|-----|---------|---------|
| `registries` | `[]` | Registry URLs to subscribe |
| `min_registry_agreement` | `0.5` | Quorum fraction (0.0–1.0) |
| `registry_refresh_interval` | `3600` | Seconds between periodic refresh (module-local `tokio::interval`) |
| `witness_mode` | `""` (strict) | `strict` strips/denies witness embeddings; `relaxed` focuses on OP_RETURN |
| `ibd_filter_enabled` | `false` | Enable IBD witness stripping |
| `on_chain_registry_builder` | `false` | Build local registry from `NewBlock` events |
| `audit_log` | `false` | Append apply/refresh lines to audit log |
| `audit_log_path` | optional | Audit log file path |

Overrides: node `[modules.selective-sync]`, env `MODULE_CONFIG_*`.

## CLI commands

| Command | Action |
|---------|--------|
| `list` | Registry URLs and last refresh |
| `subscribe <url>` | Add registry URL |
| `unsubscribe <url>` | Remove registry URL |
| `refresh` | HTTP fetch + quorum merge + auto-apply |
| `apply` | Push merged policy to node serve denylists |
| `status` | Policy counts, denylist snapshots, IBD filter state |
| `export-registry` | Export merged entries JSON |
| `build-entry` / `build-registry` | Operator tooling from tx/block hex |

## ModuleAPI: `filter_block_before_store`

Registered via `run_module_with_setup_and_api` (`register_module_api` capability).

**Request** (bincode):

```text
{ height, block, witnesses }
```

**Response**:

```text
{ block, witnesses, stripped_txids, filtered }
```

When `ibd_filter_enabled` is false, returns input unchanged. On strip, publishes `IBDBlockFiltered` (`publish_events`).

**Node integration:** `blvm-node` `module/pipeline.rs` calls this method from `ParallelIBD::do_flush_to_storage` before witness blobs are written. Fail-open on IPC timeout; `witness_mode: strict` may fail-closed inside the module.

## Serve policy

`apply_policy` resolves flagged txids and block hashes (via `get_block_by_height`) and calls:

- `merge_tx_serve_denylist`
- `merge_block_serve_denylist`

Requires `network_access`. `status` reads snapshots via `read_network`.

## Storage trees

| Tree | Content |
|------|---------|
| `registry_entries` | Quorum-merged remote policy (`StoredPolicyEntry`) |
| `on_chain_registry` | Locally indexed entries from blocks |

## Periodic refresh

`setup` spawns `tokio::time::interval` → `refresh_and_apply`. Does **not** use `register_timer` (IPC-incompatible for subprocess modules).
