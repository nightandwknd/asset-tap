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
use serde::de::{self, MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::{Map, Value};
use std::collections::HashMap;
use std::fmt;
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
    pub const BIND: &str = "bind";
    pub const IMPORT: &str = "import";
}

/// Step `kind` values on the wire. Anything else is [`PipelineStep::Other`].
pub mod kinds {
    pub const MODEL: &str = "model";
    pub const OP: &str = "op";
}

pub const ARTIFACT_IMAGE: &str = "image";
pub const ARTIFACT_MODEL: &str = "model";

pub const STEP_IMAGE: &str = "image";
pub const STEP_MODEL: &str = "model";
pub const STEP_BIND: &str = "bind";
pub const STEP_IMPORT: &str = "import";

/// `params.source` on an `import` step: the original filename.
pub const PARAM_SOURCE: &str = "source";

/// One file (or a dropped intermediate) in the bundle inventory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Artifact {
    pub id: String,
    pub role: String,
    /// Relative path. Omitted when the file was dropped but provenance stays.
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

impl Artifact {
    fn apply_model_info(&mut self, info: &ModelInfo) {
        self.file_size = Some(info.file_size);
        self.format = Some(info.format.clone());
        self.vertex_count = Some(info.vertex_count);
        self.triangle_count = Some(info.triangle_count);
    }
}

/// Provenance: an optional recipe plus an ordered list of steps.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BundlePipeline {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recipe: Option<RecipeRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<PipelineStep>,
}

impl BundlePipeline {
    /// The `import` step, if this pipeline has one.
    pub fn import_step(&self) -> Option<&PipelineStep> {
        self.steps
            .iter()
            .find(|s| matches!(s, PipelineStep::Op { op, .. } if op == ops::IMPORT))
    }

    /// `params.source` of the `import` step, if recorded.
    pub fn import_source(&self) -> Option<&str> {
        match self.import_step()? {
            PipelineStep::Op { params, .. } => params.get(PARAM_SOURCE)?.as_str(),
            _ => None,
        }
    }

    /// True when every step is an `import` op (or there are none): nothing
    /// here was generated, so the bundle can be re-described from disk.
    pub fn is_import_only(&self) -> bool {
        self.steps
            .iter()
            .all(|s| matches!(s, PipelineStep::Op { op, .. } if op == ops::IMPORT))
    }
}

/// Recipe this run executed. Unused until the registry exists; reserved so
/// writers can start stamping it without another schema bump.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecipeRef {
    pub id: String,
    pub version: String,
}

/// One pipeline step: a provider model call, or a named deterministic op.
///
/// A `kind` this build does not know is kept as [`PipelineStep::Other`] and
/// written back byte-for-byte in content, so a newer writer's bundle still
/// loads (name, tags, favorite, notes intact) and a save here does not strip
/// its provenance. Readers skip `Other`.
#[derive(Debug, Clone, PartialEq)]
pub enum PipelineStep {
    Model {
        id: String,
        provider: Option<String>,
        model: String,
        modality: String,
        prompt: Option<String>,
        user_prompt: Option<String>,
        template: Option<String>,
        params: HashMap<String, Value>,
        inputs: Vec<String>,
        outputs: Vec<String>,
        duration_ms: Option<u64>,
    },
    Op {
        id: String,
        op: String,
        params: HashMap<String, Value>,
        inputs: Vec<String>,
        outputs: Vec<String>,
        duration_ms: Option<u64>,
    },
    /// A step kind from a newer writer. `fields` holds everything but `kind`.
    Other {
        kind: String,
        fields: Map<String, Value>,
    },
}

