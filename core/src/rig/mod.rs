//! Bind a generated mesh to the shipped humanoid armature and apply a clip.
//!
//! One canonical skeleton ([`canon`], VRM-named), mesh-sampled
//! landmarks, inverse-distance weights, glTF skin + animation write.
//!
//! ```text
//! mesh.glb + clip pack
//!   → landmarks (optional) → fit + weight → apply clip (optional) → model.glb
//! ```

mod body_fit;
pub mod canon;
mod export;
mod fetch;
mod landmarks;
mod onmesh;
mod pack;
mod play;
pub(crate) mod skeleton;
mod weights;
mod write;

use std::path::{Path, PathBuf};

pub use canon::{BoneGroup, BoneScheme, HumanBone, RestJoint, SKELETON_ID, Side};
pub use fetch::{
    CLIP_PACKS_DIR_ENV, ClipPacksDownloadResult, download_clip_packs,
    install_missing_release_packs_from, missing_release_packs, release_pack_ids,
    shipped_packs_version,
};
pub use landmarks::{Landmarks, mesh_landmarks};
pub use pack::{
    CLIP_PACK_ID, ClipCatalogEntry, ClipPack, ClipPackError, DEFAULT_BIND_CLIP, PACK_MANIFEST,
    UAL1_PAGE, UAL2_PAGE, find_clip, install_pack_from, list_clips, looks_like_clip_pack, pack_dir,
    packs_root, resolve_pack, resolve_packs,
};
pub use play::{BindMarker, SkinnedClip};

/// Options for [`bind_mesh`].
#[derive(Debug, Clone)]
pub struct BindOptions {
    /// Clips to write, in order. Empty means fit only. The set is
    /// authoritative: a bake writes exactly these animations.
    pub clips: Vec<String>,
    /// Override pack directory (otherwise [`resolve_pack`]).
    pub pack_dir: Option<PathBuf>,
    /// Fit and weight only — preserve the input pose, no animation.
    pub fit_only: bool,
    /// Re-fit even when the mesh already carries a rig, discarding its pose.
    pub refit: bool,
}

impl Default for BindOptions {
    fn default() -> Self {
        Self {
            clips: vec![pack::DEFAULT_BIND_CLIP.into()],
            pack_dir: None,
            fit_only: false,
            refit: false,
        }
    }
}

/// The pack providing `options.clip`, and the animation name it resolved to.
///
/// An explicit `pack_dir` wins; otherwise every installed pack is searched, so
/// a clip resolves to whichever library actually has it rather than to
/// whichever pack happens to sort first.
fn resolve_options_clips(options: &BindOptions) -> Result<Vec<(ClipPack, String)>, BindError> {
    let mut out = Vec::with_capacity(options.clips.len());
    for clip in &options.clips {
        if let Some(dir) = &options.pack_dir {
            let pack = ClipPack::from_dir(dir)?;
            let name = pack
                .find_clip(clip)
                .ok_or_else(|| ClipPackError::UnknownClip(clip.clone()))?
                .to_string();
            out.push((pack, name));
        } else {
            out.push(find_clip(clip)?);
        }
    }
    Ok(out)
}

/// Bind `mesh_glb` to the humanoid pack and write a skinned GLB.
///
/// With [`BindOptions::fit_only`], no clip is applied. Otherwise this is
/// `fit_mesh` + `apply_clip`.
///
/// **A rigged mesh keeps its skeleton and weights unless `refit`.** That
/// rule is checked first, before fit-only / empty-clips is considered:
/// re-fitting an already-rigged mesh throws away its pose and re-weights to
/// the auto-fit, and someone who arranged joints by hand would silently lose
/// that work. So on a fitted mesh without `refit`:
///
/// - `fit_only` (or an empty clip set, which is the same request) is a no-op
///   on the rig: the file is rewritten with its existing skeleton, weights
///   **and clips**, since none were explicitly given.
/// - a clip set is baked onto the rig that is already there.
pub fn bind_mesh(
    mesh_glb: &Path,
    out_glb: &Path,
    options: &BindOptions,
) -> Result<BindReport, BindError> {
    let keep_rig = !options.refit && is_fitted(mesh_glb)?;
    if options.fit_only || options.clips.is_empty() {
        return if keep_rig {
            export::rewrite_fitted(mesh_glb, out_glb)
        } else {
            export::fit_and_write(mesh_glb, out_glb)
        };
    }
    let clips = resolve_options_clips(options)?;
    if keep_rig {
        return export::apply_clips_and_write(mesh_glb, &clips, out_glb);
    }
    export::bind_and_write(mesh_glb, &clips, out_glb)
}

/// Fit + weight a standing A- or T-pose mesh. No animation is written.
pub fn fit_mesh(
    mesh_glb: &Path,
    out_glb: &Path,
    _options: &BindOptions,
) -> Result<BindReport, BindError> {
    export::fit_and_write(mesh_glb, out_glb)
}

