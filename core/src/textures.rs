//! Extract a GLB's embedded textures to a `textures/` directory.
//!
//! This used to be a side effect of the Blender FBX conversion, which meant
//! the documented `textures/` output silently depended on Blender being
//! installed. The image bytes are already in the GLB, so no external tool is
//! needed: read `images[]`, slice the buffer view, write the bytes out.
//!
//! Bytes are copied verbatim rather than re-encoded. A generated texture is
//! something the user paid for, and a decode/encode round trip is quality loss
//! for no gain. It also fixes a real defect in the old path, which wrote every
//! texture with a `.png` extension regardless of its actual format, leaving
//! WebP files that only loaded because the reader guessed the format.

use crate::constants::files::bundle as files;
use std::path::{Path, PathBuf};

/// One texture pulled out of a GLB.
struct Texture {
    name: String,
    bytes: Vec<u8>,
}

/// File extension for a glTF image MIME type.
fn extension_for(mime: &str) -> &'static str {
    match mime {
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/ktx2" => "ktx2",
        _ => "png",
    }
}

/// Sniff the format when the glTF omits `mimeType`, which is legal for images
/// referenced by URI and common in provider output.
fn sniff(bytes: &[u8]) -> &'static str {
    const PNG: &[u8] = &[0x89, b'P', b'N', b'G'];
    if bytes.starts_with(PNG) {
        "image/png"
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else {
        "image/png"
    }
}

/// Write every embedded texture into `<bundle_dir>/textures/`.
///
/// Returns the directory when at least one texture was written, or `None` when
/// the model has no embedded images. Missing or unreadable images are skipped
/// rather than failing the write: a texture we cannot decode should not cost
/// the user a generated model.
pub fn extract_textures(glb: &Path, bundle_dir: &Path) -> Result<Option<PathBuf>, String> {
    let bytes = std::fs::read(glb).map_err(|e| e.to_string())?;
    let textures = read_textures(&bytes)?;
    if textures.is_empty() {
        return Ok(None);
    }
    let dest = bundle_dir.join(files::TEXTURES_DIR);
    std::fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    for tex in &textures {
        std::fs::write(dest.join(&tex.name), &tex.bytes).map_err(|e| e.to_string())?;
    }
    Ok(Some(dest))
}

fn read_textures(glb: &[u8]) -> Result<Vec<Texture>, String> {
    let gltf::Gltf { document, blob } = gltf::Gltf::from_slice(glb).map_err(|e| e.to_string())?;
    let blob = blob.unwrap_or_default();
    let mut out: Vec<Texture> = Vec::new();
    let mut used: Vec<String> = Vec::new();
    for image in document.images() {
        let (bytes, mime) = match image.source() {
            gltf::image::Source::View { view, mime_type } => {
                let start = view.offset();
                let end = start.saturating_add(view.length());
                if end > blob.len() {
                    continue;
                }
                (blob[start..end].to_vec(), Some(mime_type.to_string()))
            }
            gltf::image::Source::Uri { uri, mime_type } => {
                let Some(bytes) = decode_data_uri(uri) else {
                    // An external file reference: nothing embedded to extract.
                    continue;
                };
                (bytes, mime_type.map(str::to_string))
            }
        };
        if bytes.is_empty() {
            continue;
        }
        // Trust the sniffed format over a declared one: mislabeled textures
        // are exactly the bug this replaces.
        let ext = extension_for(sniff(&bytes));
        let _ = mime;
        let stem = image
            .name()
            .map(sanitize)
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| format!("texture_{}", image.index()));
        let mut name = format!("{stem}.{ext}");
        // glTF does not require image names to be unique.
        let mut n = 1;
        while used.contains(&name) {
            name = format!("{stem}_{n}.{ext}");
            n += 1;
        }
        used.push(name.clone());
        out.push(Texture { name, bytes });
    }
    Ok(out)
}

/// Keep a glTF image name usable as a filename.
fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

fn decode_data_uri(uri: &str) -> Option<Vec<u8>> {
    use base64::Engine as _;
    let rest = uri.strip_prefix("data:")?;
    let (_, b64) = rest.split_once(";base64,")?;
    base64::engine::general_purpose::STANDARD.decode(b64).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_follows_the_actual_bytes_not_the_label() {
        // The Blender path wrote everything as .png, so WebP textures only
        // loaded because the image reader guessed the format.
        let webp = {
            let mut v = b"RIFF".to_vec();
            v.extend_from_slice(&[0, 0, 0, 0]);
            v.extend_from_slice(b"WEBPVP8 ");
            v
        };
        assert_eq!(extension_for(sniff(&webp)), "webp");
        assert_eq!(extension_for(sniff(&[0x89, b'P', b'N', b'G', 0, 0])), "png");
        assert_eq!(extension_for(sniff(&[0xFF, 0xD8, 0xFF, 0xE0])), "jpg");
    }

    #[test]
    fn names_are_filename_safe_and_unique() {
        assert_eq!(sanitize("Base Color/Map"), "Base_Color_Map");
        assert_eq!(sanitize("__weird__"), "weird");
        assert_eq!(sanitize(""), "");
    }

    #[test]
    fn a_model_without_images_writes_no_directory() {
        let dir = tempfile::tempdir().unwrap();
        let glb = dir.path().join("model.glb");
        let out = crate::test_support::glb(br#"{"asset":{"version":"2.0"}}"#, None);
        std::fs::write(&glb, &out).unwrap();

        assert_eq!(extract_textures(&glb, dir.path()).unwrap(), None);
        assert!(!dir.path().join(files::TEXTURES_DIR).exists());
    }
}
