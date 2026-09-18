# Bundle Structure

Generated assets are organized into timestamped bundle directories with metadata.

## Directory Structure

```
output/
└── YYYY-MM-DD_HHMMSS/        # Timestamped bundle
    ├── bundle.json           # Metadata (see below)
    ├── image.png             # Generated image
    ├── model.glb             # 3D model (GLB format)
    └── textures/             # Extracted textures (if any)
        ├── texture_0.png
        └── ...
```

**Image-only bundles** (`--image-only` or the GUI's "Image only" checkbox) contain only `bundle.json` and `image.png`. `model.glb` and `textures/` are absent; the pipeline is one `text_to_image` step.

New writes emit `version: 2` (`artifacts` + `pipeline`) and omit v1 `config` / `model_info`. Version 1 files remain readable; they are not rewritten on load. Filenames are unchanged.

## Bundle Metadata (bundle.json)

Version 2 (current). Version 1 files remain readable — see [Version History](#version-history).

This is what a text → image → 3D run writes, key for key and in this order (a test regenerates a bundle in mock mode and checks the example's key set and order against it, object by object). Values here are from a real run; `params` holds whatever knobs the model declares.

```json
{
  "version": 2,
  "name": "a cowboy ninja",
  "created_at": "2024-12-29T15:30:45.864252Z",
  "duration_ms": 48211,
  "tags": [],
  "favorite": false,
  "notes": null,
  "generator": "asset-tap/26.9.3",
  "artifacts": [
    {
      "id": "image",
      "role": "image",
      "path": "image.png",
      "mime": "image/png",
      "sha256": "31df2c13e45aca09289d13e3bac17582314731038d4dcbf57ea40030445c63dc",
      "produced_by": "image",
      "width": 1024,
      "height": 1024,
      "file_size": 419672
    },
    {
      "id": "model",
      "role": "model",
      "path": "model.glb",
      "mime": "model/gltf-binary",
      "sha256": "9c308495260dd830d48ba937201032968318f32b7eff51fb6ac4ef5b036b84ee",
      "produced_by": "model",
      "file_size": 2739808,
      "format": "GLB",
      "vertex_count": 27398,
      "triangle_count": 9132
    }
  ],
  "primary": "model",
  "pipeline": {
    "steps": [
      {
        "kind": "model",
        "id": "image",
        "provider": "fal.ai",
        "model": "fal-ai/nano-banana-2",
        "modality": "text_to_image",
        "prompt": "a cowboy ninja",
        "params": {
          "aspect_ratio": "1:1",
          "num_images": 1,
          "output_format": "png"
        },
        "outputs": ["image"],
        "duration_ms": 9140
      },
      {
        "kind": "model",
        "id": "model",
        "provider": "fal.ai",
        "model": "fal-ai/trellis-2",
        "modality": "image_to_3d",
        "params": {
          "texture_size": 2048,
          "decimation_target": 500000
        },
        "inputs": ["image"],
        "outputs": ["model"],
        "duration_ms": 39071
      }
    ]
  }
}
```

Textures extracted from the GLB are listed too, one `role: "texture"` artifact per file under `textures/` (`id` is `tex_` + the file stem), with the same `path` / `mime` / `sha256` / `width` / `height` / `file_size` keys as the image and `produced_by` set to the step that made the model.

## File Naming Convention

**Standard names (always consistent):**

- `bundle.json` - Metadata file
- `image.png` - Generated image
- `model.glb` - 3D model
- `textures/` - Texture directory

**Rationale:**

- Consistent naming makes loading predictable
- GUI can always find `model.glb` without searching
- Bundle structure is self-documenting

## Metadata Fields

### generator

Identifies which application and version created this bundle (e.g. `"asset-tap/26.3.6"`). Useful for tracking, metrics, and certifying bundle origin. Omitted for bundles created before this field was added.

### duration_ms

Wall-clock time of the whole run, in milliseconds, at the top level. Each pipeline step carries its own `duration_ms` for the stage that wrote it. Both are `null` / absent on bundles that were imported rather than generated, and on bundles written before this field was recorded.

### artifacts / pipeline / primary

- `artifacts[]` — every file that matters: `id`, `role` (`image`, `model`, `texture`, …), relative `path` (omitted when the file was dropped but its provenance is kept), `mime`, `sha256`, `produced_by` (step id), `file_size`, and role-specific stats: `width` / `height` on images and textures, `format` / `vertex_count` / `triangle_count` on the model. Optional keys are omitted rather than written as `null`.
- `primary` — artifact id a viewer should open first (`model` when a GLB is present, otherwise `image`)
- `category` — reserved, omitted. Today's pipeline does not know if a mesh is a prop, character, or environment; a recipe can set this later
- `pipeline.steps[]` — linear. Each step is `kind: model` (provider, model, modality, prompt, params) or `kind: op` (a named deterministic operation). Steps name `inputs` / `outputs` by artifact id. The prompt is recorded once, on the first model call it was sent to: the `text_to_image` step in a two-stage run, the 3D step in a text-to-3D run. `user_prompt` and `template` sit beside it when a template expanded the prompt.

**Unknown kinds are preserved, not rejected.** A step whose `kind` this build does not know (written by a newer version) is kept as-is through load and save, so name, tags, favorite and notes still load and a save here does not strip the newer provenance. Readers skip it.

**References are repaired, not fatal.** On load, duplicate artifact ids, a `primary` that names no artifact, and any `produced_by` / `inputs` / `outputs` naming a missing id are dropped with a warning and the file is re-saved. A hand-edited manifest still opens.

### The `bind` step

Rigging a mesh appends one `kind: op` step with `op: "bind"`, taking `model` and producing `model` (bind rewrites the GLB in place):

```json
{
  "kind": "op",
  "id": "bind",
  "op": "bind",
  "params": {
    "clips": ["Walk_Loop", "Sword_Attack"],
    "skeleton": "atap-humanoid-1"
  },
  "inputs": ["model"],
  "outputs": ["model"],
  "duration_ms": 1830
}
```

Its `params` are:

- `clips` — the model's **full** animation set after the run, not the clips this run added. Bake is declarative: baking `["walk"]` onto a model that held `["walk", "run"]` leaves one animation and one entry here. An empty array means the mesh is rigged and skinned with no animation (`--fit-only`).
- `skeleton` — the skeleton the mesh is bound to (`atap-humanoid-1`). Joint names are [VRM 1.0](https://vrm.dev/) humanoid bone names, so a consumer can retarget without inspecting the armature.

Re-binding a model updates this step rather than appending another; a bundle carries at most one `bind` step because the model has one skeleton. Binding a version 1 bundle upgrades it to version 2 first (artifacts and steps described from its `config` and the files on disk), since a `bind` step has nowhere to live on a v1 file.

A model-only bundle (no image) is the same shape: one `model` artifact and one `text_to_3d` step.

### The `import` step

Wrapping a loose `.glb` or image (or a zip/folder that used non-standard names) writes one `kind: op` step with `op: "import"`. Artifacts still use the standard paths. `params.source` is the original filename when a file was renamed (`hero.glb` → `model.glb`). There is no provider step — we did not generate these files. Dropping a still and a mesh together, or attaching the missing half to an open bundle, still lands on those same paths (`image.png` / `model.glb`).

A run that was given its image (`--image input.png`, or the GUI's image slot) records the same step: an `import` op with `params.source` producing `image`, followed by the `image_to_3d` step with `inputs: ["image"]`. No `text_to_image` step is written, because no image model ran.

Attaching onto a generated bundle merges rather than restamps: the artifacts a model call produced keep their `produced_by`, only the attached file (and the textures now on disk) cite `import`. The step that used to produce the replaced file loses it from `outputs`, and a `bind` step is dropped with the mesh it rigged. Attaching to a bundle that is itself an import re-describes it from disk and keeps the `source` the first import recorded. A version 1 bundle is upgraded to version 2 before either merge, so its generation is not relabeled as an import.

### Privacy

The image reference a run was given is sanitized before it reaches `params.source` on the `import` step: a local file path is reduced to its filename (`/Users/alice/secret-project/input.png` → `input.png`). URLs (`http://`, `https://`) and data URIs pass through unchanged. This keeps shared bundles free of the originating filesystem layout. Version 1 files recorded the same sanitized value as `config.existing_image`.

## Version History

### Version 2 (Current)

- `artifacts[]`, `pipeline.steps[]`, `primary`
- `category` reserved and omitted until a recipe can name the asset
- Writers omit v1 `config` / `model_info`
- v1 files are not rewritten on load; a write that touches provenance (bind, attach) upgrades them in place

### Version 1 (Legacy, still readable)

- Initial bundle structure
- Metadata fields defined
- Standard file naming

```json
{
  "version": 1,
  "name": "a cowboy ninja",
  "created_at": "2024-12-29T15:30:45Z",
  "config": {
    "prompt": "a cowboy ninja",
    "image_model": "fal-ai/nano-banana-2",
    "model_3d": "fal-ai/trellis-2",
    "image_model_params": {
      "guidance_scale": 4.5,
      "num_inference_steps": 32
    },
    "model_3d_params": {
      "topology": "quad",
      "target_polycount": 50000
    }
  },
  "model_info": {
    "file_size": 2739808,
    "format": "GLB",
    "vertex_count": 27398,
    "triangle_count": 9132
  }
}
```

## Usage in Code

### Loading a Bundle

```rust
use std::path::Path;
use serde_json::from_str;

let bundle_path = Path::new("output/2024-12-29_153045");
let metadata_path = bundle_path.join("bundle.json");
let model_path = bundle_path.join("model.glb");

// Load metadata
let metadata_str = std::fs::read_to_string(metadata_path)?;
let metadata: BundleMetadata = from_str(&metadata_str)?;

// Load model
let model_data = std::fs::read(model_path)?;
```

### Creating a Bundle

```rust
// Create timestamped directory
let timestamp = chrono::Local::now().format("%Y-%m-%d_%H%M%S").to_string();
let bundle_path = Path::new("output").join(&timestamp);
std::fs::create_dir_all(&bundle_path)?;

// Save files
std::fs::write(bundle_path.join("image.png"), &image_bytes)?;
std::fs::write(bundle_path.join("model.glb"), &model_bytes)?;

// Save metadata
let metadata = BundleMetadata { /* ... */ };
let metadata_json = serde_json::to_string_pretty(&metadata)?;
std::fs::write(bundle_path.join("bundle.json"), metadata_json)?;
```
