+++
title = "CLI Usage"
description = "Command-line interface reference for automation and scripting."
date = 2026-02-09
weight = 4
in_search_index = true

[extra]
images = []

[taxonomies]
tags = ["reference"]
+++

Asset Tap includes a full-featured command-line interface for automation, scripting, and headless generation.

## Installation

The CLI is included with all installers, or available as a standalone download. See the [Installation page](@/docs/installation.md) for platform-specific instructions.

**macOS** -- The CLI is bundled inside the app. Create a symlink to use it from the terminal:

```bash
sudo ln -sf "/Applications/Asset Tap.app/Contents/MacOS/asset-tap" /usr/local/bin/asset-tap
```

**Linux (.deb)** -- The CLI is installed to `/usr/bin/asset-tap` automatically.

**Windows** -- The CLI is available after install if the installer adds the install directory to PATH.

## API Key Configuration

The CLI needs an API key from your provider (e.g., [fal.ai](https://fal.ai/dashboard/keys)). There are two ways to configure it:

**Option 1: Environment variable** (recommended for CLI)

```bash
export FAL_KEY=your_key_here
```

Add this to your shell profile (`~/.zshrc`, `~/.bashrc`) to persist across sessions.

**Option 2: GUI settings** (shared automatically)

If you've configured your API key in the Asset Tap GUI (Settings > API Keys), the CLI picks it up automatically -- both share the same settings file.

## Basic Usage

```bash
# Generate a 3D model from a text prompt
asset-tap --yes "a wooden treasure chest"

# Short form
asset-tap -y "a dragon"
```

The `--yes` / `-y` flag skips confirmation prompts and runs with defaults.

## Specifying Provider and Models

```bash
# Use a specific provider
asset-tap -p fal.ai -y "a spaceship"

# Choose specific models
asset-tap -p fal.ai --image-model nano-banana-2 --3d-model trellis-2 -y "a robot"

# Use premium image model
asset-tap -p fal.ai --image-model nano-banana-pro -y "a detailed castle"

# Use original Nano Banana
asset-tap -p fal.ai --image-model nano-banana -y "a simple cube"
```

## Using an Existing Image

Skip the text-to-image step by providing your own image:

```bash
# Convert an existing image to 3D
asset-tap --yes --image "photo.png"

# With a specific 3D model
asset-tap --yes --image "photo.png" --3d-model trellis-2
```

## Templates

Use prompt templates to structure your input with predefined formats:

```bash
# List available models and templates
asset-tap --list

# Use a template (your prompt becomes the template's description variable)
asset-tap -t humanoid -y "a brave knight with a glowing sword"

# Inspect a template's syntax and preview
asset-tap --inspect-template humanoid
```

## Listing Providers and Models

```bash
# List all available providers and their models
asset-tap --list-providers

# List models and templates
asset-tap --list
```

## Output

Generated assets are saved to timestamped directories. See [Bundle Structure](@/docs/bundle-structure.md) for the full output format.

```bash
# Use a custom output directory
asset-tap -o ~/my-assets -y "a treasure chest"
```

```
output/
└── 2024-12-29_153045/
    ├── bundle.json      # Metadata
    ├── image.png        # Generated image
    ├── model.glb        # 3D model
    ├── model.fbx        # FBX (if Blender installed)
    └── textures/        # Extracted textures
```

### Exporting Bundles

```bash
# Export a bundle directory as a zip archive
asset-tap --export-bundle output/2024-12-29_153045
```

## FBX Conversion

By default, Asset Tap converts GLB models to FBX if Blender is installed.

```bash
# Skip FBX conversion (GLB output only)
asset-tap --no-fbx -y "a robot"

# Convert a specific bundle or GLB file to FBX after generation
asset-tap --convert-fbx output/2024-12-29_153045
asset-tap --convert-fbx output/2024-12-29_153045/model.glb

# Batch convert all existing GLB files to FBX (no API calls)
asset-tap --convert-only
```

## Image Approval

In interactive mode, you can require approval of the generated image before proceeding to 3D generation:

```bash
# Require image approval before 3D conversion
asset-tap --approve "a detailed spaceship"
```

## Texture Conversion

Some 3D models contain WebP textures that aren't supported by all tools. Convert them to PNG:

```bash
# Convert WebP textures in existing GLB files to PNG
asset-tap --convert-webp
```

## Mock Mode (Development Only)

Mock mode is a development feature for testing the full pipeline without consuming API credits. It is **not available in release builds** — it requires building from source with the `mock` Cargo feature enabled.

When building from source, use the Makefile targets:

```bash
# Instant mock responses
make mock ARGS='-y "test prompt"'

# With realistic delays
make mock ARGS='--mock-delay -y "test prompt"'

# GUI in mock mode
make mock-gui
```

Or build with the feature explicitly:

```bash
cargo run --features mock --bin asset-tap -- --mock -y "test prompt"
```

Mock mode redirects all API requests to a local server that returns synthetic data (test images and 3D models). It validates the pipeline and configuration plumbing, not provider-specific response parsing. To verify a custom provider's response format, test against the real API.

## Complete Flag Reference

| Flag                 | Short | Description                                                   |
| -------------------- | ----- | ------------------------------------------------------------- |
| `--yes`              | `-y`  | Auto-confirm all prompts (non-interactive mode)               |
| `--provider`         | `-p`  | Provider to use (e.g., `fal.ai`)                              |
| `--image-model`      |       | Image generation model                                        |
| `--3d-model`         |       | 3D generation model                                           |
| `--image`            |       | Skip image generation, use existing image (local path or URL) |
| `--template`         | `-t`  | Use a prompt template                                         |
| `--output`           | `-o`  | Output directory for generated assets                         |
| `--list`             |       | List available models and templates                           |
| `--list-providers`   |       | List available providers and their models                     |
| `--inspect-template` |       | Inspect a template's syntax and preview                       |
| `--no-fbx`           |       | Skip FBX conversion (GLB only)                                |
| `--convert-fbx`      |       | Convert a specific GLB file or bundle directory to FBX        |
| `--convert-only`     |       | Batch convert all existing GLB files to FBX (no API calls)    |
| `--convert-webp`     |       | Convert WebP textures in GLB files to PNG                     |
| `--approve`          |       | Require image approval before 3D generation                   |
| `--export-bundle`    |       | Export a bundle directory as a zip archive                    |
