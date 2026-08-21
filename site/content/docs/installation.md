+++
title = "Installation"
description = "Download and install Asset Tap on macOS, Windows, or Linux."
date = 2026-02-09
weight = 2
in_search_index = true

[extra]
images = []

[taxonomies]
tags = ["guide"]
+++

There are two things you can install — pick what you need (or both):

- **The CLI** — `asset-tap` in your terminal, for scripting, automation, and agents.
- **The desktop app** — the GUI, installed per platform.

## Install the CLI (macOS / Linux)

```bash
curl -fsSL https://assettap.dev/install | bash
```

This installs `asset-tap` and the `atap` alias to `~/.local/bin`, verifying the download against the release's `SHA256SUMS` first. Options:

```bash
# pin a specific release
curl -fsSL https://assettap.dev/install | bash -s -- v26.8.12

# install somewhere else
curl -fsSL https://assettap.dev/install | ASSET_TAP_INSTALL_DIR=/usr/local/bin bash
```

The script is readable at [assettap.dev/install](https://assettap.dev/install), and every step it performs can be done by hand with the archives on the [Downloads](https://assettap.dev/download/) page.

## Install the CLI (Windows)

```powershell
irm https://assettap.dev/install.ps1 | iex
```

This installs `asset-tap.exe` and the `atap` alias to `%LOCALAPPDATA%\AssetTap\bin`, adds that directory to your user `PATH`, and verifies the download against the release's `SHA256SUMS` first. Works in Windows PowerShell and PowerShell 7. Options:

```powershell
# pin a specific release
$env:ASSET_TAP_VERSION = "v26.8.12"; irm https://assettap.dev/install.ps1 | iex

# install somewhere else
$env:ASSET_TAP_INSTALL_DIR = "C:\tools\asset-tap"; irm https://assettap.dev/install.ps1 | iex
```

The script is readable at [assettap.dev/install.ps1](https://assettap.dev/install.ps1).

## Install the Desktop App

Grab your platform's installer from the [Downloads](https://assettap.dev/download/) page.

### macOS

All macOS downloads are universal binaries that run natively on both Intel and Apple Silicon Macs.

1. Open [AssetTap-macos.dmg](https://github.com/nightandwknd/asset-tap/releases/latest/download/AssetTap-macos.dmg)
2. Drag **AssetTap** to your Applications folder
3. Launch from Applications or Spotlight

The app carries its own copy of the CLI internally. To use `asset-tap` from your terminal, run the [CLI install](#install-the-cli-macos-linux) above — same binary, kept current independently of the app.

### Windows

1. Run [asset-tap-windows-setup.exe](https://github.com/nightandwknd/asset-tap/releases/latest/download/asset-tap-windows-setup.exe)
2. Launch Asset Tap from the Start Menu

For the terminal, run the [Windows CLI install](#install-the-cli-windows) above.

### Linux

**Debian/Ubuntu (.deb)**

The `.deb` package installs the GUI and the CLI together, system-wide:

```bash
sudo dpkg -i asset-tap-linux-amd64.deb
```

After installation, `asset-tap-gui`, `asset-tap`, and the `atap` alias are on your `PATH` — no separate CLI install needed.

**AppImage (Universal)**

```bash
chmod +x asset-tap-linux-x86_64.AppImage
./asset-tap-linux-x86_64.AppImage
```

The AppImage is the GUI only — pair it with the [CLI install](#install-the-cli-macos-linux) above for terminal use.

## Blender (Optional)

[Blender](https://www.blender.org/download/) is required only for FBX export. On Linux, you can also install via `sudo apt install blender` or Snap/Flatpak.

## Requirements

See [Downloads](https://assettap.dev/download/) for system requirements and building from source.
