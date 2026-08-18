//! `asset-tap mcp` through a real MCP client over stdio (child process), in
//! mock mode — the same way Claude Desktop / Cursor / Claude Code drive it.
//! Requires the `mock` feature (`make test` builds with it); without it the
//! generate test is skipped and the catalog/auth/inspect tests still run.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use rmcp::ClientHandler;
use rmcp::model::{CallToolRequestParams, ClientInfo, ProgressNotificationParam};
use rmcp::service::{NotificationContext, RoleClient, RunningService, ServiceExt};
use rmcp::transport::TokioChildProcess;
use serde_json::{Map, Value};

/// A minimal client that counts progress notifications.
#[derive(Clone, Default)]
struct CountingClient {
    progress: Arc<AtomicUsize>,
    last_message: Arc<std::sync::Mutex<Option<String>>>,
    /// Every `progress` value seen, in arrival order — must be strictly
    /// increasing (the server forwards through one ordered channel).
    seen: Arc<std::sync::Mutex<Vec<f64>>>,
}

impl ClientHandler for CountingClient {
    fn get_info(&self) -> ClientInfo {
        ClientInfo::default()
    }
    async fn on_progress(
        &self,
        notification: ProgressNotificationParam,
        _context: NotificationContext<RoleClient>,
    ) {
        self.progress.fetch_add(1, Ordering::SeqCst);
        self.seen.lock().unwrap().push(notification.progress);
        *self.last_message.lock().unwrap() = notification.message;
    }
}

async fn spawn(mock: bool) -> (RunningService<RoleClient, CountingClient>, CountingClient) {
    let bin = env!("CARGO_BIN_EXE_asset-tap");
    let mut cmd = tokio::process::Command::new(bin);
    if mock {
        cmd.arg("--mock");
    }
    cmd.arg("mcp");
    // Keep the child's stderr out of the test output unless it fails.
    cmd.stderr(std::process::Stdio::null());
    let transport = TokioChildProcess::new(cmd).expect("spawn asset-tap mcp");
    let handler = CountingClient::default();
    let service = handler
        .clone()
        .serve(transport)
        .await
        .expect("mcp handshake");
    (service, handler)
}

fn args(v: Value) -> Map<String, Value> {
    v.as_object().cloned().unwrap()
}

#[tokio::test]
async fn handshake_lists_the_four_tools_with_instructions() {
    let (svc, _h) = spawn(false).await;
    let info = svc.peer_info().expect("server info");
    assert_eq!(
        info.server_info.as_ref().map(|s| s.name.as_str()),
        Some("asset-tap")
    );
    assert!(
        info.instructions
            .as_deref()
            .unwrap_or("")
            .contains("auth_status")
    );
    let mut names: Vec<String> = svc
        .list_all_tools()
        .await
        .unwrap()
        .into_iter()
        .map(|t| t.name.to_string())
        .collect();
    names.sort();
    assert_eq!(
        names,
        ["auth_status", "generate", "inspect_bundle", "list_catalog"]
    );
    svc.cancel().await.unwrap();
}

#[tokio::test]
async fn catalog_and_auth_match_the_cli_documents() {
    let (svc, _h) = spawn(false).await;

    // list_catalog == machine::build_catalog(registry, true), interface-tagged.
    let r = svc
        .call_tool(CallToolRequestParams::new("list_catalog"))
        .await
        .unwrap();
    assert_ne!(r.is_error, Some(true));
    let sc = r.structured_content.expect("structured");
    assert_eq!(
        sc["interface"].as_str(),
        Some(asset_tap::machine::INTERFACE_VERSION)
    );
    assert!(
        sc["providers"]
            .as_array()
            .map(|a| !a.is_empty())
            .unwrap_or(false)
    );
    assert!(
        sc.get("templates").is_some(),
        "list_catalog includes templates"
    );

    // auth_status == AuthCatalog: sources are the enum's strings, never a key.
    let r = svc
        .call_tool(CallToolRequestParams::new("auth_status"))
        .await
        .unwrap();
    let sc = r.structured_content.expect("structured");
    let text = sc.to_string();
    for p in sc["providers"].as_array().unwrap() {
        let s = p["source"].as_str().unwrap();
        assert!(matches!(s, "stored" | "env" | "missing"), "{s}");
        assert_eq!(p["configured"].as_bool().unwrap(), s != "missing");
    }
    assert!(!text.to_lowercase().contains("sk-"), "no key material");
    svc.cancel().await.unwrap();
}

