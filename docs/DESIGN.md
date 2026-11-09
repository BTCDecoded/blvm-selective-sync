# Selective-sync module design

## Overview

Operators maintain a **policy registry** of flagged transactions (witness or OP_RETURN embeddings). The module:

1. Fetches and quorum-merges remote registries into `ModuleDb` (`registry_entries` tree).
2. Optionally merges an on-chain-built registry (`on_chain_registry` tree).
3. Applies **serve policy** to the node (denylist hashes on `getdata`).
4. Strips **witness** and/or **OP_RETURN** data during IBD persistence; **stripped payloads are never stored locally**.
5. When the node needs complete data temporarily, it uses the existing **GetData** path (`NetworkManager::request_block`) — in memory only, not persisted.
6. Optionally skips witness download when merkle sibling paths verify (`trust_registry_stubs`).

**Trust model:** the registry is quorum policy, not a self-verifying proof. The stored block is stripped. `rehydrate_block_for_consensus` always returns not found. An IPC timeout does not apply the denylist. The stripped copy is not the block consensus validated.

Policy logic stays in this module; the node provides generic integration hooks only.

## Configuration (`config.toml`)

| Key | Default | Purpose |
|-----|---------|---------|
| `registries` | `[]` | Registry URLs to subscribe |
| `min_registry_agreement` | `0.5` | Quorum fraction (0.0–1.0) |
| `registry_refresh_interval` | `3600` | Seconds between periodic refresh (module-local `tokio::interval`) |
| `witness_mode` | `""` (strict) | `strict` strips/denies witness embeddings; `relaxed` strips/denies OP_RETURN only |
| `trust_registry_stubs` | `false` | v3: skip witness download when merkle paths verify |
| `on_chain_registry_builder` | `false` | Build local registry from `NewBlock` events |
| `audit_log` | `false` | Append apply/refresh/strip lines to audit log |
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
| `status` | Policy counts, denylist snapshots, download-skip stats |
| `export-registry` | Export merged entries JSON |
| `build-entry` / `build-registry` | Operator tooling from tx/block hex (`build-registry` fills `merkle_sibling_path`) |

## ModuleAPI

Registered via `run_module_with_setup_and_api` (`register_module_api` capability).

| Method | Role |
|--------|------|
| `filter_block_before_store` | Strip witness / OP_RETURN before node persist |
| `rehydrate_block_for_consensus` | Always `found=false` (payloads not kept) |
| `get_canonical_txids` | Always `found=false` (node GetData if needed) |
| `lookup_block_for_txids` | Resolve policy txid → block via registry + local header |
| `filter_block_download_policy` | `skip_witness` when stubs + merkle paths verify |

Short method names are unique across modules (first registrant wins).

**Node:** fail-open on IPC timeout/error. **No stripped payload retention on disk.**

## Storage trees

| Tree | Content |
|------|---------|
| `registry_entries` | Quorum-merged remote policy (`StoredPolicyEntry`) |
| `on_chain_registry` | Locally indexed entries from blocks |
| `download_policy_stats` | v3 skip-witness counters |

## Periodic refresh

`setup` spawns `tokio::time::interval` → `refresh_and_apply`. Does **not** use `register_timer` (IPC-incompatible for subprocess modules).
