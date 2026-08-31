//! Bundle manifest v2: artifacts + a linear pipeline of steps.
//!
//! v1 `bundle.json` stays readable forever (in-memory synthesize, never
//! rewritten on load). New writes emit `version: 2` with `artifacts` /
//! `pipeline` / `primary` only — no `config` / `model_info`.
//! `category` is reserved on the struct and omitted until a recipe can
//! actually name the asset.
//!
//! This module is the rails only — no recipe registry, no ops catalog, no
//! `run` command. Today's text→image→3D (and image-only / model-only) is
//! expressed as steps so later categories do not need a new shape.

use crate::bundle::sha256_hex;
use crate::constants::files::bundle as bundle_files;
use crate::history::GenerationConfig;
use crate::state::ModelInfo;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

/// Current writer schema. Readers accept 1 and 2.
pub const SCHEMA_VERSION: u32 = 2;

/// Model-id prefix used by fal (`fal-ai/trellis-2`). The provider id is `fal.ai`.
const FAL_AI_MODEL_PREFIX: &str = "fal-ai";
const FAL_AI_PROVIDER_ID: &str = "fal.ai";

/// Artifact `role` values. Free-form string on the wire so unknown future
/// roles survive a round-trip; these are the ones we write today.
pub mod roles {
    pub const IMAGE: &str = "image";
    pub const MODEL: &str = "model";
    pub const TEXTURE: &str = "texture";
}

/// Model-step `modality` values.
pub mod modalities {
    pub const TEXT_TO_IMAGE: &str = "text_to_image";
    pub const IMAGE_TO_3D: &str = "image_to_3d";
    pub const TEXT_TO_3D: &str = "text_to_3d";
}

/// Deterministic op names we emit today. The rest of the catalog is later.
pub mod ops {
    pub const FBX_EXPORT: &str = "fbx_export";
}

pub const ARTIFACT_IMAGE: &str = "image";
pub const ARTIFACT_MODEL: &str = "model";
pub const ARTIFACT_MODEL_FBX: &str = "model_fbx";

pub const STEP_IMAGE: &str = "image";
pub const STEP_MODEL: &str = "model";
pub const STEP_FBX: &str = "fbx";

/// One file (or a dropped intermediate) in the bundle inventory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    pub id: String,
    pub role: String,
    /// Relative path, or `null` when the file was dropped but provenance stays.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub produced_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub texture_kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vertex_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub triangle_count: Option<usize>,
}

/// Provenance: an optional recipe plus an ordered list of steps.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BundlePipeline {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recipe: Option<RecipeRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<PipelineStep>,
}

/// Recipe this run executed. Unused until the registry exists; reserved so
/// writers can start stamping it without another schema bump.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecipeRef {
    pub id: String,
    pub version: String,
}

/// One pipeline step: a provider model call, or a named deterministic op.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PipelineStep {
    Model {
        id: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider: Option<String>,
        model: String,
        modality: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        prompt: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        user_prompt: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        template: Option<String>,
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        params: HashMap<String, Value>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        inputs: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        outputs: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u64>,
    },
    Op {
        id: String,
        op: String,
        #[serde(default, skip_serializing_if = "HashMap::is_empty")]
        params: HashMap<String, Value>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        inputs: Vec<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        outputs: Vec<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        duration_ms: Option<u64>,
    },
}

impl PipelineStep {
    pub fn id(&self) -> &str {
        match self {
            Self::Model { id, .. } | Self::Op { id, .. } => id,
        }
    }
}

/// Inputs the writer needs besides the files already on disk.
pub struct GenerationManifest {
    pub config: GenerationConfig,
    pub model_info: Option<ModelInfo>,
    pub image_provider_id: Option<String>,
    pub model_3d_provider_id: Option<String>,
}

/// Infer a provider id from a model id (`fal-ai/trellis-2` → `fal.ai`).
pub fn provider_from_model_id(model_id: &str) -> Option<String> {
    let prefix = model_id.split('/').next()?;
    if prefix == model_id {
        return None;
    }
    match prefix {
        FAL_AI_MODEL_PREFIX => Some(FAL_AI_PROVIDER_ID.into()),
        other => Some(other.into()),
    }
}

