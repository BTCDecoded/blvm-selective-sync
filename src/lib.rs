//! blvm-selective-sync: Download policy for flagged transaction content.
//!
//! Enables node operators to avoid downloading flagged transaction content
//! during IBD while maintaining full cryptographic validity.
//!
//! See docs/blvm-selective-sync-module.md for the full design.

pub mod cli;
pub mod config;
pub mod module;
pub mod registry_entry;

pub use cli::{run_sync_policy, run_sync_policy_capture, SyncPolicyCommand};
pub use module::SyncPolicyModule;
pub use config::SyncPolicyConfig;
pub use registry_entry::{RegistryEntry, RegistryIndex};
pub use registry_entry::{build_registry_entry, infer_embedding_type, EmbeddingType};
