//! Generic HTTP client for executing provider configs.

use super::config::{HttpMethod, ModelConfig, PollingConfig, ProviderConfig, ResponseType};
use crate::constants::files::bundle as bundle_files;
use crate::constants::http::{headers, mime};
use crate::constants::polling;
use crate::types::{Progress, Stage};
use anyhow::{Context, Result, anyhow};
use reqwest::multipart;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::mpsc::UnboundedSender;

/// Maximum download size (500 MB). Prevents resource exhaustion from malicious servers.
const MAX_DOWNLOAD_SIZE: u64 = 500 * 1024 * 1024;

/// Extract the host from a URL string, handling IPv6 brackets and userinfo.
///
/// Examples:
/// - `http://example.com/path` → `"example.com"`
/// - `http://example.com:8080/path` → `"example.com"`
/// - `http://[::1]:8080/path` → `"::1"`
/// - `http://user:pass@example.com/path` → `"example.com"`
/// - `http://user@[::1]/path` → `"::1"`
fn extract_host(url: &str) -> Option<String> {
    let after_scheme = url.split("://").nth(1)?;
    let authority = after_scheme.split('/').next().unwrap_or("");

    // Strip userinfo (user:pass@)
    let host_port = if let Some(at_pos) = authority.rfind('@') {
        &authority[at_pos + 1..]
    } else {
        authority
    };

    // Handle IPv6 bracket notation: [::1] or [::1]:8080
    if host_port.starts_with('[') {
        let end_bracket = host_port.find(']')?;
        Some(host_port[1..end_bracket].to_string())
    } else {
        // IPv4 or hostname — split off port
        Some(host_port.split(':').next().unwrap_or("").to_string())
    }
}

/// Validate that a URL from an API response is safe to fetch.
///
/// Rejects non-HTTP(S) schemes and URLs pointing to private/internal IP ranges
/// to prevent SSRF attacks via malicious API responses.
/// Test-only switch to bypass SSRF validation so unit tests can point at a
/// localhost wiremock server without depending on the process-global
/// `MOCK_API` env var (which other tests mutate, causing cross-test flakiness).
#[cfg(test)]
pub(super) static SKIP_URL_VALIDATION_FOR_TEST: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

fn validate_download_url(url: &str) -> Result<()> {
    #[cfg(test)]
    if SKIP_URL_VALIDATION_FOR_TEST.load(Ordering::Relaxed) {
        return Ok(());
    }

    // Skip validation in mock mode (mock server runs on localhost)
    #[cfg(feature = "mock")]
    if crate::api::is_mock_mode() {
        return Ok(());
    }

    // Must be http or https
    if !url.starts_with("https://") && !url.starts_with("http://") {
        return Err(anyhow!(
            "Unsafe URL scheme (only http/https allowed): {}",
            url
        ));
    }

    // Hostnames are case-insensitive; normalize so `LOCALHOST`, `.LOCAL`, etc.
    // can't slip past the string checks below.
    let host = extract_host(url).unwrap_or_default().to_ascii_lowercase();

    // Block empty host
    if host.is_empty() {
        return Err(anyhow!("URL has no host: {}", url));
    }

    // Block private/reserved hostnames
    if host == "localhost" || host.ends_with(".local") || host.ends_with(".internal") {
        return Err(anyhow!("URL points to local/internal host: {}", host));
    }

    // Block private/reserved IP ranges (handles both IPv4 and IPv6). `parse`
    // handles dotted-quad and standard IPv6; `normalize_ip_host` additionally
    // catches the non-dotted integer forms (decimal `2130706433`, hex
    // `0x7f000001`) that resolve to 127.0.0.1 but don't parse as IpAddr.
    let parsed_ip = host
        .parse::<std::net::IpAddr>()
        .ok()
        .or_else(|| normalize_ip_host(&host));
    if let Some(ip) = parsed_ip {
        let is_private = match ip {
            std::net::IpAddr::V4(v4) => {
                v4.is_loopback()             // 127.0.0.0/8
                || v4.is_private()           // 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
                || v4.is_link_local()        // 169.254.0.0/16
                || v4.is_unspecified()       // 0.0.0.0
                || v4.octets()[0] == 100 && (v4.octets()[1] & 0xC0) == 64 // 100.64.0.0/10 (CGNAT)
            }
            std::net::IpAddr::V6(v6) => {
                v6.is_loopback()             // ::1
                || v6.is_unspecified()       // ::
                // IPv4-mapped IPv6 (::ffff:127.0.0.1, ::ffff:10.0.0.1, etc.)
                || if let Some(v4) = v6.to_ipv4_mapped() {
                    v4.is_loopback() || v4.is_private() || v4.is_link_local() || v4.is_unspecified()
                } else {
                    false
                }
            }
        };
        if is_private {
            return Err(anyhow!("URL points to private/reserved IP: {}", ip));
        }
    }

    Ok(())
}

/// Interpret a host string as one of the non-dotted IPv4 integer notations that
/// browsers/`curl` accept — bare decimal (`2130706433`), hex (`0x7f000001`), or
/// octal (`017700000001`) — and return the equivalent [`std::net::IpAddr`].
///
/// These forms bypass a naive `host.parse::<IpAddr>()` (which only accepts
/// dotted-quad), so an attacker could otherwise smuggle `http://2130706433/`
/// past the loopback check. Returns `None` for anything that isn't a bare
/// integer host, including normal DNS names.
fn normalize_ip_host(host: &str) -> Option<std::net::IpAddr> {
    let value = if let Some(hex) = host.strip_prefix("0x").or_else(|| host.strip_prefix("0X")) {
        u32::from_str_radix(hex, 16).ok()?
    } else if host.len() > 1 && host.starts_with('0') && host.bytes().all(|b| b.is_ascii_digit()) {
        u32::from_str_radix(host, 8).ok()?
    } else if host.bytes().all(|b| b.is_ascii_digit()) {
        host.parse::<u32>().ok()?
    } else {
        return None;
    };
    Some(std::net::IpAddr::V4(std::net::Ipv4Addr::from(value)))
}

/// Structured HTTP error carrying request/response context.
///
/// Wraps the raw HTTP context (URL, status, body) available at error sites.
/// Converted to [`crate::types::ApiError`] in `DynamicProvider` where the
/// provider name is known.
#[derive(Debug, Clone)]
pub struct HttpError {
    /// The URL that was requested.
    pub url: String,
    /// HTTP method used.
    pub method: String,
    /// HTTP status code (None for network errors or queue failures).
    pub status_code: Option<u16>,
    /// Response body or error detail.
    pub body: String,
    /// Whether this was a queue/processing failure (HTTP 200 but provider reported FAILED).
    pub is_queue_failure: bool,
}

impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(status) = self.status_code {
            write!(f, "HTTP {} at {}: {}", status, self.url, self.body)
        } else if self.is_queue_failure {
            write!(f, "Generation failed: {}", self.body)
        } else {
            write!(f, "Request to {} failed: {}", self.url, self.body)
        }
    }
}

impl std::error::Error for HttpError {}

/// Outcome of a single failed poll attempt. `retryable` distinguishes a
/// transient blip (retry with backoff) from a terminal error (fail fast so we
/// don't waste the whole retry budget on something that can't succeed).
struct PollAttemptError {
    retryable: bool,
    error: anyhow::Error,
}

/// Context for sending progress updates during polling.
struct PollingProgress {
    tx: UnboundedSender<Progress>,
    stage: Stage,
}

impl PollingProgress {
    fn send(&self, progress: Progress) {
        let _ = self.tx.send(progress);
    }
}

/// Resolve a URL that may be relative or absolute against an optional base URL.
///
/// If the path already starts with `http://` or `https://`, it is returned as-is.
/// Otherwise it is joined with the base URL, handling trailing/leading slashes.
pub fn resolve_url(base_url: Option<&str>, path: &str) -> String {
    if path.starts_with("http://") || path.starts_with("https://") {
        path.to_string()
    } else {
        let base = base_url.unwrap_or("").trim_end_matches('/');
        let path = path.trim_start_matches('/');
        if base.is_empty() {
            path.to_string()
        } else if path.is_empty() {
            base.to_string()
        } else {
            format!("{}/{}", base, path)
        }
    }
}

