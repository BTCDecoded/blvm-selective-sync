//! blvm-selective-sync: Download policy for flagged transaction content.
//!
//! Enables node operators to avoid downloading flagged transaction content
//! during IBD while maintaining full cryptographic validity.
//!
//! See docs/blvm-selective-sync-module.md for the full design.

pub mod apply_policy;
pub mod audit;
pub mod cli;
pub mod config;
pub mod db;
pub mod ibd_filter;
pub mod module;
pub mod module_api;
pub mod ops;
pub mod policy_store;
pub mod registry_entry;
pub mod registry_fetch;
pub mod withheld;

pub use cli::{SyncPolicyCommand, run_sync_policy, run_sync_policy_capture};
pub use config::SyncPolicyConfig;
pub use module::SyncPolicyModule;
pub use registry_entry::{EmbeddingType, build_registry_entry, infer_embedding_type};
pub use registry_entry::{RegistryEntry, RegistryIndex};
pub use withheld::merge_block_serve_denylist;
