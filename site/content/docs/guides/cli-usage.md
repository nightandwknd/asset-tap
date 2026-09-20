+++
title = "CLI Usage"
description = "Command-line interface reference for automation and scripting."
date = 2026-02-09
weight = 2
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

# Budget tier on Meshy (meshy-6-lite, 2-4x cheaper)
asset-tap -p meshy --3d-model meshy/v6-lite/image-to-3d "a simple cube"
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

## Image-Only Runs and Image Shape

`--image-only` stops the pipeline after text-to-image: you get a bundle with an
`image.png` and no 3D model. Nothing sets the image's shape for you, so the
model picks its own -- the default image model (`fal-ai/nano-banana-2`) often
returns a wide 1408x768, which is the wrong shape for a texture and wasteful for
a sprite. Pass the shape yourself.

The nano-banana family takes `aspect_ratio` on both providers:
`fal-ai/nano-banana-2` (the default), `fal-ai/nano-banana`,
`fal-ai/nano-banana-pro`, and Meshy's `meshy/nano-banana`,
`meshy/nano-banana-2`, `meshy/nano-banana-pro`, `meshy/gpt-image-2`. The flux
models (`fal-ai/flux-2`, `fal-ai/flux-2-pro`) have no `aspect_ratio` -- they
take `image_size` from a preset list instead.

```bash
asset-tap --image-only -y --param aspect_ratio=1:1 "mossy cobblestone"
asset-tap --image-only -y --param aspect_ratio=3:4 "a goblin archer idle pose"

# flux takes a preset instead of a ratio
asset-tap --image-only -y --image-model fal-ai/flux-2 \
  --param image_size=square_hd "mossy cobblestone"
```

On Meshy's text-to-image models, `aspect_ratio` is mutually exclusive with
Multi-View -- clear it with `--param aspect_ratio=` if you turn
`generate_multi_view` on, or the request is rejected.

## Installing the Artifact Into Your Project (`--install`)

A run always writes a full bundle. `--install PATH` additionally copies the
run's **primary artifact** where your project wants it -- `model.glb`, or
`image.png` under `--image-only`:

```bash
# Exact destination file (parent directories are created)
asset-tap -y --install Assets/Models/crate.glb "a wooden crate"

# Anything that isn't a .glb/.png file path is a directory -- existing, a
# trailing slash, or simply no extension -- and receives
# <--name, else the bundle folder name>.<ext>
asset-tap -y --name crate --install Assets/Models/ "a wooden crate"

# Image-only runs install the PNG
asset-tap --image-only -y --install Sprites/goblin.png "a goblin archer"
```

An extension that doesn't match what the run produces (`.glb` under
`--image-only`, `.png` without it) is a usage error: exit code 2, raised before
any generation starts, so a mistyped path never costs an API call. An existing
file at the destination is overwritten.

`--install` works with `--json` and changes nothing on the wire: the `result`
event is unchanged (you passed the path, so you already know it), and the
copy is logged on stderr like every other human-facing message.

## Rate Limits and Concurrency

Providers rate-limit per API key, not per machine, so several `asset-tap`
processes sharing one key share one budget. While a job is running, Asset Tap
polls the provider for status; a 429 or a 5xx on one of those polls is treated
as transient and retried with exponential backoff -- 2 seconds, doubling each
time to a 30-second cap, giving up after 5 consecutive failures (a
non-429 4xx, such as a bad key, fails immediately instead of burning the
retry budget). Retries are surfaced as progress updates, so a `--json`
consumer sees them rather than a silent stall.

That covers a blip, not a sustained overload: if you are batching, the fix is
to submit fewer jobs at once. Meshy publishes no documented safe parallelism
for its generation endpoints, so run Meshy jobs sequentially -- one prompt at a
time -- rather than fanning out and relying on the retry loop to absorb the
rejections.

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

## Rigging and Animation (experimental)

> **Experimental.** Humanoid rig and clip bake -- the panel, flags, and clip
> names may change. Fingers are not weighted. Full guide:
> [Animation (experimental)](@/docs/guides/animation.md).

Asset Tap rigs humanoid characters and bakes animation clips into the GLB. The
skeleton ships inside the binary, so rigging itself needs nothing downloaded:

```bash
# Generate a character and rig it in one run
asset-tap --rig -y -t humanoid "a knight in plate armor"

# Rig an existing mesh and bake a walk cycle
asset-tap bind --mesh model.glb --clip walk

# Rig only: a skinned T-pose with no animation
asset-tap bind --mesh model.glb --fit-only
```

`--clip` repeats, on both the generate path and `bind`. One model carries one
animation per clip, the way Mixamo and Meshy work, rather than one export per
clip:

```bash
asset-tap bind --mesh model.glb --clip walk --clip Sword_Attack --clip Idle_Loop
```

Bake is declarative: the model ends up with exactly the clips you named, so
re-running with a shorter list removes the ones you dropped. Re-running `bind`
on a mesh that is already rigged keeps its skeleton and weights, so adding a
clip cannot undo joints you arranged by hand in the GUI. Pass `--refit` when
you do want the pose discarded and recomputed.

### Animation packs

Clips come from [Quaternius](https://quaternius.com)' CC0
[Universal Animation Library](https://quaternius.com/packs/universalanimationlibrary.html)
and
[Universal Animation Library 2](https://quaternius.com/packs/universalanimationlibrary2.html).
`asset-tap clip download` pulls the Standard libraries from the latest
Asset Tap release (hash-verified, not in the binary). The remaining paid
Source clips on those pages install the same way as any other zip. Full
note: [Animation (experimental)](@/docs/guides/animation.md#clip-packs).

Install from a download's `.zip`, an extracted folder, or a single `.glb`:

```bash
# Standard packs from the latest Asset Tap release
asset-tap clip download
asset-tap --json clip download
asset-tap clip download --force

# A zip you already have (id is derived from the file name unless you pass --id)
asset-tap clip install --from Universal-Animation-Library.zip

# List every clip from every installed pack
asset-tap clip list

# ...and mark which of them are already baked into a model
asset-tap clip list --model output/2026-09-08_120000/model.glb
```

Several packs can sit side by side. Clip names are matched across all of them,
so `walk` resolves to whichever pack provides it, and a name that no installed
pack provides is a clear local error rather than a silently empty animation.

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
| `--install`          |       | Copy the finished artifact to a path (file or directory)              |
| `--list`             |       | List available models and templates                                   |
| `--list-providers`   |       | List available providers and their models                             |
| `--inspect-template` |       | Inspect a template's syntax and preview                               |
| `--convert-webp`     |       | Convert WebP textures in GLB files to PNG                             |
| `--approve`          |       | Require image approval before 3D generation                           |
| `--export-bundle`    |       | Export a bundle directory as a zip archive                            |
| `--rig`              |       | Experimental: rig the mesh to the built-in humanoid skeleton          |
| `--clip`             |       | Experimental: clip to bake after rigging (repeatable)                 |
| `--clip-pack`        |       | Experimental: animation pack directory, overriding installed packs    |