/// Merge user parameter overrides into a request body and strip null values.
///
/// Rules:
/// - Only keys declared in `parameter_defs` can be overridden (prevents
///   injection of arbitrary fields like auth headers or prompts).
/// - A null override removes the key instead of inserting null, so providers
///   fall back to their server-side default.
/// - Template-null values (e.g. `seed: null` in YAML) are always stripped
///   before the request is sent, for the same reason.
///
/// Mutates `body` in place. Non-object bodies pass through untouched.
pub(crate) fn apply_param_overrides(
    body: &mut serde_json::Value,
    params: Option<&HashMap<String, serde_json::Value>>,
    parameter_defs: &[super::config::ParameterDef],
    model_id: &str,
) {
    if let (Some(params), Some(obj)) = (params, body.as_object_mut()) {
        let allowed: std::collections::HashSet<&str> =
            parameter_defs.iter().map(|p| p.name.as_str()).collect();
        for (key, value) in params {
            if !allowed.contains(key.as_str()) {
                tracing::warn!(
                    "Ignoring undeclared parameter '{}' for model '{}'",
                    key,
                    model_id
                );
                continue;
            }
            if value.is_null() {
                obj.remove(key);
            } else {
                obj.insert(key.clone(), value.clone());
            }
        }
    }

    // Strip any remaining nulls (template defaults like `seed: null`).
    if let Some(obj) = body.as_object_mut() {
        obj.retain(|_, v| !v.is_null());
    }
}

/// Generic HTTP client that executes provider configurations.
#[derive(Clone)]
pub struct HttpProviderClient {
    config: ProviderConfig,
    client: reqwest::Client,
    /// Shared cancel flag checked during polling loops.
    cancel_flag: Arc<AtomicBool>,
}

