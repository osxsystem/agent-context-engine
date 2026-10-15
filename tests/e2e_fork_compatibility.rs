use std::process::Command;
use std::time::Duration;

use axum::{Json, Router, routing::post};
use reqwest::Client;
use serde_json::{Value, json};
use tempfile::TempDir;
use tokio::net::TcpListener;

use context_engine_rs::config::{Settings, config_path, write_settings_atomic};

mod e2e_common;

#[tokio::test]
async fn global_mcp_preserves_worktree_scope_and_progress() {
    let gateway = Router::new().route(
        "/v1/embeddings",
        post(|Json(body): Json<Value>| async move {
            let inputs = body["input"].as_array().unwrap();
            let preflight = inputs.len() == 1 && inputs[0] == "context-engine preflight";
            if body["input_type"] == "document" && !preflight {
                tokio::time::sleep(Duration::from_secs(16)).await;
            }
            let data: Vec<Value> = (0..inputs.len())
                .map(|i| json!({"index": i, "embedding": [1.0, 0.0, 0.0, 0.0]}))
                .collect();
            Json(json!({"data": data}))
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let gateway_addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, gateway).await.unwrap() });

    let home = TempDir::new().unwrap();
    let repos = TempDir::new().unwrap();
    let main = repos.path().join("main");
    let linked = repos.path().join("linked");
    std::fs::create_dir(&main).unwrap();
    let main = std::fs::canonicalize(main).unwrap();
    std::fs::write(
        main.join("probe.rs"),
        "pub fn merge_probe() -> u32 { 42 }\n",
    )
    .unwrap();
    for args in [
        vec!["init"],
        vec!["add", "probe.rs"],
        vec![
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "-m",
            "fixture",
        ],
        vec!["worktree", "add", "-b", "linked", linked.to_str().unwrap()],
    ] {
        let output = Command::new("git")
            .arg("-C")
            .arg(&main)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let repo = context_engine_rs::store::normalize_repo_path(main.to_str().unwrap());
    let mut settings = Settings {
        repos: vec![repo.clone()],
        machine_id: Some("merge-compatibility-test".to_owned()),
        worker_idle_secs: 2,
        mcp_index_wait_secs: 45,
        ..Settings::default()
    };
    settings.embedding.api_keys = vec!["local-test-key".to_owned()];
    settings.embedding.model = "merge-test-model".to_owned();
    settings.embedding.voyage_base_url = Some(format!("http://{gateway_addr}/v1"));
    write_settings_atomic(&config_path(home.path()), &settings).unwrap();
    let addr = e2e_common::start_router(&home).await;
    let client = Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .unwrap();
    let endpoint = format!("http://{addr}/mcp");
    let initialized = client
        .post(&endpoint)
        .header("accept", "application/json, text/event-stream")
        .json(&json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": {"protocolVersion": "2024-11-05", "capabilities": {},
                "clientInfo": {"name": "merge-test", "version": "1"}}
        }))
        .send()
        .await
        .unwrap();
    assert!(initialized.status().is_success());
    let session = initialized.headers()["mcp-session-id"]
        .to_str()
        .unwrap()
        .to_owned();
    initialized.text().await.unwrap();
    let response = client
        .post(&endpoint)
        .header("accept", "application/json, text/event-stream")
        .header("mcp-session-id", &session)
        .header("mcp-protocol-version", "2024-11-05")
        .json(&json!({
            "jsonrpc": "2.0", "id": 2, "method": "tools/call",
            "params": {"name": "codebase-retrieval", "arguments": {
                "information_request": "where is merge_probe defined",
                "workspace_full_path": linked,
                "scope": "docs"
            }, "_meta": {"progressToken": "merge-progress"}}
        }))
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    let wire = response.text().await.unwrap();
    let messages: Vec<Value> = wire
        .lines()
        .filter_map(|line| {
            line.strip_prefix("data:")
                .and_then(|data| serde_json::from_str(data.trim()).ok())
        })
        .collect();
    assert!(
        messages
            .iter()
            .any(|m| m["method"] == "notifications/progress"
                && m["params"]["progressToken"] == "merge-progress"),
        "missing heartbeat: {wire}"
    );
    let result = messages.iter().find(|m| m["id"] == 2).expect("tool result");
    let text = result["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("linked git worktree"), "{text}");
    assert!(text.contains(&repo), "{text}");
    assert!(text.contains("probe.rs#L"), "{text}");
    assert!(
        text.contains("[scope fallback]"),
        "docs scope must reach the worker: {text}"
    );
    let config: Value = client
        .get(format!("http://{addr}/api/config"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(
        config["repos"],
        json!([repo]),
        "linked checkout must not register another index"
    );
}