/// Guess a MIME type from a relative path.
pub fn mime_for_path(path: &str) -> Option<String> {
    match Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .as_deref()
    {
        Some("png") => Some("image/png".into()),
        Some("jpg" | "jpeg") => Some("image/jpeg".into()),
        Some("webp") => Some("image/webp".into()),
        Some("glb") => Some("model/gltf-binary".into()),
        Some("gltf") => Some("model/gltf+json".into()),
        Some("fbx") => Some("application/octet-stream".into()),
        Some("json") => Some("application/json".into()),
        _ => None,
    }
}

/// Describe the files in `bundle_dir` as v2 artifacts + steps.
pub fn describe_generation(
    bundle_dir: &Path,
    manifest: &GenerationManifest,
) -> (Vec<Artifact>, Option<String>, BundlePipeline) {
    let mut artifacts = Vec::new();
    let config = &manifest.config;

    let image_rel = bundle_files::IMAGE;
    let model_rel = bundle_files::MODEL_GLB;
    let fbx_rel = bundle_files::MODEL_FBX;
    let has_image = bundle_dir.join(image_rel).is_file();
    let has_model = bundle_dir.join(model_rel).is_file();
    let has_fbx = bundle_dir.join(fbx_rel).is_file();

    if has_image {
        let produced_by = config.image_model.as_ref().map(|_| STEP_IMAGE.to_string());
        artifacts.push(file_artifact(
            bundle_dir,
            ARTIFACT_IMAGE,
            roles::IMAGE,
            image_rel,
            produced_by,
            None,
        ));
    }

    if has_model {
        let mut art = file_artifact(
            bundle_dir,
            ARTIFACT_MODEL,
            roles::MODEL,
            model_rel,
            Some(STEP_MODEL.to_string()),
            None,
        );
        if let Some(info) = &manifest.model_info {
            art.file_size = Some(info.file_size);
            art.format = Some(info.format.clone());
            art.vertex_count = Some(info.vertex_count);
            art.triangle_count = Some(info.triangle_count);
        }
        artifacts.push(art);
    }

    if has_fbx {
        artifacts.push(file_artifact(
            bundle_dir,
            ARTIFACT_MODEL_FBX,
            roles::MODEL,
            fbx_rel,
            Some(STEP_FBX.to_string()),
            None,
        ));
    }

    if let Some(textures) = list_textures(bundle_dir) {
        for rel in textures {
            let stem = Path::new(&rel)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("texture");
            let id = format!("tex_{stem}");
            artifacts.push(file_artifact(
                bundle_dir,
                &id,
                roles::TEXTURE,
                &rel,
                has_model.then(|| STEP_MODEL.to_string()),
                None,
            ));
        }
    }

    let pipeline = steps_from_config(manifest, has_image, has_model, has_fbx);
    let primary = primary_of(has_image, has_model);

    (artifacts, primary, pipeline)
}