/// The wire shape of the kinds this build knows. `PipelineStep` wraps it so
/// unknown kinds can fall through to [`PipelineStep::Other`].
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum KnownStep {
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

impl From<KnownStep> for PipelineStep {
    fn from(step: KnownStep) -> Self {
        match step {
            KnownStep::Model {
                id,
                provider,
                model,
                modality,
                prompt,
                user_prompt,
                template,
                params,
                inputs,
                outputs,
                duration_ms,
            } => Self::Model {
                id,
                provider,
                model,
                modality,
                prompt,
                user_prompt,
                template,
                params,
                inputs,
                outputs,
                duration_ms,
            },
            KnownStep::Op {
                id,
                op,
                params,
                inputs,
                outputs,
                duration_ms,
            } => Self::Op {
                id,
                op,
                params,
                inputs,
                outputs,
                duration_ms,
            },
        }
    }
}

impl Serialize for PipelineStep {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.clone() {
            Self::Model {
                id,
                provider,
                model,
                modality,
                prompt,
                user_prompt,
                template,
                params,
                inputs,
                outputs,
                duration_ms,
            } => KnownStep::Model {
                id,
                provider,
                model,
                modality,
                prompt,
                user_prompt,
                template,
                params,
                inputs,
                outputs,
                duration_ms,
            }
            .serialize(serializer),
            Self::Op {
                id,
                op,
                params,
                inputs,
                outputs,
                duration_ms,
            } => KnownStep::Op {
                id,
                op,
                params,
                inputs,
                outputs,
                duration_ms,
            }
            .serialize(serializer),
            Self::Other { kind, fields } => {
                let mut map = serializer.serialize_map(Some(fields.len() + 1))?;
                map.serialize_entry("kind", &kind)?;
                for (k, v) in &fields {
                    map.serialize_entry(k, v)?;
                }
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for PipelineStep {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct StepVisitor;

        impl<'de> Visitor<'de> for StepVisitor {
            type Value = PipelineStep;

            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a pipeline step object with a `kind`")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Self::Value, A::Error> {
                let mut fields = Map::new();
                while let Some((k, v)) = access.next_entry::<String, Value>()? {
                    fields.insert(k, v);
                }
                let kind = match fields.get("kind") {
                    Some(Value::String(k)) => k.clone(),
                    Some(_) => return Err(de::Error::custom("step `kind` must be a string")),
                    None => return Err(de::Error::missing_field("kind")),
                };
                if kind == kinds::MODEL || kind == kinds::OP {
                    let known: KnownStep =
                        serde_json::from_value(Value::Object(fields)).map_err(de::Error::custom)?;
                    return Ok(known.into());
                }
                fields.remove("kind");
                Ok(PipelineStep::Other { kind, fields })
            }
        }

        deserializer.deserialize_map(StepVisitor)
    }
}

impl PipelineStep {
    pub fn id(&self) -> &str {
        match self {
            Self::Model { id, .. } | Self::Op { id, .. } => id,
            Self::Other { fields, .. } => fields.get("id").and_then(Value::as_str).unwrap_or(""),
        }
    }

    /// Artifact ids this step reads. Empty for [`PipelineStep::Other`].
    pub fn inputs(&self) -> &[String] {
        match self {
            Self::Model { inputs, .. } | Self::Op { inputs, .. } => inputs,
            Self::Other { .. } => &[],
        }
    }

    /// Artifact ids this step writes. Empty for [`PipelineStep::Other`].
    pub fn outputs(&self) -> &[String] {
        match self {
            Self::Model { outputs, .. } | Self::Op { outputs, .. } => outputs,
            Self::Other { .. } => &[],
        }
    }

    fn outputs_mut(&mut self) -> Option<&mut Vec<String>> {
        match self {
            Self::Model { outputs, .. } | Self::Op { outputs, .. } => Some(outputs),
            Self::Other { .. } => None,
        }
    }

    fn set_duration(&mut self, ms: Option<u64>) {
        if let Self::Model { duration_ms, .. } | Self::Op { duration_ms, .. } = self {
            *duration_ms = ms;
        }
    }
}

/// The one `bind` step constructor. `stamp_bind_step` and
/// [`steps_from_config`] both write this step, and a bundle's shape cannot
/// depend on which one ran.
///
/// `clips` is the full baked set — empty after a fit-only bind. The skeleton
/// is recorded rather than a pack id: fitting uses the embedded canonical rig
/// and touches no pack at all, so naming one was a fiction.
pub fn bind_step(clips: &[String]) -> PipelineStep {
    let mut params = HashMap::new();
    params.insert(
        "clips".into(),
        Value::Array(clips.iter().map(|c| Value::String(c.clone())).collect()),
    );
    params.insert(
        "skeleton".into(),
        Value::String(crate::rig::SKELETON_ID.into()),
    );
    PipelineStep::Op {
        id: STEP_BIND.to_string(),
        op: ops::BIND.to_string(),
        params,
        inputs: vec![ARTIFACT_MODEL.to_string()],
        outputs: vec![ARTIFACT_MODEL.to_string()],
        duration_ms: None,
    }
}

fn import_step(source: Option<&str>, outputs: Vec<String>) -> PipelineStep {
    let mut params = HashMap::new();
    if let Some(source) = source.filter(|s| !s.is_empty()) {
        params.insert(PARAM_SOURCE.into(), Value::String(source.to_string()));
    }
    PipelineStep::Op {
        id: STEP_IMPORT.to_string(),
        op: ops::IMPORT.to_string(),
        params,
        inputs: Vec::new(),
        outputs,
        duration_ms: None,
    }
}

/// Wall-clock time each stage took, for the per-step `duration_ms`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StepDurations {
    /// The `import` step when the image was supplied (`--image`).
    pub import_ms: Option<u64>,
    pub image_ms: Option<u64>,
    pub model_ms: Option<u64>,
    pub bind_ms: Option<u64>,
}

/// Inputs the writer needs besides the files already on disk.
#[derive(Debug, Clone, Default)]
pub struct GenerationManifest {
    pub config: GenerationConfig,
    pub model_info: Option<ModelInfo>,
    pub image_provider_id: Option<String>,
    pub model_3d_provider_id: Option<String>,
    pub bind: bool,
    /// The full baked set. Empty after a fit-only bind.
    pub clips: Vec<String>,
    pub durations: StepDurations,
}

impl GenerationManifest {
    /// A manifest for v1 `config` / `model_info`, providers inferred from ids.
    pub fn from_v1(config: &GenerationConfig, model_info: Option<&ModelInfo>) -> Self {
        Self {
            config: config.clone(),
            model_info: model_info.cloned(),
            image_provider_id: config
                .image_model
                .as_deref()
                .and_then(provider_from_model_id),
            model_3d_provider_id: provider_from_model_id(&config.model_3d),
            bind: false,
            clips: Vec::new(),
            durations: StepDurations::default(),
        }
    }

    /// Which step produced the image: the generation step, or the `import`
    /// op when the user supplied it (`existing_image`).
    fn image_step(&self) -> Option<&'static str> {
        if self.config.image_model.is_some() {
            Some(STEP_IMAGE)
        } else if self.config.existing_image.is_some() {
            Some(STEP_IMPORT)
        } else {
            None
        }
    }
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
        Some("gif") => Some("image/gif".into()),
        Some("avif") => Some("image/avif".into()),
        Some("glb") => Some("model/gltf-binary".into()),
        Some("gltf") => Some("model/gltf+json".into()),
        Some("fbx") => Some("application/octet-stream".into()),
        Some("json") => Some("application/json".into()),
        _ => None,
    }
}

