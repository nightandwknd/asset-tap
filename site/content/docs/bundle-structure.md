+++
title = "Bundle Structure"
description = "Understanding the output format, metadata, and file naming conventions."
date = 2026-02-09
weight = 6
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

## Bundle Metadata

The `bundle.json` file contains complete information about the generation:

```json
{
  "version": 1,
  "name": "a cowboy ninja with a leather duster, bandana mask, and dual katanas on the back",
  "created_at": "2024-12-29T15:30:45Z",

  "config": {
    "prompt": "a cowboy ninja with a leather duster, bandana mask, and dual katanas on the back",
    "image_model": "nano-banana-2",
    "model_3d": "trellis-2",
    "provider": "fal.ai",
    "export_fbx": true
  },

  "model_info": {
    "file_size": 2739808,
    "format": "GLB",
    "vertex_count": 27398,
    "triangle_count": 9132
  },

  "files": {
    "image": "image.png",
    "model": "model.glb",
    "fbx": "model.fbx",
    "textures": ["textures/texture_0.png"]
  },

  "generation_metadata": {
    "image_generation": {
      "duration_ms": 2341,
      "model": "nano-banana-2"
    },
    "model_3d_generation": {
      "duration_ms": 45823,
      "model": "trellis-2"
    },
    "fbx_conversion": {
      "duration_ms": 3421,
      "success": true
    }
  }
}
```

### Fields

**config** -- The generation settings used:

- `prompt` -- Text prompt
- `image_model` -- Image generation model
- `model_3d` -- 3D generation model
- `provider` -- Provider ID
- `export_fbx` -- Whether FBX export was requested

**model_info** -- 3D model statistics:

- `file_size` -- Size in bytes
- `format` -- Model format (GLB, FBX)
- `vertex_count` -- Number of vertices
- `triangle_count` -- Number of triangles

**files** -- Relative paths to each file in the bundle

**generation_metadata** -- Timing and status for each pipeline stage

## Output Location

**GUI**: Configured in Settings. Defaults to `~/Documents/Asset Tap/` on macOS.

**CLI**: Defaults to `./output` in the current directory. Specify a custom path with `-o`.

**Dev mode** (debug builds): Output goes to `.dev/output/` in the project root.

## Library

The GUI includes a Library view where you can browse all generated bundles, preview 3D models, and view metadata. Bundles are loaded from your configured output directory.
