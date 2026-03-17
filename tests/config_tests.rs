//! Tests for SyncPolicyConfig load/save and run_sync_policy_capture
//!
//! Uses BLVM_DATA_DIR (checked first) and a mutex to avoid env races when tests run in parallel.

use blvm_selective_sync::{
    run_sync_policy_capture, SyncPolicyConfig, SyncPolicyCommand,
};
use std::sync::Mutex;
use tempfile::TempDir;

static CONFIG_TEST_LOCK: Mutex<()> = Mutex::new(());

fn with_temp_data_dir<T, F>(f: F) -> T
where
    F: FnOnce(&TempDir) -> T,
{
    let _guard = CONFIG_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().expect("tempdir");
    std::env::set_var("BLVM_DATA_DIR", dir.path());
    let result = f(&dir);
    std::env::remove_var("BLVM_DATA_DIR");
    result
}

#[test]
fn test_config_load_default_when_missing() {
    with_temp_data_dir(|_dir| {
        let config = SyncPolicyConfig::load(SyncPolicyConfig::config_path()).expect("load");
        assert!(config.registries.is_empty());
        assert!(config.last_refresh.is_none());
    });
}

#[test]
fn test_config_save_and_load() {
    with_temp_data_dir(|_dir| {
        let mut config = SyncPolicyConfig::default();
        config.registries = vec!["https://registry.example.com".to_string()];
        config.last_refresh = Some("12345".to_string());
        config.save().expect("save");

        let loaded = SyncPolicyConfig::load(SyncPolicyConfig::config_path()).expect("load");
        assert_eq!(loaded.registries, vec!["https://registry.example.com"]);
        assert_eq!(loaded.last_refresh.as_deref(), Some("12345"));
    });
}

#[test]
fn test_config_subscribe_unsubscribe() {
    with_temp_data_dir(|_dir| {
        let mut config = SyncPolicyConfig::default();
        config.subscribe("https://a.com");
        config.subscribe("https://b.com");
        config.subscribe("https://a.com"); // duplicate, no-op
        assert_eq!(config.registries.len(), 2);
        config.unsubscribe("https://a.com");
        assert_eq!(config.registries, vec!["https://b.com"]);
    });
}

#[test]
fn test_config_apply_env_overrides() {
    with_temp_data_dir(|_dir| {
        std::env::set_var("MODULE_CONFIG_REGISTRIES", "https://env1.com, https://env2.com");
        let mut config = SyncPolicyConfig::default();
        config.registries = vec!["https://file.com".to_string()];
        config.apply_env_overrides();
        std::env::remove_var("MODULE_CONFIG_REGISTRIES");
        assert_eq!(
            config.registries,
            vec!["https://env1.com".to_string(), "https://env2.com".to_string()]
        );
    });
}

#[test]
fn test_run_sync_policy_capture_list_empty() {
    with_temp_data_dir(|_dir| {
        let (stdout, stderr, code) =
            run_sync_policy_capture(SyncPolicyCommand::List, None).expect("capture");
        assert_eq!(code, 0);
        assert!(stderr.is_empty());
        assert!(stdout.contains("No registries subscribed"));
        assert!(stdout.contains("sync-policy subscribe"));
    });
}

#[test]
fn test_run_sync_policy_capture_config_path() {
    with_temp_data_dir(|dir| {
        let (stdout, stderr, code) =
            run_sync_policy_capture(SyncPolicyCommand::ConfigPath, None).expect("capture");
        assert_eq!(code, 0);
        assert!(stderr.is_empty());
        assert!(stdout.contains("config.toml"));
        assert!(stdout.trim().contains(dir.path().to_str().unwrap()));
    });
}

#[test]
fn test_run_sync_policy_capture_subscribe_and_list() {
    with_temp_data_dir(|_dir| {
        let (stdout, stderr, code) = run_sync_policy_capture(
            SyncPolicyCommand::Subscribe {
                url: "https://test-registry.example".to_string(),
            },
            None,
        )
        .expect("capture");
        assert_eq!(code, 0);
        assert!(stderr.is_empty());
        assert!(stdout.contains("Subscribed to"));

        let (stdout, _, code) = run_sync_policy_capture(SyncPolicyCommand::List, None).expect("capture");
        assert_eq!(code, 0);
        assert!(stdout.contains("https://test-registry.example"));
    });
}

#[test]
fn test_run_sync_policy_capture_status() {
    with_temp_data_dir(|_dir| {
        let (stdout, stderr, code) =
            run_sync_policy_capture(SyncPolicyCommand::Status, None).expect("capture");
        assert_eq!(code, 0);
        assert!(stderr.is_empty());
        assert!(stdout.contains("Sync policy status"));
        assert!(stdout.contains("Registries:"));
    });
}

#[test]
fn test_infer_embedding_type_invalid_hex() {
    use blvm_selective_sync::infer_embedding_type;

    // Invalid hex returns error (integration test; unit tests in registry_entry)
    assert!(infer_embedding_type("nothex").is_err());
}