/// What is on disk, described as artifacts. Shared by every writer: only the
/// `produced_by` attribution differs between a generation and an import.
struct Inventory {
    artifacts: Vec<Artifact>,
    has_image: bool,
    has_model: bool,
}

fn inventory_on_disk(
    bundle_dir: &Path,
    model_info: Option<&ModelInfo>,
    image_step: Option<&str>,
    model_step: Option<&str>,
) -> Inventory {
    let image_rel = bundle_files::IMAGE;
    let model_rel = bundle_files::MODEL_GLB;
    let has_image = bundle_dir.join(image_rel).is_file();
    let has_model = bundle_dir.join(model_rel).is_file();

    let mut artifacts = Vec::new();
    if has_image {
        artifacts.push(file_artifact(
            bundle_dir,
            ARTIFACT_IMAGE,
            roles::IMAGE,
            image_rel,
            image_step.map(str::to_string),
            None,
        ));
    }
    if has_model {
        let mut art = file_artifact(
            bundle_dir,
            ARTIFACT_MODEL,
            roles::MODEL,
            model_rel,
            model_step.map(str::to_string),
            None,
        );
        if let Some(info) = model_info {
            art.apply_model_info(info);
        }
        artifacts.push(art);
    }
    if let Some(textures) = list_textures(bundle_dir) {
        for rel in textures {
            let id = texture_id(&rel);
            artifacts.push(file_artifact(
                bundle_dir,
                &id,
                roles::TEXTURE,
                &rel,
                has_model.then(|| model_step.map(str::to_string)).flatten(),
                None,
            ));
        }
    }

    Inventory {
        artifacts,
        has_image,
        has_model,
    }
}

fn texture_id(rel: &str) -> String {
    let stem = Path::new(rel)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("texture");
    format!("tex_{stem}")
}

/// Describe the files in `bundle_dir` as v2 artifacts + steps.
pub fn describe_generation(
    bundle_dir: &Path,
    manifest: &GenerationManifest,
) -> (Vec<Artifact>, Option<String>, BundlePipeline) {
    let inv = inventory_on_disk(
        bundle_dir,
        manifest.model_info.as_ref(),
        manifest.image_step(),
        Some(STEP_MODEL),
    );
    let pipeline = steps_from_config(manifest, inv.has_image, inv.has_model);
    let primary = primary_of(inv.has_image, inv.has_model);
    (inv.artifacts, primary, pipeline)
}

