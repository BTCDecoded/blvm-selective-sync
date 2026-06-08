//! HTTP fetch for external registry indexes.

use anyhow::{Context, Result};
use std::time::Duration;

use crate::registry_entry::RegistryIndex;

const MAX_BODY_BYTES: usize = 10 * 1024 * 1024;
const FETCH_TIMEOUT: Duration = Duration::from_secs(30);

/// Fetch a [`RegistryIndex`] from an HTTP(S) URL.
pub async fn fetch_registry_index(url: &str) -> Result<RegistryIndex> {
    let client = reqwest::Client::builder()
        .timeout(FETCH_TIMEOUT)
        .build()
        .context("Failed to build HTTP client")?;

    let response = client
        .get(url.trim())
        .send()
        .await
        .with_context(|| format!("HTTP request failed for {url}"))?
        .error_for_status()
        .with_context(|| format!("Registry URL returned error status: {url}"))?;

    let bytes = response
        .bytes()
        .await
        .with_context(|| format!("Failed to read body from {url}"))?;

    if bytes.len() > MAX_BODY_BYTES {
        anyhow::bail!(
            "Registry body too large ({} bytes > {}): {url}",
            bytes.len(),
            MAX_BODY_BYTES
        );
    }

    let index: RegistryIndex =
        serde_json::from_slice(&bytes).with_context(|| format!("Invalid registry JSON: {url}"))?;

    Ok(index)
}
