+++
title = "Bundle Structure"
description = "Understanding the output format, metadata, and file naming conventions."
date = 2026-02-09
weight = 5
in_search_index = true

[extra]
images = []

[taxonomies]
tags = ["reference"]
+++

Every generation creates a self-contained bundle directory with all output files and metadata.

## Directory Layout

```
output/
└── YYYY-MM-DD_HHMMSS/        # Timestamped bundle
    ├── bundle.json            # Metadata
    ├── image.png              # AI-generated image
    ├── model.glb              # 3D model (GLB format)
    ├── model.fbx              # FBX export (if Blender installed)
    └── textures/              # Extracted textures (if any)
        ├── texture_0.png
        └── ...
```

Bundles are named with a timestamp (`YYYY-MM-DD_HHMMSS`) so they sort chronologically and never collide.

## File Names

File names are always consistent across all bundles:

| File          | Description              |
| ------------- | ------------------------ |
| `bundle.json` | Generation metadata      |
| `image.png`   | Generated or input image |
| `model.glb`   | 3D model in GLB format   |
| `model.fbx`   | FBX export (optional)    |
| `textures/`   | Extracted texture files  |

This predictable naming means you always know exactly where to find each file.

New writes emit `bundle.json` version 2: an `artifacts` inventory and a `pipeline` of steps (the prompt and models used, in order). Version 1 files still load. The filenames above are unchanged.

## Bundle Metadata

The `bundle.json` file contains complete information about the generation:

```json
{
  "version": 2,
  "name": "My Robot",
  "created_at": "2024-12-29T15:30:45Z",
  "primary": "model",
  "artifacts": [
    {
      "id": "image",
      "role": "image",
      "path": "image.png",
      "mime": "image/png",
      "produced_by": "image"
    },
    {
      "id": "model",
      "role": "model",
      "path": "model.glb",
      "mime": "model/gltf-binary",
      "produced_by": "model",
      "vertex_count": 27398,
      "triangle_count": 9132
    }
  ],
  "pipeline": {
    "steps": [
      {
        "id": "image",
        "kind": "model",
        "provider": "fal.ai",
        "model": "fal-ai/nano-banana-2",
        "modality": "text_to_image",
        "prompt": "a cowboy ninja with a leather duster and dual katanas",
        "outputs": ["image"]
      },
      {
        "id": "model",
        "kind": "model",
        "provider": "fal.ai",
        "model": "fal-ai/trellis-2",
        "modality": "image_to_3d",
        "inputs": ["image"],
        "outputs": ["model"]
      }
    ]
  },
  "generator": "asset-tap/26.8.17"
}
```

### Fields

**Top level:**

- `version` -- Bundle format version (`2` for new writes; `1` still loads)
- `name` -- Bundle name; `null` until set with `-n/--name` or from the GUI
- `created_at` -- UTC timestamp
- `duration_ms` -- Generation time in milliseconds, when recorded
- `tags`, `favorite`, `notes` -- User-editable metadata from the GUI
- `generator` -- The Asset Tap version that produced the bundle
- `artifacts[]` -- Inventory of files (`id`, `role`, `path`, `mime`, `produced_by`)
- `pipeline.steps[]` -- Ordered provenance: `kind: model` or `kind: op`
- `primary` -- Artifact id a viewer should open first
- `category` -- reserved; omitted until a recipe can name the asset

Prompt, models, and params live on `pipeline.steps[]`. Mesh stats live on the model artifact. Version 1 files still load (`config` / `model_info`); they are not rewritten.

## Privacy

`existing_image` is sanitized before serialization: if the user provided a local file path, only the filename is recorded (e.g. `/Users/alice/secret-project/input.png` -> `input.png`). URLs (`http://`, `https://`) and data URIs pass through unchanged. This keeps shared bundles free of the originating filesystem layout.

## Output Location

**GUI**: Configured in Settings. Defaults to `~/Documents/Asset Tap/` on macOS.

**CLI**: Defaults to `./output` in the current directory. Specify a custom path with `-o`.

**Dev mode** (debug builds): Output goes to `.dev/output/` in the project root.

## Library

The GUI includes a Library view where you can browse all generated bundles, preview 3D models, and view metadata. Bundles are loaded from your configured output directory.