/// Describe files already on disk as a v2 import: artifacts plus one `import` op.
///
/// No provider steps — we did not generate these files. `source` is the
/// original filename when we renamed a loose asset (`hero.glb` → `model.glb`).
pub fn describe_import(
    bundle_dir: &Path,
    source: Option<&str>,
    model_info: Option<&ModelInfo>,
) -> (Vec<Artifact>, Option<String>, BundlePipeline) {
    let inv = inventory_on_disk(bundle_dir, model_info, Some(STEP_IMPORT), Some(STEP_IMPORT));

    let mut outputs = Vec::new();
    if inv.has_image {
        outputs.push(ARTIFACT_IMAGE.to_string());
    }
    if inv.has_model {
        outputs.push(ARTIFACT_MODEL.to_string());
    }
    let pipeline = BundlePipeline {
        recipe: None,
        steps: vec![import_step(source, outputs)],
    };

    (
        inv.artifacts,
        primary_of(inv.has_image, inv.has_model),
        pipeline,
    )
}

/// Describe files on disk with no provenance at all: artifacts, no steps.
///
/// For a v1 bundle that carried no `config` (the demo bundle), upgraded in
/// place before a write.
pub fn describe_files(
    bundle_dir: &Path,
    model_info: Option<&ModelInfo>,
) -> (Vec<Artifact>, Option<String>, BundlePipeline) {
    let inv = inventory_on_disk(bundle_dir, model_info, None, None);
    (
        inv.artifacts,
        primary_of(inv.has_image, inv.has_model),
        BundlePipeline::default(),
    )
}

/// Fold an attach into a bundle that already carries real provenance.
///
/// [`describe_import`] stamps **every** artifact it finds with the `import`
/// step, which is right for a bundle that *is* an import and wrong for a
/// generated one: assigning that list wholesale would relabel a generated
/// image as imported and point it at a step the pipeline does not contain.
/// So each existing artifact keeps its own `produced_by`, `attached_id` is
/// replaced (it is what this attach wrote), and anything else new is taken
/// as described.
///
/// Textures always follow `described`: they are whatever is on disk now.
/// Replacing the model cleared the old set, so texture artifacts that
/// `described` no longer lists are dropped, and same-named ones (a second
/// `texture_0.png`) are refreshed rather than kept with a stale hash.
///
/// Returns the ids that now cite [`STEP_IMPORT`], so the caller can make sure
/// that step exists.
pub fn merge_attached_artifacts(
    existing: &mut Vec<Artifact>,
    described: Vec<Artifact>,
    attached_id: &str,
) -> Vec<String> {
    existing.retain(|a| a.role != roles::TEXTURE || described.iter().any(|d| d.id == a.id));

    let mut imported = Vec::new();
    for artifact in described {
        let replace = artifact.id == attached_id || artifact.role == roles::TEXTURE;
        match existing.iter_mut().find(|a| a.id == artifact.id) {
            Some(slot) if replace => {
                imported.push(artifact.id.clone());
                *slot = artifact;
            }
            // Untouched by this attach — its own provenance stands.
            Some(_) => {}
            None => {
                imported.push(artifact.id.clone());
                existing.push(artifact);
            }
        }
    }
    imported
}

/// The step that used to produce `replaced` no longer does: an attach wrote
/// over that file. Drop the id from every non-import step's `outputs`, and
/// drop a `bind` step outright when the model it rigged is the one replaced —
/// the new mesh carries no rig.
pub fn retire_replaced_output(pipeline: &mut BundlePipeline, replaced: &str) {
    pipeline.steps.retain(|s| {
        !matches!(s, PipelineStep::Op { op, outputs, .. }
            if op == ops::BIND && outputs.iter().any(|o| o == replaced))
    });
    for step in &mut pipeline.steps {
        if matches!(step, PipelineStep::Op { op, .. } if op == ops::IMPORT) {
            continue;
        }
        if let Some(outputs) = step.outputs_mut() {
            outputs.retain(|o| o != replaced);
        }
    }
}

/// Append an `import` step listing `outputs`, or extend the one already there.
///
/// An artifact whose `produced_by` is [`STEP_IMPORT`] is dangling until the
/// pipeline actually holds that step.
pub fn ensure_import_step(pipeline: &mut BundlePipeline, outputs: &[String]) {
    if outputs.is_empty() {
        return;
    }
    if let Some(PipelineStep::Op {
        outputs: existing, ..
    }) = pipeline
        .steps
        .iter_mut()
        .find(|s| matches!(s, PipelineStep::Op { op, .. } if op == ops::IMPORT))
    {
        for id in outputs {
            if !existing.contains(id) {
                existing.push(id.clone());
            }
        }
        return;
    }
    pipeline.steps.push(import_step(None, outputs.to_vec()));
}

