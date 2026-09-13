//! On-demand install of the free Standard clip packs.
//!
//! The GLBs are **not** compiled into the binary. Release attaches
//! `clip-packs.zip` (hash in `clip-packs-manifest.json`); the GUI/CLI pull
//! that archive the same way they pull the demo bundle. A local directory
//! (`ASSET_TAP_CLIP_PACKS_DIR`, or `packs/` next to a debug source tree)
//! skips the network so tests and `make dev` do not need a published release.
//! Pack ids and `packs_version` are single-sourced from `packs/manifest.json`.

use super::pack::{ClipPack, ClipPackError, install_pack_from_inner, resolve_packs};
use crate::bundle::extract_zip_to_dir;
use crate::constants::files::{CLIP_PACKS_MANIFEST_URL, CLIP_PACKS_SIZE_LABEL, CLIP_PACKS_URL};
use crate::release_fetch::download_hashed_zip;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tracing::info;

/// Override the clip-pack manifest URL (tests / a non-GitHub mirror).
pub const CLIP_PACKS_MANIFEST_URL_ENV: &str = "ASSET_TAP_CLIP_PACKS_MANIFEST_URL";
/// Override the clip-pack archive URL.
pub const CLIP_PACKS_URL_ENV: &str = "ASSET_TAP_CLIP_PACKS_URL";

/// Directory of already-trimmed Standard libraries (`ual1/pack.glb`, …).
/// Mirrors `ASSET_TAP_CLIPS_DIR`: tests and a debug checkout can
/// point at [`packs/`](https://github.com/nightandwknd/asset-tap/tree/main/packs)
/// instead of GitHub Releases.
pub const CLIP_PACKS_DIR_ENV: &str = "ASSET_TAP_CLIP_PACKS_DIR";

const SHIPPED_MANIFEST_JSON: &str = include_str!("../../../packs/manifest.json");

#[derive(Debug, serde::Deserialize)]
struct ShippedManifest {
    packs_version: u32,
    packs: Vec<String>,
    size_label: String,
}

fn shipped_manifest() -> &'static ShippedManifest {
    static M: OnceLock<ShippedManifest> = OnceLock::new();
    M.get_or_init(|| {
        let m: ShippedManifest = serde_json::from_str(SHIPPED_MANIFEST_JSON)
            .expect("packs/manifest.json must deserialize");
        assert_eq!(
            m.size_label, CLIP_PACKS_SIZE_LABEL,
            "packs/manifest.json size_label must match CLIP_PACKS_SIZE_LABEL"
        );
        m
    })
}

/// Pack ids the release archive ships. Single-sourced from `packs/manifest.json`.
pub fn release_pack_ids() -> &'static [String] {
    &shipped_manifest().packs
}

/// `packs_version` from `packs/manifest.json`.
pub fn shipped_packs_version() -> u32 {
    shipped_manifest().packs_version
}

/// Result of [`download_clip_packs`].
#[derive(Debug)]
pub enum ClipPacksDownloadResult {
    /// One or more missing (or `--force` refresh) release packs were installed.
    Downloaded {
        installed: Vec<String>,
        version: u32,
    },
    /// Every release pack id is already present locally (and `--force` has
    /// nothing stamped it is allowed to replace).
    AlreadyExists { version: u32 },
}

/// Release-pack ids that are not installed yet. Existing packs (including a
/// user-installed Source upgrade) are left alone.
pub fn missing_release_packs() -> Vec<String> {
    packs_needing_install(false)
}

/// Packs we should write: missing ids, plus (when `force`) existing packs
/// that carry a `release_version` stamp. Never includes a pack installed via
/// `clip install` (`release_version` is unset).
pub fn packs_needing_install(force: bool) -> Vec<String> {
    let have = resolve_packs();
    release_pack_ids()
        .iter()
        .filter(|id| match have.iter().find(|p| p.id == **id) {
            None => true,
            Some(p) => force && p.release_version.is_some(),
        })
        .cloned()
        .collect()
}

fn installed_release_version() -> u32 {
    resolve_packs()
        .into_iter()
        .filter_map(|p| p.release_version)
        .max()
        .unwrap_or(0)
}

/// Install any missing release packs from a directory of `ual1/`, `ual2/`, …
pub fn install_missing_release_packs_from(root: &Path) -> Result<Vec<ClipPack>, ClipPackError> {
    install_release_packs_from(root, false, shipped_packs_version())
}