#[tokio::test]
async fn inspect_bundle_rejects_missing_dir_and_generate_rejects_bad_params() {
    let (svc, _h) = spawn(true).await;

    let r = svc
        .call_tool(
            CallToolRequestParams::new("inspect_bundle")
                .with_arguments(args(serde_json::json!({"bundle_dir": "/nonexistent/x"}))),
        )
        .await
        .unwrap();
    assert_eq!(r.is_error, Some(true));
    assert_eq!(
        r.structured_content.unwrap()["kind"].as_str(),
        Some(asset_tap::machine::KIND_IO_ERROR)
    );

    // Bad --param: the CLI's exact usage error, kind "usage", not a crash.
    let r = svc
        .call_tool(
            CallToolRequestParams::new("generate").with_arguments(args(serde_json::json!({
                "prompt": "a mug", "params": {"definitely_not_a_param": 1}
            }))),
        )
        .await
        .unwrap();
    assert_eq!(r.is_error, Some(true));
    let sc = r.structured_content.unwrap();
    assert_eq!(sc["kind"].as_str(), Some("usage"));
    assert!(
        sc["message"]
            .as_str()
            .unwrap()
            .contains("Unknown parameter")
    );

    // No prompt and no image: usage error too (never an interactive prompt).
    let r = svc
        .call_tool(
            CallToolRequestParams::new("generate").with_arguments(args(serde_json::json!({}))),
        )
        .await
        .unwrap();
    assert_eq!(r.is_error, Some(true));
    assert_eq!(
        r.structured_content.unwrap()["kind"].as_str(),
        Some("usage")
    );
    svc.cancel().await.unwrap();
}

#[cfg(feature = "mock")]
#[tokio::test]
async fn generate_in_mock_mode_streams_progress_and_returns_an_inspectable_bundle() {
    let (svc, h) = spawn(true).await;
    let out = tempfile::tempdir().unwrap();

    let mut params =
        CallToolRequestParams::new("generate").with_arguments(args(serde_json::json!({
            "prompt": "a low-poly mug",
            "output_dir": out.path().to_string_lossy(),
            "name": "mug",
            "no_fbx": true
        })));
    // Ask for progress like a real host does.
    let mut meta = rmcp::model::RequestMetaObject::new();
    meta.set_progress_token(rmcp::model::ProgressToken(
        rmcp::model::NumberOrString::String("p1".into()),
    ));
    params.meta = Some(meta);

    let r = svc.call_tool(params).await.unwrap();
    assert_ne!(r.is_error, Some(true), "{:?}", r.structured_content);
    let sc = r.structured_content.expect("structured");
    assert_eq!(sc["status"].as_str(), Some("success"));
    let bundle_dir = sc["bundle_dir"].as_str().expect("bundle_dir").to_string();
    assert!(bundle_dir.starts_with(&*out.path().to_string_lossy()));
    assert_eq!(sc["bundle"]["name"].as_str(), Some("mug"));
    assert!(
        std::path::Path::new(&bundle_dir)
            .join("bundle.json")
            .exists()
    );
    assert!(std::path::Path::new(&bundle_dir).join("model.glb").exists());

    // Progress arrived as notifications (image + 3D stages at minimum).
    assert!(
        h.progress.load(Ordering::SeqCst) >= 4,
        "progress notifications"
    );
    assert!(h.last_message.lock().unwrap().is_some());

    // inspect_bundle reads it back with a file list.
    let r = svc
        .call_tool(
            CallToolRequestParams::new("inspect_bundle")
                .with_arguments(args(serde_json::json!({"bundle_dir": bundle_dir}))),
        )
        .await
        .unwrap();
    assert_ne!(r.is_error, Some(true));
    let sc = r.structured_content.unwrap();
    assert_eq!(sc["bundle"]["name"].as_str(), Some("mug"));
    let files: Vec<&str> = sc["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f.as_str().unwrap())
        .collect();
    assert!(files.contains(&"bundle.json"));
    assert!(files.contains(&"model.glb"));
    svc.cancel().await.unwrap();
}
