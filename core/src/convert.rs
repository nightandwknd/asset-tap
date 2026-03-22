//! Blender GLB to FBX conversion.
//!
//! Supports finding Blender on all major platforms:
//! - **macOS**: /Applications, ~/Applications, Homebrew
//! - **Windows**: Program Files with version-specific folders
//! - **Linux**: apt/dnf (/usr/bin), Snap, Flatpak

use crate::constants::files::bundle as bundle_files;
use crate::types::{Error, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Find the Blender executable path.
///
/// Searches in the following order:
/// 1. System PATH
/// 2. Platform-specific default installation locations
/// 3. User-specific installation locations
pub fn find_blender() -> Option<String> {
    // Check PATH first (works on all platforms)
    if which::which("blender").is_ok() {
        return Some("blender".to_string());
    }

    // Platform-specific searches
    #[cfg(target_os = "macos")]
    {
        if let Some(path) = find_blender_macos() {
            return Some(path);
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(path) = find_blender_windows() {
            return Some(path);
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(path) = find_blender_linux() {
            return Some(path);
        }
    }

    None
}

/// Find Blender on macOS.
///
/// Checks:
/// - /Applications/Blender.app
/// - ~/Applications/Blender.app
/// - Homebrew Cask location
#[cfg(target_os = "macos")]
fn find_blender_macos() -> Option<String> {
    // Standard Applications folder
    let app_path = "/Applications/Blender.app/Contents/MacOS/Blender";
    if Path::new(app_path).exists() {
        return Some(app_path.to_string());
    }

    // User's Applications folder
    if let Some(home) = dirs::home_dir() {
        let user_app = home.join("Applications/Blender.app/Contents/MacOS/Blender");
        if user_app.exists() {
            return Some(user_app.to_string_lossy().to_string());
        }
    }

    // Homebrew Cask (Intel and Apple Silicon paths)
    let homebrew_paths = [
        "/opt/homebrew/Caskroom/blender", // Apple Silicon
        "/usr/local/Caskroom/blender",    // Intel
    ];

    for base in &homebrew_paths {
        if let Ok(entries) = std::fs::read_dir(base) {
            // Find the latest version directory
            for entry in entries.flatten() {
                let blender_path = entry.path().join("Blender.app/Contents/MacOS/Blender");
                if blender_path.exists() {
                    return Some(blender_path.to_string_lossy().to_string());
                }
            }
        }
    }

    None
}

/// Find Blender on Windows.
///
/// Checks Program Files for versioned Blender installations (4.0 through 5.x).
#[cfg(target_os = "windows")]
fn find_blender_windows() -> Option<String> {
    // Check Program Files locations
    let program_files = [
        std::env::var("ProgramFiles").unwrap_or_else(|_| r"C:\Program Files".to_string()),
        std::env::var("ProgramFiles(x86)")
            .unwrap_or_else(|_| r"C:\Program Files (x86)".to_string()),
    ];

    // Version-agnostic path (some installers use this)
    for pf in &program_files {
        let generic_path = PathBuf::from(pf).join(r"Blender Foundation\Blender\blender.exe");
        if generic_path.exists() {
            return Some(generic_path.to_string_lossy().to_string());
        }
    }

    // Discover versioned installations by scanning the Blender Foundation directory.
    // Windows installers create folders like "Blender 4.2", "Blender 5.0", etc.
    for pf in &program_files {
        let foundation_dir = PathBuf::from(pf).join("Blender Foundation");
        if let Ok(entries) = std::fs::read_dir(&foundation_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with("Blender ") {
                    let exe = entry.path().join("blender.exe");
                    if exe.exists() {
                        return Some(exe.to_string_lossy().to_string());
                    }
                }
            }
        }
    }

    // Check user's local app data (for per-user installations)
    if let Ok(local_app_data) = std::env::var("LOCALAPPDATA") {
        let local_path =
            PathBuf::from(local_app_data).join(r"Blender Foundation\Blender\blender.exe");
        if local_path.exists() {
            return Some(local_path.to_string_lossy().to_string());
        }
    }

    None
}

/// Find Blender on Linux.
///
/// Checks:
/// - System package manager installations (/usr/bin)
/// - Snap packages (/snap/bin)
/// - Flatpak (via flatpak run command)
/// - Common extraction locations
#[cfg(target_os = "linux")]
fn find_blender_linux() -> Option<String> {
    // System package manager (apt, dnf, pacman, etc.)
    let system_paths = ["/usr/bin/blender", "/usr/local/bin/blender"];

    for path in &system_paths {
        if Path::new(path).exists() {
            return Some(path.to_string());
        }
    }

    // Snap installation
    let snap_path = "/snap/bin/blender";
    if Path::new(snap_path).exists() {
        return Some(snap_path.to_string());
    }

    // Flatpak - check if blender is installed via flatpak
    // We return a special command that will be handled differently
    if let Ok(output) = Command::new("flatpak")
        .args(["list", "--app", "--columns=application"])
        .output()
    {
        let stdout = String::from_utf8_lossy(&output.stdout);
        if stdout.contains("org.blender.Blender") {
            // Return the flatpak run command
            return Some("flatpak run org.blender.Blender".to_string());
        }
    }

    // Check common user extraction locations
    if let Some(home) = dirs::home_dir() {
        // ~/blender or ~/bin/blender
        let user_paths = [
            home.join("blender/blender"),
            home.join("bin/blender"),
            home.join(".local/bin/blender"),
        ];

        for path in &user_paths {
            if path.exists() {
                return Some(path.to_string_lossy().to_string());
            }
        }

        // Check for extracted tarballs in common locations (blender-4.x.x-linux-x64)
        let search_dirs = [
            home.join("Downloads"),
            home.join("Applications"),
            home.clone(),
        ];

        for dir in &search_dirs {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().to_lowercase();
                    if name.starts_with("blender-") && entry.path().is_dir() {
                        let blender_exe = entry.path().join("blender");
                        if blender_exe.exists() {
                            return Some(blender_exe.to_string_lossy().to_string());
                        }
                    }
                }
            }
        }
    }

    None
}