/// Synthesize a v2 inventory from v1 `config` fields and conventional paths.
///
/// Used by readers that want artifacts without walking the directory. No
/// hashes — those require the files. Does not rewrite anything on disk.
pub fn synthesize_from_v1(
    config: Option<&GenerationConfig>,
    model_info: Option<&ModelInfo>,
) -> (Vec<Artifact>, Option<String>, BundlePipeline) {
    let Some(config) = config else {
        return (Vec::new(), None, BundlePipeline::default());
    };

    let has_image = config.image_model.is_some() || config.existing_image.is_some();
    let has_model = !config.model_3d.is_empty();
    let has_fbx = config.export_fbx && has_model;

    let mut artifacts = Vec::new();
    if has_image {
        artifacts.push(logical_artifact(
            ARTIFACT_IMAGE,
            roles::IMAGE,
            bundle_files::IMAGE,
            config.image_model.as_ref().map(|_| STEP_IMAGE.to_string()),
        ));
    }
    if has_model {
        let mut art = logical_artifact(
            ARTIFACT_MODEL,
            roles::MODEL,
            bundle_files::MODEL_GLB,
            Some(STEP_MODEL.to_string()),
        );
        if let Some(info) = model_info {
            art.file_size = Some(info.file_size);
            art.format = Some(info.format.clone());
            art.vertex_count = Some(info.vertex_count);
            art.triangle_count = Some(info.triangle_count);
        }
        artifacts.push(art);
    }
    if has_fbx {
        artifacts.push(logical_artifact(
            ARTIFACT_MODEL_FBX,
            roles::MODEL,
            bundle_files::MODEL_FBX,
            Some(STEP_FBX.to_string()),
        ));
    }

    let manifest = GenerationManifest {
        config: config.clone(),
        model_info: model_info.cloned(),
        image_provider_id: config
            .image_model
            .as_deref()
            .and_then(provider_from_model_id),
        model_3d_provider_id: provider_from_model_id(&config.model_3d),
    };
    let pipeline = steps_from_config(&manifest, has_image, has_model, has_fbx);
    let primary = primary_of(has_image, has_model);

    (artifacts, primary, pipeline)
}

fn primary_of(has_image: bool, has_model: bool) -> Option<String> {
    if has_model {
        Some(ARTIFACT_MODEL.to_string())
    } else if has_image {
        Some(ARTIFACT_IMAGE.to_string())
    } else {
        None
    }
}

fn steps_from_config(
    manifest: &GenerationManifest,
    has_image: bool,
    has_model: bool,
    has_fbx: bool,
) -> BundlePipeline {
    let config = &manifest.config;
    let mut steps = Vec::new();

    if let Some(model_id) = config.image_model.as_deref() {
        steps.push(PipelineStep::Model {
            id: STEP_IMAGE.to_string(),
            provider: manifest
                .image_provider_id
                .clone()
                .or_else(|| provider_from_model_id(model_id)),
            model: model_id.to_string(),
            modality: modalities::TEXT_TO_IMAGE.to_string(),
            prompt: config.prompt.clone(),
            user_prompt: config.user_prompt.clone(),
            template: config.template.clone(),
            params: config.image_model_params.clone(),
            inputs: Vec::new(),
            outputs: if has_image {
                vec![ARTIFACT_IMAGE.to_string()]
            } else {
                Vec::new()
            },
            duration_ms: None,
        });
    }

    if !config.model_3d.is_empty() && (has_model || has_fbx) {
        let modality = if has_image || config.image_model.is_some() {
            modalities::IMAGE_TO_3D
        } else {
            modalities::TEXT_TO_3D
        };
        let mut outputs = Vec::new();
        if has_model {
            outputs.push(ARTIFACT_MODEL.to_string());
        }
        steps.push(PipelineStep::Model {
            id: STEP_MODEL.to_string(),
            provider: manifest
                .model_3d_provider_id
                .clone()
                .or_else(|| provider_from_model_id(&config.model_3d)),
            model: config.model_3d.clone(),
            modality: modality.to_string(),
            prompt: config.prompt.clone(),
            user_prompt: config.user_prompt.clone(),
            template: config.template.clone(),
            params: config.model_3d_params.clone(),
            inputs: if has_image {
                vec![ARTIFACT_IMAGE.to_string()]
            } else {
                Vec::new()
            },
            outputs,
            duration_ms: None,
        });
    }

    if has_fbx {
        steps.push(PipelineStep::Op {
            id: STEP_FBX.to_string(),
            op: ops::FBX_EXPORT.to_string(),
            params: HashMap::new(),
            inputs: if has_model {
                vec![ARTIFACT_MODEL.to_string()]
            } else {
                Vec::new()
            },
            outputs: vec![ARTIFACT_MODEL_FBX.to_string()],
            duration_ms: None,
        });
    }

    BundlePipeline {
        recipe: None,
        steps,
    }
}