/// Bind to the arranged heads: they define the skeleton and the weights are
/// computed from it (Meshy / Mixamo). Any head off the mesh is refused as
/// [`BindError::OffMesh`] before anything is written; use
/// [`heads_off_mesh`] to check ahead of time.
pub fn fit_mesh_from_heads(
    mesh_glb: &Path,
    out_glb: &Path,
    heads: &[(String, [f32; 3])],
    options: &BindOptions,
) -> Result<BindReport, BindError> {
    fit_mesh_from_heads_keeping(mesh_glb, out_glb, heads, &[], options)
}

/// [`fit_mesh_from_heads`], re-baking `keep_clips` onto the new fit.
///
/// A re-Bind of a mesh that already carries baked animation rebuilds the
/// skin, and a fit writes exactly the clips it is given — so without this
/// the baked set was silently dropped. Pass [`baked_clip_names`] (or the
/// panel's ticked set) to keep it. Each name is resolved through the pack
/// catalog ([`find_clip`]); names that no installed pack provides come back
/// in [`BindReport::dropped_clips`] instead of failing the bind, so the
/// caller can tell the author what did not survive.
pub fn fit_mesh_from_heads_keeping(
    mesh_glb: &Path,
    out_glb: &Path,
    heads: &[(String, [f32; 3])],
    keep_clips: &[String],
    _options: &BindOptions,
) -> Result<BindReport, BindError> {
    export::fit_and_write_heads(mesh_glb, out_glb, Some(heads), keep_clips)
}

/// Auto-fit: the canonical skeleton solved onto the mesh's landmarks.
///
/// Landmark-driven, so it fails on anything it cannot read as a body. That is
/// fine because it is an accelerator the user reaches for from inside Rig, not
/// the way Rig opens: see [`default_bind_markers`]. No pack, no file write.
pub fn seed_bind_markers(mesh_glb: &Path) -> Result<Vec<BindMarker>, BindError> {
    export::seed_bind_markers(mesh_glb)
}

/// The skeleton Rig opens with: shipped rest pose, scaled to the mesh.
///
/// Succeeds on any mesh with geometry. No pack, no file write.
pub fn default_bind_markers(mesh_glb: &Path) -> Result<Vec<BindMarker>, BindError> {
    export::default_bind_markers(mesh_glb)
}

/// Heads that Bind would refuse: `(joint, metres to the nearest point on the
/// mesh surface)`, farthest first. Empty means every head is inside or
/// hugging the mesh. Surface, not nearest vertex: on coarse geometry the
/// nearest corner over-reports, and the number reaches the author as how far
/// to drag.
pub fn heads_off_mesh(
    mesh_glb: &Path,
    heads: &[(String, [f32; 3])],
) -> Result<Vec<(String, f32)>, BindError> {
    export::heads_off_mesh(mesh_glb, heads)
}

/// Apply one clip onto an already-fitted rest GLB. Does not re-weight.
pub fn apply_clip(
    rest_glb: &Path,
    out_glb: &Path,
    options: &BindOptions,
) -> Result<BindReport, BindError> {
    let clips = resolve_options_clips(options)?;
    export::apply_clips_and_write(rest_glb, &clips, out_glb)
}

/// Viewer overlay: pack clip retargeted onto `rest_glb` (same space as Bake).
///
/// Does not write `model.glb`. Missing pack / clip is a local error.
pub(crate) fn clip_overlay_for_rest(clip_id: &str, rest_glb: &Path) -> Result<Vec<u8>, BindError> {
    let (pack, clip) = find_clip(clip_id)?;
    export::animation_overlay_for_rest(&pack.gltf_path, &clip, rest_glb)
}

/// `Some(joint_count)` when the mesh carries a skin that is not ours.
///
/// [`is_fitted`] answers "is this rigged *by us*", and a foreign rig makes it
/// `false`, so every path treats such a mesh as unrigged and Bind overwrites
/// the skin. That is the right default, since we cannot animate a skeleton we
/// cannot name, but it silently discards work someone else did. Callers use
/// this to say so first.
///
/// Naming is the test, as everywhere else, and it is the same test as
/// [`is_fitted`]: a skin whose joints do not read as the canonical scheme is
/// one we do not know.
pub fn foreign_rig_joints(model_glb: &Path) -> Result<Option<usize>, BindError> {
    let doc = skeleton::import_json_only(model_glb)?;
    Ok(match skin_is_canonical(&doc) {
        Some((false, n)) => Some(n),
        _ => None,
    })
}

/// Does this model already carry a canonical rig?
///
/// True when its first skin's joints read as our own scheme — enough of
/// them to clear [`BoneScheme::detect`]'s threshold, `hips` among them —
/// which is what [`bind_mesh`] uses to decide between baking onto the
/// existing rig and fitting a fresh one. Any node called `head` somewhere in
/// the document is not a rig; a foreign skin with one such name is foreign.
pub fn is_fitted(model_glb: &Path) -> Result<bool, BindError> {
    let doc = skeleton::import_json_only(model_glb)?;
    Ok(matches!(skin_is_canonical(&doc), Some((true, _))))
}

