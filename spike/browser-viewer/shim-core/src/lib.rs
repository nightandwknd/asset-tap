//! SPIKE SHIM. Not a product crate.
//!
//! * `rig` is the REAL `core/src/rig/` tree, unmodified, pulled in by `#[path]`
//!   (so `bind_mesh`, `fit_mesh`, `SkinnedClip`, the packs code - all real).
//! * Everything else in this file is a STAND-IN for a seam to the rest of
//!   `asset-tap-core` that the rig reaches across (settings, bundle helpers,
//!   constants, network fetch). They exist only so the real rig compiles
//!   without `reqwest`/`tokio-full`/`dirs`/native-tls, none of which build for
//!   wasm32 (see spike/browser-viewer/NOTES in docs).
#![allow(dead_code, clippy::all)]

#[path = "../../../../core/src/rig/mod.rs"]
pub mod rig;

pub use rig::{
    BindError, BindMarker, BindOptions, BindReport, BoneGroup, ClipCatalogEntry, HumanBone,
    PACK_MANIFEST, Side, SkinnedClip, apply_clip, baked_clip_names, bind_mesh,
    default_bind_markers, find_clip, fit_mesh, fit_mesh_from_heads, fit_mesh_from_heads_keeping,
    foreign_rig_joints, heads_off_mesh, install_pack_from, is_fitted, list_clips, pack_dir,
    packs_root, preview_skinned_clip, resolve_pack, resolve_packs, seed_bind_markers,
};

/// STAND-IN for `core::settings` (real one uses `dirs`).
pub mod settings {
    use std::path::PathBuf;
    pub fn is_dev_mode() -> bool {
        cfg!(debug_assertions)
    }
    pub fn config_dir() -> PathBuf {
        PathBuf::from(".asset-tap-spike-config")
    }
}

/// STAND-IN for `core::constants::files`.
pub mod constants {
    pub mod files {
        pub const CLIP_PACKS_MANIFEST_URL: &str = "";
        pub const CLIP_PACKS_URL: &str = "";
        pub const CLIP_PACKS_SIZE_LABEL: &str = "15 MB";
        pub mod bundle {
            pub const METADATA: &str = "bundle.json";
            pub const IMAGE: &str = "image.png";
        }
    }
}

/// STAND-IN for the two `core::bundle` helpers pack install uses.
pub mod bundle {
    use std::path::Path;
    pub fn looks_like_bundle(path: &Path) -> bool {
        path.join("bundle.json").exists() || path.join("image.png").exists() || path.join("model.glb").exists()
    }
    pub fn extract_zip_to_dir<R: std::io::Read + std::io::Seek>(
        _archive: &mut zip::ZipArchive<R>,
        _dest: &Path,
    ) -> Result<(), String> {
        Err("spike stand-in: zip extraction not implemented".into())
    }
    pub fn sha256_hex(_d: &[u8]) -> String {
        String::new()
    }
}

/// STAND-IN for `core::release_fetch` (real one is reqwest-based).
pub mod release_fetch {
    pub struct Hashed {
        pub manifest: serde_json::Value,
        pub bytes: Vec<u8>,
    }
    pub async fn download_hashed_zip(
        _manifest_url: &str,
        _archive_url: &str,
        _on_progress: impl Fn(f32) + Send + 'static,
    ) -> anyhow::Result<Hashed> {
        anyhow::bail!("spike stand-in: network fetch not available")
    }
}

/// STAND-IN (verbatim copy of `core/src/lib.rs` `test_support`, test-only) so the REAL rig unit
/// tests, which call `crate::test_support::{env_lock, glb}`, compile in the shim.
#[cfg(test)]
pub mod test_support {
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());
    static TEMPLATES_DIR_LOCK: Mutex<()> = Mutex::new(());

    /// Acquire the process-wide lock guarding `std::env` mutation in tests.
    ///
    /// Hold the returned guard for the duration of any test that sets or
    /// removes environment variables. The guard is poison-tolerant: a panicking
    /// test still releases the lock for the next one.
    pub fn env_lock() -> MutexGuard<'static, ()> {
        ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Acquire the lock guarding the shared user-templates directory in tests.
    ///
    /// `TemplateRegistry::new()` writes the embedded templates into the shared
    /// on-disk templates dir; concurrent writers race on those files. Tests that
    /// construct a registry via `new()` (rather than the isolated
    /// `from_dir(tempdir)`) hold this so they serialize against each other while
    /// the rest of the suite runs in parallel.
    pub fn templates_dir_lock() -> MutexGuard<'static, ()> {
        TEMPLATES_DIR_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// GLB container magic (`glTF`, little-endian `u32`).
    const GLB_MAGIC: &[u8; 4] = b"glTF";
    /// GLB container version. Only 2 exists.
    const GLB_VERSION: u32 = 2;
    /// Chunk type `JSON` (glTF 2.0 §4.4.3).
    const CHUNK_JSON: u32 = 0x4E4F_534A;
    /// Chunk type `BIN\0` (glTF 2.0 §4.4.3).
    const CHUNK_BIN: u32 = 0x004E_4942;
    /// Byte alignment every GLB chunk is padded to.
    const CHUNK_ALIGN: usize = 4;

    /// Wrap a glTF JSON document, and optionally a binary buffer, in a GLB
    /// container.
    ///
    /// Seven test modules were hand-rolling this header, each with its own
    /// spelling of the chunk magics; a transposed digit there fails as
    /// "not a glTF file" a long way from the code under test. Both chunks are
    /// padded per spec: JSON with spaces, BIN with zeroes.
    ///
    /// Test-only. Production GLB writing lives in `rig::write::pack_glb`,
    /// which has to preserve an existing document's chunks rather than build
    /// a container from scratch.
    pub fn glb(json: &[u8], bin: Option<&[u8]>) -> Vec<u8> {
        fn pad(mut chunk: Vec<u8>, fill: u8) -> Vec<u8> {
            while !chunk.len().is_multiple_of(CHUNK_ALIGN) {
                chunk.push(fill);
            }
            chunk
        }

        let json = pad(json.to_vec(), b' ');
        let bin = bin.map(|b| pad(b.to_vec(), 0));

        let mut total = 12 + 8 + json.len();
        if let Some(b) = &bin {
            total += 8 + b.len();
        }

        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(GLB_MAGIC);
        out.extend_from_slice(&GLB_VERSION.to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&(json.len() as u32).to_le_bytes());
        out.extend_from_slice(&CHUNK_JSON.to_le_bytes());
        out.extend_from_slice(&json);
        if let Some(b) = &bin {
            out.extend_from_slice(&(b.len() as u32).to_le_bytes());
            out.extend_from_slice(&CHUNK_BIN.to_le_bytes());
            out.extend_from_slice(b);
        }
        debug_assert_eq!(out.len(), total, "GLB total length must match the header");
        out
    }
}
