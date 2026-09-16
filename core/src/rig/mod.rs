//! Bind a generated mesh to the shipped humanoid armature and apply a clip.
//!
//! One canonical skeleton ([`canon`], VRM-named), mesh-sampled
//! landmarks, inverse-distance weights, glTF skin + animation write.
//!
//! ```text
//! mesh.glb + clip pack
//!   → landmarks (optional) → fit + weight → apply clip (optional) → model.glb
//! ```

pub mod canon;
mod export;
mod fetch;
mod landmarks;
mod onmesh;
mod pack;
mod play;
mod skeleton;
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
    CLIP_PACK_ID, ClipCatalogEntry, ClipPack, ClipPackError, DEFAULT_BIND_CLIP, UAL1_PAGE,
    UAL2_PAGE, find_clip, install_pack_from, list_clips, pack_dir, packs_root, resolve_pack,
    resolve_packs,
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
    /// Fit and weight only — write a skinned T-pose, no animation.
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
pub fn bind_mesh(
    mesh_glb: &Path,
    out_glb: &Path,
    options: &BindOptions,
) -> Result<BindReport, BindError> {
    if options.fit_only || options.clips.is_empty() {
        return export::fit_and_write(mesh_glb, out_glb);
    }
    let clips = resolve_options_clips(options)?;
    // Re-fitting an already-rigged mesh throws away its pose and re-weights
    // to the auto-fit. Someone who arranged joints by hand and then asked for
    // another clip would silently lose that work, so bake onto the rig that
    // is already there unless a re-fit was asked for.
    if !options.refit && is_fitted(mesh_glb)? {
        return export::apply_clips_and_write(mesh_glb, &clips, out_glb);
    }
    export::bind_and_write(mesh_glb, &clips, out_glb)
}

/// Fit + weight a T-pose mesh. No animation is written.
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
    _options: &BindOptions,
) -> Result<BindReport, BindError> {
    export::fit_and_write_heads(mesh_glb, out_glb, Some(heads))
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

/// Heads that Bind would refuse: `(joint, metres to the nearest vertex)`,
/// farthest first. Empty means every head is inside or hugging the mesh.
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

/// Does this model already carry a canonical rig?
///
/// True when it has a skin and its nodes include canonical joints, which is
/// what [`bind_mesh`] uses to decide between baking onto the existing rig and
/// fitting a fresh one.
/// `Some(joint_count)` when the mesh carries a skin that is not ours.
///
/// [`is_fitted`] answers "is this rigged *by us*", and a foreign rig makes it
/// `false`, so every path treats such a mesh as unrigged and Bind overwrites
/// the skin. That is the right default, since we cannot animate a skeleton we
/// cannot name, but it silently discards work someone else did. Callers use
/// this to say so first.
///
/// Naming is the test, as everywhere else: the armature is canonicalized on
/// load, so a skin whose joints yield no [`HumanBone`] is one we do not know.
pub fn foreign_rig_joints(model_glb: &Path) -> Result<Option<usize>, BindError> {
    let doc = skeleton::import_json_only(model_glb)?;
    let Some(skin) = doc.skins().next() else {
        return Ok(None);
    };
    let joints: Vec<_> = skin.joints().collect();
    if joints.is_empty() {
        return Ok(None);
    }
    let ours = joints
        .iter()
        .filter_map(|n| n.name())
        .any(|n| canon::HumanBone::parse(n).is_some());
    Ok((!ours).then_some(joints.len()))
}

pub fn is_fitted(model_glb: &Path) -> Result<bool, BindError> {
    let doc = skeleton::import_json_only(model_glb)?;
    if doc.skins().next().is_none() {
        return Ok(false);
    }
    Ok(doc
        .nodes()
        .filter_map(|n| n.name())
        .any(|n| canon::HumanBone::parse(n).is_some()))
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
        use export::{AnimChannel, AnimData};
        let arm = canon::armature();
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
        let bytes = write::write_animation_glb(&arm, &anim, "Reach").unwrap();
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
        let ring = |v: &mut Vec<Vec3>, y: f32, r: f32| {
            for i in 0..16 {
                let a = i as f32 * std::f32::consts::TAU / 16.0;
                v.push(Vec3::new(r * a.cos(), y, 0.6 * r * a.sin()));
            }
        };
        for i in 0..24 {
            ring(&mut v, 0.2 + i as f32 * 0.058, 0.12);
        }
        for i in 0..8 {
            ring(&mut v, 1.56 + i as f32 * 0.03, 0.09);
        }
        for s in [-1.0f32, 1.0] {
            for i in 0..14 {
                let x = s * (0.13 + i as f32 * 0.045);
                for k in 0..6 {
                    let a = k as f32 * std::f32::consts::TAU / 6.0;
                    v.push(Vec3::new(x, 1.4 + 0.05 * a.cos(), 0.05 * a.sin()));
                }
            }
            for i in 0..16 {
                let y = i as f32 * 0.0125;
                for k in 0..6 {
                    let a = k as f32 * std::f32::consts::TAU / 6.0;
                    v.push(Vec3::new(s * 0.1 + 0.05 * a.cos(), y, 0.05 * a.sin()));
                }
            }
        }
        mesh_glb(&v)
    }

    /// Points as a GLB: one primitive, triangles fanned so the fit has faces.
    fn mesh_glb(v: &[Vec3]) -> Vec<u8> {
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
        let idx: Vec<u32> = (0..v.len() as u32).collect();
        let idx_off = bin.len();
        for i in &idx {
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