/// Synthesize a v2 inventory from v1 `config` fields and conventional paths.
///
/// Used by readers that want artifacts without walking the directory. No
/// hashes — those require the files. Does not rewrite anything on disk.
///
/// A v1 file with no `config` (the demo bundle) still names its model from
/// `model_info`, so `primary` resolves; with a `bundle_dir` the files on
/// disk are consulted too.
pub fn synthesize_from_v1(
    config: Option<&GenerationConfig>,
    model_info: Option<&ModelInfo>,
    bundle_dir: Option<&Path>,
) -> (Vec<Artifact>, Option<String>, BundlePipeline) {
    let on_disk = |rel: &str| bundle_dir.is_some_and(|d| d.join(rel).is_file());

    let Some(config) = config else {
        let has_image = on_disk(bundle_files::IMAGE);
        let has_model = model_info.is_some() || on_disk(bundle_files::MODEL_GLB);
        let mut artifacts = Vec::new();
        if has_image {
            artifacts.push(logical_artifact(
                ARTIFACT_IMAGE,
                roles::IMAGE,
                bundle_files::IMAGE,
                None,
            ));
        }
        if has_model {
            let mut art =
                logical_artifact(ARTIFACT_MODEL, roles::MODEL, bundle_files::MODEL_GLB, None);
            if let Some(info) = model_info {
                art.apply_model_info(info);
            }
            artifacts.push(art);
        }
        return (
            artifacts,
            primary_of(has_image, has_model),
            BundlePipeline::default(),
        );
    };

    let manifest = GenerationManifest::from_v1(config, model_info);
    let has_image = config.image_model.is_some()
        || config.existing_image.is_some()
        || on_disk(bundle_files::IMAGE);
    let has_model = !config.model_3d.is_empty() || on_disk(bundle_files::MODEL_GLB);

    let mut artifacts = Vec::new();
    if has_image {
        artifacts.push(logical_artifact(
            ARTIFACT_IMAGE,
            roles::IMAGE,
            bundle_files::IMAGE,
            manifest.image_step().map(str::to_string),
        ));
    }
    if has_model {
        let mut art = logical_artifact(
            ARTIFACT_MODEL,
            roles::MODEL,
            bundle_files::MODEL_GLB,
            (!config.model_3d.is_empty()).then(|| STEP_MODEL.to_string()),
        );
        if let Some(info) = model_info {
            art.apply_model_info(info);
        }
        artifacts.push(art);
    }
    let pipeline = steps_from_config(&manifest, has_image, has_model);
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

/// The steps a generation ran, in order: an `import` op when the image was
/// supplied rather than generated, then the model calls, then `bind`.
pub fn steps_from_config(
    manifest: &GenerationManifest,
    has_image: bool,
    has_model: bool,
) -> BundlePipeline {
    let config = &manifest.config;
    let durations = &manifest.durations;
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
            duration_ms: durations.image_ms,
        });
    } else if has_image && config.existing_image.is_some() {
        // `existing_image` is already sanitized to a filename / URL by
        // `GenerationConfig::from`, so it is safe to record as `source`.
        let mut step = import_step(
            config.existing_image.as_deref(),
            vec![ARTIFACT_IMAGE.to_string()],
        );
        step.set_duration(durations.import_ms);
        steps.push(step);
    }

    if !config.model_3d.is_empty() && has_model {
        let modality = if has_image || config.image_model.is_some() {
            modalities::IMAGE_TO_3D
        } else {
            modalities::TEXT_TO_3D
        };
        // The prompt is recorded once, on the step it was sent to. When the
        // 3D stage is the first model call (`--image` or text-to-3D) it goes
        // here, otherwise the image step already has it.
        let first_model_call = config.image_model.is_none();
        steps.push(PipelineStep::Model {
            id: STEP_MODEL.to_string(),
            provider: manifest
                .model_3d_provider_id
                .clone()
                .or_else(|| provider_from_model_id(&config.model_3d)),
            model: config.model_3d.clone(),
            modality: modality.to_string(),
            prompt: first_model_call.then(|| config.prompt.clone()).flatten(),
            user_prompt: first_model_call
                .then(|| config.user_prompt.clone())
                .flatten(),
            template: first_model_call.then(|| config.template.clone()).flatten(),
            params: config.model_3d_params.clone(),
            inputs: if has_image {
                vec![ARTIFACT_IMAGE.to_string()]
            } else {
                Vec::new()
            },
            outputs: vec![ARTIFACT_MODEL.to_string()],
            duration_ms: durations.model_ms,
        });
    }

    if manifest.bind && has_model {
        let mut step = bind_step(&manifest.clips);
        step.set_duration(durations.bind_ms);
        steps.push(step);
    }

    BundlePipeline {
        recipe: None,
        steps,
    }
}