pub(crate) fn install_release_packs_from(
    root: &Path,
    force: bool,
    release_version: u32,
) -> Result<Vec<ClipPack>, ClipPackError> {
    let mut installed = Vec::new();
    for id in packs_needing_install(force) {
        let dir = root.join(&id);
        if !dir.is_dir() {
            return Err(ClipPackError::NoLibrary(dir));
        }
        installed.push(install_pack_from_inner(
            &dir,
            Some(&id),
            Some(release_version),
        )?);
    }
    Ok(installed)
}

/// Fetch the free Standard packs (or install them from a local shipped dir).
///
/// Existing pack ids are never replaced unless `force` is set, and `--force`
/// only refreshes packs stamped by a previous `clip download`. A Source
/// install (`release_version` unset) stays Source.
/// The `on_progress` callback matches [`crate::download_demo_bundle`].
pub async fn download_clip_packs(
    force: bool,
    on_progress: impl Fn(f32) + Send + 'static,
) -> anyhow::Result<ClipPacksDownloadResult> {
    if packs_needing_install(force).is_empty() {
        return Ok(ClipPacksDownloadResult::AlreadyExists {
            version: installed_release_version(),
        });
    }

    if let Some(dir) = local_shipped_packs_dir() {
        info!("Installing clip packs from {}", dir.display());
        let version = shipped_packs_version();
        let installed = install_release_packs_from(&dir, force, version)?;
        return Ok(ClipPacksDownloadResult::Downloaded {
            installed: installed.into_iter().map(|p| p.id).collect(),
            version,
        });
    }

    fetch_and_install(force, on_progress).await
}

fn env_url(var: &str, default: &str) -> String {
    std::env::var(var)
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default.to_string())
}

fn clip_pack_urls() -> (String, String) {
    (
        env_url(CLIP_PACKS_MANIFEST_URL_ENV, CLIP_PACKS_MANIFEST_URL),
        env_url(CLIP_PACKS_URL_ENV, CLIP_PACKS_URL),
    )
}

fn urls_overridden() -> bool {
    [CLIP_PACKS_MANIFEST_URL_ENV, CLIP_PACKS_URL_ENV]
        .iter()
        .any(|var| std::env::var(var).ok().is_some_and(|s| !s.is_empty()))
}

fn local_shipped_packs_dir() -> Option<PathBuf> {
    // A URL override means the caller wants the network path (tests).
    if urls_overridden() {
        return None;
    }
    if let Ok(raw) = std::env::var(CLIP_PACKS_DIR_ENV)
        && !raw.is_empty()
    {
        let dir = PathBuf::from(raw);
        if shipped_dir_is_complete(&dir) {
            return Some(dir);
        }
    }
    if cfg!(debug_assertions) {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../packs");
        if shipped_dir_is_complete(&dir) {
            return dir.canonicalize().ok();
        }
    }
    None
}

fn shipped_dir_is_complete(dir: &Path) -> bool {
    release_pack_ids().iter().all(|id| {
        dir.join(id).join("pack.glb").is_file() || dir.join(id).join("pack.gltf").is_file()
    })
}

