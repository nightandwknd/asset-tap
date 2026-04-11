//! Tests for dynamic model discovery system.

use asset_tap_core::providers::{
    ProviderCapability, ProviderConfig,
    config::{
        DiscoveryConfig, DiscoveryEndpoint, DiscoveryFieldMapping, HttpMethod,
        ProviderMetadataConfig, RequestTemplate, ResponseTemplate, ResponseType,
    },
    discovery::ModelDiscoveryClient,
    discovery_cache::DiscoveryCache,
    openapi::OpenApiParser,
};
use serde_json::json;
use std::collections::HashMap;

/// Create a test provider config with discovery enabled.
fn create_test_discovery_config() -> ProviderConfig {
    ProviderConfig {
        provider: ProviderMetadataConfig {
            id: "test-provider".to_string(),
            name: "Test Provider".to_string(),
            description: "Test provider with discovery".to_string(),
            env_vars: vec![],
            base_url: Some("https://api.example.com".to_string()),
            upload: None,
            auth_format: None,
            api_key_url: None,
            website_url: None,
            docs_url: None,
            discovery: Some(DiscoveryConfig {
                enabled: true,
                text_to_image: Some(DiscoveryEndpoint {
                    endpoint: "https://api.example.com/models".to_string(),
                    params: {
                        let mut params = HashMap::new();
                        params.insert("category".to_string(), "text-to-image".to_string());
                        params
                    },
                    models_field: "models".to_string(),
                    fetch_schemas: true,
                    schema_expand_param: Some("expand".to_string()),
                    field_mapping: DiscoveryFieldMapping {
                        id_field: "id".to_string(),
                        name_field: "name".to_string(),
                        description_field: Some("description".to_string()),
                        endpoint_field: Some("endpoint".to_string()),
                        status_field: Some("status".to_string()),
                        active_status_value: Some("active".to_string()),
                        openapi_field: Some("openapi".to_string()),
                    },
                }),
                image_to_3d: None,
                cache_ttl_secs: 3600,
                require_auth: false,
                timeout_secs: 5,
            }),
        },
        text_to_image: vec![],
        image_to_3d: vec![],
    }
}