/// `(canonical?, joint_count)` for the first skin, or `None` when the model
/// has no skin (or an empty one). Shared by [`is_fitted`] and
/// [`foreign_rig_joints`] so the two can never disagree on what "ours" means.
fn skin_is_canonical(doc: &gltf::Document) -> Option<(bool, usize)> {
    let skin = doc.skins().next()?;
    let names: Vec<&str> = skin.joints().filter_map(|n| n.name()).collect();
    let count = skin.joints().count();
    if count == 0 {
        return None;
    }
    let ours = BoneScheme::detect(names.iter().copied()) == Some(BoneScheme::Vrm)
        && names.contains(&HumanBone::Hips.as_str());
    Some((ours, count))
}

/// Animation names already baked into `model_glb`, in file order.
///
/// This is what pre-checks the export set when a previously baked model is
/// opened, so add/remove is a matter of ticking boxes rather than remembering
/// what went in.
pub fn baked_clip_names(model_glb: &Path) -> Result<Vec<String>, BindError> {
    let doc = skeleton::import_json_only(model_glb)?;
    Ok(doc
        .animations()
        .filter_map(|a| a.name().map(str::to_string))
        .collect())
}

/// CPU clip for native Preview: fitted rest + catalog overlay (no file write).
pub fn preview_skinned_clip(clip_id: &str, rest_glb: &Path) -> Result<SkinnedClip, BindError> {
    let overlay = clip_overlay_for_rest(clip_id, rest_glb)?;
    let mut clip = SkinnedClip::from_glb(rest_glb).map_err(BindError::Failed)?;
    clip.overlay_from_slice(&overlay)
        .map_err(BindError::Failed)?;
    Ok(clip)
}

/// What the bind wrote — enough for logs and bundle provenance.
#[derive(Debug, Clone)]
pub struct BindReport {
    pub pack_id: String,
    pub clips: Vec<String>,
    pub joint_count: usize,
    pub vertex_count: usize,
    pub landmarks: Landmarks,
    /// Placeable joints whose head moved more than 1 cm from the auto-fit.
    /// Zero for an auto-fit bind, `apply_clip`, and `rebake_mesh`.
    pub moved_joints: usize,
    /// Largest such move, metres.
    pub max_moved_m: f32,
    /// Clips the caller asked to keep across a re-fit that no installed pack
    /// could supply, so they are not in `clips`. Empty on every other path.
    pub dropped_clips: Vec<String>,
}