impl HttpProviderClient {
    /// Create a new HTTP provider client.
    pub fn new(config: ProviderConfig) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(polling::DEFAULT_HTTP_TIMEOUT_SECS))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()
            .expect("Failed to create HTTP client");

        Self {
            config,
            client,
            cancel_flag: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Set a shared cancel flag that will be checked during polling loops.
    /// When the flag is set to `true`, polling will abort and send a cancel
    /// request to the server.
    pub fn set_cancel_flag(&mut self, flag: Arc<AtomicBool>) {
        self.cancel_flag = flag;
    }

    /// Clone of the shared cancel flag, so a rebuilt client can inherit the
    /// wiring an active pipeline installed via [`Self::set_cancel_flag`].
    pub fn cancel_flag(&self) -> Arc<AtomicBool> {
        self.cancel_flag.clone()
    }

    /// Resolve a relative or absolute URL against this provider's base URL.
    fn resolve_url(&self, path: &str) -> String {
        resolve_url(self.config.provider.base_url.as_deref(), path)
    }

    /// Generate an image using text-to-image model.
    pub async fn generate_image(
        &self,
        prompt: &str,
        model_id: &str,
        params: Option<&HashMap<String, serde_json::Value>>,
        progress: UnboundedSender<Progress>,
    ) -> Result<Vec<u8>> {
        tracing::debug!(
            "generate_image called with model_id: {}, base_url: {:?}",
            model_id,
            self.config.provider.base_url
        );

        let model = self
            .config
            .text_to_image
            .iter()
            .find(|m| m.id == model_id)
            .ok_or_else(|| anyhow!("Model not found: {}", model_id))?;

        tracing::debug!(
            "Found model: {} with endpoint: {}",
            model.id,
            model.endpoint
        );

        let polling_progress = Some(PollingProgress {
            tx: progress,
            stage: Stage::ImageGeneration,
        });
        self.execute_model(model, &[("prompt", prompt)], params, polling_progress)
            .await
    }

    /// Generate a 3D model using image-to-3D model with file upload.
    pub async fn generate_3d(
        &self,
        image_path: &Path,
        model_id: &str,
        params: Option<&HashMap<String, serde_json::Value>>,
        progress: UnboundedSender<Progress>,
    ) -> Result<Vec<u8>> {
        let model = self
            .config
            .image_to_3d
            .iter()
            .find(|m| m.id == model_id)
            .ok_or_else(|| anyhow!("Model not found: {}", model_id))?;

        let polling_progress = Some(PollingProgress {
            tx: progress,
            stage: Stage::Model3DGeneration,
        });
        self.execute_model_with_file(model, image_path, params, polling_progress)
            .await
    }

    /// Execute a model with an image URL parameter.
    pub async fn execute_model_with_url(
        &self,
        model: &ModelConfig,
        image_url: &str,
        params: Option<&HashMap<String, serde_json::Value>>,
        progress: UnboundedSender<Progress>,
    ) -> Result<Vec<u8>> {
        let polling_progress = Some(PollingProgress {
            tx: progress,
            stage: Stage::Model3DGeneration,
        });
        self.execute_model(model, &[("image_url", image_url)], params, polling_progress)
            .await
    }

    /// Upload image bytes and get a public URL using the provider's upload config.
    pub async fn upload_image(&self, image_data: &[u8]) -> Result<String> {
        let upload_config = self
            .config
            .provider
            .upload
            .as_ref()
            .ok_or_else(|| anyhow!("Provider does not support file uploads"))?;

        let url = self.resolve_url(&upload_config.endpoint);

        use super::config::UploadType;
        match upload_config.request.upload_type {
            UploadType::Multipart => self.upload_multipart(&url, image_data, upload_config).await,
            UploadType::InitiateThenPut => {
                self.upload_initiate_then_put(&url, image_data, upload_config)
                    .await
            }
        }
    }

    /// Execute a model configuration.
    async fn execute_model(
        &self,
        model: &ModelConfig,
        variables: &[(&str, &str)],
        params: Option<&HashMap<String, serde_json::Value>>,
        polling_progress: Option<PollingProgress>,
    ) -> Result<Vec<u8>> {
        // Build endpoint URL
        let url = self.resolve_url(&model.endpoint);

        tracing::debug!("Executing model request to: {}", url);

        // Build request
        let mut request = match model.method {
            HttpMethod::GET => self.client.get(&url),
            HttpMethod::POST => self.client.post(&url),
            HttpMethod::PUT => self.client.put(&url),
            HttpMethod::DELETE => self.client.delete(&url),
            HttpMethod::PATCH => self.client.patch(&url),
        };

        // Add headers with interpolation
        for (key, value_template) in &model.request.headers {
            let value = self
                .interpolate(value_template, variables)
                .with_context(|| {
                    format!("Failed to interpolate header {}: {}", key, value_template)
                })?;
            request = request.header(key, value);
        }

        // If model has no Authorization header, inject provider-level auth.
        // This ensures discovered models (which have empty headers) still authenticate.
        if !model.request.headers.contains_key(headers::AUTHORIZATION)
            && let Some(auth_value) = self.config.format_auth_header()
        {
            request = request.header(headers::AUTHORIZATION, auth_value);
        }

        // Add body if present
        if let Some(body_template) = &model.request.body {
            let mut body = self.interpolate_json(body_template, variables)?;
            apply_param_overrides(&mut body, params, &model.parameters, &model.id);
            request = request.json(&body);
        }

        // Send request
        let response = request
            .send()
            .await
            .with_context(|| format!("Failed to send HTTP request to {}", url))?;

        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await.unwrap_or_default();
            tracing::error!(
                http.url = %url,
                http.method = ?model.method,
                http.status = %status.as_u16(),
                "HTTP error {}: {}", status, error_body
            );
            return Err(HttpError {
                url: url.clone(),
                method: format!("{:?}", model.method),
                status_code: Some(status.as_u16()),
                body: error_body,
                is_queue_failure: false,
            }
            .into());
        }

        // Resolve auth headers for use in polling requests
        let auth_headers = self.resolve_auth_headers(&model.request.headers);

        // Extract result based on response type
        self.extract_response(response, &model.response, &auth_headers, polling_progress)
            .await
    }

    /// Execute a model with file upload.
    async fn execute_model_with_file(
        &self,
        model: &ModelConfig,
        file_path: &Path,
        params: Option<&HashMap<String, serde_json::Value>>,
        polling_progress: Option<PollingProgress>,
    ) -> Result<Vec<u8>> {
        let url = self.resolve_url(&model.endpoint);

        // Build multipart form
        let multipart_config = model
            .request
            .multipart
            .as_ref()
            .ok_or_else(|| anyhow!("Model does not support file uploads"))?;

        let file_bytes = tokio::fs::read(file_path)
            .await
            .context("Failed to read file")?;

        let file_name = file_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file");

        let file_part = multipart::Part::bytes(file_bytes).file_name(file_name.to_string());

        let mut form = multipart::Form::new().part(multipart_config.file_field.clone(), file_part);

        // User param overrides, restricted to the model's declared parameters
        // (same allow-list guard as the JSON-body path) so tuned slider/`--param`
        // values reach multipart models instead of being silently dropped.
        let allowed: std::collections::HashSet<&str> =
            model.parameters.iter().map(|p| p.name.as_str()).collect();
        let overrides: HashMap<&str, &serde_json::Value> = params
            .map(|p| {
                p.iter()
                    .filter(|(k, _)| allowed.contains(k.as_str()))
                    .map(|(k, v)| (k.as_str(), v))
                    .collect()
            })
            .unwrap_or_default();

        // Add additional fields, letting a matching override replace the
        // template's value. JSON strings render without quotes; other scalars
        // via their JSON representation.
        for (key, value_template) in &multipart_config.fields {
            let value = match overrides.get(key.as_str()) {
                Some(serde_json::Value::Null) => continue, // unset → omit field
                Some(v) => v
                    .as_str()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| v.to_string()),
                None => self.interpolate(value_template, &[])?,
            };
            form = form.text(key.clone(), value);
        }

        // Build request
        let mut request = self.client.post(&url);

        // Add headers (excluding Content-Type, which is set by multipart)
        for (key, value_template) in &model.request.headers {
            if !key.eq_ignore_ascii_case(headers::CONTENT_TYPE) {
                let value = self.interpolate(value_template, &[])?;
                request = request.header(key, value);
            }
        }

        // If model has no Authorization header, inject provider-level auth
        if !model.request.headers.contains_key(headers::AUTHORIZATION)
            && let Some(auth_value) = self.config.format_auth_header()
        {
            request = request.header(headers::AUTHORIZATION, auth_value);
        }

        // Send request
        let response = request
            .multipart(form)
            .send()
            .await
            .context("Failed to send multipart request")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await.unwrap_or_default();
            return Err(HttpError {
                url: url.clone(),
                method: "POST".to_string(),
                status_code: Some(status.as_u16()),
                body: error_body,
                is_queue_failure: false,
            }
            .into());
        }

        // Resolve auth headers for use in polling requests
        let auth_headers = self.resolve_auth_headers(&model.request.headers);

        // Extract result
        self.extract_response(response, &model.response, &auth_headers, polling_progress)
            .await
    }

    /// Extract response based on template.
    async fn extract_response(
        &self,
        response: reqwest::Response,
        template: &super::config::ResponseTemplate,
        auth_headers: &HashMap<String, String>,
        polling_progress: Option<PollingProgress>,
    ) -> Result<Vec<u8>> {
        match template.response_type {
            ResponseType::Binary => {
                // Direct binary response
                let bytes = response.bytes().await.context("Failed to read response")?;
                Ok(bytes.to_vec())
            }
            ResponseType::Base64 => {
                // Extract base64 from JSON response
                let json: serde_json::Value = response.json().await?;
                let base64_str = self.extract_json_field(&json, template.field.as_deref())?;
                use base64::Engine;
                let decoded = base64::engine::general_purpose::STANDARD
                    .decode(base64_str.trim())
                    .context("Failed to decode base64 response")?;
                Ok(decoded)
            }
            ResponseType::Json => {
                // Extract URL from JSON and download
                let json: serde_json::Value = response.json().await?;
                let url = self.extract_json_field(&json, template.field.as_deref())?;
                self.download_file(&url).await
            }
            ResponseType::Url => {
                // Response body is the URL
                let url = response.text().await?;
                self.download_file(&url).await
            }
            ResponseType::Polling => {
                // Polling-based async response
                let json: serde_json::Value = response.json().await?;
                let polling = template
                    .polling
                    .as_ref()
                    .ok_or_else(|| anyhow!("Polling config required for polling response type"))?;
                self.poll_for_result(&json, polling, auth_headers, polling_progress)
                    .await
            }
        }
    }

    /// Extract a field from JSON response.
    fn extract_json_field(&self, json: &serde_json::Value, field: Option<&str>) -> Result<String> {
        let field_path = field.unwrap_or("");

        if field_path.is_empty() {
            // Return entire JSON as string
            return Ok(json.to_string());
        }

        // Simple JSONPath-like extraction (supports "field", "field.nested", "array[0]")
        let parts: Vec<&str> = field_path.split('.').collect();
        let mut current = json;

        for part in parts {
            // Check for array index
            if let Some(idx_start) = part.find('[') {
                let idx_end = part.find(']').ok_or_else(|| {
                    anyhow!("Missing closing bracket in field path: {}", field_path)
                })?;
                let field_name = &part[..idx_start];
                let idx_str = &part[idx_start + 1..idx_end];
                let idx: usize = idx_str.parse().context("Invalid array index")?;

                current = current
                    .get(field_name)
                    .and_then(|v| v.get(idx))
                    .ok_or_else(|| anyhow!("Field not found: {}", field_path))?;
            } else {
                current = current
                    .get(part)
                    .ok_or_else(|| anyhow!("Field not found: {}", field_path))?;
            }
        }

        // Convert to string
        match current {
            serde_json::Value::String(s) => Ok(s.clone()),
            other => Ok(other.to_string()),
        }
    }

    /// Substitute `${field}` tokens in a template string with values pulled
    /// from a JSON response via `extract_json_field`. Supports nested paths
    /// (e.g. `${data.id}`) and array indexing (e.g. `${items[0]}`).
    fn substitute_response_fields(
        &self,
        template: &str,
        json: &serde_json::Value,
    ) -> Result<String> {
        let mut out = String::with_capacity(template.len());
        let mut rest = template;
        while let Some(start) = rest.find("${") {
            out.push_str(&rest[..start]);
            let after = &rest[start + 2..];
            let end = after
                .find('}')
                .ok_or_else(|| anyhow!("Unterminated ${{ in status_url_template: {}", template))?;
            let field_path = &after[..end];
            let value = self.extract_json_field(json, Some(field_path))?;
            out.push_str(&value);
            rest = &after[end + 1..];
        }
        out.push_str(rest);
        Ok(out)
    }

    /// Resolve auth headers for use in polling and other authenticated requests.
    ///
    /// If the model's headers include an Authorization header, uses those.
    /// Otherwise falls back to provider-level API key authentication.
    /// This ensures discovered models (which have empty headers) still authenticate.
    fn resolve_auth_headers(
        &self,
        model_headers: &HashMap<String, String>,
    ) -> HashMap<String, String> {
        let mut resolved = HashMap::new();
        for (key, value_template) in model_headers {
            if let Ok(value) = self.interpolate(value_template, &[]) {
                resolved.insert(key.clone(), value);
            }
        }

        // If no Authorization header from model, inject provider-level auth
        if !resolved.contains_key(headers::AUTHORIZATION)
            && let Some(auth_value) = self.config.format_auth_header()
        {
            resolved.insert(headers::AUTHORIZATION.to_string(), auth_value);
        }

        resolved
    }

    /// Apply resolved headers to a request builder.
    fn apply_headers(
        &self,
        mut request: reqwest::RequestBuilder,
        headers: &HashMap<String, String>,
    ) -> reqwest::RequestBuilder {
        for (key, value) in headers {
            request = request.header(key, value);
        }
        request
    }

    /// Perform a single poll request and return the parsed status JSON.
    ///
    /// On failure, returns a [`PollAttemptError`] whose `retryable` flag tells
    /// the caller whether to back off and retry or fail fast:
    /// - **Retryable**: connection error, request timeout, 5xx, or 429 (the
    ///   task is still running remotely; the blip is on the network/server).
    /// - **Terminal**: any other 4xx (400/401/403/404, …) — a bad request or
    ///   auth/URL problem that retrying can't fix, so we don't burn the retry
    ///   budget on it.
    async fn poll_once(
        &self,
        poll_url: &str,
        auth_headers: &HashMap<String, String>,
    ) -> std::result::Result<serde_json::Value, PollAttemptError> {
        let request = self.client.get(poll_url);
        let request = self.apply_headers(request, auth_headers);
        let response = match request.send().await {
            Ok(r) => r,
            // Transport-level failures (connection reset, timeout, DNS) are
            // always transient — the remote task keeps running.
            Err(e) => {
                return Err(PollAttemptError {
                    retryable: true,
                    error: anyhow::Error::new(e)
                        .context(format!("Poll request to {} failed", poll_url)),
                });
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            // 5xx and 429 (rate limit) are transient; other 4xx are terminal.
            let retryable =
                status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS;
            let error_body = response.text().await.unwrap_or_default();
            tracing::warn!(
                http.url = %poll_url,
                http.status = %status.as_u16(),
                retryable,
                "Polling request returned HTTP {}: {}", status, error_body
            );
            return Err(PollAttemptError {
                retryable,
                error: HttpError {
                    url: poll_url.to_string(),
                    method: "GET".to_string(),
                    status_code: Some(status.as_u16()),
                    body: error_body,
                    is_queue_failure: false,
                }
                .into(),
            });
        }

        // A body that won't parse as JSON is treated as transient — a
        // truncated response or a transient proxy error page can cause it, and
        // a retry against a healthy backend will usually succeed.
        response.json().await.map_err(|e| PollAttemptError {
            retryable: true,
            error: anyhow::Error::new(e)
                .context(format!("Failed to parse poll response from {}", poll_url)),
        })
    }

    async fn poll_for_result(
        &self,
        initial_response: &serde_json::Value,
        polling: &PollingConfig,
        auth_headers: &HashMap<String, String>,
        progress: Option<PollingProgress>,
    ) -> Result<Vec<u8>> {
        let status_url = if let Some(template) = polling.status_url_template.as_deref() {
            // Template-based poll URL: substitute ${field} tokens from initial response.
            // Used by providers (e.g., Meshy) that return only a task id, not a full URL.
            self.substitute_response_fields(template, initial_response)?
        } else {
            self.extract_json_field(initial_response, Some(&polling.status_field))?
        };

        // Handle relative status URLs
        let full_status_url = self.resolve_url(&status_url);

        // Validate poll URL to prevent SSRF via malicious API responses
        validate_download_url(&full_status_url)?;

        // Append provider-specific query params to poll URL (e.g., ?logs=1 for fal.ai)
        let poll_url = if let Some(ref params) = polling.poll_query_params {
            if full_status_url.contains('?') {
                format!("{}&{}", full_status_url, params.trim_start_matches('?'))
            } else {
                let separator = if params.starts_with('?') { "" } else { "?" };
                format!("{}{}{}", full_status_url, separator, params)
            }
        } else {
            full_status_url.clone()
        };

        tracing::info!(
            "Polling for result (interval: {}ms, max: {} attempts)",
            polling.interval_ms,
            polling.max_attempts
        );

        // Emit initial queued status
        if let Some(ref p) = progress {
            p.send(Progress::queued(p.stage, 0));
        }

        let poll_start = std::time::Instant::now();
        let mut last_status = String::new();
        let mut seen_log_count = 0;
        let mut last_console_log = std::time::Instant::now();
        // Consecutive transient failures (network error / 5xx). Reset to 0 on
        // any successful poll. When it exceeds the cap we give up and cancel.
        let mut consecutive_failures: u32 = 0;

        for attempt in 0..polling.max_attempts {
            // Check cancel flag at the top of every iteration so a cancel
            // request that lands while we were sleeping (or before the loop
            // even starts) is honored before the next HTTP call.
            if self.cancel_flag.load(Ordering::Relaxed) {
                tracing::info!("Cancel flag detected during polling — cancelling server request");
                self.send_cancel_request(&full_status_url, polling, auth_headers)
                    .await;
                return Err(anyhow!("Generation cancelled by user"));
            }

            // Poll first, sleep last. Real APIs that return COMPLETED on the
            // first request (cached results, fast-path responses) should not
            // pay an arbitrary `interval_ms` of latency before we even ask.
            // The mock server returns COMPLETED instantly, so this also makes
            // mock-mode pipeline tests run effectively at memory speed instead
            // of paying 1-2s per polling stage.
            //
            // A paid task is already running remotely, so a *transient* network
            // error or 5xx/429 here is retried (with backoff) up to
            // MAX_CONSECUTIVE_POLL_FAILURES rather than aborting the whole
            // generation on the first blip. A *terminal* error (4xx that isn't
            // 429) fails fast — retrying a 401/404 can't succeed, so there's no
            // point spending the retry budget on it.
            let json = match self.poll_once(&poll_url, auth_headers).await {
                Ok(json) => {
                    consecutive_failures = 0;
                    json
                }
                Err(poll_err) if !poll_err.retryable => {
                    tracing::error!("Terminal polling error, aborting: {}", poll_err.error);
                    self.send_cancel_request(&full_status_url, polling, auth_headers)
                        .await;
                    return Err(poll_err.error.context("Polling failed (request cancelled)"));
                }
                Err(poll_err) => {
                    consecutive_failures += 1;
                    if consecutive_failures
                        > crate::constants::errors::MAX_CONSECUTIVE_POLL_FAILURES
                    {
                        tracing::error!(
                            "Giving up after {} consecutive poll failures: {}",
                            consecutive_failures,
                            poll_err.error
                        );
                        self.send_cancel_request(&full_status_url, polling, auth_headers)
                            .await;
                        return Err(poll_err.error.context(format!(
                            "Polling failed after {} consecutive transient errors (request cancelled)",
                            consecutive_failures
                        )));
                    }

                    // Exponential backoff capped at MAX_POLL_RETRY_BACKOFF_SECS.
                    let backoff = (crate::constants::errors::POLL_RETRY_BASE_BACKOFF_SECS
                        << (consecutive_failures - 1).min(6))
                    .min(crate::constants::errors::MAX_POLL_RETRY_BACKOFF_SECS);
                    tracing::warn!(
                        "Transient poll failure {}/{}, retrying in {}s: {}",
                        consecutive_failures,
                        crate::constants::errors::MAX_CONSECUTIVE_POLL_FAILURES,
                        backoff,
                        poll_err.error
                    );
                    if let Some(ref p) = progress {
                        p.send(Progress::retrying(
                            p.stage,
                            consecutive_failures,
                            crate::constants::errors::MAX_CONSECUTIVE_POLL_FAILURES,
                            backoff,
                            "Temporary network issue while checking status".to_string(),
                        ));
                    }
                    tokio::time::sleep(Duration::from_secs(backoff)).await;
                    continue;
                }
            };

            let status = self.extract_json_field(&json, Some(&polling.status_check_field))?;

            // Log status transitions at INFO level for console visibility
            let elapsed = poll_start.elapsed().as_secs();
            if status != last_status {
                tracing::info!(
                    "Status: {} -> {} ({}s elapsed)",
                    if last_status.is_empty() {
                        "SUBMITTED"
                    } else {
                        &last_status
                    },
                    status,
                    elapsed
                );
                last_console_log = std::time::Instant::now();
            } else if last_console_log.elapsed().as_secs() >= 30 {
                // Periodic heartbeat so the console isn't silent during long waits
                tracing::info!("Still {} ({}s elapsed)", status, elapsed);
                last_console_log = std::time::Instant::now();
            }

            // Emit progress based on status
            if let Some(ref p) = progress {
                let elapsed = poll_start.elapsed().as_secs();

                if status == "IN_QUEUE" {
                    // Extract queue position if available
                    let position = json
                        .get("queue_position")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32;
                    p.send(Progress::queued(p.stage, position));
                } else if status == "IN_PROGRESS" {
                    // Status changed to in-progress
                    if last_status != "IN_PROGRESS" {
                        p.send(Progress::processing(
                            p.stage,
                            Some("Processing...".to_string()),
                        ));
                    } else {
                        // Periodic elapsed time update
                        p.send(Progress::processing(
                            p.stage,
                            Some(format!("Processing... ({}s elapsed)", elapsed)),
                        ));
                    }

                    // Forward any log messages from the API
                    if let Some(logs) = json.get("logs").and_then(|v| v.as_array()) {
                        for log_entry in logs.iter().skip(seen_log_count) {
                            if let Some(message) = log_entry.get("message").and_then(|m| m.as_str())
                            {
                                p.send(Progress::log(p.stage, message.to_string()));
                            }
                        }
                        seen_log_count = logs.len();
                    }
                }

                last_status = status.clone();
            } else {
                last_status = status.clone();
            }

            if status == polling.success_value {
                let total_elapsed = poll_start.elapsed().as_secs();
                tracing::info!(
                    "Generation complete ({}s total polling time)",
                    total_elapsed
                );

                // Emit downloading status
                if let Some(ref p) = progress {
                    p.send(Progress::processing(
                        p.stage,
                        Some("Downloading result...".to_string()),
                    ));
                }

                // Success - extract result
                // If response_url_field is set, fetch the actual result from that URL first
                if let Some(ref response_url_field) = polling.response_url_field {
                    let response_url = self.extract_json_field(&json, Some(response_url_field))?;
                    // Validate response URL to prevent SSRF
                    validate_download_url(&response_url)?;
                    tracing::debug!("Fetching result from response URL: {}", response_url);
                    let request = self.client.get(&response_url);
                    let request = self.apply_headers(request, auth_headers);
                    let result_response = request.send().await?;

                    if !result_response.status().is_success() {
                        let status = result_response.status();
                        let error_body = result_response.text().await.unwrap_or_default();
                        tracing::error!(
                            http.url = %response_url,
                            http.status = %status.as_u16(),
                            "Result fetch failed with HTTP {}: {}", status, error_body
                        );
                        return Err(HttpError {
                            url: response_url.clone(),
                            method: "GET".to_string(),
                            status_code: Some(status.as_u16()),
                            body: error_body,
                            is_queue_failure: false,
                        }
                        .into());
                    }

                    let result_json: serde_json::Value = result_response.json().await?;
                    // Unwrap response envelope if configured (e.g., fal.ai wraps output in "response")
                    let payload = if let Some(ref envelope_field) = polling.response_envelope_field
                    {
                        result_json
                            .get(envelope_field.as_str())
                            .unwrap_or(&result_json)
                    } else {
                        &result_json
                    };
                    let result_url =
                        self.extract_json_field(payload, Some(&polling.result_field))?;
                    return self.download_file(&result_url).await;
                }
                // Otherwise extract result directly from the status response
                let result_url = self.extract_json_field(&json, Some(&polling.result_field))?;
                return self.download_file(&result_url).await;
            }

            if let Some(ref failure_value) = polling.failure_value
                && status == *failure_value
            {
                let error_detail = json
                    .get("error")
                    .and_then(|e| e.as_str())
                    .unwrap_or(&status);
                tracing::error!(
                    http.url = %full_status_url,
                    error.detail = %error_detail,
                    "Generation failed: {}", error_detail
                );
                return Err(HttpError {
                    url: full_status_url.clone(),
                    method: "GET".to_string(),
                    status_code: None,
                    body: error_detail.to_string(),
                    is_queue_failure: true,
                }
                .into());
            }

            tracing::debug!(
                "Poll attempt {}/{}: status = {}",
                attempt + 1,
                polling.max_attempts,
                status
            );

            // Sleep at the end of the loop body, not the start. The first
            // poll already happened above; the sleep is the gap between
            // *this* attempt and the *next* one.
            tokio::time::sleep(Duration::from_millis(polling.interval_ms)).await;
        }

        // Cancel the request on the server to avoid burning credits
        tracing::warn!(
            "Polling timeout after {} attempts — cancelling request",
            polling.max_attempts,
        );
        self.send_cancel_request(&full_status_url, polling, auth_headers)
            .await;

        Err(anyhow!(
            "Polling timeout after {} attempts (request cancelled)",
            polling.max_attempts
        ))
    }

    /// Send a cancel request to the provider's server for the given status URL.
    async fn send_cancel_request(
        &self,
        status_url: &str,
        polling: &PollingConfig,
        auth_headers: &HashMap<String, String>,
    ) {
        let cancel_url = if let Some(ref template) = polling.cancel_url_template {
            template.replace("${status_url}", status_url)
        } else {
            // Default: replace /status with /cancel, strip query params
            status_url
                .replace("/status", "/cancel")
                .split('?')
                .next()
                .unwrap_or(status_url)
                .to_string()
        };
        // Default to PUT to preserve existing fal.ai behavior; providers using
        // REST-style cancel (e.g., Meshy DELETE) configure `cancel_method`.
        let method = polling
            .cancel_method
            .as_ref()
            .copied()
            .unwrap_or(super::config::HttpMethod::PUT);
        tracing::info!(
            "Sending cancel request ({}) to {}",
            method.as_str(),
            cancel_url
        );
        let cancel_request = match method {
            super::config::HttpMethod::GET => self.client.get(&cancel_url),
            super::config::HttpMethod::POST => self.client.post(&cancel_url),
            super::config::HttpMethod::PUT => self.client.put(&cancel_url),
            super::config::HttpMethod::DELETE => self.client.delete(&cancel_url),
            super::config::HttpMethod::PATCH => self.client.patch(&cancel_url),
        };
        let cancel_request = self.apply_headers(cancel_request, auth_headers);
        match cancel_request.send().await {
            Ok(resp) if resp.status().is_success() => {
                tracing::info!("Successfully cancelled remote request");
            }
            Ok(resp) => {
                tracing::warn!("Cancel request returned HTTP {}", resp.status());
            }
            Err(e) => {
                tracing::warn!("Failed to cancel remote request: {}", e);
            }
        }
    }

    /// Download a file from URL.
    ///
    /// Validates the URL against SSRF and enforces a size limit.
    async fn download_file(&self, url: &str) -> Result<Vec<u8>> {
        let full_url = self.resolve_url(url);

        // Validate URL to prevent SSRF via malicious API responses
        validate_download_url(&full_url)?;

        tracing::info!("Downloading result file");

        let response = self.client.get(&full_url).send().await?;

        if !response.status().is_success() {
            return Err(anyhow!("Failed to download file: {}", response.status()));
        }

        // Fast-reject if the advertised length already exceeds the cap.
        let content_length = response.content_length();
        if let Some(len) = content_length
            && len > MAX_DOWNLOAD_SIZE
        {
            return Err(anyhow!(
                "Download too large ({} bytes, max {} bytes)",
                len,
                MAX_DOWNLOAD_SIZE
            ));
        }

        // Stream the body and enforce the cap as bytes arrive. A server that
        // omits or lies about Content-Length can't exhaust memory this way —
        // we bail the moment the running total crosses MAX_DOWNLOAD_SIZE
        // instead of buffering the entire (potentially unbounded) body first.
        use futures::StreamExt as _;
        let mut buf: Vec<u8> = Vec::with_capacity(
            content_length
                .map(|l| l.min(MAX_DOWNLOAD_SIZE) as usize)
                .unwrap_or(0),
        );
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context("Failed to read download")?;
            if buf.len() as u64 + chunk.len() as u64 > MAX_DOWNLOAD_SIZE {
                return Err(anyhow!(
                    "Download too large (exceeded max {} bytes)",
                    MAX_DOWNLOAD_SIZE
                ));
            }
            buf.extend_from_slice(&chunk);
        }
        Ok(buf)
    }

    /// Interpolate `${...}` variables in a template string.
    ///
    /// Resolution is a **single left-to-right pass**: each `${token}` is
    /// resolved exactly once against the provided variables and this provider's
    /// declared env vars, and the substituted text is never re-scanned. This is
    /// deliberate — a naive multi-pass `replace` would let a user-supplied value
    /// (e.g. a prompt containing the literal text `${FAL_KEY}`) get expanded
    /// into a real API key on a later pass and leak it into the outgoing request
    /// body. Env vars only resolve for names in `provider.env_vars`.
    fn interpolate(&self, template: &str, variables: &[(&str, &str)]) -> Result<String> {
        let lookup = |name: &str| -> Option<String> {
            if let Some((_, value)) = variables.iter().find(|(k, _)| *k == name) {
                return Some((*value).to_string());
            }
            if self.config.provider.env_vars.iter().any(|e| e == name) {
                return std::env::var(name).ok();
            }
            None
        };

        let mut result = String::with_capacity(template.len());
        let bytes = template.as_bytes();
        let mut i = 0;
        let mut had_unresolved = false;
        while i < template.len() {
            if bytes[i] == b'$'
                && i + 1 < template.len()
                && bytes[i + 1] == b'{'
                && let Some(end_rel) = template[i + 2..].find('}')
            {
                let name = &template[i + 2..i + 2 + end_rel];
                match lookup(name) {
                    Some(value) => result.push_str(&value),
                    None => {
                        // Leave the token verbatim and flag it, matching the
                        // previous soft-fail behavior for missing values.
                        had_unresolved = true;
                        result.push_str(&template[i..i + 2 + end_rel + 1]);
                    }
                }
                i += 2 + end_rel + 1;
                continue;
            }
            // Copy this byte through. `template` is valid UTF-8 and `${`/`}` are
            // ASCII, so byte indexing here always lands on char boundaries.
            let ch = template[i..].chars().next().unwrap();
            result.push(ch);
            i += ch.len_utf8();
        }

        if had_unresolved {
            tracing::warn!("Template contains unresolved variables: {}", template);
        }

        Ok(result)
    }

    /// Interpolate variables in a JSON template.
    fn interpolate_json(
        &self,
        template: &serde_json::Value,
        variables: &[(&str, &str)],
    ) -> Result<serde_json::Value> {
        match template {
            serde_json::Value::String(s) => {
                Ok(serde_json::Value::String(self.interpolate(s, variables)?))
            }
            serde_json::Value::Object(map) => {
                let mut result = serde_json::Map::new();
                for (key, value) in map {
                    result.insert(key.clone(), self.interpolate_json(value, variables)?);
                }
                Ok(serde_json::Value::Object(result))
            }
            serde_json::Value::Array(arr) => {
                let mut result = Vec::new();
                for item in arr {
                    result.push(self.interpolate_json(item, variables)?);
                }
                Ok(serde_json::Value::Array(result))
            }
            other => Ok(other.clone()),
        }
    }

    /// Upload using single-step multipart.
    async fn upload_multipart(
        &self,
        url: &str,
        image_data: &[u8],
        upload_config: &super::config::UploadConfig,
    ) -> Result<String> {
        let file_field = upload_config
            .request
            .file_field
            .as_ref()
            .ok_or_else(|| anyhow!("file_field required for multipart upload"))?;

        let file_part = multipart::Part::bytes(image_data.to_vec())
            .file_name(bundle_files::IMAGE)
            .mime_str(mime::IMAGE_PNG)?;

        let mut form = multipart::Form::new().part(file_field.clone(), file_part);

        // Add additional fields
        for (key, value_template) in &upload_config.request.fields {
            let value = self.interpolate(value_template, &[])?;
            form = form.text(key.clone(), value);
        }

        let mut request = self.client.post(url);

        // Add headers (excluding Content-Type)
        for (key, value_template) in &upload_config.request.headers {
            if !key.eq_ignore_ascii_case(headers::CONTENT_TYPE) {
                let value = self.interpolate(value_template, &[])?;
                request = request.header(key, value);
            }
        }

        let response = request.multipart(form).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await.unwrap_or_default();
            return Err(HttpError {
                url: url.to_string(),
                method: "POST".to_string(),
                status_code: Some(status.as_u16()),
                body: error_body,
                is_queue_failure: false,
            }
            .into());
        }

        let json: serde_json::Value = response.json().await?;
        self.extract_json_field(&json, Some(&upload_config.response.file_url_field))
    }

    /// Upload using two-step initiate-then-put.
    async fn upload_initiate_then_put(
        &self,
        initiate_url: &str,
        image_data: &[u8],
        upload_config: &super::config::UploadConfig,
    ) -> Result<String> {
        // Step 1: Initiate upload to get upload URL
        let initiate_body = upload_config
            .request
            .initiate_body
            .as_ref()
            .ok_or_else(|| anyhow!("initiate_body required for initiate_then_put upload"))?;

        let mut request = self.client.post(initiate_url);

        for (key, value_template) in &upload_config.request.headers {
            let value = self.interpolate(value_template, &[])?;
            request = request.header(key, value);
        }

        let interpolated_body = self.interpolate_json(initiate_body, &[])?;
        let response = request.json(&interpolated_body).send().await?;

        if !response.status().is_success() {
            let status = response.status();
            let error_body = response.text().await.unwrap_or_default();
            return Err(HttpError {
                url: initiate_url.to_string(),
                method: "POST".to_string(),
                status_code: Some(status.as_u16()),
                body: error_body,
                is_queue_failure: false,
            }
            .into());
        }

        let json: serde_json::Value = response.json().await?;

        let upload_url_field = upload_config
            .response
            .upload_url_field
            .as_ref()
            .ok_or_else(|| anyhow!("upload_url_field required for initiate_then_put"))?;

        let upload_url = self.extract_json_field(&json, Some(upload_url_field))?;
        // Validate upload URL to prevent SSRF via malicious initiate response
        validate_download_url(&upload_url)?;

        let file_url =
            self.extract_json_field(&json, Some(&upload_config.response.file_url_field))?;

        // Step 2: PUT raw file bytes to upload URL
        // Provider expects raw binary data with Content-Type header, NOT multipart form data.
        // Determine content type from the initiate_body config or default to image/png.
        let content_type = upload_config
            .request
            .initiate_body
            .as_ref()
            .and_then(|body| body.get("content_type"))
            .and_then(|v| v.as_str())
            .unwrap_or(mime::IMAGE_PNG);

        let put_response = self
            .client
            .put(&upload_url)
            .header(headers::CONTENT_TYPE, content_type)
            .body(image_data.to_vec())
            .send()
            .await?;

        if !put_response.status().is_success() {
            let status = put_response.status();
            let error_body = put_response.text().await.unwrap_or_default();
            return Err(HttpError {
                url: upload_url.clone(),
                method: "PUT".to_string(),
                status_code: Some(status.as_u16()),
                body: error_body,
                is_queue_failure: false,
            }
            .into());
        }

        Ok(file_url)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_ip_host_integer_forms() {
        use std::net::{IpAddr, Ipv4Addr};
        // Decimal, hex, and octal notations that all resolve to 127.0.0.1 —
        // these bypass a naive IpAddr::parse and must be caught by the SSRF
        // guard.
        let loopback = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        assert_eq!(normalize_ip_host("2130706433"), Some(loopback));
        assert_eq!(normalize_ip_host("0x7f000001"), Some(loopback));
        assert_eq!(normalize_ip_host("017700000001"), Some(loopback));
        // Real hostnames and dotted-quads are not integer forms.
        assert_eq!(normalize_ip_host("api.example.com"), None);
        assert_eq!(normalize_ip_host("127.0.0.1"), None); // handled by IpAddr::parse instead
    }

    #[test]
    fn test_resolve_url_absolute_http() {
        assert_eq!(
            resolve_url(Some("https://api.example.com"), "https://other.com/path"),
            "https://other.com/path"
        );
        assert_eq!(
            resolve_url(Some("https://api.example.com"), "http://other.com/path"),
            "http://other.com/path"
        );
    }

    #[test]
    fn test_resolve_url_relative_with_base() {
        assert_eq!(
            resolve_url(Some("https://api.example.com"), "/v1/models"),
            "https://api.example.com/v1/models"
        );
        // Trailing slash on base, leading slash on path
        assert_eq!(
            resolve_url(Some("https://api.example.com/"), "/v1/models"),
            "https://api.example.com/v1/models"
        );
        // No leading slash on path
        assert_eq!(
            resolve_url(Some("https://api.example.com"), "v1/models"),
            "https://api.example.com/v1/models"
        );
    }

    #[test]
    fn test_resolve_url_no_base() {
        assert_eq!(resolve_url(None, "/v1/models"), "v1/models");
        assert_eq!(resolve_url(Some(""), "/v1/models"), "v1/models");
    }

    #[test]
    fn test_resolve_url_empty_path() {
        assert_eq!(
            resolve_url(Some("https://api.example.com"), ""),
            "https://api.example.com"
        );
        assert_eq!(resolve_url(None, ""), "");
    }

    #[test]
    fn test_interpolate() {
        let _env = crate::test_support::env_lock();
        unsafe { std::env::set_var("TEST_KEY", "secret123") };

        let config = ProviderConfig {
            provider: super::super::config::ProviderMetadataConfig {
                id: "test".to_string(),
                name: "Test".to_string(),
                description: "Test".to_string(),
                env_vars: vec!["TEST_KEY".to_string()],
                base_url: None,
                upload: None,
                api_key_url: None,
                website_url: None,
                docs_url: None,
                discovery: None,
                auth_format: None,
            },
            text_to_image: vec![],
            image_to_3d: vec![],
        };

        let client = HttpProviderClient::new(config);

        let result = client.interpolate("Bearer ${TEST_KEY}", &[]).unwrap();
        assert_eq!(result, "Bearer secret123");

        let result = client
            .interpolate("Prompt: ${prompt}", &[("prompt", "test")])
            .unwrap();
        assert_eq!(result, "Prompt: test");

        // A user-supplied value that itself contains an env-var token must NOT
        // be re-expanded — otherwise a prompt could exfiltrate the API key into
        // the request body. The token stays literal.
        let result = client
            .interpolate("Prompt: ${prompt}", &[("prompt", "leak ${TEST_KEY}")])
            .unwrap();
        assert_eq!(result, "Prompt: leak ${TEST_KEY}");
        assert!(
            !result.contains("secret123"),
            "API key must not leak via prompt text"
        );

        // Unknown tokens are left verbatim (soft-fail), and multiple tokens on
        // one line each resolve once.
        let result = client
            .interpolate("${prompt} / ${TEST_KEY} / ${unknown}", &[("prompt", "p")])
            .unwrap();
        assert_eq!(result, "p / secret123 / ${unknown}");
    }

    #[test]
    fn test_substitute_response_fields() {
        let config = ProviderConfig {
            provider: super::super::config::ProviderMetadataConfig {
                id: "test".to_string(),
                name: "Test".to_string(),
                description: "Test".to_string(),
                env_vars: vec![],
                base_url: None,
                upload: None,
                api_key_url: None,
                website_url: None,
                docs_url: None,
                discovery: None,
                auth_format: None,
            },
            text_to_image: vec![],
            image_to_3d: vec![],
        };
        let client = HttpProviderClient::new(config);

        let json = serde_json::json!({
            "result": "abc-123",
            "data": { "id": "nested-id" },
            "items": ["first", "second"]
        });

        assert_eq!(
            client
                .substitute_response_fields("/openapi/v1/x/${result}", &json)
                .unwrap(),
            "/openapi/v1/x/abc-123"
        );
        assert_eq!(
            client
                .substitute_response_fields("/p/${data.id}/sub", &json)
                .unwrap(),
            "/p/nested-id/sub"
        );
        assert_eq!(
            client
                .substitute_response_fields("/p/${items[1]}", &json)
                .unwrap(),
            "/p/second"
        );
        assert!(
            client
                .substitute_response_fields("/p/${unterminated", &json)
                .is_err()
        );
    }

    #[test]
    fn test_http_error_display_with_status_code() {
        let err = HttpError {
            url: "https://api.example.com/v1/generate".to_string(),
            method: "POST".to_string(),
            status_code: Some(422),
            body: "Validation error: invalid prompt".to_string(),
            is_queue_failure: false,
        };
        assert_eq!(
            err.to_string(),
            "HTTP 422 at https://api.example.com/v1/generate: Validation error: invalid prompt"
        );
    }

    #[test]
    fn test_http_error_display_queue_failure() {
        let err = HttpError {
            url: "https://queue.fal.run/model/requests/abc123/status".to_string(),
            method: "GET".to_string(),
            status_code: None,
            body: "GPU out of memory".to_string(),
            is_queue_failure: true,
        };
        assert_eq!(err.to_string(), "Generation failed: GPU out of memory");
    }

    #[test]
    fn test_http_error_display_network_failure() {
        let err = HttpError {
            url: "https://api.example.com/v1/generate".to_string(),
            method: "POST".to_string(),
            status_code: None,
            body: "Connection refused".to_string(),
            is_queue_failure: false,
        };
        assert_eq!(
            err.to_string(),
            "Request to https://api.example.com/v1/generate failed: Connection refused"
        );
    }

    #[test]
    fn test_http_error_is_std_error() {
        let err = HttpError {
            url: "https://example.com".to_string(),
            method: "GET".to_string(),
            status_code: Some(500),
            body: "Internal server error".to_string(),
            is_queue_failure: false,
        };
        // Verify it can be wrapped in anyhow and downcast back
        let anyhow_err: anyhow::Error = err.into();
        let recovered = anyhow_err.downcast::<HttpError>().unwrap();
        assert_eq!(recovered.status_code, Some(500));
        assert_eq!(recovered.url, "https://example.com");
        assert_eq!(recovered.method, "GET");
    }

    #[test]
    fn test_extract_json_field() {
        let config = ProviderConfig {
            provider: super::super::config::ProviderMetadataConfig {
                id: "test".to_string(),
                name: "Test".to_string(),
                description: "Test".to_string(),
                env_vars: vec![],
                base_url: None,
                upload: None,
                api_key_url: None,
                website_url: None,
                docs_url: None,
                discovery: None,
                auth_format: None,
            },
            text_to_image: vec![],
            image_to_3d: vec![],
        };

        let client = HttpProviderClient::new(config);

        let json: serde_json::Value = serde_json::json!({
            "result": {
                "url": "https://example.com/image.png"
            },
            "images": ["url1", "url2"]
        });

        assert_eq!(
            client
                .extract_json_field(&json, Some("result.url"))
                .unwrap(),
            "https://example.com/image.png"
        );

        assert_eq!(
            client.extract_json_field(&json, Some("images[0]")).unwrap(),
            "url1"
        );
    }

    /// Golden-fixture test: the exact `result_field` / `status_check_field`
    /// JSONPath strings that the shipped provider YAMLs declare must extract
    /// cleanly from realistic recorded response shapes. This catches a typo in a
    /// `field:` path or a provider changing its response envelope — failures
    /// that otherwise only surface against the live API.
    #[test]
    fn test_provider_response_paths_resolve_against_recorded_shapes() {
        let client = HttpProviderClient::new(ProviderConfig {
            provider: super::super::config::ProviderMetadataConfig {
                id: "t".into(),
                name: "t".into(),
                description: "t".into(),
                env_vars: vec![],
                base_url: None,
                upload: None,
                api_key_url: None,
                website_url: None,
                docs_url: None,
                discovery: None,
                auth_format: None,
            },
            text_to_image: vec![],
            image_to_3d: vec![],
        });

        // fal.ai image models: result_field = "images[0].url"
        let fal_image = serde_json::json!({
            "images": [{ "url": "https://fal.media/out.png", "width": 1024 }],
            "seed": 42,
        });
        assert_eq!(
            client
                .extract_json_field(&fal_image, Some("images[0].url"))
                .unwrap(),
            "https://fal.media/out.png"
        );

        // fal.ai 3D models: result_field = "model_glb.url"
        let fal_3d = serde_json::json!({
            "model_glb": { "url": "https://fal.media/model.glb", "file_size": 12345 }
        });
        assert_eq!(
            client
                .extract_json_field(&fal_3d, Some("model_glb.url"))
                .unwrap(),
            "https://fal.media/model.glb"
        );

        // Meshy image: result_field = "image_urls[0]", status_check_field = "status"
        let meshy_image = serde_json::json!({
            "status": "SUCCEEDED",
            "image_urls": ["https://assets.meshy.ai/img.png"],
        });
        assert_eq!(
            client
                .extract_json_field(&meshy_image, Some("status"))
                .unwrap(),
            "SUCCEEDED"
        );
        assert_eq!(
            client
                .extract_json_field(&meshy_image, Some("image_urls[0]"))
                .unwrap(),
            "https://assets.meshy.ai/img.png"
        );

        // Meshy 3D: result_field = "model_urls.glb"
        let meshy_3d = serde_json::json!({
            "status": "SUCCEEDED",
            "model_urls": { "glb": "https://assets.meshy.ai/m.glb", "fbx": "https://assets.meshy.ai/m.fbx" },
        });
        assert_eq!(
            client
                .extract_json_field(&meshy_3d, Some("model_urls.glb"))
                .unwrap(),
            "https://assets.meshy.ai/m.glb"
        );

        // A drifted/missing field must error, not silently return the whole doc.
        assert!(
            client
                .extract_json_field(&fal_image, Some("images[0].nonexistent"))
                .is_err()
        );
    }

    // ---- apply_param_overrides -------------------------------------------

    fn param_def(name: &str) -> super::super::config::ParameterDef {
        super::super::config::ParameterDef {
            name: name.into(),
            label: name.into(),
            description: None,
            param_type: super::super::config::ParameterType::Integer,
            default: serde_json::json!(0),
            min: None,
            max: None,
            step: None,
            options: None,
            widget: None,
        }
    }

    #[test]
    fn override_inserts_declared_key() {
        let mut body = serde_json::json!({ "seed": 42 });
        let mut params = HashMap::new();
        params.insert("seed".into(), serde_json::json!(100));
        apply_param_overrides(&mut body, Some(&params), &[param_def("seed")], "m");
        assert_eq!(body, serde_json::json!({ "seed": 100 }));
    }

    #[test]
    fn null_override_removes_declared_key() {
        // The user-facing contract: clearing a field in the GUI (null override)
        // drops the key from the body so the provider picks its own default.
        let mut body = serde_json::json!({ "seed": 42 });
        let mut params = HashMap::new();
        params.insert("seed".into(), serde_json::Value::Null);
        apply_param_overrides(&mut body, Some(&params), &[param_def("seed")], "m");
        assert_eq!(body, serde_json::json!({}));
    }

    #[test]
    fn undeclared_override_is_ignored() {
        // Prevents injection of arbitrary fields (prompt, auth, etc.).
        let mut body = serde_json::json!({ "seed": 42 });
        let mut params = HashMap::new();
        params.insert("prompt".into(), serde_json::json!("injected"));
        apply_param_overrides(&mut body, Some(&params), &[param_def("seed")], "m");
        assert_eq!(body, serde_json::json!({ "seed": 42 }));
    }

    #[test]
    fn template_null_is_stripped() {
        // YAML `seed: null` means "leave unset" — never send a literal null.
        let mut body = serde_json::json!({ "prompt": "x", "seed": null });
        apply_param_overrides(&mut body, None, &[], "m");
        assert_eq!(body, serde_json::json!({ "prompt": "x" }));
    }

    #[test]
    fn non_object_body_passes_through() {
        let mut body = serde_json::json!("just a string");
        apply_param_overrides(&mut body, None, &[], "m");
        assert_eq!(body, serde_json::json!("just a string"));
    }
}

