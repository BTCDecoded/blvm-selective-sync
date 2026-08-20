//! HTTP fetch tests for registry_fetch (mock TCP server).

use blvm_selective_sync::registry_entry::{EmbeddingType, RegistryEntry, RegistryIndex};
use blvm_selective_sync::registry_fetch::fetch_registry_index;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::thread;

fn serve_once(status: &str, body: &[u8]) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let status = status.to_string();
    let body = body.to_vec();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept");
        let mut buf = [0u8; 1024];
        let _ = stream.read(&mut buf);
        let header = format!(
            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = stream.write_all(header.as_bytes());
        let _ = stream.write_all(&body);
    });
    format!("http://{addr}/registry.json")
}

fn sample_index() -> Vec<u8> {
    let index = RegistryIndex {
        entries: vec![RegistryEntry {
            txid: "ab".repeat(32),
            block_height: Some(1),
            block_position: Some(0),
            embedding_type: EmbeddingType::Witness,
            merkle_sibling_path: None,
            outputs: vec![],
            inputs: vec![],
        }],
    };
    serde_json::to_vec(&index).expect("json")
}

#[tokio::test]
async fn fetch_registry_index_parses_json() {
    let url = serve_once("200 OK", &sample_index());
    let index = fetch_registry_index(&url).await.expect("fetch");
    assert_eq!(index.entries.len(), 1);
    assert_eq!(index.entries[0].txid.len(), 64);
}

#[tokio::test]
async fn fetch_registry_index_rejects_error_status() {
    let url = serve_once("404 Not Found", b"missing");
    let err = fetch_registry_index(&url).await.expect_err("should fail");
    assert!(err.to_string().contains("error status") || err.to_string().contains("404"));
}
