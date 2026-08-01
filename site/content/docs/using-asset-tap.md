+++
title = "Using Asset Tap"
description = "Complete guide to the Asset Tap GUI -- generating 3D models, using the viewer, managing your library, and configuring settings."
date = 2026-02-10
weight = 3
in_search_index = true

[extra]
images = []

[taxonomies]
tags = ["guide"]
+++

This guide walks through the Asset Tap GUI from first launch to exporting your 3D models.

## First Launch

When you open Asset Tap for the first time, the welcome screen asks for your API key. Enter a key from either supported provider -- a [fal.ai API key](https://fal.ai/dashboard/keys) or a [Meshy API key](https://www.meshy.ai/settings/api) -- and click **Save**. You only need one to run the full pipeline. You can add the other later (or swap between them) in Settings.

## Main Window

The main window has two areas: the **sidebar** on the left for inputs and controls, and the **viewer** on the right for previewing results.

### Sidebar

The sidebar is where you configure and launch generations:

- **Prompt** -- Type a text description of the 3D model you want (e.g., "a cowboy ninja with a leather duster, bandana mask, and dual katanas on the back")
- **Template** -- Optionally select a prompt template to structure your input
- **Provider** -- Select the AI provider to use
- **Image Model** -- Choose which model generates the image from your text
- **Image Model Settings** -- Per-model parameters declared by the provider (aspect ratio, resolution, seed, and so on), shown as sliders, checkboxes, and dropdowns. Values persist per provider and model.
- **3D Model** -- Choose which model converts the image to 3D
- **3D Model Settings** -- The same, for the image-to-3D model (topology, polycount, PBR, texture resolution, and so on)
- **Image only (skip 3D)** -- Stop after the image; no GLB or FBX is written
- **Export FBX** -- Convert the GLB to FBX (requires Blender)
- **Generate** -- Start the generation pipeline

### Using an Existing Image

If you already have an image, you can skip the text-to-image step. Use the image input in the sidebar to load a file directly -- Asset Tap will send it straight to the image-to-3D model.

## Generating a Model

Click **Generate** to start the pipeline. Asset Tap runs in stages:

1. **Image Generation** -- Your text prompt is sent to the AI provider, which returns an image
2. **Image Approval** -- Review the generated image before proceeding to 3D conversion
3. **3D Generation** -- The image is converted to a 3D model (GLB format)

### Image Approval

After the image is generated, Asset Tap shows you a preview so you can decide whether to proceed. If the image doesn't match what you had in mind, you can go back and adjust your prompt.

## 3D Viewer

Once generation completes, the 3D model loads in the built-in viewer. You can interact with the model directly:

Controls follow Blender's conventions.

**Mouse:**

- **Rotate** -- Left-drag, or middle-click drag, to orbit around the model
- **Pan** -- Shift + left-drag, or Shift + middle-click drag
- **Zoom** -- Scroll wheel (no modifier)

**Trackpad:**

- **Rotate** -- Two-finger scroll to orbit
- **Pan** -- Shift + two-finger scroll
- **Zoom** -- Pinch, or Ctrl/Cmd + two-finger scroll

A **Reset View** button in the viewer toolbar restores the default camera position.

The viewer supports models from all providers and handles vertex colors, textures, and node transforms automatically.

## Library

The Library view lets you browse all previously generated models. Each entry shows the prompt, timestamp, and a quick preview. Click any entry to load it in the 3D viewer.

Bundles are loaded from your configured output directory. See [Bundle Structure](@/docs/bundle-structure.md) for details on the output format.

## Settings

Open Settings from the gear icon to configure:

- **API Keys** -- Add or update provider API keys
- **Output Directory** -- Choose where generated models are saved
- **FBX Export** -- Enable automatic GLB-to-FBX conversion (requires [Blender](https://www.blender.org/download/))

## Templates

Asset Tap includes prompt templates that help structure your text input for better results. Select a template from the sidebar dropdown, fill in the variables, and the template generates an optimized prompt.

You can browse available templates with the template selector in the sidebar.

## FBX Export

If you have Blender installed, Asset Tap can automatically convert GLB models to FBX format for use in game engines like Unity and Unreal Engine. Enable FBX export in Settings.

The exported FBX file is saved alongside the GLB in the same bundle directory.

You can also convert existing bundles to FBX after generation. In the GUI, open a bundle that has a GLB and click **Convert to FBX**. From the CLI, use `asset-tap --convert-fbx <path>` with a bundle directory or GLB file.

## What's Next

- [CLI Usage](@/docs/cli-usage.md) -- Automate generation from the command line
- [Providers](@/docs/providers.md) -- Available models and custom provider configuration
- [Bundle Structure](@/docs/bundle-structure.md) -- Understanding the output format