async fn fetch_and_install(
    force: bool,
    on_progress: impl Fn(f32) + Send + 'static,
) -> anyhow::Result<ClipPacksDownloadResult> {
    let (manifest_url, archive_url) = clip_pack_urls();
    let hashed = download_hashed_zip(&manifest_url, &archive_url, on_progress).await?;
    let version = hashed
        .manifest
        .get("packs_version")
        .and_then(|v| v.as_u64())
        .map(|v| v as u32)
        .unwrap_or(0);
    let bytes = hashed.bytes;

    let installed = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<String>> {
        let tmp = tempfile::tempdir()?;
        let cursor = std::io::Cursor::new(bytes);
        let mut archive = zip::ZipArchive::new(cursor)?;
        extract_zip_to_dir(&mut archive, tmp.path()).map_err(|e| anyhow::anyhow!("{e}"))?;
        let packs = install_release_packs_from(tmp.path(), force, version)?;
        Ok(packs.into_iter().map(|p| p.id).collect())
    })
    .await??;

    Ok(ClipPacksDownloadResult::Downloaded { installed, version })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rig::canon::armature;
    use crate::rig::export::{AnimChannel, AnimData};
    use crate::rig::pack::{CLIPS_DIR_ENV, install_pack_from};
    use crate::rig::write::write_animation_glb;

    fn glb_with_animation() -> Vec<u8> {
        let arm = armature();
        let hips = arm.name_to_index["hips"];
        let anim = AnimData {
            channels: vec![AnimChannel {
                node: hips,
                path: "translation",
                times: vec![0.0, 1.0],
                values: vec![0.0; 6],
                interpolation: "LINEAR",
            }],
        };
        write_animation_glb(&arm, &anim, "Walk_Loop").unwrap()
    }

    fn write_shipped(root: &Path, id: &str) {
        let dir = root.join(id);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("pack.glb"), glb_with_animation()).unwrap();
    }

    #[test]
    fn shipped_manifest_matches_constants() {
        let m = shipped_manifest();
        assert_eq!(m.size_label, CLIP_PACKS_SIZE_LABEL);
        assert!(!m.packs.is_empty());
        assert_eq!(release_pack_ids(), m.packs.as_slice());
        assert_eq!(shipped_packs_version(), m.packs_version);
    }

    #[test]
    fn missing_packs_skip_ids_already_installed() {
        let _guard = crate::test_support::env_lock();
        let home = tempfile::tempdir().unwrap();
        // SAFETY: serialized by `env_lock`.
        unsafe { std::env::set_var(CLIPS_DIR_ENV, home.path()) };

        let shipped = tempfile::tempdir().unwrap();
        write_shipped(shipped.path(), "ual1");
        write_shipped(shipped.path(), "ual2");
        install_pack_from(&shipped.path().join("ual1"), Some("ual1")).unwrap();

        assert_eq!(missing_release_packs(), vec!["ual2".to_string()]);
        let got = install_missing_release_packs_from(shipped.path()).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id, "ual2");
        assert_eq!(got[0].release_version, Some(shipped_packs_version()));
        assert!(missing_release_packs().is_empty());
    }

    #[test]
    fn already_installed_source_is_not_replaced() {
        let _guard = crate::test_support::env_lock();
        let home = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var(CLIPS_DIR_ENV, home.path()) };

        let shipped = tempfile::tempdir().unwrap();
        write_shipped(shipped.path(), "ual1");
        write_shipped(shipped.path(), "ual2");
        let first = install_pack_from(&shipped.path().join("ual1"), Some("ual1")).unwrap();
        let first_len = std::fs::metadata(&first.gltf_path).unwrap().len();

        // A second "upgrade" zip would be a different file; we must not copy it.
        std::fs::write(shipped.path().join("ual1/pack.glb"), [0u8; 8]).unwrap();
        let added = install_missing_release_packs_from(shipped.path()).unwrap();
        assert_eq!(
            added.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            vec!["ual2"]
        );
        let still = resolve_packs()
            .into_iter()
            .find(|p| p.id == "ual1")
            .unwrap();
        assert_eq!(
            std::fs::metadata(&still.gltf_path).unwrap().len(),
            first_len,
            "existing pack must not be overwritten"
        );
        assert_eq!(still.release_version, None);
    }

    #[test]
    fn force_does_not_replace_unstamped_source() {
        let _guard = crate::test_support::env_lock();
        let home = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var(CLIPS_DIR_ENV, home.path()) };

        let shipped = tempfile::tempdir().unwrap();
        write_shipped(shipped.path(), "ual1");
        write_shipped(shipped.path(), "ual2");
        let first = install_pack_from(&shipped.path().join("ual1"), Some("ual1")).unwrap();
        let first_len = std::fs::metadata(&first.gltf_path).unwrap().len();
        std::fs::write(shipped.path().join("ual1/pack.glb"), [0u8; 8]).unwrap();

        let added = install_release_packs_from(shipped.path(), true, 1).unwrap();
        assert_eq!(
            added.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            vec!["ual2"],
            "--force must not replace a clip-install / Source pack"
        );
        let still = resolve_packs()
            .into_iter()
            .find(|p| p.id == "ual1")
            .unwrap();
        assert_eq!(
            std::fs::metadata(&still.gltf_path).unwrap().len(),
            first_len
        );
        assert_eq!(still.release_version, None);
    }

    #[test]
    fn force_replaces_stamped_standard() {
        let _guard = crate::test_support::env_lock();
        let home = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var(CLIPS_DIR_ENV, home.path()) };

        let shipped = tempfile::tempdir().unwrap();
        write_shipped(shipped.path(), "ual1");
        write_shipped(shipped.path(), "ual2");
        let first = install_release_packs_from(shipped.path(), false, 1).unwrap();
        assert_eq!(first.len(), 2);
        assert!(first.iter().all(|p| p.release_version == Some(1)));

        let replacement = glb_with_animation();
        std::fs::write(shipped.path().join("ual1/pack.glb"), &replacement).unwrap();
        let refreshed = install_release_packs_from(shipped.path(), true, 2).unwrap();
        assert_eq!(refreshed.len(), 2);
        let ual1 = resolve_packs()
            .into_iter()
            .find(|p| p.id == "ual1")
            .unwrap();
        assert_eq!(ual1.release_version, Some(2));
        assert_eq!(
            std::fs::metadata(&ual1.gltf_path).unwrap().len(),
            replacement.len() as u64
        );
    }
}