/// Check if Blender is available on the system.
pub fn is_blender_available() -> bool {
    find_blender().is_some()
}

/// Run a Blender command, handling special cases like Flatpak.
///
/// The `blender_cmd` may be:
/// - A simple path: `/usr/bin/blender`
/// - A multi-part command: `flatpak run org.blender.Blender`
fn run_blender_command_with_env(
    blender_cmd: &str,
    args: &[&str],
    env_vars: &[(&str, &str)],
) -> std::io::Result<std::process::Output> {
    if blender_cmd.starts_with("flatpak run ") {
        // Flatpak command: split into parts
        let parts: Vec<&str> = blender_cmd.split_whitespace().collect();
        Command::new(parts[0])
            .args(&parts[1..])
            .args(args)
            .envs(env_vars.iter().copied())
            .output()
    } else {
        // Regular executable path
        Command::new(blender_cmd)
            .args(args)
            .envs(env_vars.iter().copied())
            .output()
    }
}

/// Convert a GLB file to FBX using Blender CLI.
///
/// # Arguments
///
/// * `glb_path` - Path to the input GLB file
/// * `custom_blender_path` - Optional custom Blender path (overrides auto-detection)
///
/// # Returns
///
/// A tuple of (FBX path, optional textures directory path), or None if Blender is not available.
pub fn convert_glb_to_fbx(
    glb_path: &Path,
    custom_blender_path: Option<&str>,
) -> Result<Option<(PathBuf, Option<PathBuf>)>> {
    let blender = match custom_blender_path {
        Some(path) if !path.is_empty() => path.to_string(),
        _ => match find_blender() {
            Some(b) => b,
            None => return Ok(None),
        },
    };

    let fbx_path = glb_path.with_extension("fbx");
    let textures_dir = glb_path
        .parent()
        .ok_or_else(|| {
            crate::types::Error::Io(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("GLB path has no parent directory: {}", glb_path.display()),
            ))
        })?
        .join(bundle_files::TEXTURES_DIR);

    // Blender Python script for conversion
    // Paths are passed via environment variables to avoid injection via Python string literals
    let script = r#"
import bpy
import os

# Read paths from environment variables (avoids string injection)
glb_path = os.environ["ASSET_TAP_GLB_PATH"]
textures_dir = os.environ["ASSET_TAP_TEXTURES_DIR"]
fbx_path = os.environ["ASSET_TAP_FBX_PATH"]

# Clear default scene
bpy.ops.wm.read_factory_settings(use_empty=True)

