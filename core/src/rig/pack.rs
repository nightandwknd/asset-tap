//! Clip packs: animation libraries installed on disk.
//!
//! A pack supplies **clips only**. The skeleton is embedded (see
//! [`crate::rig::canon`]), so rigging never requires a pack and installing one
//! only adds animations. Packs are discovered by scanning [`packs_root`], so
//! several can be installed side by side and their catalogs merge.
//!
//! The libraries we document are Quaternius's CC0
//! [Universal Animation Library](https://quaternius.com/packs/universalanimationlibrary.html)
//! (`ual1`) and
//! [Universal Animation Library 2](https://quaternius.com/packs/universalanimationlibrary2.html)
//! (`ual2`). The free Standard libraries also ship as a hashed release
//! artifact (`clip download`); paid Source zips install through the same
//! local path as any other download.
//!
//! Layout, one directory per pack:
//!
//! ```text
//! <packs_root>/ual1/pack.glb    the animation library
//! <packs_root>/ual1/pack.json   id, display name, and the clip names
//! ```
//!
//! The manifest exists so [`list_clips`] stays cheap: the GUI calls it while
//! building a combo box, and re-parsing multi-megabyte GLBs per frame is not
//! an option. It is written at install time and rebuilt if missing.

use crate::settings::{config_dir, is_dev_mode};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Default pack id, and the provenance value recorded for older bundles.
pub const CLIP_PACK_ID: &str = "ual1";

/// [Quaternius](https://quaternius.com) Universal Animation Library (CC0).
pub const UAL1_PAGE: &str = "https://quaternius.com/packs/universalanimationlibrary.html";
/// [Quaternius](https://quaternius.com) Universal Animation Library 2 (CC0).
pub const UAL2_PAGE: &str = "https://quaternius.com/packs/universalanimationlibrary2.html";

/// The clip a bind falls back to when the caller names none.
///
/// An alias rather than a pack-specific name, so it resolves through the
/// alias table against whichever library happens to be installed.
pub const DEFAULT_BIND_CLIP: &str = "walk";

/// Cached pack description written beside the GLB.
const PACK_MANIFEST: &str = "pack.json";
/// Normalised pack GLB name inside an installed pack directory.
const PACK_MODEL: &str = "pack.glb";

/// An installed pack: where its animations live and what they are called.
#[derive(Debug, Clone)]
pub struct ClipPack {
    pub id: String,
    pub name: String,
    /// The glTF/GLB holding the animations.
    pub gltf_path: PathBuf,
    /// Animation names, from the manifest or read from the file.
    pub clips: Vec<String>,
    /// Set when this pack was installed by `clip download` from our release.
    /// User/`clip install` packs leave this unset so `--force` will not replace them.
    pub release_version: Option<u32>,
}

impl ClipPack {
    /// Load a pack from a directory, preferring its cached manifest.
    pub fn from_dir(dir: &Path) -> Result<Self, ClipPackError> {
        if let Some(pack) = Manifest::load(dir) {
            return Ok(pack);
        }
        let model =
            pick_pack_model(dir).ok_or_else(|| ClipPackError::Missing(dir.to_path_buf()))?;
        Self::from_model(&model, None)
    }

    /// Load a pack straight from a glTF/GLB, reading its animation names.
    pub fn from_model(model: &Path, id: Option<&str>) -> Result<Self, ClipPackError> {
        let clips = animation_names(model)?;
        if clips.is_empty() {
            return Err(ClipPackError::NoAnimations(model.to_path_buf()));
        }
        let id = id.map(str::to_string).unwrap_or_else(|| derive_id(model));
        Ok(Self {
            name: display_name(&id),
            id,
            gltf_path: model.to_path_buf(),
            clips,
            release_version: None,
        })
    }

    /// The animation matching `requested`: exact name first, then aliases in
    /// preference order.
    ///
    /// Alias order is authoritative, not the pack's animation order — `run`
    /// must mean the same clip regardless of how a library happens to sort.
    pub fn find_clip(&self, requested: &str) -> Option<&str> {
        let exact = self
            .clips
            .iter()
            .find(|c| c.eq_ignore_ascii_case(requested));
        if let Some(c) = exact {
            return Some(c.as_str());
        }
        clip_aliases(requested)
            .iter()
            .find_map(|alias| self.clips.iter().find(|c| c.eq_ignore_ascii_case(alias)))
            .map(String::as_str)
    }
}

