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

### Fields

**config** -- The generation settings used:

- `prompt` -- Text prompt
- `image_model` -- Image generation model
- `model_3d` -- 3D generation model
- `export_fbx` -- Whether FBX export was requested
- `image_model_params` -- User-tuned parameter overrides applied to the image model (omitted when empty)
- `model_3d_params` -- User-tuned parameter overrides applied to the 3D model (omitted when empty)

**model_info** -- 3D model statistics:

- `file_size` -- Size in bytes
- `format` -- Model format (GLB, FBX)
- `vertex_count` -- Number of vertices
- `triangle_count` -- Number of triangles

## Privacy

`existing_image` is sanitized before serialization: if the user provided a local file path, only the filename is recorded (e.g. `/Users/alice/secret-project/input.png` -> `input.png`). URLs (`http://`, `https://`) and data URIs pass through unchanged. This keeps shared bundles free of the originating filesystem layout.

## Output Location

**GUI**: Configured in Settings. Defaults to `~/Documents/Asset Tap/` on macOS.

**CLI**: Defaults to `./output` in the current directory. Specify a custom path with `-o`.

**Dev mode** (debug builds): Output goes to `.dev/output/` in the project root.

## Library

The GUI includes a Library view where you can browse all generated bundles, preview 3D models, and view metadata. Bundles are loaded from your configured output directory.