#[cfg(all(test, feature = "mock"))]
mod fetch_tests {
    use super::*;
    use crate::bundle::sha256_hex;
    use crate::rig::canon::armature;
    use crate::rig::export::{AnimChannel, AnimData};
    use crate::rig::pack::CLIPS_DIR_ENV;
    use crate::rig::write::write_animation_glb;
    use std::io::Write;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn glb_with_animation() -> Vec<u8> {
        let arm = armature();
        let hips = arm.name_to_index["hips"];
        let anim = AnimData {
            channels: vec![AnimChannel {
                node: hips,
                path: "translation",
                times: vec![0.0, 1.0],
                values: vec![0.0; 6],
                interpolation: "LINEAR",
            }],
        };
        write_animation_glb(&arm, &anim, "Walk_Loop").unwrap()
    }

    fn zip_release_packs() -> Vec<u8> {
        let glb = glb_with_animation();
        let mut buf = std::io::Cursor::new(Vec::new());
        {
            let mut zip = zip::ZipWriter::new(&mut buf);
            let options = zip::write::SimpleFileOptions::default();
            for id in ["ual1", "ual2"] {
                zip.start_file::<_, ()>(format!("{id}/pack.glb"), options)
                    .unwrap();
                zip.write_all(&glb).unwrap();
            }
            zip.finish().unwrap();
        }
        buf.into_inner()
    }

    #[tokio::test]
    #[allow(clippy::await_holding_lock)] // env_lock must span the whole test
    async fn hashed_release_installs_and_stamps() {
        let _guard = crate::test_support::env_lock();
        struct ClearUrls;
        impl Drop for ClearUrls {
            fn drop(&mut self) {
                unsafe {
                    std::env::remove_var(CLIP_PACKS_MANIFEST_URL_ENV);
                    std::env::remove_var(CLIP_PACKS_URL_ENV);
                }
            }
        }
        let _clear_urls = ClearUrls;
        let home = tempfile::tempdir().unwrap();
        unsafe {
            std::env::set_var(CLIPS_DIR_ENV, home.path());
            std::env::remove_var(CLIP_PACKS_DIR_ENV);
        }

        let zip = zip_release_packs();
        let hash = sha256_hex(&zip);
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/clip-packs-manifest.json"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "sha256": hash,
                "packs_version": 9,
                "packs": ["ual1", "ual2"],
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/clip-packs.zip"))
            .respond_with(ResponseTemplate::new(200).set_body_bytes(zip))
            .mount(&server)
            .await;

        unsafe {
            std::env::set_var(
                CLIP_PACKS_MANIFEST_URL_ENV,
                format!("{}/clip-packs-manifest.json", server.uri()),
            );
            std::env::set_var(
                CLIP_PACKS_URL_ENV,
                format!("{}/clip-packs.zip", server.uri()),
            );
        }

        let result = download_clip_packs(false, |_| {}).await.unwrap();
        match result {
            ClipPacksDownloadResult::Downloaded { installed, version } => {
                assert_eq!(version, 9);
                assert_eq!(installed, vec!["ual1".to_string(), "ual2".to_string()]);
            }
            other => panic!("expected download, got {other:?}"),
        }
        let packs = resolve_packs();
        assert!(packs.iter().all(|p| p.release_version == Some(9)));

        let again = download_clip_packs(false, |_| {}).await.unwrap();
        match again {
            ClipPacksDownloadResult::AlreadyExists { version } => assert_eq!(version, 9),
            other => panic!("expected already-exists, got {other:?}"),
        }
    }
}
