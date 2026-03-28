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

First, grab the right package for your platform from the [Downloads](https://assettap.dev/download/) page.

## macOS

All macOS downloads are universal binaries that run natively on both Intel and Apple Silicon Macs.

**DMG Installer (Recommended)**

The DMG includes both the GUI application and the CLI tool.

1. Open the DMG file
2. Drag **AssetTap** to your Applications folder
3. **First launch:** macOS will block the app because it isn't signed with an Apple Developer certificate yet. To allow it:
   - Open **System Settings → Privacy & Security**, scroll down, and click **Open Anyway** next to the "Asset Tap.app was blocked" message:
     ![Open Anyway in macOS Privacy & Security settings](/images/screenshots/open_anyway.png)
   - Or run this once in Terminal:
     ```bash
     xattr -cr "/Applications/Asset Tap.app"
     ```
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
tar -xzf asset-tap-cli-macos.tar.gz
sudo mv asset-tap /usr/local/bin/
```

## Windows

**NSIS Installer (Recommended)**

1. Run the installer
2. Launch Asset Tap from the Start Menu

**CLI-Only Download (Alternative)**

```powershell
Expand-Archive asset-tap-cli-windows.zip -DestinationPath .
```

## Linux

**Debian/Ubuntu (.deb)**

The `.deb` package installs both the GUI and CLI.

```bash
sudo dpkg -i asset-tap-linux-amd64.deb
```

After installation, both `asset-tap-gui` and `asset-tap` are available system-wide.

**AppImage (Universal)**

```bash
chmod +x asset-tap-linux-x86_64.AppImage
./asset-tap-linux-x86_64.AppImage
```

Note: The AppImage contains only the GUI. For the CLI, download the standalone archive from the [Downloads](https://assettap.dev/download/) page and extract:

```bash
tar -xzf asset-tap-cli-linux.tar.gz
sudo mv asset-tap /usr/local/bin/
```

## Blender (Optional)

[Blender](https://www.blender.org/download/) is required only for FBX export. On Linux, you can also install via `sudo apt install blender` or Snap/Flatpak.

## Requirements

See [Downloads](https://assettap.dev/download/) for system requirements and building from source.