fn file_artifact(
    bundle_dir: &Path,
    id: &str,
    role: &str,
    rel: &str,
    produced_by: Option<String>,
    texture_kind: Option<String>,
) -> Artifact {
    let path = bundle_dir.join(rel);
    let bytes = std::fs::read(&path).ok();
    let sha256 = bytes.as_deref().map(sha256_hex);
    let (width, height) = if role == roles::IMAGE || role == roles::TEXTURE {
        image::image_dimensions(&path)
            .ok()
            .map(|(w, h)| (Some(w), Some(h)))
            .unwrap_or((None, None))
    } else {
        (None, None)
    };

    Artifact {
        id: id.to_string(),
        role: role.to_string(),
        path: Some(rel.to_string()),
        mime: mime_for_path(rel),
        sha256,
        produced_by,
        width,
        height,
        texture_kind,
        file_size: bytes.as_ref().map(|b| b.len() as u64),
        format: None,
        vertex_count: None,
        triangle_count: None,
    }
}

fn logical_artifact(id: &str, role: &str, rel: &str, produced_by: Option<String>) -> Artifact {
    Artifact {
        id: id.to_string(),
        role: role.to_string(),
        path: Some(rel.to_string()),
        mime: mime_for_path(rel),
        sha256: None,
        produced_by,
        width: None,
        height: None,
        texture_kind: None,
        file_size: None,
        format: None,
        vertex_count: None,
        triangle_count: None,
    }
}