/// Serialised form of [`ClipPack`].
#[derive(Serialize, Deserialize)]
struct Manifest {
    id: String,
    name: String,
    file: String,
    clips: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    release_version: Option<u32>,
}

impl Manifest {
    fn load(dir: &Path) -> Option<ClipPack> {
        let raw = fs::read_to_string(dir.join(PACK_MANIFEST)).ok()?;
        let m: Manifest = serde_json::from_str(&raw).ok()?;
        let gltf_path = dir.join(&m.file);
        if !gltf_path.is_file() || m.clips.is_empty() {
            return None;
        }
        Some(ClipPack {
            id: m.id,
            name: m.name,
            gltf_path,
            clips: m.clips,
            release_version: m.release_version,
        })
    }

    fn write(pack: &ClipPack, dir: &Path) -> Result<(), ClipPackError> {
        let file = pack
            .gltf_path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| PACK_MODEL.into());
        let m = Manifest {
            id: pack.id.clone(),
            name: pack.name.clone(),
            file,
            clips: pack.clips.clone(),
            release_version: pack.release_version,
        };
        let json =
            serde_json::to_string_pretty(&m).map_err(|e| ClipPackError::Install(e.to_string()))?;
        fs::write(dir.join(PACK_MANIFEST), json).map_err(|e| ClipPackError::Install(e.to_string()))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ClipPackError {
    #[error(
        "no animation pack in {}. Add one with `asset-tap clip download` or `clip install --from DIR`",
        .0.display()
    )]
    Missing(PathBuf),
    #[error("{} has no animations", .0.display())]
    NoAnimations(PathBuf),
    #[error(
        "no animation library in {}. Point at a Quaternius download: the .zip, \
         the folder you extracted it to, or the .glb itself",
        .0.display()
    )]
    NoLibrary(PathBuf),
    #[error("clip pack install failed: {0}")]
    Install(String),
    #[error("clip '{0}' is not in any installed pack")]
    UnknownClip(String),
}

/// Environment override for [`packs_root`], mirroring `ASSET_TAP_MOCK_DEMO_DIR`.
/// Lets a pack library live outside the config dir, and lets tests install
/// without touching a real one.
pub const CLIPS_DIR_ENV: &str = "ASSET_TAP_CLIPS_DIR";

/// Where packs live: `$ASSET_TAP_CLIPS_DIR`, else `.dev/clips` in dev, else the
/// config dir.
pub fn packs_root() -> PathBuf {
    if let Ok(dir) = std::env::var(CLIPS_DIR_ENV)
        && !dir.is_empty()
    {
        return PathBuf::from(dir);
    }
    if is_dev_mode() {
        PathBuf::from(".dev/clips")
    } else {
        config_dir().join("clips")
    }
}

/// Directory for one pack id.
pub fn pack_dir(id: &str) -> PathBuf {
    packs_root().join(id)
}

/// Every installed pack, in stable id order.
///
/// `ASSET_TAP_CLIP_PACK` overrides discovery entirely and may point at either
/// a pack directory or a glTF/GLB.
pub fn resolve_packs() -> Vec<ClipPack> {
    if let Ok(raw) = std::env::var("ASSET_TAP_CLIP_PACK") {
        let path = PathBuf::from(raw);
        let pack = if path.is_file() {
            ClipPack::from_model(&path, None)
        } else {
            ClipPack::from_dir(&path)
        };
        return pack.into_iter().collect();
    }
    let Ok(entries) = packs_root().read_dir() else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs.iter()
        .filter_map(|d| ClipPack::from_dir(d).ok())
        .collect()
}

/// The first installed pack.
pub fn resolve_pack() -> Result<ClipPack, ClipPackError> {
    resolve_packs()
        .into_iter()
        .next()
        .ok_or_else(|| ClipPackError::Missing(packs_root()))
}

/// The pack holding `clip`, with the animation name it resolved to.
pub fn find_clip(clip: &str) -> Result<(ClipPack, String), ClipPackError> {
    for pack in resolve_packs() {
        if let Some(name) = pack.find_clip(clip) {
            let name = name.to_string();
            return Ok((pack, name));
        }
    }
    Err(ClipPackError::UnknownClip(clip.to_string()))
}

