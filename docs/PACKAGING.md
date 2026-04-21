# Packaging & Distribution Guide

This guide covers how to build installer packages for Asset Tap across macOS, Windows, and Linux.

## Overview

Asset Tap uses [cargo-packager](https://github.com/crabnebula-dev/cargo-packager) to create native installers for all platforms:

- **macOS**: `.app` bundle and `.dmg` installer
- **Windows**: `.exe` NSIS installer
- **Linux**: `.deb` package and `.AppImage`

**Important**: `cargo-packager` does NOT automatically build binaries. Our Makefile handles building before packaging to ensure reliability. See [cargo-packager documentation](https://docs.crabnebula.dev/packager/) for details.

## Quick Start

### Local Development

Build packages for your current platform:

```bash
# Install cargo-packager
make install-packager

# Build for specific platform
make package-macos     # macOS only
make package-windows   # Windows only
make package-linux     # Linux only
```

### CI/CD

The GitHub Actions workflow (`.github/workflows/release.yaml`) automatically builds and releases packages on push to `main` using CalVer versioning (YY.MM.PATCH). All three platforms (macOS, Linux, Windows) build and package in parallel.

## Package Configuration

### GUI Application

Configuration is in `gui/Cargo.toml` under `[package.metadata.packager]`:

```toml
[package.metadata.packager]
name = "Asset Tap"
product_name = "AssetTap"
identifier = "com.nightandwknd.asset-tap"
category = "Graphics and Design"
description = "AI-powered text-to-3D model generation"
```

### Platform-Specific Settings

**macOS** (`[package.metadata.packager.macos]`):

- Minimum system version: macOS 10.15+
- Creates `.app` bundle and `.dmg` disk image
- App bundle is self-contained with all dependencies

**Windows** (`[package.metadata.packager.windows]`):

- NSIS installer with Start Menu shortcuts
- Automatic PATH setup (optional during install)
- Uninstaller included

**Linux** (`[package.metadata.packager.linux]`):

- `.deb` for Debian/Ubuntu distributions
- `.AppImage` for universal Linux support
- Desktop entry with proper categories

## Output Files

### GUI Installers

| Platform | File                              | Architecture               | Installation Method            |
| -------- | --------------------------------- | -------------------------- | ------------------------------ |
| macOS    | `AssetTap-macos.dmg`              | Universal (arm64 + x86_64) | Drag to Applications folder    |
| Windows  | `asset-tap-windows-setup.exe`     | x86_64                     | Run installer wizard           |
| Linux    | `asset-tap-linux-amd64.deb`       | x86_64                     | `sudo dpkg -i` or double-click |
| Linux    | `asset-tap-linux-x86_64.AppImage` | x86_64                     | `chmod +x` and run directly    |

### CLI Archives

For advanced users who want just the CLI tool:

| Platform | File                         | Architecture               | Format      |
| -------- | ---------------------------- | -------------------------- | ----------- |
| macOS    | `asset-tap-cli-macos.tar.gz` | Universal (arm64 + x86_64) | Tarball     |
| Windows  | `asset-tap-cli-windows.zip`  | x86_64                     | ZIP archive |
| Linux    | `asset-tap-cli-linux.tar.gz` | x86_64                     | Tarball     |

## CLI Bundling Strategy

Every installer includes the CLI alongside the GUI. Users get both tools in one download.

### Per-Platform Approach

| Platform       | CLI Location                               | How CLI Gets in PATH                      | Notes                                    |
| -------------- | ------------------------------------------ | ----------------------------------------- | ---------------------------------------- |
| macOS          | `.app/Contents/MacOS/asset-tap`            | User creates symlink to `/usr/local/bin/` | Symlink auto-updates when app is updated |
| Windows        | `C:\Program Files\Asset Tap\asset-tap.exe` | NSIS installer adds install dir to PATH   | No user action needed after install      |
| Linux `.deb`   | `/usr/bin/asset-tap`                       | `dpkg` installs to PATH automatically     | No user action needed after install      |
| Linux AppImage | N/A — AppImage is single-file              | Separate tarball download                 | AppImage can't bundle extra binaries     |

### macOS

Three-step process in `Makefile` and CI:

1. `cargo packager --formats app` creates the `.app` with only the GUI
2. CLI binary copied into `.app/Contents/MacOS/`
3. `hdiutil create` creates the `.dmg` from the modified `.app`

Users symlink after install:

```bash
sudo ln -sf "/Applications/Asset Tap.app/Contents/MacOS/asset-tap" /usr/local/bin/asset-tap
```

### Windows

The NSIS installer is created by `cargo-packager`. The CLI is shipped as a standalone `.zip` archive alongside the installer.

**Packaging script**: `scripts/package-windows.sh`

1. `cargo packager --formats nsis` creates the NSIS installer with the GUI
2. CLI binary archived into `asset-tap-cli-windows.zip`

### Linux

**Packaging script**: `scripts/package-linux.sh`

1. `cargo packager --formats deb,appimage` creates both formats
2. CLI binary is injected into the `.deb` via `dpkg-deb` repack (installed to `/usr/bin/`)
3. CLI binary archived into `asset-tap-cli-linux.tar.gz` (for AppImage users)

**AppImage**: Single-file format, can't include the CLI. AppImage users download the CLI tarball separately.

### Standalone CLI Archives

All platforms also publish a standalone CLI download (tarball or zip) for users who:

- Only want the CLI (no GUI)
- Need to deploy to servers or CI environments
- Use AppImage on Linux

## Building Locally

### Prerequisites

1. **Rust toolchain** (stable)
2. **cargo-packager**: `cargo install cargo-packager --locked`
3. **Platform-specific tools**:
   - macOS: Xcode Command Line Tools
   - Windows: NSIS (installed automatically by cargo-packager)
   - Linux: `dpkg`, `fuse` (for AppImage)

### Build Commands

```bash
# Platform-specific packaging (builds automatically)
make package-macos              # macOS (native arch only, fast)
make package-macos-universal    # macOS universal (arm64 + x86_64, release quality)
make package-windows            # Windows only
make package-linux              # Linux only

# Outputs in target/release/ directory
ls -la target/release/AssetTap.*
```

**Note**: The Makefile targets automatically build release binaries before packaging. If you need to package manually:

```bash
# Build release binaries first
make build

# Then package GUI application
cd gui
cargo packager --release
```

### Cross-Platform Notes

**You cannot build installers for other platforms from your current OS.** For example:

- macOS `.dmg` can only be built on macOS
- Windows `.exe` can only be built on Windows
- Linux `.deb`/`.AppImage` can only be built on Linux

Use GitHub Actions or dedicated build machines for multi-platform releases.

## Design Decisions

### Why We Build Explicitly Before Packaging

We use Makefile dependencies (`package-macos: build`) instead of `beforePackagingCommand` in `Cargo.toml`. Here's why:

**Our approach (Makefile-based):**

```makefile
package-macos: install-packager build
    cd gui && cargo packager --release --formats app    # 1. Create .app
    cp target/release/asset-tap gui/dist/...MacOS/      # 2. Bundle CLI into .app
    hdiutil create ... gui/dist/AssetTap.dmg            # 3. Create DMG from .app
```

The packaging is split into three steps: cargo-packager creates the `.app`, we inject the CLI binary, then `hdiutil` creates the DMG. This ensures the DMG contains both the GUI and CLI binaries.

**Alternative (not used):**

```toml
[package.metadata.packager]
before-packaging-command = "cargo build --release --workspace"
```

**Reasons for our choice:**

1. **Explicit is better than implicit**: Developers can see the build step happens
2. **Consistency**: Matches our GitHub Actions workflow pattern
3. **Debuggability**: Easier to debug build failures separately from packaging failures
4. **Flexibility**: Can customize build steps without modifying `Cargo.toml`
5. **Documentation**: Clearer for contributors to understand the workflow

Both approaches are valid, but we prioritize clarity and consistency.

### Why Universal Binaries on macOS

macOS releases ship as **universal binaries** containing both arm64 (Apple Silicon) and x86_64 (Intel) architectures. This means one download works natively on all Macs.

**How it works:**

1. Build for both targets on the ARM64 CI runner (cross-compilation to x86_64 is supported natively by Apple's toolchain)
2. Combine with `lipo -create` into a single universal binary
3. Package the universal binary with `cargo-packager`

```bash
# Build both architectures
cargo build --release --package asset-tap-gui --target aarch64-apple-darwin
cargo build --release --package asset-tap-gui --target x86_64-apple-darwin

# Combine into universal binary
lipo -create -output target/release/asset-tap-gui \
    target/aarch64-apple-darwin/release/asset-tap-gui \
    target/x86_64-apple-darwin/release/asset-tap-gui

# Verify
file target/release/asset-tap-gui
# → Mach-O universal binary with 2 architectures: [x86_64, arm64]
```

**Why `lipo` instead of cargo-packager native support?** Neither Cargo nor cargo-packager support building universal binaries directly. The `lipo` approach is standard practice for Rust projects (used by Tauri and others).

**Local development**: Use `make package-macos` for fast single-arch builds during development. Use `make package-macos-universal` for release-quality universal builds.

## GitHub Actions Workflows

### CI (Pull Requests)

Triggered on every PR to `main`. Three layers execute sequentially, with jobs within each layer running in parallel.

```
PR opened/updated
│
├─ Layer 0 (all parallel, no dependencies) ────────────────────────┐
│  ┌─────────┐ ┌──────┐ ┌───────┐ ┌──────┐ ┌──────┐ ┌───────┐      │
│  │ Format  │ │ Lint │ │ Check │ │ Test │ │ Docs │ │ Audit │      │
│  │ rustfmt │ │clippy│ │ cargo │ │ cov  │ │ doc  │ │ audit │      │
│  │ dprint  │ │      │ │ check │ │ llvm │ │      │ │       │      │
│  │ edcfg   │ │      │ │       │ │  cov │ │      │ │       │      │
│  └─────────┘ └──────┘ └───┬───┘ └──────┘ └──────┘ └───────┘      │
│                           │                                      │
│  ┌───────┐ ┌───────────────┐                                     │
│  │ Udeps │ │Version Preview│                                     │
│  └───────┘ └───────────────┘                                     │
├──────────────────────────────────────────────────────────────────┘
│
├─ Layer 1 (after Check passes) ─────────────────────────────────────────────┐
│  ┌───────────────────────┐ ┌───────────────────────┐ ┌────────────────────┐│
│  │ Build & Package       │ │ Build & Package       │ │ Build & Package    ││
│  │ (macOS) macos-latest  │ │ (Linux) ubuntu-latest │ │ (Win) windows-lat. ││
│  │                       │ │                       │ │                    ││
│  │ cargo build           │ │ cargo build           │ │ cargo build        ││
│  │ cargo packager        │ │ package-linux.sh      │ │ cargo packager     ││
│  │ package-macos.sh      │ │                       │ │                    ││
│  │                       │ │ Artifacts:            │ │ Artifacts:         ││
│  │ Artifacts:            │ │ → ..asset-tap-        │ │ → ..asset-tap-     ││
│  │ → ..asset-tap-macos   │ │   linux-deb           │ │   windows          ││
│  │                       │ │ → ..asset-tap-        │ │                    ││
│  │                       │ │   binaries-linux      │ │                    ││
│  └───────────────────────┘ └───────────┬───────────┘ └────────────────────┘│
├────────────────────────────────────────┼───────────────────────────────────┘
│                                │
├─ Layer 2 (after Linux build completes) ──────────────────────────┐
│                                ▼                                 │
│                       ┌─────────────┐                            │
│                       │  CLI Tests  │                            │
│                       │  (Linux)    │                            │
│                       └─────────────┘                            │
├──────────────────────────────────────────────────────────────────┘
│
▼
All checks pass → PR mergeable
```

**Shared action:** All build + package logic lives in `.github/actions/build-and-package/`, used by both CI and Release workflows. CI uploads artifacts with a `-pr-{N}` suffix (e.g., `asset-tap-macos-pr-7`). The Linux binary artifact is also uploaded for CLI tests.

**PR artifacts** (7-day retention): `asset-tap-macos-pr-{N}` (DMG), `asset-tap-linux-deb-pr-{N}`, `asset-tap-linux-appimage-pr-{N}`, `asset-tap-windows-pr-{N}` (NSIS installer), `asset-tap-binaries-linux-pr-{N}` (CLI binary for tests).

### Release (Push to Main)

Triggered on every push to `main`. Creates a release commit (version + changelog), tags it, then builds all platforms from the tag in parallel.

```
Push to main
│
▼
┌──────────────────────────┐
│ Prepare Release (ubuntu) │
│                          │
│ 1. CalVer: YY.MM.PATCH   │
│ 2. git-cliff → changelog │
│ 3. Stamp Cargo.toml      │
│ 4. Commit + tag + push   │
│    [skip ci]             │
└────────────┬─────────────┘
             │ (if should_release)
             │ builds checkout tag
             │
             ├──────────────────────┬──────────────────────┐
             ▼                      ▼                      ▼
┌──────────────────────┐  ┌──────────────────────┐  ┌──────────────────────┐
│ Build & Package      │  │ Build & Package      │  │ Build & Package      │
│ (macOS) macos-latest │  │ (Linux) ubuntu-lat.  │  │ (Win) windows-lat.   │
│                      │  │                      │  │                      │
│ Checks out tag       │  │ Checks out tag       │  │ Checks out tag       │
│ Version already in   │  │ Version already in   │  │ Version already in   │
│ Cargo.toml           │  │ Cargo.toml           │  │ Cargo.toml           │
│                      │  │                      │  │                      │
│ → asset-tap-macos    │  │ → asset-tap-linux-deb│  │ → asset-tap-windows  │
│ → asset-tap-cli-macos│  │ → ..linux-appimage   │  │ → asset-tap-cli-win. │
│                      │  │ → asset-tap-cli-linux│  │                      │
└──────────┬───────────┘  └──────────┬───────────┘  └──────────┬───────────┘
           │                         │                         │
           └─────────────────────────┼─────────────────────────┘
                                     ▼
                 ┌──────────────────────────┐
                 │   Create GitHub Release  │
                 │      (ubuntu)            │
                 │                          │
                 │ 1. Download all artifacts│
                 │ 2. Rename/organize       │
                 │ 3. gh release create     │
                 │    (git-cliff notes)     │
                 └──────────────────────────┘
```

**Release artifacts (up to 8 files):**

| Artifact                          | Platform | Type                  |
| --------------------------------- | -------- | --------------------- |
| `AssetTap-macos.dmg`              | macOS    | GUI + CLI (Universal) |
| `asset-tap-cli-macos.tar.gz`      | macOS    | CLI only (Universal)  |
| `asset-tap-linux-amd64.deb`       | Linux    | GUI + CLI             |
| `asset-tap-linux-x86_64.AppImage` | Linux    | GUI only              |
| `asset-tap-cli-linux.tar.gz`      | Linux    | CLI only              |
| `asset-tap-windows-setup.exe`     | Windows  | GUI (NSIS installer)  |
| `asset-tap-cli-windows.zip`       | Windows  | CLI only              |

**Version flow:** CalVer `YY.MM.PATCH` — same month increments patch (26.03.1 → 26.03.2), new month resets (26.03.2 → 26.04.1). The release commit stamps the version into `Cargo.toml` and generates `CHANGELOG.md`; workspace members inherit via `version.workspace = true`. Build jobs checkout the tagged commit, so the version in source matches the tag.

**Changelog:** Release notes are generated by [git-cliff](https://git-cliff.org/) using Conventional Commits (config: `cliff.toml`). Commits are grouped by type (Features, Bug Fixes, CI/CD, etc.) with merge commits filtered out. The changelog is included in the release commit alongside the version stamp, so the tagged commit is the canonical source of truth. The `[skip ci]` flag prevents the release commit from re-triggering the workflow.

### Triggering a Release

Any push to `main` with new commits since the last tag triggers a release. The version is determined automatically using CalVer (YY.MM.PATCH):

- Same month as last release → increment patch (e.g., 26.03.1 → 26.03.2)
- New month → reset patch to 1 (e.g., 26.03.2 → 26.04.1)

The workflow automatically:

1. Determines the CalVer version from git tags and date
2. Generates changelog with git-cliff
3. Creates a release commit (stamps `Cargo.toml` + `CHANGELOG.md`) and tag
4. Builds all 3 platforms from the tagged commit (macOS universal, Linux, Windows)
5. Creates a GitHub Release with all artifacts and changelog

## Customization

### App Icons

Icons are configured in `gui/Cargo.toml`:

```toml
[package.metadata.packager]
icons = ["../assets/icon.png", "../assets/icon.ico"]
```

To replace them, update the files in `assets/` and rebuild. Recommended sizes:

- PNG: 512x512 (used by macOS and Linux)
- ICO: 256x256 (used by Windows)

### Custom Install Locations

**Windows**: Modify NSIS template (advanced)
**macOS**: Not customizable (always `/Applications`)
**Linux**: Packager handles standard locations

### Including Additional Files

To bundle assets with installers:

```toml
[package.metadata.packager]
resources = [
  "templates/*.yaml",
  "providers/*.yaml",
]
```

## Testing Packages

### macOS

```bash
# Build DMG
make package-macos

# Verify .app contains both binaries
ls -la target/release/Asset Tap.app/Contents/MacOS/
# Should show: asset-tap-gui AND asset-tap

# Test installation
open target/release/AssetTap.dmg
# Drag to Applications, then launch

# Verify app bundle
codesign -dv target/release/Asset Tap.app

# Test CLI symlink
sudo ln -sf "/Applications/Asset Tap.app/Contents/MacOS/asset-tap" /usr/local/bin/asset-tap
asset-tap --help
asset-tap --list-providers

# Clean up test symlink
sudo rm /usr/local/bin/asset-tap
```

### Windows

```bash
# Build installer
make package-windows

# Test in VM or Windows machine
.\dist\*-setup.exe

# After install, verify CLI is in PATH
asset-tap --help
asset-tap --list-providers
```

### Linux

```bash
# Build packages
make package-linux

# Test .deb
sudo dpkg -i dist/*.deb
asset-tap-gui    # GUI launches
asset-tap --help # CLI works

# Test AppImage
chmod +x dist/*.AppImage
./dist/*.AppImage

# CLI for AppImage users: test standalone tarball
tar -xzf asset-tap-cli-linux.tar.gz
./asset-tap --help
```

## Troubleshooting

### "cargo-packager not found"

```bash
cargo install cargo-packager --locked
```

### macOS: "Developer cannot be verified"

Official release builds from GitHub are signed with Apple Developer ID and notarized — users should not see this dialog. If you build locally without setting `APPLE_SIGNING_IDENTITY`, the app is ad-hoc signed and users must:

1. Right-click the app
2. Select "Open"
3. Confirm in dialog

### Linux: AppImage won't run

Ensure FUSE is installed:

```bash
sudo apt install fuse libfuse2
```

### Windows: Installer blocked by SmartScreen

Users may see a SmartScreen warning. Click "More info" then "Run anyway" to proceed.

## macOS Code Signing & Notarization

Official releases are signed with an Apple Developer ID certificate and notarized by Apple so users don't see Gatekeeper warnings.

### How it works

[scripts/package-macos.sh](../scripts/package-macos.sh) checks for `APPLE_SIGNING_IDENTITY`:

- **Set:** signs with Developer ID + hardened runtime, signs the DMG, submits to `notarytool`, staples the ticket.
- **Unset:** falls back to ad-hoc signing (fine for local dev; users get the Gatekeeper dialog).

Entitlements live at [gui/entitlements.plist](../gui/entitlements.plist). The current set (`allow-jit`, `allow-unsigned-executable-memory`) covers the OpenGL/glow rendering path. If notarization fails with library-validation errors, add `com.apple.security.cs.disable-library-validation` — but try without it first, since tighter entitlements are better.

### CI secrets (GitHub Actions)

The release workflow reads these secrets from the `release` GitHub Environment:

| Secret                         | Purpose                                                                 |
| ------------------------------ | ----------------------------------------------------------------------- |
| `APPLE_CERTIFICATE_P12_BASE64` | Base64-encoded `.p12` containing Developer ID cert + private key        |
| `APPLE_CERTIFICATE_PASSWORD`   | Password used when exporting the `.p12`                                 |
| `APPLE_SIGNING_IDENTITY`       | Full identity, e.g. `Developer ID Application: Night and Wknd (TEAMID)` |
| `APPLE_ID`                     | Apple ID email used for notarization                                    |
| `APPLE_APP_PASSWORD`           | App-specific password from appleid.apple.com                            |
| `APPLE_TEAM_ID`                | 10-character team ID                                                    |

The workflow imports the `.p12` into a temporary keychain for the duration of the job, then signs + notarizes both the DMG and the standalone CLI tarball.

### Protected environment setup

Signing secrets live in a GitHub Environment (not repo secrets) so they're gated behind manual approval. This defends against supply-chain attacks where a malicious workflow edit could otherwise exfiltrate the signing identity.

To set up (one-time):

1. Repo → **Settings** → **Environments** → **New environment** → name it `release`
2. Add all six `APPLE_*` secrets above as **environment secrets** (not repository secrets)
3. **Deployment protection rules:**
   - ✅ **Required reviewers** — add yourself (and anyone else who should approve releases)
   - ✅ **Deployment branches** → "Selected branches" → add rule for `main` only
4. Save

When a release workflow run reaches the `package-macos` job, it will pause and notify reviewers. The job (and its secrets) only runs after approval.

**Solo dev note:** Leave "Prevent self-review" **off** — otherwise you'll deadlock yourself on every release.

### Signing locally

You generally don't need to — ad-hoc signing works for local development. If you want to produce a signed build locally (e.g. to test the full pipeline):

```bash
export APPLE_SIGNING_IDENTITY="Developer ID Application: Night and Wknd (TEAMID)"
export APPLE_ID="your@email.com"
export APPLE_APP_PASSWORD="xxxx-xxxx-xxxx-xxxx"
export APPLE_TEAM_ID="TEAMID"

make package-macos-universal
```

The cert must already be in your login Keychain. `security find-identity -v -p codesigning` lists available identities.

### Verifying a signed build

```bash
make verify-sign-macos
```

Or run the checks individually:

```bash
codesign -dv --verbose=4 "target/release/Asset Tap.app"  # Signature details
xcrun stapler validate target/release/AssetTap.dmg       # Notarization ticket
spctl -a -vv -t install target/release/AssetTap.dmg      # Simulate Gatekeeper
```

## Advanced: Manual Packaging

If you need to customize beyond cargo-packager:

### macOS App Bundle Structure

```
Asset Tap.app/
└── Contents/
    ├── Info.plist              # App metadata
    ├── MacOS/
    │   ├── asset-tap-gui  # GUI binary (primary)
    │   └── asset-tap      # CLI binary (bundled)
    └── Resources/
        └── AppIcon.icns        # Icon
```

The CLI binary is injected into the `.app` after cargo-packager creates it. Users symlink it to their PATH:

```bash
sudo ln -sf "/Applications/Asset Tap.app/Contents/MacOS/asset-tap" /usr/local/bin/asset-tap
```

Create manually with:

```bash
mkdir -p Asset Tap.app/Contents/MacOS
mkdir -p Asset Tap.app/Contents/Resources
cp target/release/asset-tap-gui Asset Tap.app/Contents/MacOS/
# Create Info.plist...
```

### Windows NSIS Script

For custom installers, see `cargo-packager`'s generated `.nsi` files in `dist/` for reference.

### Linux Desktop Entry

cargo-packager generates `.desktop` files automatically. Template is in `gui/Cargo.toml`:

```ini
[Desktop Entry]
Type=Application
Name=Asset Tap
Exec={{bin}}
Icon={{icon}}
Categories=Graphics;3DGraphics;
Terminal=false
```

## Further Reading

- [cargo-packager documentation](https://github.com/crabnebula-dev/cargo-packager)
- [macOS App Bundle structure](https://developer.apple.com/library/archive/documentation/CoreFoundation/Conceptual/CFBundles/BundleTypes/BundleTypes.html)
- [NSIS installer documentation](https://nsis.sourceforge.io/Docs/)
- [Linux Desktop Entry spec](https://specifications.freedesktop.org/desktop-entry-spec/latest/)
