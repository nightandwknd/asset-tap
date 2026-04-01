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
fn validate_download_url(url: &str) -> Result<()> {
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

    let host = extract_host(url).unwrap_or_default();

    // Block empty host
    if host.is_empty() {
        return Err(anyhow!("URL has no host: {}", url));
    }

    // Block private/reserved hostnames
    if host == "localhost" || host.ends_with(".local") || host.ends_with(".internal") {
        return Err(anyhow!("URL points to local/internal host: {}", host));
    }

    // Block private/reserved IP ranges (handles both IPv4 and IPv6)
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
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

            // Merge user parameter overrides into the body.
            // Only allow keys that are declared in the model's `parameters` list
            // to prevent injection of arbitrary fields (e.g., overriding prompt or auth).
            if let (Some(params), Some(obj)) = (params, body.as_object_mut()) {
                let allowed: std::collections::HashSet<&str> =
                    model.parameters.iter().map(|p| p.name.as_str()).collect();
                for (key, value) in params {
                    if allowed.contains(key.as_str()) {
                        obj.insert(key.clone(), value.clone());
                    } else {
                        tracing::warn!(
                            "Ignoring undeclared parameter '{}' for model '{}'",
                            key,
                            model.id
                        );
                    }
                }
            }

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
        _params: Option<&HashMap<String, serde_json::Value>>,
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

        // Add additional fields
        for (key, value_template) in &multipart_config.fields {
            let value = self.interpolate(value_template, &[])?;
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

    /// Poll for async result, emitting progress updates during the wait.
    async fn poll_for_result(
        &self,
        initial_response: &serde_json::Value,
        polling: &PollingConfig,
        auth_headers: &HashMap<String, String>,
        progress: Option<PollingProgress>,
    ) -> Result<Vec<u8>> {
        let status_url = self.extract_json_field(initial_response, Some(&polling.status_field))?;

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

        for attempt in 0..polling.max_attempts {
            tokio::time::sleep(Duration::from_millis(polling.interval_ms)).await;

            // Check cancel flag between poll iterations
            if self.cancel_flag.load(Ordering::Relaxed) {
                tracing::info!("Cancel flag detected during polling — cancelling server request");
                self.send_cancel_request(&full_status_url, polling, auth_headers)
                    .await;
                return Err(anyhow!("Generation cancelled by user"));
            }

            let request = self.client.get(&poll_url);
            let request = self.apply_headers(request, auth_headers);
            let response = request.send().await?;

            if !response.status().is_success() {
                let status = response.status();
                let error_body = response.text().await.unwrap_or_default();
                tracing::error!(
                    http.url = %poll_url,
                    http.status = %status.as_u16(),
                    "Polling request failed with HTTP {}: {}", status, error_body
                );
                return Err(HttpError {
                    url: poll_url.clone(),
                    method: "GET".to_string(),
                    status_code: Some(status.as_u16()),
                    body: error_body,
                    is_queue_failure: false,
                }
                .into());
            }

            let json: serde_json::Value = response.json().await?;

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
        tracing::info!("Sending cancel request to {}", cancel_url);
        let cancel_request = self.client.put(&cancel_url);
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

        // Enforce size limit to prevent resource exhaustion
        if let Some(len) = response.content_length()
            && len > MAX_DOWNLOAD_SIZE
        {
            return Err(anyhow!(
                "Download too large ({} bytes, max {} bytes)",
                len,
                MAX_DOWNLOAD_SIZE
            ));
        }

        let bytes = response.bytes().await.context("Failed to read download")?;
        if bytes.len() as u64 > MAX_DOWNLOAD_SIZE {
            return Err(anyhow!(
                "Download too large ({} bytes, max {} bytes)",
                bytes.len(),
                MAX_DOWNLOAD_SIZE
            ));
        }
        Ok(bytes.to_vec())
    }

    /// Interpolate variables in a template string.
    fn interpolate(&self, template: &str, variables: &[(&str, &str)]) -> Result<String> {
        let mut result = template.to_string();

        // Interpolate provided variables
        for (key, value) in variables {
            result = result.replace(&format!("${{{}}}", key), value);
        }

        // Interpolate environment variables
        for env_var in &self.config.provider.env_vars {
            if let Ok(value) = std::env::var(env_var) {
                result = result.replace(&format!("${{{}}}", env_var), &value);
            }
        }

        // Check for unresolved variables
        if result.contains("${") {
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
        unsafe { std::env::set_var("TEST_KEY", "secret123") };

        let config = ProviderConfig {
            config_version: 0,
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
            config_version: 0,
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
}
