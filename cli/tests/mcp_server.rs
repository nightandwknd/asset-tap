//! `asset-tap mcp` through a real MCP client over stdio (child process), in
//! mock mode — the same way Claude Desktop / Cursor / Claude Code drive it.
//! Requires the `mock` feature (`make test` builds with it); without it the
//! generate test is skipped and the catalog/auth/inspect tests still run.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use rmcp::ClientHandler;
use rmcp::model::{CallToolRequestParams, ClientConfig, ProgressNotificationParam};
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
    fn get_info(&self) -> ClientConfig {
        ClientConfig::default()
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
async fn handshake_lists_the_tools_with_instructions() {
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
        [
            "auth_status",
            "clip_download",
            "generate",
            "inspect_bundle",
            "list_catalog"
        ]
    );
    assert!(
        info.instructions
            .as_deref()
            .unwrap_or("")
            .contains("clip_download")
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
    assert!(
        sc["clips"].is_array(),
        "list_catalog includes the installed clip ids (what generate.clips[] accepts)"
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
            "name": "mug"
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

/// `install` reaches the same pre-flight the CLI runs before `start`, so a
/// path whose extension can't be what the run produces is a usage error here
/// too — not a surprise after a paid generation.
#[tokio::test]
async fn install_with_the_wrong_extension_is_a_usage_error() {
    let (svc, _h) = spawn(false).await;
    let r = svc
        .call_tool(
            CallToolRequestParams::new("generate").with_arguments(args(serde_json::json!({
                "prompt": "a mug",
                "image_only": true,
                "install": "/tmp/asset-tap-mcp-install-test.glb",
            }))),
        )
        .await
        .unwrap();
    assert_eq!(r.is_error, Some(true));
    let sc = r.structured_content.unwrap();
    assert_eq!(sc["kind"].as_str(), Some("usage"));
    let message = sc["message"].as_str().unwrap();
    assert!(message.contains("--install"), "{message}");
    assert!(message.contains(".png"), "{message}");
    svc.cancel().await.unwrap();
}

/// A `params` entry whose condition the model declares as unmet is the same
/// usage error the CLI raises for `--param`, because MCP goes through
/// `resolve_param_overrides` too — the host gets told before anything is paid
/// for, rather than after Meshy rejects the request.
#[tokio::test]
async fn conditional_param_violation_is_a_usage_error() {
    // Mock mode: the check must fire before any provider key is consulted,
    // and CI has no Meshy key, so a real-mode spawn would report
    // `missing_api_key` first and never reach the condition.
    let (svc, _h) = spawn(true).await;
    let r = svc
        .call_tool(
            CallToolRequestParams::new("generate").with_arguments(args(serde_json::json!({
                "prompt": "a mug",
                "model_3d": "meshy/v6/image-to-3d",
                // origin_at only applies with auto_size on, which is off here.
                "params": { "origin_at": "center" },
            }))),
        )
        .await
        .unwrap();
    assert_eq!(r.is_error, Some(true));
    let sc = r.structured_content.unwrap();
    assert_eq!(sc["kind"].as_str(), Some("usage"));
    let message = sc["message"].as_str().unwrap();
    assert!(message.contains("origin_at"), "{message}");
    assert!(message.contains("auto_size"), "{message}");
    svc.cancel().await.unwrap();
}

/// The happy path: `install` is mapped to `--install` and the primary
/// artifact lands at the exact path the caller named, alongside the bundle.
#[cfg(feature = "mock")]
#[tokio::test]
async fn install_copies_the_primary_artifact_to_the_named_path() {
    let (svc, _h) = spawn(true).await;
    let out = tempfile::tempdir().unwrap();
    let installed = out.path().join("concept.png");

    let r = svc
        .call_tool(
            CallToolRequestParams::new("generate").with_arguments(args(serde_json::json!({
                "prompt": "a low-poly mug",
                "image_only": true,
                "output_dir": out.path().to_string_lossy(),
                "install": installed.to_string_lossy(),
            }))),
        )
        .await
        .unwrap();
    assert_ne!(r.is_error, Some(true), "{:?}", r.structured_content);
    let sc = r.structured_content.expect("structured");
    assert_eq!(sc["status"].as_str(), Some("success"));
    assert!(installed.is_file(), "{} not written", installed.display());
    // The bundle is still written in full; --install is a copy, not a move.
    let bundle_dir = sc["bundle_dir"].as_str().expect("bundle_dir");
    assert!(std::path::Path::new(bundle_dir).join("image.png").exists());
    svc.cancel().await.unwrap();
}

#[tokio::test]
async fn clip_download_returns_the_cli_document() {
    let clips = tempfile::tempdir().unwrap();
    let packs = format!("{}/../packs", env!("CARGO_MANIFEST_DIR"));
    let bin = env!("CARGO_BIN_EXE_asset-tap");
    let mut cmd = tokio::process::Command::new(bin);
    cmd.arg("mcp");
    cmd.env("ASSET_TAP_CLIPS_DIR", clips.path());
    cmd.env("ASSET_TAP_CLIP_PACKS_DIR", &packs);
    cmd.stderr(std::process::Stdio::null());
    let transport = TokioChildProcess::new(cmd).expect("spawn asset-tap mcp");
    let handler = CountingClient::default();
    let svc = handler
        .clone()
        .serve(transport)
        .await
        .expect("mcp handshake");

    let r = svc
        .call_tool(CallToolRequestParams::new("clip_download"))
        .await
        .unwrap();
    assert_ne!(r.is_error, Some(true));
    let sc = r.structured_content.expect("structured");
    assert_eq!(sc["status"].as_str(), Some("success"));
    assert_eq!(sc["already_exists"], false);
    let installed: Vec<&str> = sc["installed"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert!(installed.contains(&"ual1"));
    assert!(installed.contains(&"ual2"));

    let r = svc
        .call_tool(CallToolRequestParams::new("clip_download"))
        .await
        .unwrap();
    let sc = r.structured_content.expect("structured");
    assert_eq!(sc["already_exists"], true);
    assert_eq!(sc["installed"].as_array().map(|a| a.len()), Some(0));
    svc.cancel().await.unwrap();
}