#[test]
fn test_openapi_parser_simple_schema() {
    let openapi = json!({
        "paths": {
            "/generate": {
                "post": {
                    "requestBody": {
                        "content": {
                            "application/json": {
                                "schema": {
                                    "properties": {
                                        "prompt": {
                                            "type": "string",
                                            "description": "The text prompt"
                                        },
                                        "seed": {
                                            "type": "integer",
                                            "default": 42
                                        },
                                        "num_images": {
                                            "type": "integer",
                                            "default": 1
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    });

    let model = OpenApiParser::parse_model(
        "test-model".to_string(),
        "Test Model".to_string(),
        Some("A test model".to_string()),
        &openapi,
        "https://api.example.com",
    )
    .unwrap();

    assert_eq!(model.id, "test-model");
    assert_eq!(model.name, "Test Model");
    assert_eq!(model.description, "A test model");
    assert_eq!(model.endpoint, "/generate");
    assert_eq!(model.method, HttpMethod::POST);

    // Check request body
    let body = model.request.body.unwrap();
    assert_eq!(body.get("prompt").unwrap().as_str().unwrap(), "${prompt}");
    assert_eq!(body.get("seed").unwrap().as_i64().unwrap(), 42);
    assert_eq!(body.get("num_images").unwrap().as_i64().unwrap(), 1);

    // Check response config
    assert_eq!(model.response.response_type, ResponseType::Polling);
    assert!(model.response.polling.is_some());
}

#[test]
fn test_openapi_parser_image_to_3d_schema() {
    let openapi = json!({
        "paths": {
            "/generate-3d": {
                "post": {
                    "requestBody": {
                        "content": {
                            "application/json": {
                                "schema": {
                                    "properties": {
                                        "image_url": {
                                            "type": "string",
                                            "description": "URL of the input image"
                                        },
                                        "resolution": {
                                            "type": "integer",
                                            "default": 1024
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    });

    let model = OpenApiParser::parse_model(
        "trellis-2".to_string(),
        "TRELLIS 2".to_string(),
        None,
        &openapi,
        "https://api.example.com",
    )
    .unwrap();

    let body = model.request.body.unwrap();
    assert_eq!(
        body.get("image_url").unwrap().as_str().unwrap(),
        "${image_url}"
    );
    assert_eq!(body.get("resolution").unwrap().as_i64().unwrap(), 1024);
}

#[test]
fn test_openapi_parser_skips_status_endpoints() {
    let openapi = json!({
        "paths": {
            "/generate/requests/{request_id}/status": {
                "get": {
                    "description": "Check status"
                }
            },
            "/generate": {
                "post": {
                    "requestBody": {
                        "content": {
                            "application/json": {
                                "schema": {
                                    "properties": {
                                        "prompt": {"type": "string"}
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    });

    let model =
        OpenApiParser::parse_model("test".to_string(), "Test".to_string(), None, &openapi, "")
            .unwrap();

    // Should find /generate, not the status endpoint
    assert_eq!(model.endpoint, "/generate");
}

#[test]
fn test_openapi_parser_missing_paths() {
    let openapi = json!({
        "openapi": "3.0.0",
        "info": {"title": "Test", "version": "1.0"}
    });

    let result =
        OpenApiParser::parse_model("test".to_string(), "Test".to_string(), None, &openapi, "");

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("missing 'paths'"));
}

#[test]
fn test_openapi_parser_no_post_endpoint() {
    let openapi = json!({
        "paths": {
            "/status": {
                "get": {
                    "description": "Get status"
                }
            }
        }
    });

    let result =
        OpenApiParser::parse_model("test".to_string(), "Test".to_string(), None, &openapi, "");

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("No POST endpoint"));
}

#[test]
fn test_discovery_cache_insert_and_get() {
    let mut cache = DiscoveryCache::new();

    let model = create_test_model("model-1");
    let models = vec![model];

    cache.insert(
        "test-provider".to_string(),
        ProviderCapability::TextToImage,
        models.clone(),
        3600,
    );

    let cached = cache.get("test-provider", ProviderCapability::TextToImage);
    assert!(cached.is_some());
    assert_eq!(cached.unwrap().len(), 1);
    assert_eq!(cached.unwrap()[0].id, "model-1");
}

#[test]
fn test_discovery_cache_expiry() {
    let mut cache = DiscoveryCache::new();

    let model = create_test_model("model-1");
    let models = vec![model];

    // Insert with 0 second TTL (immediately expired)
    cache.insert(
        "test-provider".to_string(),
        ProviderCapability::TextToImage,
        models,
        0,
    );

    // Give it a moment to expire
    std::thread::sleep(std::time::Duration::from_millis(10));

    // Should return None because entry is expired
    let cached = cache.get("test-provider", ProviderCapability::TextToImage);
    assert!(cached.is_none());
}

#[test]
fn test_discovery_cache_invalidate() {
    let mut cache = DiscoveryCache::new();

    let model = create_test_model("model-1");
    let models = vec![model];

    cache.insert(
        "test-provider".to_string(),
        ProviderCapability::TextToImage,
        models,
        3600,
    );

    // Verify it's there
    assert!(
        cache
            .get("test-provider", ProviderCapability::TextToImage)
            .is_some()
    );

    // Invalidate
    cache.invalidate("test-provider", ProviderCapability::TextToImage);

    // Should be gone
    assert!(
        cache
            .get("test-provider", ProviderCapability::TextToImage)
            .is_none()
    );
}

#[test]
fn test_discovery_cache_clear() {
    let mut cache = DiscoveryCache::new();

    cache.insert(
        "provider-1".to_string(),
        ProviderCapability::TextToImage,
        vec![create_test_model("model-1")],
        3600,
    );

    cache.insert(
        "provider-2".to_string(),
        ProviderCapability::ImageTo3D,
        vec![create_test_model("model-2")],
        3600,
    );

    assert_eq!(cache.len(), 2);

    cache.clear();

    assert_eq!(cache.len(), 0);
    assert!(cache.is_empty());
}

#[test]
fn test_discovery_cache_multiple_capabilities() {
    let mut cache = DiscoveryCache::new();

    let text_models = vec![create_test_model("text-model")];
    let image_models = vec![create_test_model("3d-model")];

    cache.insert(
        "provider-1".to_string(),
        ProviderCapability::TextToImage,
        text_models,
        3600,
    );

    cache.insert(
        "provider-1".to_string(),
        ProviderCapability::ImageTo3D,
        image_models,
        3600,
    );

    // Should have separate entries for each capability
    assert_eq!(cache.len(), 2);

    let text = cache.get("provider-1", ProviderCapability::TextToImage);
    let image = cache.get("provider-1", ProviderCapability::ImageTo3D);

    assert!(text.is_some());
    assert!(image.is_some());
    assert_eq!(text.unwrap()[0].id, "text-model");
    assert_eq!(image.unwrap()[0].id, "3d-model");
}

#[test]
fn test_discovery_client_creation() {
    let config = create_test_discovery_config();
    let _client = ModelDiscoveryClient::new(config.clone());

    // Client should be created successfully
    // (Can't test much more without actual HTTP requests)
    assert!(config.provider.discovery.is_some());
}

#[test]
fn test_discovery_config_validation() {
    let config = create_test_discovery_config();

    assert!(config.provider.discovery.is_some());
    let discovery = config.provider.discovery.as_ref().unwrap();

    assert!(discovery.enabled);
    assert!(discovery.text_to_image.is_some());
    assert_eq!(discovery.cache_ttl_secs, 3600);
    assert_eq!(discovery.timeout_secs, 5);
    assert!(!discovery.require_auth);

    let endpoint = discovery.text_to_image.as_ref().unwrap();
    assert_eq!(endpoint.endpoint, "https://api.example.com/models");
    assert!(endpoint.fetch_schemas);
    assert_eq!(endpoint.schema_expand_param, Some("expand".to_string()));
}

#[test]
fn test_field_mapping_defaults() {
    let mapping = DiscoveryFieldMapping::default();

    assert_eq!(mapping.id_field, "endpoint_id");
    assert_eq!(mapping.name_field, "display_name");
    assert_eq!(mapping.description_field, Some("description".to_string()));
    assert_eq!(mapping.endpoint_field, Some("endpoint_id".to_string()));
    assert_eq!(mapping.status_field, Some("status".to_string()));
    assert_eq!(mapping.active_status_value, Some("active".to_string()));
    assert_eq!(mapping.openapi_field, Some("openapi".to_string()));
}

// Helper function to create a test model
fn create_test_model(id: &str) -> asset_tap_core::providers::config::ModelConfig {
    use asset_tap_core::providers::config::ModelConfig;

    ModelConfig {
        id: id.to_string(),
        name: format!("Test Model {}", id),
        description: "Test model".to_string(),
        endpoint: "/test".to_string(),
        method: HttpMethod::POST,
        request: RequestTemplate {
            headers: HashMap::new(),
            body: None,
            multipart: None,
        },
        response: ResponseTemplate {
            response_type: ResponseType::Json,
            field: None,
            polling: None,
        },
        is_default: false,
        cost_per_run: None,
        parameters: vec![],
    }
}

#[test]
fn test_openapi_field_mapping() {
    // Test that known field names are mapped to template variables
    let openapi = json!({
        "paths": {
            "/generate": {
                "post": {
                    "requestBody": {
                        "content": {
                            "application/json": {
                                "schema": {
                                    "properties": {
                                        "text": {"type": "string"},
                                        "description": {"type": "string"},
                                        "image": {"type": "string"},
                                        "input_image": {"type": "string"}
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    });

    let model =
        OpenApiParser::parse_model("test".to_string(), "Test".to_string(), None, &openapi, "")
            .unwrap();

    let body = model.request.body.unwrap();

    // All these should map to template variables
    assert_eq!(body.get("text").unwrap().as_str().unwrap(), "${prompt}");
    assert_eq!(
        body.get("description").unwrap().as_str().unwrap(),
        "${prompt}"
    );
    assert_eq!(body.get("image").unwrap().as_str().unwrap(), "${image_url}");
    assert_eq!(
        body.get("input_image").unwrap().as_str().unwrap(),
        "${image_url}"
    );
}

#[test]
fn test_openapi_optional_fields_skipped() {
    // Test that fields without defaults are skipped
    let openapi = json!({
        "paths": {
            "/generate": {
                "post": {
                    "requestBody": {
                        "content": {
                            "application/json": {
                                "schema": {
                                    "properties": {
                                        "prompt": {"type": "string"},
                                        "optional_param": {"type": "string"}
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    });

    let model =
        OpenApiParser::parse_model("test".to_string(), "Test".to_string(), None, &openapi, "")
            .unwrap();

    let body = model.request.body.unwrap();

    // prompt should be present
    assert!(body.get("prompt").is_some());

    // optional_param should be skipped (no default, not a known field)
    assert!(body.get("optional_param").is_none());
}