/// Drop references to artifact ids that do not exist. Returns one message per
/// fix so the loader can log them; the file is repaired, never rejected.
pub fn prune_dangling_refs(
    artifacts: &mut [Artifact],
    primary: &mut Option<String>,
    pipeline: Option<&mut BundlePipeline>,
) -> Vec<String> {
    let mut issues = Vec::new();
    let ids: std::collections::HashSet<String> = artifacts.iter().map(|a| a.id.clone()).collect();
    let step_ids: std::collections::HashSet<String> = pipeline
        .as_deref()
        .map(|p| p.steps.iter().map(|s| s.id().to_string()).collect())
        .unwrap_or_default();

    if let Some(p) = primary.as_deref()
        && !ids.contains(p)
    {
        issues.push(format!("primary `{p}` names no artifact, cleared"));
        *primary = None;
    }
    for art in artifacts.iter_mut() {
        if let Some(step) = art.produced_by.as_deref()
            && !step_ids.contains(step)
        {
            issues.push(format!(
                "artifact `{}` produced_by `{step}` names no step, cleared",
                art.id
            ));
            art.produced_by = None;
        }
    }
    if let Some(pipeline) = pipeline {
        for step in &mut pipeline.steps {
            let id = step.id().to_string();
            let (inputs, outputs) = match step {
                PipelineStep::Model {
                    inputs, outputs, ..
                }
                | PipelineStep::Op {
                    inputs, outputs, ..
                } => (inputs, outputs),
                PipelineStep::Other { .. } => continue,
            };
            for (label, list) in [("inputs", inputs), ("outputs", outputs)] {
                let before = list.len();
                list.retain(|a| ids.contains(a));
                if list.len() != before {
                    issues.push(format!(
                        "step `{id}` {label} named {} missing artifact(s), dropped",
                        before - list.len()
                    ));
                }
            }
        }
    }
    issues
}