/// Wiremock-driven tests for the polling loop (`poll_for_result`). These cover
/// the retry/backoff/give-up behavior and the Meshy-style `status_url_template`
/// task-id variant — paths that are otherwise unexercised because Meshy is
/// hidden in mock mode. Runs under `--features mock` (which pulls in wiremock).
#[cfg(all(test, feature = "mock"))]
mod poll_tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn client_for(base_url: &str) -> HttpProviderClient {
        let config = ProviderConfig {
            provider: super::super::config::ProviderMetadataConfig {
                id: "test".to_string(),
                name: "Test".to_string(),
                description: "Test".to_string(),
                env_vars: vec![],
                base_url: Some(base_url.to_string()),
                upload: None,
                api_key_url: None,
                website_url: None,
                docs_url: None,
                discovery: None,
                auth_format: None,
            },
            text_to_image: vec![],
            image_to_3d: vec![],
        };
        HttpProviderClient::new(config)
    }

    /// Parse a PollingConfig from YAML so serde fills all the defaulted fields.
    fn polling(yaml: &str) -> PollingConfig {
        serde_yaml_ng::from_str(yaml).expect("valid polling yaml")
    }

    // A fast polling config: near-instant interval, plenty of attempts.
    fn fast_polling(extra: &str) -> PollingConfig {
        polling(&format!(
            "status_field: status_url\n\
             status_check_field: status\n\
             success_value: COMPLETED\n\
             failure_value: FAILED\n\
             result_field: result_url\n\
             interval_ms: 1\n\
             max_attempts: 20\n\
             {extra}"
        ))
    }

    #[tokio::test]
    async fn poll_retries_transient_5xx_then_succeeds() {
        // Bypass SSRF validation so the wiremock localhost URLs are allowed,
        // without touching the process-global MOCK_API env (which leaks between
        // tests). Defaults false, so this doesn't weaken other tests.
        super::SKIP_URL_VALIDATION_FOR_TEST.store(true, std::sync::atomic::Ordering::Relaxed);

        let server = MockServer::start().await;

        // First status poll fails with 503 (transient), then COMPLETED. Only
        // one failure so the test doesn't pay multiple backoff sleeps; the
        // retry path is what's under test, not the exact count. `up_to_n_times`
        // (not `.expect`) avoids a drop-time panic if the count differs.
        Mock::given(method("GET"))
            .and(path("/status"))
            .respond_with(ResponseTemplate::new(503))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/status"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "COMPLETED",
                "result_url": format!("{}/result", server.uri()),
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/result"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"MODELBYTES".to_vec()))
            .mount(&server)
            .await;

        let client = client_for(&server.uri());
        let initial = serde_json::json!({ "status_url": format!("{}/status", server.uri()) });
        // Zero backoff base would still sleep 2^0*2s; shrink by clamping via a
        // tiny interval isn't enough (backoff uses its own constant), so this
        // test tolerates the ~2s first-retry backoff. Keep attempts low.
        let cfg = fast_polling("");

        let result = client
            .poll_for_result(&initial, &cfg, &HashMap::new(), None)
            .await
            .expect("should succeed after retries");
        assert_eq!(result, b"MODELBYTES");
    }

    #[tokio::test]
    async fn poll_reports_failure_status() {
        super::SKIP_URL_VALIDATION_FOR_TEST.store(true, std::sync::atomic::Ordering::Relaxed);
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/status"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "FAILED",
                "error": "model exploded",
            })))
            .mount(&server)
            .await;

        let client = client_for(&server.uri());
        let initial = serde_json::json!({ "status_url": format!("{}/status", server.uri()) });
        let cfg = fast_polling("");

        let err = client
            .poll_for_result(&initial, &cfg, &HashMap::new(), None)
            .await
            .expect_err("FAILED status should error");
        assert!(err.to_string().contains("model exploded") || err.to_string().contains("FAILED"));
    }

    #[tokio::test]
    async fn poll_fails_fast_on_terminal_4xx() {
        // A 404 (terminal, non-retryable) must abort immediately — exactly one
        // poll request — rather than burning the whole retry budget. `.expect(1)`
        // asserts the request count, so a regression to retry-on-4xx fails here.
        super::SKIP_URL_VALIDATION_FOR_TEST.store(true, std::sync::atomic::Ordering::Relaxed);
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/status"))
            .respond_with(ResponseTemplate::new(404))
            .expect(1)
            .mount(&server)
            .await;

        let client = client_for(&server.uri());
        let initial = serde_json::json!({ "status_url": format!("{}/status", server.uri()) });
        let cfg = fast_polling("");

        let err = client
            .poll_for_result(&initial, &cfg, &HashMap::new(), None)
            .await
            .expect_err("terminal 4xx should abort");
        assert!(
            err.to_string().to_lowercase().contains("cancelled") || err.to_string().contains("404")
        );
        // `.expect(1)` is verified on server drop — if the loop retried, the
        // mock would have been hit >1 time and the drop would panic.
        drop(server);
    }

    #[tokio::test]
    async fn poll_builds_url_from_status_url_template() {
        // Meshy-style: the initial response carries only a task id; the poll URL
        // is built from `status_url_template`.
        super::SKIP_URL_VALIDATION_FOR_TEST.store(true, std::sync::atomic::Ordering::Relaxed);
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/tasks/task-123"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "status": "COMPLETED",
                "result_url": format!("{}/result", server.uri()),
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/result"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(b"GLB".to_vec()))
            .mount(&server)
            .await;

        let client = client_for(&server.uri());
        // Initial response is just a task id under `result`.
        let initial = serde_json::json!({ "result": "task-123" });
        let cfg = fast_polling(&format!(
            "status_url_template: '{}/tasks/${{result}}'",
            server.uri()
        ));

        let result = client
            .poll_for_result(&initial, &cfg, &HashMap::new(), None)
            .await
            .expect("template-built poll URL should resolve and complete");
        assert_eq!(result, b"GLB");
    }
}