/// Install a Quaternius download as a pack.
///
/// `src` may be the `.zip`, the folder it was extracted to, or a glTF/GLB
/// directly. Anything but a single glTF is searched for the animation library
/// by picking the file with the most animations, which is what skips the
/// mannequin meshes shipped alongside it. `_RM` (root motion) variants are
/// ignored: they translate the root, which walks the model out of the
/// viewport.
///
/// Installing is **additive across packs and replacing within one**: the id is
/// derived without the distribution tier, so adding UAL2 beside UAL1 gives you
/// both catalogs, while upgrading UAL1 Standard to Source overwrites that
/// pack instead of duplicating all of its clips.
pub fn install_pack_from(src: &Path, id: Option<&str>) -> Result<ClipPack, ClipPackError> {
    install_pack_from_inner(src, id, None)
}

pub(crate) fn install_pack_from_inner(
    src: &Path,
    id: Option<&str>,
    release_version: Option<u32>,
) -> Result<ClipPack, ClipPackError> {
    let src = src
        .canonicalize()
        .map_err(|e| ClipPackError::Install(e.to_string()))?;
    // Every Quaternius download is a zip. Requiring the user to extract it
    // first, then find the right subfolder, is work the installer can do.
    let staged = if is_zip(&src) {
        Some(unzip_to_temp(&src)?)
    } else {
        None
    };
    let root = staged
        .as_ref()
        .map(|d| d.path().to_path_buf())
        .unwrap_or_else(|| src.clone());
    let model = if root.is_file() {
        root.clone()
    } else {
        pick_pack_model(&root).ok_or_else(|| ClipPackError::NoLibrary(src.clone()))?
    };
    let mut pack = ClipPack::from_model(&model, id)?;
    pack.release_version = release_version;

    let dir = pack_dir(&pack.id);
    fs::create_dir_all(&dir).map_err(|e| ClipPackError::Install(e.to_string()))?;
    let ext = model
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("glb")
        .to_ascii_lowercase();
    let dest = dir.join(format!("pack.{ext}"));
    fs::copy(&model, &dest).map_err(|e| ClipPackError::Install(e.to_string()))?;
    // A .gltf keeps its buffers in a sidecar .bin; a .glb is self-contained.
    if ext == "gltf"
        && let Some(stem) = model.file_stem()
        && let Some(parent) = model.parent()
    {
        let bin = parent.join(format!("{}.bin", stem.to_string_lossy()));
        if bin.is_file() {
            fs::copy(&bin, dir.join("pack.bin"))
                .map_err(|e| ClipPackError::Install(e.to_string()))?;
        }
    }

    let installed = ClipPack {
        gltf_path: dest,
        ..pack
    };
    Manifest::write(&installed, &dir)?;
    Ok(installed)
}

/// One row in the merged clip catalog.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipCatalogEntry {
    /// The animation name, which is also how a clip is requested.
    pub id: String,
    pub name: String,
    pub pack_id: String,
    pub pack_name: String,
}

/// Every clip across every installed pack.
///
/// Only real, installed animations appear — there is no "missing" row, because
/// a clip exists exactly when the pack providing it is installed. Packs are
/// merged in [`resolve_packs`] order and the first to claim a name wins, which
/// matters only for `A_TPose` (the sole name both Quaternius libraries share).
pub fn list_clips() -> Vec<ClipCatalogEntry> {
    let mut out: Vec<ClipCatalogEntry> = Vec::new();
    for pack in resolve_packs() {
        for clip in &pack.clips {
            if out.iter().any(|e| e.id.eq_ignore_ascii_case(clip)) {
                continue;
            }
            out.push(ClipCatalogEntry {
                id: clip.clone(),
                name: pretty_clip_name(clip),
                pack_id: pack.id.clone(),
                pack_name: pack.name.clone(),
            });
        }
    }
    out
}

/// Pick the animation library from a download tree: the glTF/GLB with the most
/// animations, ignoring root-motion variants.
fn pick_pack_model(dir: &Path) -> Option<PathBuf> {
    let mut best: Option<(usize, PathBuf)> = None;
    for path in gltf_candidates(dir, 0) {
        let Ok(names) = animation_names(&path) else {
            continue;
        };
        if names.is_empty() {
            continue;
        }
        if best.as_ref().is_none_or(|(n, _)| names.len() > *n) {
            best = Some((names.len(), path));
        }
    }
    best.map(|(_, p)| p)
}

