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

## CLI — one line (macOS / Linux)

```bash
curl -fsSL https://assettap.dev/install | bash
```

No sudo: this installs `asset-tap` and the `atap` alias to `~/.local/bin` and verifies the download against the release's `SHA256SUMS` before installing. Options:

```bash
# pin a specific release
curl -fsSL https://assettap.dev/install | bash -s -- v26.8.12

# install somewhere else
curl -fsSL https://assettap.dev/install | ASSET_TAP_INSTALL_DIR=/usr/local/bin bash
```

Prefer to see what you're running first? The script lives at [assettap.dev/install](https://assettap.dev/install) — read it, then run it. Every command it performs can also be done by hand from the [Downloads](https://assettap.dev/download/) page.

For the **desktop app** (or the Windows CLI), grab the right package for your platform from the [Downloads](https://assettap.dev/download/) page and follow your platform's section below.

## macOS

All macOS downloads are universal binaries that run natively on both Intel and Apple Silicon Macs.

**DMG Installer (Recommended)**

The DMG includes both the GUI application and the CLI tool.

1. Open the DMG file
2. Drag **AssetTap** to your Applications folder
3. Launch from Applications or Spotlight

**CLI Setup (Optional)**

The CLI is bundled inside the app. To use it from the terminal, create a symlink:

```bash
sudo ln -sf "/Applications/Asset Tap.app/Contents/MacOS/asset-tap" /usr/local/bin/asset-tap
sudo ln -sf "/Applications/Asset Tap.app/Contents/MacOS/asset-tap" /usr/local/bin/atap   # optional short alias
```

Verify it works:

```bash
asset-tap --help
```

**CLI-Only Download (Alternative)**

If you only need the CLI without the GUI:

```bash
tar -xzf asset-tap-cli-macos.tar.gz
sudo mv asset-tap atap /usr/local/bin/   # atap = short alias, shipped in the archive
```

## Windows

**NSIS Installer (Recommended)**

1. Run the installer
2. Launch Asset Tap from the Start Menu

**CLI-Only Download (Alternative)**

```powershell
Expand-Archive asset-tap-cli-windows.zip -DestinationPath .
```

The zip contains `asset-tap.exe` and `atap.cmd` (a short alias); add the extracted folder to your `PATH` and both work.

## Linux

**Debian/Ubuntu (.deb)**

The `.deb` package installs both the GUI and CLI.

```bash
sudo dpkg -i asset-tap-linux-amd64.deb
```

After installation, `asset-tap-gui`, `asset-tap`, and the `atap` alias are available system-wide.

**AppImage (Universal)**

```bash
chmod +x asset-tap-linux-x86_64.AppImage
./asset-tap-linux-x86_64.AppImage
```

Note: The AppImage contains only the GUI. For the CLI, download the standalone archive from the [Downloads](https://assettap.dev/download/) page and extract:

```bash
tar -xzf asset-tap-cli-linux.tar.gz
sudo mv asset-tap atap /usr/local/bin/   # atap = short alias, shipped in the archive
```

## Blender (Optional)

[Blender](https://www.blender.org/download/) is required only for FBX export. On Linux, you can also install via `sudo apt install blender` or Snap/Flatpak.

## Requirements

See [Downloads](https://assettap.dev/download/) for system requirements and building from source.