# Import GLB
bpy.ops.import_scene.gltf(filepath=glb_path)

# Create textures directory
os.makedirs(textures_dir, exist_ok=True)

# Configure render settings for PNG output
# save_render() uses these settings to properly convert image formats
bpy.context.scene.render.image_settings.file_format = 'PNG'
bpy.context.scene.render.image_settings.color_mode = 'RGBA'
bpy.context.scene.render.image_settings.compression = 15

# Save all textures to external files with numbered naming
texture_index = 0
for image in bpy.data.images:
    if image.packed_file or image.pixels:
        # Use consistent texture_N naming instead of Blender's internal names
        name = f"texture_{texture_index}.png"
        filepath = os.path.join(textures_dir, name)

        try:
            # Use save_render() which properly converts formats using scene settings
            # This correctly handles WebP, JPEG, and other formats -> PNG conversion
            image.save_render(filepath)
            print(f"Saved texture: {name}")
            texture_index += 1
        except Exception as e:
            print(f"Could not save {name}: {e}")

# NOTE: We intentionally do NOT re-export the GLB here to preserve the original
# asset from the API. The original GLB maintains full integrity of the API output.
# If you need a Blender-processed GLB, you can manually export it via the GUI.

# Export FBX with textures copied to folder
bpy.ops.export_scene.fbx(
    filepath=fbx_path,
    use_selection=False,
    apply_scale_options='FBX_SCALE_ALL',
    path_mode='COPY',
    embed_textures=True,
    bake_space_transform=True,
)
"#;

    let output = run_blender_command_with_env(
        &blender,
        &["--background", "--python-expr", script],
        &[
            ("ASSET_TAP_GLB_PATH", glb_path.to_string_lossy().as_ref()),
            (
                "ASSET_TAP_TEXTURES_DIR",
                textures_dir.to_string_lossy().as_ref(),
            ),
            ("ASSET_TAP_FBX_PATH", fbx_path.to_string_lossy().as_ref()),
        ],
    )
    .map_err(Error::Io)?;

    if output.status.success() && fbx_path.exists() {
        let textures = if textures_dir.exists() && textures_dir.read_dir()?.next().is_some() {
            Some(textures_dir)
        } else {
            // Remove empty textures directory
            let _ = std::fs::remove_dir(&textures_dir);
            None
        };
        Ok(Some((fbx_path, textures)))
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let truncated: String = stderr.chars().take(200).collect();
        Err(Error::Pipeline(format!(
            "Blender conversion failed: {}",
            truncated
        )))
    }
}

/// Convert all GLB files in generation directories to FBX.
///
/// Scans the output directory for timestamped generation directories
/// (e.g., `output/20241229_153045/`) and converts any GLB files to FBX.
///
/// # Arguments
///
/// * `output_dir` - Base output directory containing generation subdirectories
///
/// # Returns
///
/// A tuple of (converted count, skipped count, failed count).
pub fn convert_existing_models(output_dir: &Path) -> Result<(usize, usize, usize)> {
    let mut converted = 0;
    let mut skipped = 0;
    let mut failed = 0;

    if !output_dir.exists() {
        return Ok((0, 0, 0));
    }

    // Scan generation directories
    for entry in std::fs::read_dir(output_dir)? {
        let entry = entry?;
        let path = entry.path();

        // Only process directories (generation directories)
        if !path.is_dir() {
            continue;
        }

        // Scan files in the generation directory
        if let Ok(files) = std::fs::read_dir(&path) {
            for file_entry in files.flatten() {
                let file_path = file_entry.path();

                if file_path.extension().map(|e| e == "glb").unwrap_or(false) {
                    let fbx_path = file_path.with_extension("fbx");

                    if fbx_path.exists() {
                        skipped += 1;
                        continue;
                    }

                    match convert_glb_to_fbx(&file_path, None) {
                        Ok(Some(_)) => converted += 1,
                        Ok(None) => {
                            // Blender not available
                            return Ok((converted, skipped, failed));
                        }
                        Err(_) => failed += 1,
                    }
                }
            }
        }
    }

    Ok((converted, skipped, failed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_blender() {
        // Just test that it doesn't panic
        let _ = find_blender();
    }

    #[test]
    fn test_is_blender_available() {
        // Just test that it doesn't panic
        let _ = is_blender_available();
    }
}
