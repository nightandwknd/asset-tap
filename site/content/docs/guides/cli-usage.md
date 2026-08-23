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

Asset Tap includes a full-featured command-line interface for automation, scripting, and headless generation. (The same capabilities are also exposed as an [MCP server](@/docs/guides/mcp.md) for MCP hosts like Claude Desktop and Cursor.)

## Installation

> **Alias:** release installs also provide `atap`, a short alias for `asset-tap` (symlink on macOS/Linux, `atap.cmd` on Windows). Every command on this page works with either name.

**macOS / Linux** -- one line (checksum-verified, installs `asset-tap` + `atap` to `~/.local/bin`):

```bash
curl -fsSL https://assettap.dev/install | bash
```

**Windows** -- one line in PowerShell (checksum-verified, installs to `%LOCALAPPDATA%\AssetTap\bin` and registers it on your user `PATH`):

```powershell
irm https://assettap.dev/install.ps1 | iex
```

(The Linux `.deb` desktop package also installs the CLI system-wide -- see the [Installation page](@/docs/getting-started/installation.md) for every option.)

## API Key Configuration

The CLI needs an API key from at least one provider -- [fal.ai](https://fal.ai/dashboard/keys) or [Meshy](https://www.meshy.ai/settings/api). A single key unlocks the full pipeline. There are two ways to configure keys:

**Option 1: Environment variable** (recommended for CLI)

```bash
# Pick one (or both); Asset Tap uses whichever provider owns the model you select.
export FAL_KEY=your_fal_key
export MESHY_API_KEY=your_meshy_key
```

Add these to your shell profile (`~/.zshrc`, `~/.bashrc`) to persist across sessions.

**Option 2: GUI settings** (shared automatically)

If you've configured your API key in the Asset Tap GUI (Settings > API Keys), the CLI picks it up automatically -- both share the same settings file.

## Basic Usage

```bash
# Generate a 3D model from a text prompt
asset-tap "a wooden treasure chest"

# Run interactively; you'll be asked to describe what you want to create
asset-tap
```

## Specifying Provider and Models

```bash
# Use a specific provider
asset-tap -p fal.ai "a spaceship"

# Choose specific models
asset-tap -p fal.ai --image-model fal-ai/nano-banana-2 --3d-model fal-ai/trellis-2 "a robot"

# Use premium image model
asset-tap -p fal.ai --image-model fal-ai/nano-banana-pro "a detailed castle"

# Native Meshy end-to-end (requires MESHY_API_KEY)
asset-tap -p meshy --image-model meshy/nano-banana-pro --3d-model meshy/v6/image-to-3d "a detailed castle"

# Budget tier on Meshy (meshy-5, 2-4x cheaper)
asset-tap -p meshy --3d-model meshy/v5/image-to-3d "a simple cube"
```

> **Tip:** If you omit `-p/--provider`, Asset Tap routes the request to whichever provider owns the model you pick. `--3d-model fal-ai/trellis-2` goes to fal.ai; `--3d-model meshy/v6/image-to-3d` goes to Meshy. Set the provider explicitly only when you're not specifying a model.

## Using an Existing Image

Skip the text-to-image step by providing your own image:

```bash
# Convert an existing image to 3D
asset-tap --image "photo.png"

# With a specific 3D model
asset-tap --image "photo.png" --3d-model fal-ai/trellis-2
```

## Tuning Model Parameters

Models declare user-tunable parameters in their provider YAML (e.g. `guidance_scale`, `target_polycount`, `enable_pbr`). Override them from the command line with `--param KEY=VALUE`:

```bash
# Override a single parameter
asset-tap -y "a robot" --image-model fal-ai/flux-2 --param guidance_scale=7.0

# Multiple parameters
asset-tap -y "a robot" --param guidance_scale=7.0 --param num_inference_steps=10

# 3D model parameters (auto-routed to whichever model declares them)
asset-tap -y "a robot" --3d-model fal-ai/meshy/v6/image-to-3d --param topology=quad --param enable_pbr=false
```

Value types are auto-detected (`true`/`false` -> bool, integers, floats, strings). An empty value (`--param seed=`) clears the parameter so the provider applies its own default.

Parameters are validated against the models the run actually uses: under `--image-only` no image-to-3D parameter is accepted, and with `--image` no text-to-image parameter is. An invalid name or value is a usage error: exit code 2, with the valid parameters for each active model listed.

The applied overrides are recorded into `bundle.json` under `config.image_model_params` and `config.model_3d_params`, and shown in the GUI's bundle info panel.

## Templates

Use prompt templates to structure your input with predefined formats:

```bash
# List available models and templates
asset-tap --list

# Use a template (your prompt becomes the template's description variable)
asset-tap -t humanoid "a brave knight with a glowing sword"

# Inspect a template's syntax and preview
asset-tap --inspect-template humanoid
```

## Scripts and Non-Interactive Use

The CLI is already script-friendly out of the box; no special flag needed. If stdin isn't a terminal (piped, redirected, or running in CI), the CLI will not try to read a prompt interactively. Just pass your prompt as an argument:

```bash
# Works directly in scripts, CI, cron, etc.
asset-tap "a wooden treasure chest"

# Omitting the prompt from a non-interactive shell fails fast with a clear error
echo "" | asset-tap            # Error: No prompt provided. Pass a prompt as an argument:
```

### Image Approval Auto-Confirm (`-y` / `--yes`)

If you've enabled the image approval step (via `--approve` or the GUI setting `require_image_approval`), the CLI will pause after image generation and ask you to confirm before running the 3D conversion. Pass `-y` / `--yes` to skip that confirmation and proceed automatically; useful when you want the approval behavior in interactive runs but not in batch scripts.

```bash
# Normally prompts after the image is generated
asset-tap --approve "a wooden treasure chest"

# Skips the prompt, proceeds straight to 3D
asset-tap --approve -y "a wooden treasure chest"
```

If you don't use `--approve` and don't have approval enabled in settings, `-y` is a no-op.

## Listing Providers and Models

```bash
# List all available providers and their models
asset-tap --list-providers

# List models and templates
asset-tap --list
```

## Output

Generated assets are saved to timestamped directories. See [Bundle Structure](@/docs/guides/bundle-structure.md) for the full output format.

```bash
# Use a custom output directory
asset-tap -o ~/my-assets "a treasure chest"
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

A bundle needs a name before it can be exported. Set one at generation time with `-n/--name`, or pass it alongside `--export-bundle`:

```bash
# Name at generation time
asset-tap -y --name "My Robot" "a robot"

# Or name an existing bundle while exporting it
asset-tap --export-bundle output/2024-12-29_153045 --name "My Robot"
```

Exporting a bundle that has no name exits with an error telling you which command to run.

## FBX Conversion

By default, Asset Tap converts GLB models to FBX if Blender is installed.

```bash
# Skip FBX conversion (GLB output only)
asset-tap --fbx "a robot"

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

## Complete Flag Reference

| Flag                 | Short | Description                                                           |
| -------------------- | ----- | --------------------------------------------------------------------- |
| `--yes`              | `-y`  | Auto-confirm the image approval step                                  |
| `--provider`         | `-p`  | Provider to use (e.g., `fal.ai`)                                      |
| `--image-model`      |       | Image generation model                                                |
| `--3d-model`         |       | 3D generation model                                                   |
| `--image`            |       | Skip image generation, use existing image (local path or URL)         |
| `--image-only`       |       | Stop after image generation; no 3D model                              |
| `--template`         | `-t`  | Use a prompt template                                                 |
| `--output`           | `-o`  | Output directory for generated assets                                 |
| `--name`             | `-n`  | Name the generated bundle (or an existing one with `--export-bundle`) |
| `--list`             |       | List available models and templates                                   |
| `--list-providers`   |       | List available providers and their models                             |
| `--inspect-template` |       | Inspect a template's syntax and preview                               |
| `--fbx`              |       | Also convert the model to FBX (requires Blender; GLB-only is default) |
| `--convert-fbx`      |       | Convert a specific GLB file or bundle directory to FBX                |
| `--convert-only`     |       | Batch convert all existing GLB files to FBX (no API calls)            |
| `--convert-webp`     |       | Convert WebP textures in GLB files to PNG                             |
| `--approve`          |       | Require image approval before 3D generation                           |
| `--export-bundle`    |       | Export a bundle directory as a zip archive                            |