/// Artifact ids that appear more than once, keeping the first occurrence.
pub fn dedupe_artifact_ids(artifacts: &mut Vec<Artifact>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut dropped = Vec::new();
    artifacts.retain(|a| {
        if seen.insert(a.id.clone()) {
            true
        } else {
            dropped.push(format!("duplicate artifact id `{}` dropped", a.id));
            false
        }
    });
    dropped
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
    fn synthesize_v1_two_stage() {
        let config = v1_config();
        let (artifacts, primary, pipeline) = synthesize_from_v1(Some(&config), None, None);
        assert_eq!(primary.as_deref(), Some(ARTIFACT_MODEL));
        assert_eq!(
            artifacts.iter().map(|a| a.id.as_str()).collect::<Vec<_>>(),
            [ARTIFACT_IMAGE, ARTIFACT_MODEL]
        );
        assert_eq!(pipeline.steps.len(), 2);
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
    }

    #[test]
    fn synthesize_image_only() {
        let config = GenerationConfig {
            image_model: Some("fal-ai/nano-banana-2".into()),
            model_3d: String::new(),
            prompt: Some("icon".into()),
            ..Default::default()
        };
        let (artifacts, primary, pipeline) = synthesize_from_v1(Some(&config), None, None);
        assert_eq!(artifacts.len(), 1);
        assert_eq!(primary.as_deref(), Some(ARTIFACT_IMAGE));
        assert_eq!(pipeline.steps.len(), 1);
    }

    #[test]
    fn synthesize_model_only_is_text_to_3d() {
        let config = GenerationConfig {
            image_model: None,
            model_3d: "fal-ai/hunyuan-world".into(),
            prompt: Some("a chair".into()),
            ..Default::default()
        };
        let (artifacts, primary, pipeline) = synthesize_from_v1(Some(&config), None, None);
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
    fn describe_import_is_one_op_not_a_generation() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(bundle_files::MODEL_GLB), b"fake-glb").unwrap();

        let (artifacts, primary, pipeline) = describe_import(dir.path(), Some("hero.glb"), None);
        assert_eq!(primary.as_deref(), Some(ARTIFACT_MODEL));
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].produced_by.as_deref(), Some(STEP_IMPORT));
        assert_eq!(pipeline.steps.len(), 1);
        match &pipeline.steps[0] {
            PipelineStep::Op {
                op,
                params,
                outputs,
                ..
            } => {
                assert_eq!(op, ops::IMPORT);
                assert_eq!(params["source"], "hero.glb");
                assert_eq!(outputs, &["model"]);
            }
            other => panic!("expected import op, got {other:?}"),
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
            ..Default::default()
        };
        let (artifacts, primary, pipeline) = describe_generation(dir.path(), &manifest);
        assert_eq!(primary.as_deref(), Some(ARTIFACT_MODEL));
        assert_eq!(artifacts.len(), 2);
        assert!(artifacts[0].sha256.is_some());
        assert_eq!(artifacts[0].mime.as_deref(), Some("image/png"));
        assert_eq!(pipeline.steps.len(), 2);
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
                "model_3d": "fal-ai/trellis-2"
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

    #[test]
    fn unknown_step_kind_is_preserved_not_rejected() {
        let json = r#"{
            "kind": "future_thing",
            "id": "ft",
            "widget": {"depth": 3},
            "outputs": ["model"]
        }"#;
        let step: PipelineStep = serde_json::from_str(json).expect("unknown kind parses");
        assert_eq!(step.id(), "ft");
        assert!(step.outputs().is_empty(), "readers skip Other");
        let back = serde_json::to_value(&step).unwrap();
        assert_eq!(back["kind"], "future_thing");
        assert_eq!(back["widget"]["depth"], 3);
        assert_eq!(back["outputs"], serde_json::json!(["model"]));
        let again: PipelineStep = serde_json::from_value(back).unwrap();
        assert_eq!(again, step);
    }

    #[test]
    fn known_kind_with_bad_shape_is_still_an_error() {
        let json = r#"{"kind": "model", "id": "x"}"#;
        assert!(serde_json::from_str::<PipelineStep>(json).is_err());
        let json = r#"{"id": "x"}"#;
        assert!(serde_json::from_str::<PipelineStep>(json).is_err());
    }

    #[test]
    fn supplied_image_is_an_import_step_feeding_image_to_3d() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(bundle_files::IMAGE), b"png").unwrap();
        std::fs::write(dir.path().join(bundle_files::MODEL_GLB), b"glb").unwrap();
        let manifest = GenerationManifest {
            config: GenerationConfig {
                existing_image: Some("input.png".into()),
                image_model: None,
                model_3d: "fal-ai/trellis-2".into(),
                ..Default::default()
            },
            durations: StepDurations {
                import_ms: Some(3),
                model_ms: Some(40),
                ..Default::default()
            },
            ..Default::default()
        };
        let (artifacts, _, pipeline) = describe_generation(dir.path(), &manifest);
        let image = artifacts.iter().find(|a| a.id == ARTIFACT_IMAGE).unwrap();
        assert_eq!(image.produced_by.as_deref(), Some(STEP_IMPORT));
        assert_eq!(pipeline.steps.len(), 2);
        match &pipeline.steps[0] {
            PipelineStep::Op {
                op,
                params,
                outputs,
                duration_ms,
                ..
            } => {
                assert_eq!(op, ops::IMPORT);
                assert_eq!(params[PARAM_SOURCE], "input.png");
                assert_eq!(outputs, &[ARTIFACT_IMAGE]);
                assert_eq!(*duration_ms, Some(3));
            }
            other => panic!("expected import op, got {other:?}"),
        }
        match &pipeline.steps[1] {
            PipelineStep::Model {
                modality,
                inputs,
                duration_ms,
                ..
            } => {
                assert_eq!(modality, modalities::IMAGE_TO_3D);
                assert_eq!(inputs, &[ARTIFACT_IMAGE]);
                assert_eq!(*duration_ms, Some(40));
            }
            other => panic!("expected model step, got {other:?}"),
        }
    }

    #[test]
    fn prompt_is_recorded_once_on_the_step_it_went_to() {
        let (_, _, two_stage) = synthesize_from_v1(Some(&v1_config()), None, None);
        match &two_stage.steps[1] {
            PipelineStep::Model { prompt, .. } => assert!(prompt.is_none()),
            other => panic!("{other:?}"),
        }
        let config = GenerationConfig {
            image_model: None,
            model_3d: "fal-ai/hunyuan-world".into(),
            prompt: Some("a chair".into()),
            ..Default::default()
        };
        let (_, _, text_to_3d) = synthesize_from_v1(Some(&config), None, None);
        match &text_to_3d.steps[0] {
            PipelineStep::Model { prompt, .. } => assert_eq!(prompt.as_deref(), Some("a chair")),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn synthesize_without_config_still_names_the_model() {
        let info = ModelInfo {
            file_size: 10,
            format: "GLB".into(),
            vertex_count: 3,
            triangle_count: 1,
        };
        let (artifacts, primary, pipeline) = synthesize_from_v1(None, Some(&info), None);
        assert_eq!(primary.as_deref(), Some(ARTIFACT_MODEL));
        assert_eq!(artifacts.len(), 1);
        assert_eq!(artifacts[0].vertex_count, Some(3));
        assert!(pipeline.steps.is_empty());

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(bundle_files::IMAGE), b"png").unwrap();
        let (artifacts, primary, _) = synthesize_from_v1(None, None, Some(dir.path()));
        assert_eq!(primary.as_deref(), Some(ARTIFACT_IMAGE));
        assert_eq!(artifacts.len(), 1);
    }

    #[test]
    fn retire_replaced_output_drops_bind_and_old_producer_output() {
        let mut pipeline = BundlePipeline {
            recipe: None,
            steps: vec![
                PipelineStep::Model {
                    id: STEP_MODEL.into(),
                    provider: None,
                    model: "m".into(),
                    modality: modalities::TEXT_TO_3D.into(),
                    prompt: None,
                    user_prompt: None,
                    template: None,
                    params: HashMap::new(),
                    inputs: vec![],
                    outputs: vec![ARTIFACT_MODEL.into()],
                    duration_ms: None,
                },
                bind_step(&["walk".into()]),
            ],
        };
        retire_replaced_output(&mut pipeline, ARTIFACT_MODEL);
        assert_eq!(pipeline.steps.len(), 1, "bind on the old mesh is gone");
        assert!(pipeline.steps[0].outputs().is_empty());
    }

    #[test]
    fn merge_refreshes_textures_from_disk() {
        let mut existing = vec![
            logical_artifact(
                ARTIFACT_IMAGE,
                roles::IMAGE,
                "image.png",
                Some("image".into()),
            ),
            logical_artifact(
                "tex_old",
                roles::TEXTURE,
                "textures/old.png",
                Some("model".into()),
            ),
            logical_artifact(
                "tex_same",
                roles::TEXTURE,
                "textures/same.png",
                Some("model".into()),
            ),
        ];
        let mut same = logical_artifact(
            "tex_same",
            roles::TEXTURE,
            "textures/same.png",
            Some(STEP_IMPORT.into()),
        );
        same.sha256 = Some("new".into());
        let described = vec![
            logical_artifact(
                ARTIFACT_IMAGE,
                roles::IMAGE,
                "image.png",
                Some(STEP_IMPORT.into()),
            ),
            logical_artifact(
                ARTIFACT_MODEL,
                roles::MODEL,
                "model.glb",
                Some(STEP_IMPORT.into()),
            ),
            same,
        ];
        let imported = merge_attached_artifacts(&mut existing, described, ARTIFACT_MODEL);
        let ids: Vec<&str> = existing.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["image", "tex_same", "model"]);
        assert_eq!(existing[0].produced_by.as_deref(), Some("image"));
        assert_eq!(existing[1].sha256.as_deref(), Some("new"));
        assert_eq!(imported, ["model", "tex_same"]);
    }

    #[test]
    fn prune_dangling_refs_repairs_without_failing() {
        let mut artifacts = vec![logical_artifact(
            ARTIFACT_MODEL,
            roles::MODEL,
            "model.glb",
            Some("ghost".into()),
        )];
        let mut primary = Some("nope".to_string());
        let mut pipeline = BundlePipeline {
            recipe: None,
            steps: vec![PipelineStep::Op {
                id: "x".into(),
                op: "y".into(),
                params: HashMap::new(),
                inputs: vec!["image".into(), ARTIFACT_MODEL.into()],
                outputs: vec!["nothing".into()],
                duration_ms: None,
            }],
        };
        let issues = prune_dangling_refs(&mut artifacts, &mut primary, Some(&mut pipeline));
        assert_eq!(issues.len(), 4, "{issues:?}");
        assert!(primary.is_none());
        assert!(artifacts[0].produced_by.is_none());
        assert_eq!(pipeline.steps[0].inputs(), [ARTIFACT_MODEL]);
        assert!(pipeline.steps[0].outputs().is_empty());
    }
}