/// glTF/GLB files under `dir`, skipping `_RM` root-motion variants.
fn gltf_candidates(dir: &Path, depth: usize) -> Vec<PathBuf> {
    const MAX_DEPTH: usize = 3;
    let mut out = Vec::new();
    let Ok(entries) = dir.read_dir() else {
        return out;
    };
    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            if depth < MAX_DEPTH {
                out.extend(gltf_candidates(&path, depth + 1));
            }
            continue;
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if ext != "glb" && ext != "gltf" {
            continue;
        }
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        if stem.ends_with("_rm") {
            continue;
        }
        out.push(path);
    }
    out
}

fn is_zip(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
}

/// Unpack an archive to a scratch directory that lives until the copy is done.
fn unzip_to_temp(zip_path: &Path) -> Result<tempfile::TempDir, ClipPackError> {
    let file = fs::File::open(zip_path).map_err(|e| ClipPackError::Install(e.to_string()))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| ClipPackError::Install(e.to_string()))?;
    let dir = tempfile::tempdir().map_err(|e| ClipPackError::Install(e.to_string()))?;
    crate::bundle::extract_zip_to_dir(&mut archive, dir.path()).map_err(ClipPackError::Install)?;
    Ok(dir)
}

fn animation_names(gltf: &Path) -> Result<Vec<String>, ClipPackError> {
    let doc = crate::rig::skeleton::import_json_only(gltf)
        .map_err(|e| ClipPackError::Install(e.to_string()))?;
    Ok(doc
        .animations()
        .filter_map(|a| a.name().map(|n| n.to_string()))
        .collect())
}

/// `UAL1_Standard.glb` → `ual1`. Distribution tier is not part of the id: a
/// user upgrading Standard to Source replaces the pack rather than gaining a
/// second copy of every clip.
fn derive_id(model: &Path) -> String {
    let stem = model
        .file_stem()
        .map(|s| s.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_else(|| CLIP_PACK_ID.into());
    let mut id = stem.as_str();
    for tier in ["_standard", "_source", "_pro", "_free"] {
        id = id.strip_suffix(tier).unwrap_or(id);
    }
    let slug: String = id
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        CLIP_PACK_ID.to_string()
    } else {
        slug
    }
}

fn display_name(id: &str) -> String {
    match id {
        "ual1" => "Universal Animation Library".into(),
        "ual2" => "Universal Animation Library 2".into(),
        other => other.to_string(),
    }
}

/// Turn a pack's raw animation name into something readable in a menu.
///
/// Display only — [`ClipCatalogEntry::id`] stays verbatim, because that is what
/// resolves against the pack and what a user cross-references against
/// Quaternius' own animation viewer.
///
/// `Loop` is deliberately kept. It looks like noise on the ~40% of clips that
/// carry it, but `Jump_Loop` sits beside `Jump_Start` and `Jump_Land`, so
/// dropping it would collapse a real distinction into three identical rows.
fn pretty_clip_name(clip: &str) -> String {
    if clip.eq_ignore_ascii_case("A_TPose") {
        return "T-Pose".into();
    }
    let mut words: Vec<String> = Vec::new();
    for token in clip.split('_') {
        for word in split_camel(token) {
            words.push(expand_abbreviation(&word));
        }
    }
    words.join(" ")
}

