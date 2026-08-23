+++
title = "Download"
description = "Get the Asset Tap desktop app or the CLI: one pipeline, two interfaces."
template = "page.html"
in_search_index = true
+++

Asset Tap is **one pipeline with two interfaces**: a desktop app for visual, interactive generation, and a CLI for terminals, scripts, and agents. The CLI also serves the [MCP server](@/docs/guides/mcp.md), so MCP-capable agents use the CLI install. Install either, or both; they share configuration and provider keys.

## Desktop app

Point-and-click generation with live previews. Recommended if you're starting out.

| Platform | Package                                                                                                                               | Notes                    |
| -------- | ------------------------------------------------------------------------------------------------------------------------------------- | ------------------------ |
| macOS    | [AssetTap-macos.dmg](https://github.com/nightandwknd/asset-tap/releases/latest/download/AssetTap-macos.dmg)                           | GUI + CLI, Universal     |
| Windows  | [asset-tap-windows-setup.exe](https://github.com/nightandwknd/asset-tap/releases/latest/download/asset-tap-windows-setup.exe)         | GUI, x86_64              |
| Linux    | [asset-tap-linux-amd64.deb](https://github.com/nightandwknd/asset-tap/releases/latest/download/asset-tap-linux-amd64.deb)             | GUI + CLI, Debian/Ubuntu |
| Linux    | [asset-tap-linux-x86_64.AppImage](https://github.com/nightandwknd/asset-tap/releases/latest/download/asset-tap-linux-x86_64.AppImage) | GUI, Universal Linux     |

After installing, follow the [first-asset walkthrough](@/docs/getting-started/first-asset.md).

## CLI

For terminals, CI, and agent workflows, including the [MCP server](@/docs/guides/mcp.md).

**macOS / Linux**

```bash
curl -fsSL https://assettap.dev/install | bash
```

Installs `asset-tap` and the `atap` alias to `~/.local/bin`, checksum-verified. The script is [readable on GitHub](https://github.com/nightandwknd/asset-tap/blob/main/site/static/install.sh).

**Windows**

```powershell
irm https://assettap.dev/install.ps1 | iex
```

Installs `asset-tap.exe` and `atap` to `%LOCALAPPDATA%\AssetTap\bin` and registers it on your user PATH, checksum-verified. The script is [readable on GitHub](https://github.com/nightandwknd/asset-tap/blob/main/site/static/install.ps1).

Install options (pinning a release, choosing the install directory) are in the [installation guide](@/docs/getting-started/installation.md).

**CLI-only archives:** [macOS](https://github.com/nightandwknd/asset-tap/releases/latest/download/asset-tap-cli-macos.tar.gz) · [Windows](https://github.com/nightandwknd/asset-tap/releases/latest/download/asset-tap-cli-windows.zip) · [Linux](https://github.com/nightandwknd/asset-tap/releases/latest/download/asset-tap-cli-linux.tar.gz)

Next: [CLI usage](@/docs/guides/cli-usage.md).

## Requirements

- **OS**: macOS 10.15+ (Intel or Apple Silicon), Windows 10+, Linux (glibc 2.31+)
- **API key**: from a supported provider ([fal.ai](https://fal.ai/dashboard/keys) or [Meshy](https://www.meshy.ai)) or one you [configure yourself](@/docs/guides/providers.md#adding-custom-providers)
- **Blender** (optional): required only for FBX export

## Build from source

```bash
git clone https://github.com/nightandwknd/asset-tap.git
cd asset-tap
make build
```

See the [Development Guide](https://github.com/nightandwknd/asset-tap/blob/main/docs/DEVELOPMENT.md) for details. All download links point to the [latest release](https://github.com/nightandwknd/asset-tap/releases/latest).
