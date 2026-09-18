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

/// Extensions that carry glTF animation data.
pub const GLTF_EXTS: &[&str] = &["glb", "gltf"];

/// True when `path` has an extension in [`GLTF_EXTS`].
pub fn is_gltf_path(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| GLTF_EXTS.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// Cached pack description written beside the GLB.
pub const PACK_MANIFEST: &str = "pack.json";
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
        if !is_gltf_path(&path) {
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

/// True when a dropped path should install as a clip pack, not import as a bundle.
///
/// The GUI drop router uses this so a Quaternius zip/folder/GLB is not wrapped
/// into the library as `model.glb`. A taphub bundle (`bundle.json` / `image.png`)
/// always wins, even when the mesh already has baked clips.
pub fn looks_like_clip_pack(path: &Path) -> bool {
    if path.file_name().is_some_and(|n| n == PACK_MANIFEST) {
        return true;
    }
    // Only a directory, an archive, or a glTF can hold clips. Without this a
    // still called `armor_standard.png` matched on its name alone and the
    // importer refused it with "drop packs on the Animation panel" — a dead
    // end, since nothing but the File menu could then import it.
    if !can_hold_clips(path) {
        return false;
    }
    if strong_name_hints_pack(path) {
        return true;
    }
    if path.is_dir() {
        return dir_looks_like_pack(path);
    }
    if !weak_name_hints_pack(path) {
        return is_zip(path) && path.is_file() && zip_looks_like_pack(path);
    }
    // `pack` / `*_standard` / `*_source` are weak: `hero_Source.glb` is a
    // character and `Pack_Standard.zip` could be anything. Corroborate with
    // content whenever there is content to read — a rigged character ships at
    // most an idle, a library ships dozens — and fall back to the name only
    // for a path that isn't on disk (a zip entry, a name-only query).
    if is_zip(path) {
        return if path.is_file() {
            zip_looks_like_pack(path)
        } else {
            true
        };
    }
    match gltf_holds_a_library(path) {
        Some(verdict) => verdict,
        // Unreadable: trust the name only for a path that isn't on disk.
        None => !path.is_file(),
    }
}

/// Whether `gltf` carries enough clips to be a library, or `None` if it
/// can't be read.
fn gltf_holds_a_library(gltf: &Path) -> Option<bool> {
    animation_names(gltf)
        .ok()
        .map(|names| names.len() >= MIN_PACK_CLIPS)
}

/// Clips a weakly-named glTF must carry before it counts as a library.
///
/// Quaternius's libraries ship dozens; a character export ships an idle and
/// maybe a walk. Two was thin enough that anyone baking a couple of clips onto
/// `hero_Source.glb` got it classified as a pack.
const MIN_PACK_CLIPS: usize = 8;

/// glTF entries a weakly-named archive must hold before it counts as a
/// library. The archive stand-in for [`MIN_PACK_CLIPS`] — entry names are free
/// to read, while counting clips would mean decompressing, which the hover
/// path cannot afford. Keep the two in step.
const MIN_PACK_GLTF_ENTRIES: usize = 8;

/// How many meshes [`dir_holds_a_library`] will parse before giving up.
/// A library puts its clips in one file or a handful; this only has to be
/// deep enough to find one of them.
const MAX_CLASSIFY_CANDIDATES: usize = 24;

/// Whether `path` is a shape that could contain animation clips at all.
fn can_hold_clips(path: &Path) -> bool {
    path.is_dir() || is_zip(path) || is_gltf_path(path)
}

fn file_name_lower(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// Strong tokens are read from the filename *and* its immediate parent, so
/// `ual2/pack.glb` and `Universal-Animation-Library/Walking.glb` are packs.
/// Deliberately not the whole path: a project folder that happens to contain
/// "universal animation" must not reclassify everything beneath it.
fn strong_name_hints_pack(path: &Path) -> bool {
    if strong_path_hints_pack(&file_name_lower(path)) {
        return true;
    }
    path.parent()
        .is_some_and(|parent| strong_path_hints_pack(&file_name_lower(parent)))
}

fn weak_name_hints_pack(path: &Path) -> bool {
    weak_path_hints_pack(&file_name_lower(path))
}

/// Names only a Quaternius library carries, safe to trust on their own.
fn strong_path_hints_pack(name: &str) -> bool {
    let name = name.replace('\\', "/");
    name.contains("universal animation")
        || name.contains("universal-animation")
        || name.contains("universalanimation")
        || name.contains("clip-packs")
        || name.contains("clip_packs")
        || name.contains("unreal-godot")
        || ual_token(&name)
}

/// Tokens a user's own asset can plausibly carry, so they need corroborating
/// content before a path is treated as a pack.
fn weak_path_hints_pack(name: &str) -> bool {
    let name = name.replace('\\', "/");
    let stem = name.rsplit('/').next().unwrap_or(&name);
    let stem = stem.rsplit_once('.').map(|(s, _)| s).unwrap_or(stem);
    stem == "pack" || stem.ends_with("_standard") || stem.ends_with("_source")
}

/// `ual1` / `ual2` as a path or filename token, not a substring of `actual1`.
fn ual_token(name: &str) -> bool {
    for key in ["ual1", "ual2"] {
        let Some(i) = name.find(key) else {
            continue;
        };
        let before = i == 0 || !name.as_bytes()[i - 1].is_ascii_alphanumeric();
        let after_i = i + key.len();
        let after = after_i == name.len() || !name.as_bytes()[after_i].is_ascii_alphanumeric();
        if before && after {
            return true;
        }
    }
    false
}

fn dir_looks_like_pack(dir: &Path) -> bool {
    if dir.join(PACK_MANIFEST).is_file() || dir.join(PACK_MODEL).is_file() {
        return true;
    }
    if crate::bundle::looks_like_bundle(dir) {
        return false;
    }
    let Ok(entries) = dir.read_dir() else {
        return false;
    };
    let mut weak = false;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if strong_path_hints_pack(&name) {
            return true;
        }
        if weak_path_hints_pack(&name) {
            weak = true;
        }
    }
    // `exports/knight_Source.glb` is a character export, not a library. A weak
    // token needs the clip count behind it, exactly as a lone mesh does — the
    // same hole one level up.
    weak && dir_holds_a_library(dir)
}

/// Whether any mesh under `dir` carries a library's worth of clips.
///
/// Bounded, unlike [`pick_pack_model`]: this runs on the UI thread while a
/// folder is hovered, and parsing every glTF in a big export dump would stall
/// the frame. Install still uses the exhaustive pick, where correctness beats
/// latency.
fn dir_holds_a_library(dir: &Path) -> bool {
    gltf_candidates(dir, 0)
        .into_iter()
        .take(MAX_CLASSIFY_CANDIDATES)
        .any(|model| gltf_holds_a_library(&model).unwrap_or(false))
}

fn zip_looks_like_pack(path: &Path) -> bool {
    let Ok(file) = fs::File::open(path) else {
        return false;
    };
    let Ok(mut archive) = zip::ZipArchive::new(file) else {
        return false;
    };
    let mut saw_bundle = false;
    let mut saw_pack = false;
    // The archive's own name counts: a real `Pack_Standard.zip` carries the
    // token on the zip, while its entries are plain `Walking.glb`.
    let mut weak = weak_name_hints_pack(path);
    let mut gltfs = 0usize;
    let n = archive.len().min(400);
    for i in 0..n {
        let Ok(entry) = archive.by_index(i) else {
            continue;
        };
        let name = entry.name().replace('\\', "/").to_ascii_lowercase();
        if name.split('/').next() == Some("__macosx")
            || name
                .rsplit('/')
                .next()
                .is_some_and(|n| n == ".ds_store" || n.starts_with("._"))
        {
            continue;
        }
        let file_name = name.rsplit('/').next().unwrap_or(&name);
        if file_name == crate::constants::files::bundle::METADATA
            || file_name == crate::constants::files::bundle::IMAGE
        {
            saw_bundle = true;
        }
        if strong_path_hints_pack(&name) {
            saw_pack = true;
        } else if name.split('/').any(weak_path_hints_pack) {
            // Any segment, since the token is as often on the folder inside
            // the archive as on the file.
            weak = true;
        }
        if is_gltf_path(Path::new(&name)) {
            gltfs += 1;
        }
    }
    if saw_bundle {
        return false;
    }
    // A weak token alone would match an archive of somebody's character
    // exports. A library carries many meshes; an export carries one or two.
    saw_pack || (weak && gltfs >= MIN_PACK_GLTF_ENTRIES)
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
    fn looks_like_clip_pack_from_quaternius_names() {
        assert!(looks_like_clip_pack(Path::new(
            "Universal Animation Library 2[Source].zip"
        )));
        assert!(looks_like_clip_pack(Path::new(
            "Universal-Animation-Library.zip"
        )));
        assert!(looks_like_clip_pack(Path::new("UAL1_Standard.glb")));
        assert!(looks_like_clip_pack(Path::new("ual2/pack.glb")));
        assert!(looks_like_clip_pack(Path::new("clip-packs.zip")));
        assert!(looks_like_clip_pack(Path::new("Pack_Standard.zip")));
        // Token, not a substring of "actual1".
        assert!(!looks_like_clip_pack(Path::new("actual1.zip")));
        assert!(!looks_like_clip_pack(Path::new("helmet.zip")));
        assert!(!looks_like_clip_pack(Path::new("model.glb")));
        assert!(!looks_like_clip_pack(Path::new("hero.glb")));
        assert!(!looks_like_clip_pack(Path::new("run/bundle.json")));
    }

    #[test]
    fn a_weakly_named_users_own_file_is_not_a_pack() {
        let dir = tempfile::tempdir().unwrap();

        // A still can never hold clips, whatever it is called.
        let png = dir.path().join("armor_standard.png");
        fs::write(&png, b"png").unwrap();
        assert!(!looks_like_clip_pack(&png));

        // A character mesh that happens to end in `_Source`.
        let hero = dir.path().join("hero_Source.glb");
        fs::write(&hero, glb_without_animations()).unwrap();
        assert!(!looks_like_clip_pack(&hero));

        // The same weak token on a file that really is a library.
        let library = dir.path().join("hero_Source.glb");
        fs::write(&library, glb_with_animation()).unwrap();
        assert!(
            !looks_like_clip_pack(&library),
            "one clip is a rigged character, not a library"
        );

        // Our own installed pack still classifies, on the strong parent token.
        let installed = dir.path().join("ual2");
        fs::create_dir_all(&installed).unwrap();
        let pack = installed.join("pack.glb");
        fs::write(&pack, glb_without_animations()).unwrap();
        assert!(looks_like_clip_pack(&pack));
    }

    /// A GLB declaring `n` named animations. Only the names are read by
    /// [`animation_names`], so the channels can stay empty.
    fn glb_with_clips(n: usize) -> Vec<u8> {
        let anims: Vec<String> = (0..n)
            .map(|i| format!(r#"{{"name":"Clip_{i}","channels":[],"samplers":[]}}"#))
            .collect();
        let json = format!(
            r#"{{"asset":{{"version":"2.0"}},"animations":[{}]}}"#,
            anims.join(",")
        );
        crate::test_support::glb(json.as_bytes(), None)
    }

    #[test]
    fn a_weak_name_needs_a_librarys_worth_of_clips() {
        let dir = tempfile::tempdir().unwrap();
        let few = dir.path().join("hero_Source.glb");
        fs::write(&few, glb_with_clips(MIN_PACK_CLIPS - 1)).unwrap();
        assert!(
            !looks_like_clip_pack(&few),
            "a couple of baked clips on a character is not a library"
        );

        let many = dir.path().join("other_Source.glb");
        fs::write(&many, glb_with_clips(MIN_PACK_CLIPS)).unwrap();
        assert!(looks_like_clip_pack(&many), "at the threshold it is one");
    }

    #[test]
    fn an_installed_pack_directory_classifies_without_a_manifest() {
        // The shipped layout before install writes pack.json: packs/<id>/pack.glb.
        let dir = tempfile::tempdir().unwrap();
        let pack = dir.path().join("somepack");
        fs::create_dir_all(&pack).unwrap();
        fs::write(pack.join("pack.glb"), glb_without_animations()).unwrap();
        assert!(
            looks_like_clip_pack(&pack),
            "pack.glb names the directory a pack regardless of clip count"
        );
    }

    #[test]
    fn a_weakly_named_archive_needs_enough_meshes() {
        let dir = tempfile::tempdir().unwrap();
        let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
        for i in 0..MIN_PACK_GLTF_ENTRIES - 1 {
            entries.push((format!("Pack_Standard/{i}.glb"), glb_with_animation()));
        }
        let borrowed: Vec<(&str, Vec<u8>)> = entries
            .iter()
            .map(|(n, b)| (n.as_str(), b.clone()))
            .collect();
        let under = dir.path().join("under.zip");
        write_zip(&under, &borrowed);
        assert!(
            !looks_like_clip_pack(&under),
            "one short of the threshold is still an export dump"
        );
    }

    #[test]
    fn a_folder_of_character_exports_is_not_a_pack() {
        let dir = tempfile::tempdir().unwrap();
        let exports = dir.path().join("exports");
        fs::create_dir_all(&exports).unwrap();
        // Weak `_Source` token on a mesh with one clip: a character export.
        fs::write(exports.join("knight_Source.glb"), glb_with_animation()).unwrap();
        assert!(
            !looks_like_clip_pack(&exports),
            "a weak name in a folder needs the clip count behind it too"
        );

        // A strong token in the folder still classifies without counting.
        let library = dir.path().join("Universal-Animation-Library");
        fs::create_dir_all(&library).unwrap();
        fs::write(library.join("Walking.glb"), glb_with_animation()).unwrap();
        assert!(looks_like_clip_pack(&library));
    }

    #[test]
    fn a_zip_of_character_exports_is_not_a_pack() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("exports.zip");
        write_zip(
            &zip_path,
            &[
                ("knight_Source.glb", glb_with_animation()),
                ("knight_Source.png", b"png".to_vec()),
            ],
        );
        assert!(
            !looks_like_clip_pack(&zip_path),
            "one mesh behind a weak token is an export, not a library"
        );

        // Many meshes behind the same weak token is a library.
        let library = dir.path().join("archive.zip");
        let mut entries: Vec<(&str, Vec<u8>)> = vec![("Pack_Standard/readme.txt", b"x".to_vec())];
        let names = [
            "Pack_Standard/a.glb",
            "Pack_Standard/b.glb",
            "Pack_Standard/c.glb",
            "Pack_Standard/d.glb",
            "Pack_Standard/e.glb",
            "Pack_Standard/f.glb",
            "Pack_Standard/g.glb",
            "Pack_Standard/h.glb",
        ];
        for name in names {
            entries.push((name, glb_with_animation()));
        }
        write_zip(&library, &entries);
        assert!(looks_like_clip_pack(&library));
    }

    #[test]
    fn looks_like_clip_pack_dir_and_zip_contents() {
        let dir = tempfile::tempdir().unwrap();
        // A taphub bundle folder is never a pack, even beside a UAL-ish glb name.
        let bundle = dir.path().join("run");
        fs::create_dir_all(&bundle).unwrap();
        fs::write(bundle.join("image.png"), b"png").unwrap();
        fs::write(bundle.join("UAL1.glb"), glb_with_animation()).unwrap();
        assert!(!looks_like_clip_pack(&bundle));

        let pack_dir = dir.path().join("extracted");
        fs::create_dir_all(pack_dir.join("Unreal-Godot")).unwrap();
        fs::write(pack_dir.join("Unreal-Godot/UAL2.glb"), glb_with_animation()).unwrap();
        assert!(looks_like_clip_pack(&pack_dir));

        let zip_path = dir.path().join("mystery.zip");
        write_zip(
            &zip_path,
            &[("Pack/Unreal-Godot/Pack_Standard.glb", glb_with_animation())],
        );
        assert!(looks_like_clip_pack(&zip_path));

        let bundle_zip = dir.path().join("bundle.zip");
        write_zip(
            &bundle_zip,
            &[
                ("bundle.json", b"{}".to_vec()),
                ("image.png", b"png".to_vec()),
                ("model.glb", glb_with_animation()),
            ],
        );
        assert!(!looks_like_clip_pack(&bundle_zip));
    }

    #[test]
    fn unnamed_animated_glb_is_not_a_pack() {
        // Drop routing is name/layout, not "has clips". File → Install still
        // accepts any library; a knight mesh with baked walk must import.
        let dir = tempfile::tempdir().unwrap();
        let glb = dir.path().join("knight.glb");
        fs::write(&glb, glb_with_animation()).unwrap();
        assert!(!looks_like_clip_pack(&glb));
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
        assert!(pack_dir("pack").join(PACK_MANIFEST).is_file());
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