/// `ClimbUp` -> [`Climb`, `Up`]; `Death01` -> [`Death`, `01`].
fn split_camel(token: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut prev: Option<char> = None;
    for ch in token.chars() {
        // Break letter -> digit (`Death01`) but never digit -> letter, or a
        // unit like the `1m` in `ClimbUp_1m` comes apart.
        let boundary = prev.is_some_and(|p| {
            (ch.is_ascii_uppercase() && !p.is_ascii_uppercase())
                || (ch.is_ascii_digit() && !p.is_ascii_digit())
        });
        if boundary && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        cur.push(ch);
        prev = Some(ch);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Quaternius' shorthand, spelled out.
fn expand_abbreviation(word: &str) -> String {
    match word.to_ascii_lowercase().as_str() {
        "fwd" => "Forward".into(),
        "bwd" => "Backward".into(),
        "rec" => "Recovery".into(),
        _ => word.to_string(),
    }
}

/// Short names people actually type, in preference order, mapped onto the
/// library's real clip names. First entry that exists wins.
fn clip_aliases(requested: &str) -> &'static [&'static str] {
    match requested.to_ascii_lowercase().as_str() {
        "walk" => &["Walk_Loop", "Walk_Formal_Loop"],
        "run" => &["Sprint_Loop", "Jog_Fwd_Loop"],
        "jog" => &["Jog_Fwd_Loop"],
        "sprint" => &["Sprint_Loop"],
        "idle" => &["Idle_Loop"],
        "tpose" | "t-pose" => &["A_TPose"],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quaternius_pack_pages_are_the_official_listings() {
        assert_eq!(
            UAL1_PAGE,
            "https://quaternius.com/packs/universalanimationlibrary.html"
        );
        assert_eq!(
            UAL2_PAGE,
            "https://quaternius.com/packs/universalanimationlibrary2.html"
        );
    }

    #[test]
    fn ids_drop_the_distribution_tier() {
        assert_eq!(derive_id(Path::new("/x/UAL1_Standard.glb")), "ual1");
        assert_eq!(derive_id(Path::new("/x/UAL2_Standard.glb")), "ual2");
        assert_eq!(derive_id(Path::new("/x/UAL2_Source.glb")), "ual2");
        // A pack we have never seen still gets a usable id.
        assert_eq!(derive_id(Path::new("/x/Mixamo Pack.glb")), "mixamo-pack");
    }

    fn ual1_like() -> ClipPack {
        ClipPack {
            id: "ual1".into(),
            name: "UAL".into(),
            gltf_path: PathBuf::from("pack.glb"),
            release_version: None,
            clips: vec![
                "A_TPose".into(),
                "Jog_Fwd_Loop".into(),
                "Idle_Loop".into(),
                "Sprint_Loop".into(),
                "Walk_Loop".into(),
            ],
        }
    }

    #[test]
    fn alias_order_beats_pack_order() {
        // `Jog_Fwd_Loop` sorts before `Sprint_Loop` in the library, so a
        // pack-order search silently made `run` mean jog.
        assert_eq!(ual1_like().find_clip("run"), Some("Sprint_Loop"));
        assert_eq!(ual1_like().find_clip("jog"), Some("Jog_Fwd_Loop"));
    }

    #[test]
    fn aliases_resolve_to_real_quaternius_clips() {
        let p = ual1_like();
        assert_eq!(p.find_clip("walk"), Some("Walk_Loop"));
        assert_eq!(p.find_clip("idle"), Some("Idle_Loop"));
        assert_eq!(p.find_clip("tpose"), Some("A_TPose"));
        // `run` has no literal clip in the library; the old catalog
        // advertised one anyway and binding it failed outright.
        assert_eq!(p.find_clip("Run_Loop"), None);
        assert_eq!(p.find_clip("nope"), None);
    }

    #[test]
    fn find_clip_prefers_an_exact_name_over_an_alias() {
        let pack = ual1_like();
        assert_eq!(pack.find_clip("Walk_Loop"), Some("Walk_Loop"));
        assert_eq!(pack.find_clip("walk_loop"), Some("Walk_Loop"));
        assert_eq!(pack.find_clip("Sprint_Loop"), Some("Sprint_Loop"));
    }

    /// A GLB holding one animation: stands in for the animation library.
    fn glb_with_animation() -> Vec<u8> {
        use crate::rig::export::{AnimChannel, AnimData};
        let arm = crate::rig::canon::armature();
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
        crate::rig::write::write_animation_glb(&arm, &anim, "Walk_Loop").unwrap()
    }

    /// A valid GLB with no animations: stands in for the character mesh that
    /// ships beside the library and must never be chosen.
    fn glb_without_animations() -> Vec<u8> {
        crate::test_support::glb(br#"{"asset":{"version":"2.0"}}"#, None)
    }

    fn write_zip(path: &Path, files: &[(&str, Vec<u8>)]) {
        use std::io::Write as _;
        let file = fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        for (name, bytes) in files {
            zip.start_file::<_, ()>(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn install_from_a_zip_picks_the_library_over_the_mannequin() {
        let _guard = crate::test_support::env_lock();
        let home = tempfile::tempdir().unwrap();
        // SAFETY: serialized by `env_lock`.
        unsafe { std::env::set_var(CLIPS_DIR_ENV, home.path()) };

        let src = tempfile::tempdir().unwrap();
        let zip_path = src.path().join("Pack_Standard.zip");
        // The shape of a real download: a character mesh with no animations
        // beside the library, plus a root-motion variant to ignore.
        write_zip(
            &zip_path,
            &[
                (
                    "Pack/Female Mannequin/Mannequin_F.glb",
                    glb_without_animations(),
                ),
                ("Pack/Unreal-Godot/Pack_Standard.glb", glb_with_animation()),
                (
                    "Pack/Unreal-Godot/Pack_Standard_RM.glb",
                    glb_with_animation(),
                ),
            ],
        );

        let pack = install_pack_from(&zip_path, None).expect("install from zip");
        assert_eq!(pack.id, "pack", "tier suffix dropped");
        assert!(
            pack.gltf_path.starts_with(home.path()),
            "installed under the override"
        );
        assert!(pack.gltf_path.is_file());
        assert!(
            !pack.gltf_path.to_string_lossy().contains("_RM"),
            "root-motion variant must never be chosen"
        );
        assert!(!pack.clips.is_empty(), "the library, not the mannequin");

        // The manifest is what makes `list_clips` cheap; it must be written.
        assert!(pack_dir("pack").join("pack.json").is_file());
        let reloaded = ClipPack::from_dir(&pack_dir("pack")).unwrap();
        assert_eq!(reloaded.clips, pack.clips);

        // SAFETY: serialized by `env_lock`.
        unsafe { std::env::remove_var(CLIPS_DIR_ENV) };
    }

    #[test]
    fn a_zip_without_a_library_reports_that_and_not_a_parse_error() {
        // A .zip is a file, so the installer used to hand it straight to the
        // glTF parser and report "invalid glTF ... expected value at line 1",
        // which says nothing about what the user picked.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nothing-useful.zip");
        {
            let file = fs::File::create(&path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            zip.start_file::<_, ()>("readme.txt", zip::write::SimpleFileOptions::default())
                .unwrap();
            use std::io::Write as _;
            zip.write_all(b"no animations here").unwrap();
            zip.finish().unwrap();
        }
        let err = install_pack_from(&path, None).unwrap_err();
        assert!(
            matches!(err, ClipPackError::NoLibrary(_)),
            "expected NoLibrary, got {err:?}"
        );
        assert!(err.to_string().contains("no animation library"), "{err}");
    }

    #[test]
    fn root_motion_variants_are_never_offered() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["UAL1_Standard.glb", "UAL1_Standard_RM.glb"] {
            fs::write(dir.path().join(name), b"not a real glb").unwrap();
        }
        let found = gltf_candidates(dir.path(), 0);
        assert_eq!(found.len(), 1);
        assert!(found[0].ends_with("UAL1_Standard.glb"));
    }

    #[test]
    fn clip_names_are_readable_in_menus() {
        assert_eq!(pretty_clip_name("Walk_Loop"), "Walk Loop");
        assert_eq!(
            pretty_clip_name("Sitting_Talking_Loop"),
            "Sitting Talking Loop"
        );
        // CamelCase runs and letter/digit boundaries.
        assert_eq!(pretty_clip_name("PickUp_Table"), "Pick Up Table");
        assert_eq!(pretty_clip_name("ClimbUp_1m"), "Climb Up 1m");
        assert_eq!(
            pretty_clip_name("NinjaJump_Idle_Loop"),
            "Ninja Jump Idle Loop"
        );
        assert_eq!(pretty_clip_name("Death01"), "Death 01");
        // Abbreviations nobody should have to decode.
        assert_eq!(pretty_clip_name("Crouch_Fwd_Loop"), "Crouch Forward Loop");
        assert_eq!(pretty_clip_name("Melee_Hook_Rec"), "Melee Hook Recovery");
        // The one name that reads badly under the general rules.
        assert_eq!(pretty_clip_name("A_TPose"), "T-Pose");
    }

    #[test]
    fn loop_is_kept_because_it_distinguishes_a_family() {
        // Stripping it would render these three rows identically.
        assert_eq!(pretty_clip_name("Jump_Start"), "Jump Start");
        assert_eq!(pretty_clip_name("Jump_Loop"), "Jump Loop");
        assert_eq!(pretty_clip_name("Jump_Land"), "Jump Land");
    }

    #[test]
    fn ids_stay_verbatim_so_they_still_resolve() {
        // Display cleanup must not leak into the id: it is the key that
        // matches the pack, and what a user matches against Quaternius' docs.
        let pack = ual1_like();
        assert_eq!(pack.find_clip("Walk_Loop"), Some("Walk_Loop"));
        assert_eq!(pack.find_clip("Walk Loop"), None);
    }
}
