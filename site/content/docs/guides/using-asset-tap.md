+++
title = "Using Asset Tap"
description = "Complete guide to the Asset Tap GUI -- generating 3D models, using the viewer, managing your library, and configuring settings."
date = 2026-02-10
weight = 1
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
- **Image only (skip 3D)** -- Stop after the image; no GLB is written
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

The 3D tab is an inspector (Grid, Axes, Reset View). A **Reset View** button in the viewer toolbar restores the default camera.

### Animation (experimental)

**Animate** opens an optional panel beside the viewer: Rig, then Bind, then
the clip list. This path is [experimental](@/docs/guides/animation.md).
Fingers are not weighted, and the panel may change.

**Download Universal Animation Libraries** (Help, or Animate → **Add pack...**) fetches
the Standard libraries from the latest release (hash-verified).
Already-installed packs are left alone, so a Source upgrade is not overwritten.

On an unbound mesh the first step is **Rig**: pose the shipped skeleton on a
frozen mesh. Dragging a joint never deforms the model. **Auto-fit** is a
button, not something that runs on open. **Bind** skins the mesh; a joint
off the body is refused by name. After a bind, click a clip to play it
(preview never writes) and **Bake** the ticked set into `model.glb`.

The full loop, CLI and MCP included: [Animation (experimental)](@/docs/guides/animation.md).

The viewer supports models from all providers and handles vertex colors, textures, and node transforms automatically.

## Library

The Library view lets you browse all previously generated models. Each entry shows the prompt, timestamp, and a quick preview. Click any entry to load it in the 3D viewer.

Bundles are loaded from your configured output directory. See [Bundle Structure](@/docs/guides/bundle-structure.md) for details on the output format.

## Importing Bundles

Bring assets into your library from anywhere -- a CLI run that used a custom
output directory, a bundle someone shared, an exported archive, or a lone
`.glb` / image:

- **Drag & drop (zones, not the whole window):**
  - **Sidebar** “Drop image here” — pipeline input (skip text-to-image).
  - **Bundle Info** — always a **new** library bundle
    (`.glb`, image, zip, folder, `bundle.json`). Drop a still and a mesh
    together to pair them.
  - **Empty Image or 3D tab** on an open bundle — attach the missing
    `image.png` / `model.glb`. The **Add … to this bundle** picker does
    the same. File → Import always creates a new bundle.
  - **Animation panel** — clip packs only. A UAL zip dropped on the
    preview is not wrapped as `model.glb`.
  - Dropped somewhere that isn't a zone — the menu bar, the progress
    pane — and nothing is imported; a toast says where it belongs.
  - On **Linux**, zones are unavailable: the window system reports no
    cursor position while files are being dragged, so a drop is routed by
    what it is instead of where it landed — a still becomes the pipeline
    input, a pack installs, and anything else becomes a new bundle. Use
    the **File** menu and the **Add … to this bundle** pickers to reach
    the other targets.
- **File → Import Bundle...** -- pick a `.zip`, a `.glb`, an image, or
  navigate into a bundle folder and double-click its `bundle.json`.
- **File → Import Bundle Folder...** -- single-click the folder, then Open
  (double-clicking navigates into it -- that's the OS file picker, not us).
- **File → Install Animation Pack...** -- a UAL `.zip` / `.glb`, or
  **Install Animation Pack Folder...** for an extracted download. Same
  as Animate → Add pack, or dropping the file on the Animation panel.

A loose `.glb` or image is copied into a new timestamped bundle with the
standard filenames (`model.glb`, `image.png`) and a `bundle.json`. Imports
leave the source untouched. Zips made with macOS's built-in Compress work fine.

## Settings

Open Settings from the gear icon to configure:

- **API Keys** -- Add or update provider API keys
- **Output Directory** -- Choose where generated models are saved

## Templates

Asset Tap includes prompt templates that help structure your text input for better results. Select a template from the sidebar dropdown, fill in the variables, and the template generates an optimized prompt.

You can browse available templates with the template selector in the sidebar.

## What's Next

- [CLI Usage](@/docs/guides/cli-usage.md) -- Automate generation from the command line
- [MCP Server](@/docs/guides/mcp.md) -- Drive the same pipeline from an MCP host
- [Providers](@/docs/guides/providers.md) -- Available models and custom provider configuration
- [Bundle Structure](@/docs/guides/bundle-structure.md) -- Understanding the output format
- [Animation (experimental)](@/docs/guides/animation.md) -- Rig a humanoid and bake clips
