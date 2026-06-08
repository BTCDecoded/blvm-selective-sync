# blvm-selective-sync

BLVM module: selective sync — download policy for flagged transaction content during initial block download (IBD) and on the P2P serve path.

Part of [Bitcoin Commons](https://btcdecoded.org) BLVM. Loaded by the node as a subprocess module.

## What it does

- **Registry policy** — Subscribe to remote registry URLs; quorum-merge entries into module-local storage.
- **P2P serve policy** — `apply` pushes merged tx/block hashes to node serve denylists (`merge_*_serve_denylist`).
- **IBD witness filter** — When `ibd_filter_enabled = true`, strips flagged witness stacks before the node persists blocks during IBD (`filter_block_before_store` ModuleAPI).
- **On-chain indexer** — Optional `on_chain_registry_builder` builds a local registry from `NewBlock` events.
- **CLI** — `blvm sync-policy …` subcommands (list, subscribe, refresh, apply, status, export-registry, build-entry, build-registry).

## Quick start

1. Load the module (`blvm-selective-sync = "0.1.*"` in `[modules]`, or `blvm module load selective-sync`).
2. Subscribe and refresh:

```bash
blvm sync-policy subscribe https://example.com/registry.json
blvm sync-policy refresh    # fetch + quorum merge + auto-apply denylists
blvm sync-policy apply      # re-apply denylists without fetch
blvm sync-policy status
```

3. Enable IBD filtering in module `config.toml` (path from `blvm sync-policy config-path`):

```toml
ibd_filter_enabled = true
witness_mode = "strict"   # or "relaxed"
```

Node `[modules.selective-sync]` can override these keys via the SDK `#[config]` macro.

## Capabilities (`module.toml`)

```toml
read_blockchain
subscribe_events
register_module_api
network_access
read_network
publish_events
```

## Building

```bash
cargo build
cargo test
```

Local monorepo builds use `[patch.crates-io]` sibling paths (stripped in CI/release).

## Design

See [docs/DESIGN.md](docs/DESIGN.md) for architecture, config keys, and ModuleAPI contract.

## License

MIT

## Links

- [Bitcoin Commons](https://btcdecoded.org)
- [blvm](https://github.com/BTCDecoded/blvm)
- [blvm-sdk](https://github.com/BTCDecoded/blvm-sdk)
