+++
title = "Getting Started"
description = "Set up your API key, launch Asset Tap, and generate your first 3D model from a text prompt."
date = 2026-02-09
weight = 1
in_search_index = true

[extra]
images = []

[taxonomies]
tags = ["guide"]
+++

## Get an API Key

Asset Tap works in two AI-powered steps: first it generates an image from your text prompt (text-to-image), then it converts that image into a 3D model (image-to-3D). You'll need an API key from at least one provider that supports these models.

**Included providers** -- pick either one (a single key unlocks the full pipeline):

- [fal.ai](https://fal.ai) -- [Get API Key](https://fal.ai/dashboard/keys). Pay-per-generation.
- [Meshy AI](https://www.meshy.ai) -- [Get API Key](https://www.meshy.ai/settings/api). Subscription + credits.

You can also [add your own providers](@/docs/providers.md#adding-custom-providers) by creating YAML configuration files.

## Launch the Application

Open Asset Tap from your Applications folder, Start Menu, or wherever you installed it.

**macOS:** On first launch, macOS will block the app because it isn't signed with an Apple Developer certificate yet. Go to **System Settings → Privacy & Security** and click **Open Anyway**, or run `xattr -cr "/Applications/Asset Tap.app"` in Terminal. You only need to do this once.

On first launch, you'll be prompted to enter your API key.

## Generate Your First Model

1. Enter a text prompt (e.g., "a cowboy ninja with a leather duster, bandana mask, and dual katanas on the back")
2. Select your text-to-image model and image-to-3D model from the dropdowns
3. Click **Generate**
4. Review the AI-generated image and approve it for 3D conversion
5. The image-to-3D model creates your 3D model -- preview it in the built-in viewer

That's it! Your generated assets are saved to a timestamped output directory. See [Using Asset Tap](@/docs/using-asset-tap.md) for the full GUI guide, or [Bundle Structure](@/docs/bundle-structure.md) for the output format.

## What's Next

- [Using Asset Tap](@/docs/using-asset-tap.md) -- Full GUI guide with viewer, library, and settings
- [Installation](@/docs/installation.md) -- Download and install for your platform
- [CLI Usage](@/docs/cli-usage.md) -- Automate generation from the command line
- [Providers](@/docs/providers.md) -- Available models and custom provider configuration
