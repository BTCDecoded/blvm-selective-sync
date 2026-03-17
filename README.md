# blvm-selective-sync

BLVM module: selective sync — a download policy for flagged transaction content during initial block download (IBD).

Part of [Bitcoin Commons](https://btcdecoded.org) BLVM. Loaded by the blvm node as a subprocess module. Lets node operators avoid downloading content that is marked as flagged (e.g. by policy or compliance) while still maintaining full cryptographic validity of the chain.

## What it does

- **Sync policy** — Configurable rules (e.g. `sync-policy.json`) that determine which transaction outputs or content are skipped during IBD.
- **Module API** — Registers with the node via the BLVM module system (CLI subcommands, config path, status).
- **CLI** — Commands such as `status`, `config-path`, and policy capture for testing.

Typically used in a workspace that also contains **blvm-node**, **blvm-sdk**, and **blvm-protocol** (path dependencies).

## Usage

Load the module when running the node (e.g. via `config.toml` or `blvm module load blvm-selective-sync`). Configure the sync policy in the path reported by `config-path` (e.g. `sync-policy.json` in the module data dir).

## Building

From a workspace that includes blvm-node, blvm-sdk, and blvm-protocol:

```bash
cargo build -p blvm-selective-sync
```

Or from this directory (with path deps resolved):

```bash
cargo build
cargo test
```

## Design

See the design doc in the main BLVM/docs tree (e.g. `docs/blvm-selective-sync-module.md` or equivalent) for the full design and policy format.

## License

MIT. See [LICENSE](LICENSE) if present.

## Links

- [Bitcoin Commons](https://btcdecoded.org)
- [blvm](https://github.com/BTCDecoded/blvm) — node binary that loads this module
- [blvm-sdk](https://github.com/BTCDecoded/blvm-sdk) — module API used by this crate