/// Bind failures. Missing pack is the one agents can recover from
/// (`asset-tap clip download` or `clip install`).
#[derive(Debug, thiserror::Error)]
pub enum BindError {
    #[error("{0}")]
    Pack(#[from] ClipPackError),
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid glTF ({path}): {message}")]
    Gltf { path: PathBuf, message: String },
    /// Arranged heads that are neither inside nor hugging the mesh, farthest
    /// first. A bone with no mesh near it drives nothing; refuse, don't write.
    #[error("{}", off_mesh_message(.0))]
    OffMesh(Vec<(String, f32)>),
    #[error("{0}")]
    Failed(String),
}

fn off_mesh_message(off: &[(String, f32)]) -> String {
    let mut names: Vec<String> = off
        .iter()
        .take(3)
        .map(|(n, d)| format!("{} ({d:.2} m)", user_facing_joint_name(n)))
        .collect();
    if off.len() > 3 {
        names.push(format!("+{} more", off.len() - 3));
    }
    let noun = if off.len() == 1 {
        "joint is"
    } else {
        "joints are"
    };
    format!(
        "{} {noun} off the mesh: {}. Park every joint on the body, then Bind.",
        off.len(),
        names.join(", ")
    )
}

/// Mixamo legend nouns for chrome and errors. The GLB still stores VRM names.
fn user_facing_joint_name(name: &str) -> String {
    HumanBone::parse(name)
        .map(|b| b.panel_label())
        .unwrap_or_else(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;
    use skeleton::Armature;

    /// A rig we cannot name is reported, so Bind can say it before replacing it.
    ///
    /// `is_fitted` answers "rigged *by us*" and a foreign skin makes it false,
    /// so every path treats such a mesh as unrigged and the bind overwrites the
    /// skin. Correct, since we cannot animate a skeleton we cannot name, but
    /// silently discarding someone's rigging work is a poor surprise.
    #[test]
    fn a_rig_we_cannot_name_is_reported_rather_than_silently_replaced() {
        let dir = tempfile::tempdir().unwrap();

        // Ours: canonical joint names.
        let mine = dir.path().join("mine.glb");
        let arm = canon::armature();
        std::fs::write(&mine, tests_support_skinned(&arm.skin, true)).unwrap();
        assert_eq!(foreign_rig_joints(&mine).unwrap(), None, "our own rig");
        assert!(is_fitted(&mine).unwrap(), "and it reads as fitted");

        // Someone else's: a skin whose joints mean nothing to us.
        let theirs = dir.path().join("theirs.glb");
        std::fs::write(&theirs, tests_support_skinned(&arm.skin, false)).unwrap();
        assert_eq!(
            foreign_rig_joints(&theirs).unwrap(),
            Some(arm.skin.len()),
            "a foreign rig is reported, with its joint count"
        );
        assert!(
            !is_fitted(&theirs).unwrap(),
            "and still reads as unfitted, which is what makes the warning necessary"
        );

        // No skin at all is not a foreign rig, just an unrigged mesh.
        let bare = dir.path().join("bare.glb");
        std::fs::write(&bare, humanoid_glb()).unwrap();
        assert_eq!(foreign_rig_joints(&bare).unwrap(), None);
    }

    /// A GLB carrying a skin whose joints are named ours or theirs.
    fn tests_support_skinned(skin: &[usize], canonical: bool) -> Vec<u8> {
        let arm = canon::armature();
        let nodes: Vec<serde_json::Value> = arm
            .joints
            .iter()
            .enumerate()
            .map(|(i, j)| {
                let name = if canonical {
                    j.name.clone()
                } else {
                    format!("bip01_bone_{i}")
                };
                let mut n = serde_json::json!({ "name": name });
                let kids: Vec<usize> = arm
                    .joints
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| c.parent == Some(i))
                    .map(|(k, _)| k)
                    .collect();
                if !kids.is_empty() {
                    n["children"] = serde_json::json!(kids);
                }
                n
            })
            .collect();
        let json = serde_json::json!({
            "asset": { "version": "2.0" },
            "scene": 0,
            "scenes": [{ "nodes": [0] }],
            "nodes": nodes,
            "skins": [{ "joints": skin }],
        });
        crate::test_support::glb(&serde_json::to_vec(&json).unwrap(), None)
    }

    /// What Preview shows must be what Bake writes.
    ///
    /// The two go different ways round. Bake rebases and retargets against
    /// `canon::armature()`; Preview does it against the *pack's* own rest
    /// (`animation_overlay_for_rest`). Nothing checked that those agree, and a
    /// divergence is invisible in the worst way: the author approves one
    /// animation in the viewer and ships another. Cross-pack is where it would
    /// show first, since the canonical rest was derived from one library.
    #[test]
    fn preview_and_bake_agree_pose_for_pose() {
        let _env = crate::test_support::env_lock();
        let dir = tempfile::tempdir().unwrap();

        // SAFETY: serialized by env_lock().
        unsafe { std::env::set_var(pack::CLIPS_DIR_ENV, dir.path()) };

        let src = dir.path().join("library.glb");
        std::fs::write(&src, moving_pack_glb()).unwrap();
        install_pack_from(&src, Some("t")).expect("install the synthetic pack");

        let mesh = dir.path().join("mesh.glb");
        std::fs::write(&mesh, humanoid_glb()).unwrap();
        let rest = dir.path().join("rest.glb");
        fit_mesh(&mesh, &rest, &BindOptions::default()).expect("fit");

        let preview = preview_skinned_clip("Reach", &rest).expect("preview");

        let baked = dir.path().join("baked.glb");
        let options = BindOptions {
            clips: vec!["Reach".into()],
            ..BindOptions::default()
        };
        bind_mesh(&rest, &baked, &options).expect("bake");
        let played = SkinnedClip::from_glb(&baked).expect("load the bake");

        // SAFETY: serialized by env_lock().
        unsafe { std::env::remove_var(pack::CLIPS_DIR_ENV) };

        assert!(
            (preview.duration - played.duration).abs() < 1e-4,
            "durations differ: preview {} baked {}",
            preview.duration,
            played.duration
        );

        let mut moved = 0.0f32;
        let mut worst = 0.0f32;
        let (mut a, mut b, mut rest_pose) = (Vec::new(), Vec::new(), Vec::new());
        preview.sample_positions_rest_into(&mut rest_pose);
        for i in 0..=6 {
            let t = preview.duration * i as f32 / 6.0;
            preview.sample_positions_into(t, &mut a);
            played.sample_positions_into(t, &mut b);
            assert_eq!(a.len(), b.len(), "vertex counts differ at t={t}");
            for ((p, q), r) in a.iter().zip(b.iter()).zip(rest_pose.iter()) {
                worst = worst.max(dist(*p, *q));
                moved = moved.max(dist(*p, *r));
            }
        }
        // The clip has to actually move the mesh, or agreeing is meaningless.
        assert!(moved > 0.01, "the fixture clip barely deforms: {moved}");
        assert!(worst < 1e-4, "preview and bake diverge by {worst} m");
    }

    /// A pack whose rest differs from the embedded one must still preview
    /// and bake to the same pose.
    ///
    /// [`preview_and_bake_agree_pose_for_pose`] builds its pack from
    /// `canon::armature()`, so it could not see the two paths retargeting
    /// against different rests: Bake shifted translations from the canonical
    /// rest, Preview from the pack's. Identical while the pack *is* the
    /// canonical rest; a library authored on a taller mannequin split them.
    #[test]
    fn preview_and_bake_agree_on_a_pack_whose_rest_differs() {
        let _env = crate::test_support::env_lock();
        let dir = tempfile::tempdir().unwrap();
        // SAFETY: serialized by env_lock().
        unsafe { std::env::set_var(pack::CLIPS_DIR_ENV, dir.path()) };

        // The same clip, on an armature whose hips rest 12 cm higher and
        // whose upper arm sits 3 cm further out than the embedded skeleton.
        let mut arm = canon::armature();
        let hips = arm.name_to_index["hips"];
        arm.joints[hips].translation.y += 0.12;
        let upper = arm.name_to_index[HumanBone::LeftUpperArm.as_str()];
        arm.joints[upper].translation.x += 0.03;
        let src = dir.path().join("tall.glb");
        std::fs::write(&src, moving_pack_glb_on(&arm)).unwrap();
        install_pack_from(&src, Some("tall")).expect("install");

        let (preview, played) = preview_and_bake(dir.path(), "Reach");
        // SAFETY: serialized by env_lock().
        unsafe { std::env::remove_var(pack::CLIPS_DIR_ENV) };

        let (moved, worst) = compare_playback(&preview, &played);
        assert!(moved > 0.01, "the fixture clip barely deforms: {moved}");
        assert!(worst < 1e-4, "preview and bake diverge by {worst} m");
    }

    /// A CUBICSPLINE library clip previews and bakes to the same pose, and
    /// actually curves: the writer keeps the three-samples-per-key layout,
    /// so the player has to evaluate it rather than read tangents as keys.
    #[test]
    fn cubic_spline_clips_preview_and_bake_alike() {
        let _env = crate::test_support::env_lock();
        let dir = tempfile::tempdir().unwrap();
        // SAFETY: serialized by env_lock().
        unsafe { std::env::set_var(pack::CLIPS_DIR_ENV, dir.path()) };

        let src = dir.path().join("cubic.glb");
        std::fs::write(&src, cubic_pack_glb()).unwrap();
        install_pack_from(&src, Some("cubic")).expect("install");

        let (preview, played) = preview_and_bake(dir.path(), "Bob");
        // SAFETY: serialized by env_lock().
        unsafe { std::env::remove_var(pack::CLIPS_DIR_ENV) };

        let (moved, worst) = compare_playback(&preview, &played);
        assert!(moved > 0.01, "the fixture clip barely deforms: {moved}");
        assert!(worst < 1e-4, "preview and bake diverge by {worst} m");
        // Both keys are at rest; only the tangents move the body mid-clip
        // (along the hips' local axis, whatever `root`'s rest rotation makes
        // of that in world space). Read as LINEAR the clip would be
        // motionless at every key and wildly wrong between them.
        let (mut rest_pose, mut mid, mut end) = (Vec::new(), Vec::new(), Vec::new());
        preview.sample_positions_rest_into(&mut rest_pose);
        preview.sample_positions_into(preview.duration * 0.5, &mut mid);
        preview.sample_positions_into(preview.duration, &mut end);
        let far = |a: &[[f32; 3]], b: &[[f32; 3]]| {
            a.iter()
                .zip(b.iter())
                .map(|(p, q)| dist(*p, *q))
                .fold(0.0f32, f32::max)
        };
        let lift = far(&rest_pose, &mid);
        assert!(lift > 0.02, "the tangents should move the mesh: {lift}");
        assert!(far(&rest_pose, &end) < 1e-3, "and both keys sit at rest");
    }

    /// Fit a fresh humanoid, then preview and bake `clip` onto it.
    fn preview_and_bake(dir: &Path, clip: &str) -> (SkinnedClip, SkinnedClip) {
        let mesh = dir.join("mesh.glb");
        std::fs::write(&mesh, humanoid_glb()).unwrap();
        let rest = dir.join("rest.glb");
        fit_mesh(&mesh, &rest, &BindOptions::default()).expect("fit");
        let preview = preview_skinned_clip(clip, &rest).expect("preview");
        let baked = dir.join("baked.glb");
        let options = BindOptions {
            clips: vec![clip.into()],
            ..BindOptions::default()
        };
        bind_mesh(&rest, &baked, &options).expect("bake");
        let played = SkinnedClip::from_glb(&baked).expect("load the bake");
        assert!(
            (preview.duration - played.duration).abs() < 1e-4,
            "durations differ: preview {} baked {}",
            preview.duration,
            played.duration
        );
        (preview, played)
    }

    /// `(how far the clip moves the mesh, worst preview/bake disagreement)`.
    fn compare_playback(preview: &SkinnedClip, played: &SkinnedClip) -> (f32, f32) {
        let mut moved = 0.0f32;
        let mut worst = 0.0f32;
        let (mut a, mut b, mut rest_pose) = (Vec::new(), Vec::new(), Vec::new());
        preview.sample_positions_rest_into(&mut rest_pose);
        for i in 0..=6 {
            let t = preview.duration * i as f32 / 6.0;
            preview.sample_positions_into(t, &mut a);
            played.sample_positions_into(t, &mut b);
            assert_eq!(a.len(), b.len(), "vertex counts differ at t={t}");
            for ((p, q), r) in a.iter().zip(b.iter()).zip(rest_pose.iter()) {
                worst = worst.max(dist(*p, *q));
                moved = moved.max(dist(*p, *r));
            }
        }
        (moved, worst)
    }

    /// `bind --fit-only` on a rigged mesh is a no-op on the rig.
    ///
    /// It used to reach `fit_and_write` before the fitted check, so a mesh
    /// whose joints had been arranged by hand was silently re-fitted to the
    /// auto-fit, and its baked clips went with it. The contract is the one
    /// `bind_mesh` already applied to clips: a rigged mesh keeps skeleton and
    /// weights unless `--refit`.
    #[test]
    fn fit_only_on_a_rigged_mesh_keeps_the_rig_and_its_clips() {
        let _env = crate::test_support::env_lock();
        let dir = tempfile::tempdir().unwrap();
        // SAFETY: serialized by env_lock().
        unsafe { std::env::set_var(pack::CLIPS_DIR_ENV, dir.path()) };
        let src = dir.path().join("library.glb");
        std::fs::write(&src, moving_pack_glb()).unwrap();
        install_pack_from(&src, Some("t")).expect("install");

        let mesh = dir.path().join("mesh.glb");
        std::fs::write(&mesh, humanoid_glb()).unwrap();
        let model = dir.path().join("model.glb");
        // Fit, nudge a knee by hand, then bake a clip: an authored rig.
        fit_mesh(&mesh, &model, &BindOptions::default()).expect("fit");
        let mut heads: Vec<(String, [f32; 3])> = seed_bind_markers(&mesh)
            .unwrap()
            .into_iter()
            .map(|m| (m.name, m.world))
            .collect();
        let knee = heads
            .iter_mut()
            .find(|(n, _)| n == HumanBone::LeftLowerLeg.as_str())
            .unwrap();
        knee.1[1] -= 0.06;
        fit_mesh_from_heads(&mesh, &model, &heads, &BindOptions::default()).expect("bind");
        let knee_y = |p: &Path| {
            SkinnedClip::from_glb(p)
                .unwrap()
                .bind_markers()
                .into_iter()
                .find(|m| m.name == HumanBone::LeftLowerLeg.as_str())
                .unwrap()
                .world[1]
        };
        let authored = knee_y(&model);
        bind_mesh(
            &model,
            &model,
            &BindOptions {
                clips: vec!["Reach".into()],
                ..Default::default()
            },
        )
        .expect("bake");
        assert_eq!(baked_clip_names(&model).unwrap(), ["Reach"]);

        // Fit-only, and an empty set: neither re-fits, neither strips.
        for options in [
            BindOptions {
                fit_only: true,
                clips: Vec::new(),
                ..Default::default()
            },
            BindOptions {
                clips: Vec::new(),
                ..Default::default()
            },
        ] {
            let report = bind_mesh(&model, &model, &options).expect("fit-only on rigged");
            assert_eq!(report.clips, ["Reach"], "existing clips are reported");
            assert!(
                (knee_y(&model) - authored).abs() < 1e-5,
                "the authored knee moved: {} -> {}",
                authored,
                knee_y(&model)
            );
            assert_eq!(baked_clip_names(&model).unwrap(), ["Reach"]);
        }

        // `--refit` is the explicit way to throw the pose away.
        bind_mesh(
            &model,
            &model,
            &BindOptions {
                fit_only: true,
                clips: Vec::new(),
                refit: true,
                ..Default::default()
            },
        )
        .expect("refit");
        assert!(
            (knee_y(&model) - authored).abs() > 0.03,
            "refit should return to the auto-fit knee"
        );
        assert!(baked_clip_names(&model).unwrap().is_empty());

        // SAFETY: serialized by env_lock().
        unsafe { std::env::remove_var(pack::CLIPS_DIR_ENV) };
    }

    /// Re-Bind keeps the clips it is asked to keep, and names the ones it
    /// could not source rather than failing.
    #[test]
    fn a_refit_from_heads_keeps_the_baked_clips_it_is_told_to() {
        let _env = crate::test_support::env_lock();
        let dir = tempfile::tempdir().unwrap();
        // SAFETY: serialized by env_lock().
        unsafe { std::env::set_var(pack::CLIPS_DIR_ENV, dir.path()) };
        let src = dir.path().join("library.glb");
        std::fs::write(&src, moving_pack_glb()).unwrap();
        install_pack_from(&src, Some("t")).expect("install");

        let mesh = dir.path().join("mesh.glb");
        std::fs::write(&mesh, humanoid_glb()).unwrap();
        let model = dir.path().join("model.glb");
        bind_mesh(
            &mesh,
            &model,
            &BindOptions {
                clips: vec!["Reach".into()],
                ..Default::default()
            },
        )
        .expect("bind");
        let baked = baked_clip_names(&model).unwrap();
        assert_eq!(baked, ["Reach"]);

        let heads: Vec<(String, [f32; 3])> = seed_bind_markers(&model)
            .unwrap()
            .into_iter()
            .map(|m| (m.name, m.world))
            .collect();

        // Without the keep list the old behaviour stands: a fit writes
        // exactly the clips it is given, which is none.
        fit_mesh_from_heads(&model, &model, &heads, &BindOptions::default()).expect("re-bind");
        assert!(baked_clip_names(&model).unwrap().is_empty());

        // With it, the baked set survives the re-fit, and a name no pack has
        // is reported rather than fatal.
        let mut keep = baked.clone();
        keep.push("Foreign_Clip".into());
        let report =
            fit_mesh_from_heads_keeping(&model, &model, &heads, &keep, &BindOptions::default())
                .expect("re-bind keeping clips");
        assert_eq!(baked_clip_names(&model).unwrap(), baked);
        assert_eq!(report.clips, baked);
        assert_eq!(report.dropped_clips, ["Foreign_Clip"]);

        // SAFETY: serialized by env_lock().
        unsafe { std::env::remove_var(pack::CLIPS_DIR_ENV) };
    }

    /// One recognizable name in a foreign skin does not make it ours.
    ///
    /// `is_fitted` used to be true if *any* node in the document parsed as a
    /// canonical bone, so a rig from elsewhere with a joint called `head` was
    /// treated as fitted by us, and Bind baked onto a skeleton it could not
    /// animate. The threshold is `BoneScheme::detect`'s, on the skin's joints.
    #[test]
    fn a_foreign_skin_with_one_canonical_name_is_not_fitted() {
        let dir = tempfile::tempdir().unwrap();
        let arm = canon::armature();
        let mut glb = tests_support_skinned(&arm.skin, false);
        // Rename one foreign joint to `head`, in place, in the JSON chunk.
        let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        let mut doc: serde_json::Value = serde_json::from_slice(&glb[20..20 + json_len]).unwrap();
        doc["nodes"][5]["name"] = serde_json::json!("head");
        glb = crate::test_support::glb(&serde_json::to_vec(&doc).unwrap(), None);
        let path = dir.path().join("theirs.glb");
        std::fs::write(&path, glb).unwrap();

        assert!(
            !is_fitted(&path).unwrap(),
            "one `head` is not a rig of ours"
        );
        assert_eq!(
            foreign_rig_joints(&path).unwrap(),
            Some(arm.skin.len()),
            "and it is reported as foreign"
        );
    }

    #[test]
    fn off_mesh_errors_use_legend_names_not_vrm_wire() {
        let msg = off_mesh_message(&[
            ("leftShoulder".into(), 0.07),
            ("rightShoulder".into(), 0.06),
        ]);
        assert!(msg.contains("Left clavicle"));
        assert!(msg.contains("Right clavicle"));
        assert!(!msg.contains("leftShoulder"));
    }

    fn dist(p: [f32; 3], q: [f32; 3]) -> f32 {
        ((p[0] - q[0]).powi(2) + (p[1] - q[1]).powi(2) + (p[2] - q[2]).powi(2)).sqrt()
    }

    /// A library with one clip that swings an arm, so the comparison has
    /// something to compare.
    fn moving_pack_glb() -> Vec<u8> {
        moving_pack_glb_on(&canon::armature())
    }

    /// One clip that lifts the hips with CUBICSPLINE tangents only: both keys
    /// sit at rest, so a player that ignores tangents sees no motion.
    fn cubic_pack_glb() -> Vec<u8> {
        use export::{AnimChannel, AnimData};
        let arm = canon::armature();
        let hips = arm.name_to_index["hips"];
        let r = arm.joints[hips].translation;
        // Per key: in-tangent, value, out-tangent. Out of key 0 rises at
        // 0.4 m/s; into key 1 falls at the same rate. Peak ≈ +5 cm.
        let anim = AnimData {
            channels: vec![AnimChannel {
                node: hips,
                path: "translation",
                times: vec![0.0, 1.0],
                values: vec![
                    0.0, 0.0, 0.0, r.x, r.y, r.z, 0.0, 0.4, 0.0, //
                    0.0, -0.4, 0.0, r.x, r.y, r.z, 0.0, 0.0, 0.0,
                ],
                interpolation: "CUBICSPLINE",
            }],
        };
        let bytes = write::write_animation_glb(&arm, &anim, "Bob").unwrap();
        with_skin(&bytes, &arm.skin)
    }

    /// [`moving_pack_glb`] authored against `arm`'s rest.
    fn moving_pack_glb_on(arm: &Armature) -> Vec<u8> {
        use export::{AnimChannel, AnimData};
        let node = arm.name_to_index[HumanBone::LeftUpperArm.as_str()];
        let hips = arm.name_to_index["hips"];
        let rest_hips = arm.joints[hips].translation;
        // Both kinds of channel. Rotation alone would not exercise
        // `retarget_translations`, and a fixture that cannot fail is worse than
        // no fixture: the first version of this test passed happily with a
        // deliberate divergence injected into the preview path.
        let anim = AnimData {
            channels: vec![
                AnimChannel {
                    node,
                    path: "rotation",
                    times: vec![0.0, 0.5, 1.0],
                    // Identity, a quarter turn about Z, and back.
                    values: vec![
                        0.0,
                        0.0,
                        0.0,
                        1.0, //
                        0.0,
                        0.0,
                        0.382_683_4,
                        0.923_879_5, //
                        0.0,
                        0.0,
                        0.0,
                        1.0,
                    ],
                    interpolation: "LINEAR",
                },
                AnimChannel {
                    node: hips,
                    path: "translation",
                    times: vec![0.0, 0.5, 1.0],
                    values: vec![
                        rest_hips.x,
                        rest_hips.y,
                        rest_hips.z, //
                        rest_hips.x,
                        rest_hips.y + 0.15,
                        rest_hips.z, //
                        rest_hips.x,
                        rest_hips.y,
                        rest_hips.z,
                    ],
                    interpolation: "LINEAR",
                },
            ],
        };
        // `write_animation_glb` is the *overlay* writer and emits no `skins`,
        // but a pack is loaded as an armature, which needs one. Only
        // `skin.joints()` is read, so the joint list alone is enough.
        let bytes = write::write_animation_glb(arm, &anim, "Reach").unwrap();
        with_skin(&bytes, &arm.skin)
    }

    /// Add a `skins` entry naming `joints`, repacking the container.
    fn with_skin(glb: &[u8], joints: &[usize]) -> Vec<u8> {
        let json_len = u32::from_le_bytes(glb[12..16].try_into().unwrap()) as usize;
        let mut doc: serde_json::Value = serde_json::from_slice(&glb[20..20 + json_len]).unwrap();
        let bin_start = 20 + json_len + 8;
        let bin_len =
            u32::from_le_bytes(glb[20 + json_len..24 + json_len].try_into().unwrap()) as usize;
        doc["skins"] = serde_json::json!([{ "joints": joints }]);
        crate::test_support::glb(
            &serde_json::to_vec(&doc).unwrap(),
            Some(&glb[bin_start..bin_start + bin_len]),
        )
    }

    /// A standing figure the landmark fit can read: torso, head, arms, legs.
    fn humanoid_glb() -> Vec<u8> {
        let mut v: Vec<Vec3> = Vec::new();
        let mut surfaces = Vec::new();
        let ring = |v: &mut Vec<Vec3>, y: f32, r: f32| {
            for i in 0..16 {
                let a = i as f32 * std::f32::consts::TAU / 16.0;
                v.push(Vec3::new(r * a.cos(), y, 0.6 * r * a.sin()));
            }
        };
        surfaces.push((v.len(), 24, 16));
        for i in 0..24 {
            ring(&mut v, 0.2 + i as f32 * 0.058, 0.12);
        }
        surfaces.push((v.len(), 8, 16));
        for i in 0..8 {
            ring(&mut v, 1.56 + i as f32 * 0.03, 0.09);
        }
        for s in [-1.0f32, 1.0] {
            surfaces.push((v.len(), 14, 6));
            for i in 0..14 {
                let x = s * (0.13 + i as f32 * 0.045);
                for k in 0..6 {
                    let a = k as f32 * std::f32::consts::TAU / 6.0;
                    v.push(Vec3::new(x, 1.4 + 0.05 * a.cos(), 0.05 * a.sin()));
                }
            }
            surfaces.push((v.len(), 16, 6));
            for i in 0..16 {
                let y = i as f32 * 0.0125;
                for k in 0..6 {
                    let a = k as f32 * std::f32::consts::TAU / 6.0;
                    v.push(Vec3::new(s * 0.1 + 0.05 * a.cos(), y, 0.05 * a.sin()));
                }
            }
        }
        // Connect each stack of rings into a closed surface. Sequential point
        // triples made disconnected ring caps, falsely classifying the middle
        // of the torso as outside once the fitter placed joints there.
        let mut indices = Vec::new();
        for (start, rings, sides) in surfaces {
            for r in 0..rings - 1 {
                for k in 0..sides {
                    let a = start + r * sides + k;
                    let b = start + r * sides + (k + 1) % sides;
                    indices.extend([a, b, a + sides, b, b + sides, a + sides].map(|i| i as u32));
                }
            }
            for base in [start, start + (rings - 1) * sides] {
                for k in 1..sides - 1 {
                    indices.extend([base, base + k, base + k + 1].map(|i| i as u32));
                }
            }
        }
        mesh_glb(&v, &indices)
    }

    fn mesh_glb(v: &[Vec3], idx: &[u32]) -> Vec<u8> {
        let mut bin: Vec<u8> = Vec::new();
        let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
        for p in v {
            lo = lo.min(*p);
            hi = hi.max(*p);
            for c in p.to_array() {
                bin.extend_from_slice(&c.to_le_bytes());
            }
        }
        let pos_len = bin.len();
        let idx_off = bin.len();
        for i in idx {
            bin.extend_from_slice(&i.to_le_bytes());
        }
        let idx_len = bin.len() - idx_off;
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        let json = serde_json::json!({
            "asset": {"version": "2.0"},
            "scene": 0,
            "scenes": [{"nodes": [0]}],
            "nodes": [{"mesh": 0, "name": "body"}],
            "meshes": [{"primitives": [{"attributes": {"POSITION": 0}, "indices": 1}]}],
            "accessors": [
                {"bufferView": 0, "componentType": 5126, "count": v.len(), "type": "VEC3",
                 "min": lo.to_array(), "max": hi.to_array()},
                {"bufferView": 1, "componentType": 5125, "count": idx.len(), "type": "SCALAR"}
            ],
            "bufferViews": [
                {"buffer": 0, "byteOffset": 0, "byteLength": pos_len},
                {"buffer": 0, "byteOffset": idx_off, "byteLength": idx_len}
            ],
            "buffers": [{"byteLength": bin.len()}]
        });
        crate::test_support::glb(&serde_json::to_vec(&json).unwrap(), Some(&bin))
    }
}
