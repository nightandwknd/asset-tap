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

## macOS

All macOS downloads are universal binaries that run natively on both Intel and Apple Silicon Macs.

**DMG Installer (Recommended)**

The DMG includes both the GUI application and the CLI tool.

1. Download [AssetTap-macos.dmg](https://github.com/nightandwknd/asset-tap/releases/latest/download/AssetTap-macos.dmg)
2. Open the DMG file
3. Drag **AssetTap** to your Applications folder
4. Launch from Applications or Spotlight

**CLI Setup (Optional)**

The CLI is bundled inside the app. To use it from the terminal, create a symlink:

```bash
sudo ln -sf "/Applications/Asset Tap.app/Contents/MacOS/asset-tap" /usr/local/bin/asset-tap
```

Verify it works:

```bash
asset-tap --help
```

**CLI-Only Download (Alternative)**

If you only need the CLI without the GUI:

```bash
curl -LO https://github.com/nightandwknd/asset-tap/releases/latest/download/asset-tap-cli-macos.tar.gz
tar -xzf asset-tap-cli-macos.tar.gz
sudo mv asset-tap /usr/local/bin/
```

## Windows

**NSIS Installer (Recommended)**

1. Download the installer from [Releases](https://github.com/nightandwknd/asset-tap/releases/latest)
2. Run the installer
3. Launch Asset Tap from the Start Menu

**CLI-Only Download (Alternative)**

```powershell
Invoke-WebRequest -Uri https://github.com/nightandwknd/asset-tap/releases/latest/download/asset-tap-cli-windows.zip -OutFile asset-tap-cli-windows.zip
Expand-Archive asset-tap-cli-windows.zip -DestinationPath .
```

## Linux

**Debian/Ubuntu (.deb)**

The `.deb` package installs both the GUI and CLI.

```bash
curl -LO https://github.com/nightandwknd/asset-tap/releases/latest/download/asset-tap-linux-amd64.deb
sudo dpkg -i asset-tap-linux-amd64.deb
```

After installation, both `asset-tap-gui` and `asset-tap` are available system-wide.

**AppImage (Universal)**

```bash
curl -LO https://github.com/nightandwknd/asset-tap/releases/latest/download/asset-tap-linux-x86_64.AppImage
chmod +x asset-tap-linux-x86_64.AppImage
./asset-tap-linux-x86_64.AppImage
```

Note: The AppImage contains only the GUI. For the CLI, download the standalone archive:

```bash
curl -LO https://github.com/nightandwknd/asset-tap/releases/latest/download/asset-tap-cli-linux.tar.gz
tar -xzf asset-tap-cli-linux.tar.gz
sudo mv asset-tap /usr/local/bin/
```

## Requirements

- **Operating System**: macOS 10.15+, Windows 10+, Linux (glibc 2.31+)
- **AI Provider API Key**: From [fal.ai](https://fal.ai/dashboard/keys) or a custom provider
- **Blender** (optional): Required only for FBX export
  - macOS: [Blender.org](https://www.blender.org/download/)
  - Linux: `sudo apt install blender` or Snap/Flatpak
  - Windows: [Blender.org](https://www.blender.org/download/)

## Building from Source

```bash
# Clone repository
git clone https://github.com/nightandwknd/asset-tap.git
cd asset-tap

# Build
make build

# Run GUI
make gui

# Run CLI
make cli ARGS='-y "a robot"'
```

See the [Development Guide](https://github.com/nightandwknd/asset-tap/blob/main/docs/DEVELOPMENT.md) for detailed setup instructions.