fn list_textures(bundle_dir: &Path) -> Option<Vec<String>> {
    let dir = bundle_dir.join(bundle_files::TEXTURES_DIR);
    let entries = std::fs::read_dir(&dir).ok()?;
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter_map(|e| {
            let name = e.file_name();
            let name = name.to_str()?;
            if name.starts_with('.') {
                return None;
            }
            Some(format!("{}/{name}", bundle_files::TEXTURES_DIR))
        })
        .collect();
    names.sort();
    if names.is_empty() { None } else { Some(names) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bundle::BundleMetadata;

    fn v1_config() -> GenerationConfig {
        GenerationConfig {
            prompt: Some("a crate".into()),
            user_prompt: None,
            template: None,
            existing_image: None,
            image_model: Some("fal-ai/nano-banana-2".into()),
            model_3d: "fal-ai/trellis-2".into(),
            export_fbx: true,
            image_model_params: HashMap::new(),
            model_3d_params: HashMap::new(),
        }
    }

    #[test]
    fn provider_from_fal_and_meshy_ids() {
        assert_eq!(
            provider_from_model_id("fal-ai/trellis-2").as_deref(),
            Some("fal.ai")
        );
        assert_eq!(
            provider_from_model_id("meshy/meshy-6").as_deref(),
            Some("meshy")
        );
        assert_eq!(provider_from_model_id("trellis-2"), None);
    }

    #[test]
    fn synthesize_v1_three_stage() {
        let config = v1_config();
        let (artifacts, primary, pipeline) = synthesize_from_v1(Some(&config), None);
        assert_eq!(primary.as_deref(), Some(ARTIFACT_MODEL));
        assert_eq!(
            artifacts.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(),
            [ARTIFACT_IMAGE, ARTIFACT_MODEL, ARTIFACT_MODEL_FBX]
        );
        assert_eq!(pipeline.steps.len(), 3);
        match &pipeline.steps[0] {
            PipelineStep::Model {
                modality, model, ..
            } => {
                assert_eq!(modality, modalities::TEXT_TO_IMAGE);
                assert_eq!(model, "fal-ai/nano-banana-2");
            }
            other => panic!("expected model step, got {other:?}"),
        }
        match &pipeline.steps[1] {
            PipelineStep::Model {
                modality, inputs, ..
            } => {
                assert_eq!(modality, modalities::IMAGE_TO_3D);
                assert_eq!(inputs, &["image"]);
            }
            other => panic!("expected model step, got {other:?}"),
        }
        match &pipeline.steps[2] {
            PipelineStep::Op { op, .. } => assert_eq!(op, ops::FBX_EXPORT),
            other => panic!("expected op step, got {other:?}"),
        }
    }

    #[test]
    fn synthesize_image_only() {
        let config = GenerationConfig {
            image_model: Some("fal-ai/nano-banana-2".into()),
            model_3d: String::new(),
            export_fbx: false,
            prompt: Some("icon".into()),
            ..Default::default()
        };
        let (artifacts, primary, pipeline) = synthesize_from_v1(Some(&config), None);
        assert_eq!(artifacts.len(), 1);
        assert_eq!(primary.as_deref(), Some(ARTIFACT_IMAGE));
        assert_eq!(pipeline.steps.len(), 1);
    }

    #[test]
    fn synthesize_model_only_is_text_to_3d() {
        let config = GenerationConfig {
            image_model: None,
            model_3d: "fal-ai/hunyuan-world".into(),
            export_fbx: false,
            prompt: Some("a chair".into()),
            ..Default::default()
        };
        let (artifacts, primary, pipeline) = synthesize_from_v1(Some(&config), None);
        assert_eq!(artifacts.len(), 1);
        assert_eq!(primary.as_deref(), Some(ARTIFACT_MODEL));
        match &pipeline.steps[0] {
            PipelineStep::Model { modality, .. } => {
                assert_eq!(modality, modalities::TEXT_TO_3D);
            }
            other => panic!("expected model step, got {other:?}"),
        }
    }

    #[test]
    fn describe_generation_hashes_files_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(bundle_files::IMAGE), b"fake-png").unwrap();
        std::fs::write(dir.path().join(bundle_files::MODEL_GLB), b"fake-glb").unwrap();

        let manifest = GenerationManifest {
            config: v1_config(),
            model_info: None,
            image_provider_id: Some("fal.ai".into()),
            model_3d_provider_id: Some("fal.ai".into()),
        };
        let (artifacts, primary, pipeline) = describe_generation(dir.path(), &manifest);
        assert_eq!(primary.as_deref(), Some(ARTIFACT_MODEL));
        assert_eq!(artifacts.len(), 2);
        assert!(artifacts[0].sha256.is_some());
        assert_eq!(artifacts[0].mime.as_deref(), Some("image/png"));
        assert_eq!(pipeline.steps.len(), 2); // no fbx file on disk
        match &pipeline.steps[0] {
            PipelineStep::Model { provider, .. } => {
                assert_eq!(provider.as_deref(), Some("fal.ai"));
            }
            other => panic!("expected model step, got {other:?}"),
        }
    }

    #[test]
    fn v1_json_still_deserializes() {
        let json = r#"{
            "version": 1,
            "name": "crate",
            "created_at": "2024-12-29T15:30:45Z",
            "config": {
                "prompt": "a crate",
                "image_model": "fal-ai/nano-banana-2",
                "model_3d": "fal-ai/trellis-2",
                "export_fbx": false
            }
        }"#;
        let parsed: BundleMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.version, 1);
        assert!(parsed.artifacts.is_empty());
        assert!(parsed.pipeline.is_none());
        let inventory = parsed.artifact_inventory();
        assert_eq!(inventory.len(), 2);
        assert_eq!(inventory[0].id, ARTIFACT_IMAGE);
        assert_eq!(inventory[1].id, ARTIFACT_MODEL);
    }

    #[test]
    fn v2_step_tag_round_trips() {
        let step = PipelineStep::Model {
            id: "image".into(),
            provider: Some("fal.ai".into()),
            model: "fal-ai/nano-banana-2".into(),
            modality: modalities::TEXT_TO_IMAGE.into(),
            prompt: Some("x".into()),
            user_prompt: None,
            template: None,
            params: HashMap::new(),
            inputs: vec![],
            outputs: vec!["image".into()],
            duration_ms: None,
        };
        let value = serde_json::to_value(&step).unwrap();
        assert_eq!(value["kind"], "model");
        let back: PipelineStep = serde_json::from_value(value).unwrap();
        assert_eq!(back.id(), "image");
    }
}
