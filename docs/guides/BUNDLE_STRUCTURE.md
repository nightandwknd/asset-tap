# Bundle Structure

Generated assets are organized into timestamped bundle directories with metadata.

## Directory Structure

```
output/
└── YYYY-MM-DD_HHMMSS/        # Timestamped bundle
    ├── bundle.json           # Metadata (see below)
    ├── image.png             # Generated image
    ├── model.glb             # 3D model (GLB format)
    ├── model.fbx             # FBX export (if Blender available)
    └── textures/             # Extracted textures (if any)
        ├── texture_0.png
        └── ...
```

## Bundle Metadata (bundle.json)

```json
{
  "version": 1,
  "name": "a cowboy ninja with a leather duster, bandana mask, and dual katanas on the back",
  "created_at": "2024-12-29T15:30:45Z",

  "generator": "asset-tap/26.3.6",

  "config": {
    "prompt": "a cowboy ninja with a leather duster, bandana mask, and dual katanas on the back",
    "user_prompt": "a cowboy ninja with a leather duster, bandana mask, and dual katanas on the back",
    "image_model": "fal-ai/nano-banana-2",
    "model_3d": "fal-ai/trellis-2",
    "export_fbx": true,
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

## File Naming Convention

**Standard names (always consistent):**

- `bundle.json` - Metadata file
- `image.png` - Generated image
- `model.glb` - 3D model
- `model.fbx` - FBX export (if created)
- `textures/` - Texture directory

**Rationale:**

- Consistent naming makes loading predictable
- GUI can always find `model.glb` without searching
- Bundle structure is self-documenting

## Metadata Fields

### generator

Identifies which application and version created this bundle (e.g. `"asset-tap/26.3.6"`). Useful for tracking, metrics, and certifying bundle origin. Omitted for bundles created before this field was added.

### config

Generation configuration:

- `prompt` - Text prompt sent to the API (after template expansion, if any)
- `user_prompt` - Original user input before template expansion (omitted when no template was used)
- `template` - Template name used for prompt expansion (omitted when no template was used)
- `image_model` - Image generation model used
- `model_3d` - 3D generation model used
- `export_fbx` - Whether FBX export was requested
- `image_model_params` - User-tuned parameter overrides applied to the image model (omitted when empty)
- `model_3d_params` - User-tuned parameter overrides applied to the 3D model (omitted when empty)

### model_info

3D model statistics:

- `file_size` - File size in bytes
- `format` - Model format (GLB, FBX)
- `vertex_count` - Number of vertices
- `triangle_count` - Number of triangles

### Privacy

`existing_image` is sanitized before serialization: if the user provided a local file path, only the filename is recorded (e.g. `/Users/alice/secret-project/input.png` → `input.png`). URLs (`http://`, `https://`) and data URIs pass through unchanged. This keeps shared bundles free of the originating filesystem layout.

## Version History

### Version 1 (Current)

- Initial bundle structure
- Metadata fields defined
- Standard file naming

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
