//! Fit, weight, and write a skinned+animated GLB.

use super::landmarks::mesh_landmarks;
use super::skeleton::{self, Armature};
use super::weights::{segments_from_hierarchy, smooth_bind};
use super::write::{self, SourceGltf};
use super::{BindError, BindReport, canon, pack::ClipPack};
use glam::{Mat4, Quat, Vec2, Vec3};
use std::collections::HashMap;
use std::path::Path;

pub fn bind_and_write(
    mesh_glb: &Path,
    clips: &[(ClipPack, String)],
    out_glb: &Path,
) -> Result<BindReport, BindError> {
    let fitted = fit_from_mesh(mesh_glb, None)?;
    let prepared = prepare_clips(clips, &fitted.rest_before, &fitted.arm)?;
    write_fitted(out_glb, &fitted, &prepared.refs())?;
    Ok(fitted.into_report(&prepared.pack_id(), prepared.names()))
}

pub fn fit_and_write(mesh_glb: &Path, out_glb: &Path) -> Result<BindReport, BindError> {
    fit_and_write_heads(mesh_glb, out_glb, None, &[])
}

/// Bind from arranged heads. The heads define the skeleton and the weights
/// are computed from that skeleton (Meshy / Mixamo). A head off the mesh is
/// refused by name before anything is written — see [`super::onmesh`].
///
/// `keep_clips` are animation names to re-bake onto the new fit, resolved
/// through the pack catalog ([`super::find_clip`]). A re-Bind of a mesh that
/// already carries baked clips used to write an empty set and silently drop
/// them; passing `baked_clip_names` here keeps them. Names that no installed
/// pack provides any more (a foreign animation, an uninstalled pack) are not
/// an error: they come back in [`BindReport::dropped_clips`] so the caller can
/// say so.
pub fn fit_and_write_heads(
    mesh_glb: &Path,
    out_glb: &Path,
    heads: Option<&[(String, [f32; 3])]>,
    keep_clips: &[String],
) -> Result<BindReport, BindError> {
    let fitted = fit_from_mesh(mesh_glb, heads)?;
    let mut resolved = Vec::with_capacity(keep_clips.len());
    let mut dropped = Vec::new();
    for clip in keep_clips {
        match super::find_clip(clip) {
            Ok(found) => resolved.push(found),
            Err(_) => dropped.push(clip.clone()),
        }
    }
    let prepared = prepare_clips(&resolved, &fitted.rest_before, &fitted.arm)?;
    write_fitted(out_glb, &fitted, &prepared.refs())?;
    let pack_id = if resolved.is_empty() {
        canon::SKELETON_ID.to_string()
    } else {
        prepared.pack_id()
    };
    let mut report = fitted.into_report(&pack_id, prepared.names());
    report.dropped_clips = dropped;
    Ok(report)
}

/// Rewrite an already-fitted mesh unchanged: same skeleton, weights and
/// clips. What `bind --fit-only` means on a rigged mesh without `--refit`.
///
/// Goes through the same staged, validated write as every other path so a
/// damaged source is refused rather than copied.
pub fn rewrite_fitted(rest_glb: &Path, out_glb: &Path) -> Result<BindReport, BindError> {
    let (rest_doc, rest_buffers) = skeleton::load_document(rest_glb)?;
    let baked = bake_mesh(&rest_doc, &rest_buffers)?;
    let clips: Vec<String> = rest_doc
        .animations()
        .filter_map(|a| a.name().map(str::to_string))
        .collect();
    let joint_count = rest_doc.skins().next().map_or(0, |s| s.joints().count());
    let bytes = std::fs::read(rest_glb).map_err(|e| BindError::Io {
        path: rest_glb.to_path_buf(),
        source: e,
    })?;
    write::write_bytes_atomic(out_glb, &bytes)?;
    let landmarks = mesh_landmarks(&baked.positions_flat()).unwrap_or_else(|_| dummy_landmarks());
    Ok(BindReport {
        pack_id: canon::SKELETON_ID.to_string(),
        clips,
        joint_count,
        vertex_count: baked.vertex_count(),
        landmarks,
        moved_joints: 0,
        max_moved_m: 0.0,
        dropped_clips: Vec::new(),
    })
}

/// Heads Bind would refuse, farthest first. Reads only the mesh.
pub fn heads_off_mesh(
    mesh_glb: &Path,
    heads: &[(String, [f32; 3])],
) -> Result<Vec<(String, f32)>, BindError> {
    let (mesh_doc, mesh_buffers, _) = load_mesh_inputs(mesh_glb)?;
    let baked = bake_mesh(&mesh_doc, &mesh_buffers)?;
    let shell = super::onmesh::Shell::from_baked(&baked);
    Ok(super::onmesh::off_mesh(&shell, heads))
}

/// Canonical armature posed to the mesh. No weights, no write — Rig seed.
pub fn seed_bind_markers(mesh_glb: &Path) -> Result<Vec<super::BindMarker>, BindError> {
    let (arm, _) = pose_armature(mesh_glb)?;
    Ok(markers_from_arm(&arm))
}

/// Fraction of a figure's height spanned by the joint chain, ankle to head
/// joint. The remainder is the foot below the ankle and the skull above the
/// head joint.
const JOINT_SPAN_OF_HEIGHT: f32 = 0.88;

/// Fraction of the mesh's width the arm span may take, fingertip to fingertip.
const JOINT_SPAN_OF_WIDTH: f32 = 0.94;

/// Where the ankles sit above the soles, as a fraction of height.
const ANKLES_ABOVE_FLOOR: f32 = 0.06;

/// The shipped skeleton scaled to the mesh, with no attempt to read the body.
///
/// This is what Rig opens with. It cannot fail on any mesh, which is the point:
/// auto-fit is landmark-driven and a landmark it cannot find used to leave the
/// workbench with no joints at all and no way to summon any. Placement is the
/// user's job and this is their starting pose; [`seed_bind_markers`] is the
/// accelerator they can reach for once they are in there.
///
/// The fit is deliberately dumb: uniform scale so the skeleton spans the mesh's
/// height, centered on the mesh, feet on its floor. Nothing here inspects the
/// silhouette, so nothing here has an opinion about whether the mesh is a
/// character.
pub fn default_bind_markers(mesh_glb: &Path) -> Result<Vec<super::BindMarker>, BindError> {
    let (mesh_doc, mesh_buffers, _) = load_mesh_inputs(mesh_glb)?;
    let baked = bake_mesh(&mesh_doc, &mesh_buffers)?;
    let positions = baked.positions_flat();
    if positions.is_empty() {
        return Err(BindError::Failed("mesh has no geometry".into()));
    }
    let (mut lo, mut hi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
    for p in &positions {
        lo = lo.min(*p);
        hi = hi.max(*p);
    }

    let mut arm = canon::armature();
    let heads = arm.world_heads();
    let (mut rlo, mut rhi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
    for h in &heads {
        rlo = rlo.min(*h);
        rhi = rhi.max(*h);
    }

    // The joint chain is shorter than the figure it belongs to: the lowest
    // joint is an ankle sitting above the sole, and the highest is a head joint
    // sitting below the crown. Mapping joint-span onto full mesh height would
    // stretch the skeleton past both ends, so it lands in the middle band
    // instead.
    let mesh_h = (hi.y - lo.y).max(1e-6);
    let by_height = (mesh_h * JOINT_SPAN_OF_HEIGHT) / (rhi.y - rlo.y).max(1e-6);
    // A T-pose is about as wide as it is tall, so height alone throws the hands
    // outside anything narrower than a person. Take whichever axis binds first
    // and the whole skeleton starts inside the mesh, wherever it came from.
    let by_width = ((hi.x - lo.x).max(1e-6) * JOINT_SPAN_OF_WIDTH) / (rhi.x - rlo.x).max(1e-6);
    let scale = by_height.min(by_width).clamp(1e-3, 1e3);
    for j in &mut arm.joints {
        j.translation *= scale;
    }
    // Re-measure after scaling, then sit the skeleton on the mesh's floor and
    // center it left-to-right and front-to-back.
    let heads = arm.world_heads();
    let (mut slo, mut shi) = (Vec3::splat(f32::INFINITY), Vec3::splat(f32::NEG_INFINITY));
    for h in &heads {
        slo = slo.min(*h);
        shi = shi.max(*h);
    }
    let skel_h = (shi.y - slo.y).max(1e-6);
    let want = Vec3::new(
        (lo.x + hi.x) * 0.5,
        lo.y + skel_h * (ANKLES_ABOVE_FLOOR / JOINT_SPAN_OF_HEIGHT),
        (lo.z + hi.z) * 0.5,
    );
    let have = Vec3::new((slo.x + shi.x) * 0.5, slo.y, (slo.z + shi.z) * 0.5);
    if let Some(root) = arm.joints.first_mut() {
        root.translation += want - have;
    }
    Ok(markers_from_arm(&arm))
}

/// Declarative multi-clip bake: the output's animations become exactly `clips`.
///
/// Clips may come from different packs — each is rebased into the canonical
/// armature's index space before it is written, so one file can hold
/// animations sourced from several libraries. An empty set strips animation.
pub fn apply_clips_and_write(
    rest_glb: &Path,
    clips: &[(ClipPack, String)],
    out_glb: &Path,
) -> Result<BindReport, BindError> {
    let (rest_doc, rest_buffers) = skeleton::load_document(rest_glb)?;
    let baked = bake_mesh(&rest_doc, &rest_buffers)?;
    let rest_before = canon::armature();
    let mut arm = rest_before.clone();
    overlay_trs_by_name(&mut arm, &skeleton::load_from_gltf(&rest_doc)?);

    let prepared = prepare_clips(clips, &rest_before, &arm)?;
    let bytes = write::patch_animations_glb(rest_glb, &arm, &prepared.refs())?;
    write::write_bytes_atomic(out_glb, &bytes)?;

    let landmarks = mesh_landmarks(&baked.positions_flat()).unwrap_or_else(|_| dummy_landmarks());
    Ok(BindReport {
        pack_id: prepared.pack_id(),
        clips: prepared.names(),
        joint_count: arm.skin.len(),
        vertex_count: baked.vertex_count(),
        landmarks,
        moved_joints: 0,
        max_moved_m: 0.0,
        dropped_clips: Vec::new(),
    })
}

struct Fitted {
    baked: BakedMesh,
    source: SourceGltf,
    arm: Armature,
    rest_before: Armature,
    skin: super::weights::Skinning,
    ibms: Vec<Mat4>,
    landmarks: super::landmarks::Landmarks,
    moved: MovedJoints,
}

/// How far the arranged heads departed from the auto-fit — proof to the
/// author that Bind consumed the pose.
#[derive(Debug, Clone, Copy, Default)]
struct MovedJoints {
    count: usize,
    max_m: f32,
}

impl MovedJoints {
    /// Placeable joints whose world head moved more than 1 cm.
    fn between(auto: &Armature, arranged: &Armature) -> Self {
        let a = auto.world_heads();
        let b = arranged.world_heads();
        let mut out = Self::default();
        for (i, joint) in arranged.joints.iter().enumerate() {
            if !crate::rig::weights::is_placeable_joint(&joint.name) {
                continue;
            }
            let Some(before) = a.get(i) else {
                continue;
            };
            let d = (b[i] - *before).length();
            if d > 0.01 {
                out.count += 1;
                out.max_m = out.max_m.max(d);
            }
        }
        out
    }
}

impl Fitted {
    fn into_report(self, pack_id: &str, clips: Vec<String>) -> BindReport {
        BindReport {
            pack_id: pack_id.to_string(),
            clips,
            joint_count: self.arm.skin.len(),
            vertex_count: self.baked.vertex_count(),
            landmarks: self.landmarks,
            moved_joints: self.moved.count,
            max_moved_m: self.moved.max_m,
            dropped_clips: Vec::new(),
        }
    }
}

fn load_mesh_inputs(
    path: &Path,
) -> Result<(gltf::Document, Vec<gltf::buffer::Data>, SourceGltf), BindError> {
    let (doc, buffers) = skeleton::import_no_images(path)?;
    let source = SourceGltf::load(path)?;
    Ok((doc, buffers, source))
}

/// Fit the *embedded* canonical skeleton to the mesh — no clip pack involved.
/// Packs supply clips; the skeleton ships with the binary, so Rig and Bind
/// work on a fresh install with nothing downloaded.
fn pose_armature(mesh_glb: &Path) -> Result<(Armature, FittedPose), BindError> {
    let (mesh_doc, mesh_buffers, source) = load_mesh_inputs(mesh_glb)?;
    let baked = bake_mesh(&mesh_doc, &mesh_buffers)?;
    let positions = baked.positions_flat();
    let mut landmarks = mesh_landmarks(&positions).map_err(BindError::Failed)?;
    super::body_fit::refine_arms(&positions, &mut landmarks);
    let mut arm = canon::armature();
    let rest_before = arm.clone();
    skeleton::fit(&mut arm, &landmarks, &positions);
    // Auto-fit must hand back a pose Bind will accept. Landmarks are centroids
    // of bands, so a sleeve or a pauldron can put a shoulder outside the body,
    // and Bind then refuses the fit the author just asked for.
    let shell = super::onmesh::Shell::from_baked(&baked);
    let pulled = arm.pull_heads_onto_mesh(&|p| shell.is_on_mesh(p));
    if !pulled.is_empty() {
        tracing::debug!(joints = %pulled.join(", "), "pulled off-mesh heads inboard");
    }
    Ok((
        arm,
        FittedPose {
            baked,
            source,
            rest_before,
            landmarks,
        },
    ))
}

struct FittedPose {
    baked: BakedMesh,
    source: SourceGltf,
    rest_before: Armature,
    landmarks: super::landmarks::Landmarks,
}

/// Heads define the skeleton; weights follow it (Meshy / Mixamo). Move the
/// knee and the leg bends at the new knee because the verts around it are
/// reassigned. Weighting the auto-fit and only moving pivots leaves verts
/// owned by a bone they now sit on the wrong side of — a pinch every stride.
///
/// A head off the mesh gets ~0 inverse-distance weight and drives nothing,
/// so Bind refuses it by name rather than writing a rig that looks stock.
fn fit_from_mesh(
    mesh_glb: &Path,
    heads: Option<&[(String, [f32; 3])]>,
) -> Result<Fitted, BindError> {
    let (auto_arm, pose) = pose_armature(mesh_glb)?;
    let mut arm = auto_arm.clone();
    let mut moved = MovedJoints::default();
    if let Some(heads) = heads.filter(|h| !h.is_empty()) {
        let shell = super::onmesh::Shell::from_baked(&pose.baked);
        let off = super::onmesh::off_mesh(&shell, heads);
        if !off.is_empty() {
            return Err(BindError::OffMesh(off));
        }
        arm.apply_world_heads_parent_first(heads);
        moved = MovedJoints::between(&auto_arm, &arm);
    }
    let (skin, ibms) = weight_armature(&arm, &pose.baked.positions_flat());
    Ok(Fitted {
        baked: pose.baked,
        source: pose.source,
        arm,
        rest_before: pose.rest_before,
        skin,
        ibms,
        landmarks: pose.landmarks,
        moved,
    })
}

fn markers_from_arm(arm: &Armature) -> Vec<super::BindMarker> {
    let worlds = arm.world_matrices();
    let mut out = Vec::new();
    for &ji in &arm.skin {
        let Some(joint) = arm.joints.get(ji) else {
            continue;
        };
        if !crate::rig::weights::is_placeable_joint(&joint.name) {
            continue;
        }
        if ji >= worlds.len() {
            continue;
        }
        let w = worlds[ji].transform_point3(Vec3::ZERO);
        out.push(super::BindMarker {
            name: joint.name.clone(),
            parent_name: bind_ancestor_name(arm, joint.parent),
            world: w.to_array(),
        });
    }
    out
}

fn bind_ancestor_name(arm: &Armature, mut parent: Option<usize>) -> String {
    while let Some(i) = parent {
        let Some(j) = arm.joints.get(i) else {
            break;
        };
        if crate::rig::weights::is_placeable_joint(&j.name) {
            return j.name.clone();
        }
        parent = j.parent;
    }
    String::new()
}

fn weight_armature(arm: &Armature, positions: &[Vec3]) -> (super::weights::Skinning, Vec<Mat4>) {
    let worlds = arm.world_matrices();
    let heads: Vec<Vec3> = worlds
        .iter()
        .map(|m| m.transform_point3(Vec3::ZERO))
        .collect();
    let children = arm.children();
    let segs: Vec<_> = arm
        .skin
        .iter()
        .enumerate()
        .filter(|(_, ji)| crate::rig::weights::is_bind_bone(&arm.joints[**ji].name))
        .map(|(si, &ji)| {
            let kids: Vec<Vec3> = children[ji]
                .iter()
                .filter_map(|c| {
                    if !crate::rig::weights::is_bind_bone(&arm.joints[*c].name) {
                        return None;
                    }
                    arm.skin.iter().position(|&x| x == *c).map(|_| heads[*c])
                })
                .collect();
            let a = heads[ji];
            let b = if kids.is_empty() {
                a + Vec3::Y * 0.05
            } else {
                kids.iter().copied().sum::<Vec3>() / kids.len() as f32
            };
            crate::rig::weights::BoneSeg {
                joint: si as u16,
                a,
                b,
            }
        })
        .collect();
    let segs = if segs.iter().all(|s| (s.a - s.b).length_squared() < 1e-10) {
        let full = segments_from_hierarchy(&heads, &children, 0.05);
        arm.skin
            .iter()
            .enumerate()
            .filter(|(_, ji)| crate::rig::weights::is_bind_bone(&arm.joints[**ji].name))
            .map(|(si, &ji)| crate::rig::weights::BoneSeg {
                joint: si as u16,
                a: full[ji].a,
                b: full[ji].b,
            })
            .collect()
    } else {
        segs
    };
    let skin = smooth_bind(positions, &segs, 2.5);
    (skin, inverse_bind_matrices(arm))
}

fn inverse_bind_matrices(arm: &Armature) -> Vec<Mat4> {
    let worlds = arm.world_matrices();
    arm.skin.iter().map(|&ji| worlds[ji].inverse()).collect()
}

fn overlay_trs_by_name(dst: &mut Armature, src: &Armature) {
    for j in &mut dst.joints {
        if let Some(&i) = src.name_to_index.get(&j.name) {
            j.translation = src.joints[i].translation;
            j.rotation = src.joints[i].rotation;
            j.scale = src.joints[i].scale;
        }
    }
}

fn write_fitted(
    out_glb: &Path,
    fitted: &Fitted,
    anims: &[(&AnimData, &str)],
) -> Result<(), BindError> {
    let glb = write::write_skinned_glb(
        &fitted.source,
        &fitted.baked,
        &fitted.arm,
        &fitted.skin,
        &fitted.ibms,
        anims,
    )?;
    write::write_bytes_atomic(out_glb, &glb)
}

fn dummy_landmarks() -> super::landmarks::Landmarks {
    super::landmarks::Landmarks {
        hips: Vec3::ZERO,
        head: Vec3::Y,
        hand_l: Vec3::X,
        hand_r: -Vec3::X,
        shoulder_l: Vec3::new(0.3, 0.8, 0.0),
        shoulder_r: Vec3::new(-0.3, 0.8, 0.0),
        hip_l: Vec3::new(0.1, 0.0, 0.0),
        hip_r: Vec3::new(-0.1, 0.0, 0.0),
        ankle_l: Vec3::new(0.1, -0.9, 0.0),
        ankle_r: Vec3::new(-0.1, -0.9, 0.0),
        toe_l: Vec3::new(0.1, -0.95, 0.1),
        toe_r: Vec3::new(-0.1, -0.95, 0.1),
        toe_tip_l: Vec3::new(0.1, -0.95, 0.15),
        toe_tip_r: Vec3::new(-0.1, -0.95, 0.15),
        up: 1,
    }
}

#[derive(Debug)]
pub(super) struct BakedMesh {
    pub(super) parts: Vec<BakedPart>,
}

#[derive(Debug)]
pub(super) struct BakedPart {
    pub(super) name: Option<String>,
    pub(super) primitives: Vec<BakedPrimitive>,
}

#[derive(Debug)]
pub(super) struct BakedPrimitive {
    pub(super) positions: Vec<Vec3>,
    pub(super) normals: Option<Vec<Vec3>>,
    pub(super) texcoords0: Option<Vec<Vec2>>,
    pub(super) texcoords1: Option<Vec<Vec2>>,
    pub(super) colors: Option<Vec<[f32; 4]>>,
    pub(super) tangents: Option<Vec<[f32; 4]>>,
    pub(super) indices: Vec<u32>,
    pub(super) material: Option<usize>,
    pub(super) mode: u32,
}

impl BakedMesh {
    pub(super) fn vertex_count(&self) -> usize {
        self.parts
            .iter()
            .flat_map(|p| p.primitives.iter())
            .map(|p| p.positions.len())
            .sum()
    }

    pub(super) fn positions_flat(&self) -> Vec<Vec3> {
        self.parts
            .iter()
            .flat_map(|p| p.primitives.iter())
            .flat_map(|p| p.positions.iter().copied())
            .collect()
    }
}

fn bake_mesh(doc: &gltf::Document, buffers: &[gltf::buffer::Data]) -> Result<BakedMesh, BindError> {
    if doc.meshes().next().is_none() {
        return Err(BindError::Failed("mesh glTF has no mesh".into()));
    }
    let mut parts = Vec::new();
    for mesh in doc.meshes() {
        let world = mesh_world_for(doc, mesh.index());
        let normal_m = normal_matrix(world);
        let mut primitives = Vec::new();
        for prim in mesh.primitives() {
            if prim.morph_targets().len() > 0 {
                return Err(BindError::Failed(
                    "morph targets are not supported; Fit would discard them".into(),
                ));
            }
            match prim.mode() {
                gltf::mesh::Mode::Triangles
                | gltf::mesh::Mode::TriangleStrip
                | gltf::mesh::Mode::TriangleFan => {}
                other => {
                    return Err(BindError::Failed(format!(
                        "unsupported primitive mode {other:?}"
                    )));
                }
            }
            let reader = prim.reader(|b| Some(buffers.get(b.index())?.0.as_slice()));
            let positions: Vec<Vec3> = reader
                .read_positions()
                .ok_or_else(|| BindError::Failed("primitive missing POSITION".into()))?
                .map(|p| world.transform_point3(Vec3::from_array(p)))
                .collect();
            if positions.is_empty() {
                return Err(BindError::Failed(
                    "primitive has no POSITION vertices".into(),
                ));
            }
            // A NaN or infinite vertex poisons every distance downstream:
            // landmark bands, inverse-distance weights, the on-mesh shell.
            // Refuse it here, once, with a message, rather than let it surface
            // as a sort panic on the GUI thread.
            if let Some(i) = positions.iter().position(|p| !p.is_finite()) {
                return Err(BindError::Failed(format!(
                    "mesh has a non-finite POSITION (vertex {i}); the model is damaged"
                )));
            }
            let normals = reader.read_normals().map(|n| {
                n.map(|v| {
                    normal_m
                        .transform_vector3(Vec3::from_array(v))
                        .normalize_or_zero()
                })
                .collect()
            });
            let texcoords0 = reader
                .read_tex_coords(0)
                .map(|uv| uv.into_f32().map(Vec2::from_array).collect());
            let texcoords1 = reader
                .read_tex_coords(1)
                .map(|uv| uv.into_f32().map(Vec2::from_array).collect());
            let colors = reader.read_colors(0).map(|c| c.into_rgba_f32().collect());
            let tangents = reader.read_tangents().map(|t| t.collect());
            let indices: Vec<u32> = match reader.read_indices() {
                Some(idx) => idx.into_u32().collect(),
                None => (0..positions.len() as u32).collect(),
            };
            primitives.push(BakedPrimitive {
                positions,
                normals,
                texcoords0,
                texcoords1,
                colors,
                tangents,
                indices,
                material: prim.material().index(),
                mode: primitive_mode(prim.mode()),
            });
        }
        if primitives.is_empty() {
            continue;
        }
        parts.push(BakedPart {
            name: mesh.name().map(|s| s.to_string()),
            primitives,
        });
    }
    if parts.is_empty() {
        return Err(BindError::Failed("mesh glTF has no primitives".into()));
    }
    Ok(BakedMesh { parts })
}

/// The matrix that carries normals through `world`: the inverse-transpose of
/// its upper 3×3.
///
/// A rotation pulled out of the matrix (`Quat::from_mat4`) is only right for a
/// rigid transform. Provider output routinely carries a non-uniform node scale
/// (a Z-up bake, a squashed axis), and under that the rotation is ill-defined
/// and the normals it produces tilt off the surface. A singular matrix has no
/// inverse; fall back to the plain 3×3 rather than emit NaN normals.
fn normal_matrix(world: Mat4) -> Mat4 {
    let m = glam::Mat3::from_mat4(world);
    let det = m.determinant();
    if det.is_finite() && det.abs() > 1e-12 {
        Mat4::from_mat3(m.inverse().transpose())
    } else {
        Mat4::from_mat3(m)
    }
}

fn primitive_mode(mode: gltf::mesh::Mode) -> u32 {
    match mode {
        gltf::mesh::Mode::Points => 0,
        gltf::mesh::Mode::Lines => 1,
        gltf::mesh::Mode::LineLoop => 2,
        gltf::mesh::Mode::LineStrip => 3,
        gltf::mesh::Mode::Triangles => 4,
        gltf::mesh::Mode::TriangleStrip => 5,
        gltf::mesh::Mode::TriangleFan => 6,
    }
}

fn mesh_world_for(doc: &gltf::Document, mesh_index: usize) -> Mat4 {
    for node in doc.nodes() {
        if node.mesh().map(|m| m.index()) == Some(mesh_index) {
            return node_world(doc, node.index());
        }
    }
    Mat4::IDENTITY
}

fn node_world(doc: &gltf::Document, index: usize) -> Mat4 {
    let nodes: Vec<_> = doc.nodes().collect();
    let mut parent_of = HashMap::new();
    for n in &nodes {
        for c in n.children() {
            parent_of.insert(c.index(), n.index());
        }
    }
    let mut chain = Vec::new();
    let mut i = Some(index);
    while let Some(cur) = i {
        chain.push(cur);
        i = parent_of.get(&cur).copied();
    }
    chain.reverse();
    let mut m = Mat4::IDENTITY;
    for idx in chain {
        let (t, r, s) = nodes[idx].transform().decomposed();
        m *= Mat4::from_scale_rotation_translation(
            Vec3::from_array(s),
            Quat::from_array(r),
            Vec3::from_array(t),
        );
    }
    m
}

pub(super) struct AnimData {
    pub(super) channels: Vec<AnimChannel>,
}

pub(super) struct AnimChannel {
    pub(super) node: usize,
    pub(super) path: &'static str,
    pub(super) times: Vec<f32>,
    pub(super) values: Vec<f32>,
    pub(super) interpolation: &'static str,
}

/// Find `requested` among the document's animations.
///
/// Exact (case-insensitive) only, deliberately: short names like `walk` are
/// expanded by [`ClipPack::find_clip`](crate::rig::ClipPack::find_clip) before
/// a clip ever reaches here, and every caller goes through it. A second alias
/// table at this level could only disagree with that one, and the copy that
/// used to live here still spoke the pre-canonical `walk_loop` / `a_walk`
/// vocabulary.
fn resolve_clip_name(doc: &gltf::Document, requested: &str) -> Result<String, BindError> {
    let names: Vec<String> = doc
        .animations()
        .filter_map(|a| a.name().map(|n| n.to_string()))
        .collect();
    if names.is_empty() {
        return Err(BindError::Failed("clip pack has no animations".into()));
    }
    names
        .iter()
        .find(|n| n.eq_ignore_ascii_case(requested))
        .cloned()
        .ok_or_else(|| {
            BindError::Failed(format!(
                "clip '{requested}' not in pack; have: {}",
                names.join(", ")
            ))
        })
}

fn extract_animation(
    doc: &gltf::Document,
    buffers: &[gltf::buffer::Data],
    name: &str,
) -> Result<AnimData, BindError> {
    let anim = doc
        .animations()
        .find(|a| a.name() == Some(name))
        .ok_or_else(|| BindError::Failed(format!("animation '{name}' missing after resolve")))?;
    let mut channels = Vec::new();
    for ch in anim.channels() {
        let reader = ch.reader(|b| Some(buffers.get(b.index())?.0.as_slice()));
        let times: Vec<f32> = reader
            .read_inputs()
            .ok_or_else(|| BindError::Failed("animation channel missing times".into()))?
            .collect();
        let path = match ch.target().property() {
            gltf::animation::Property::Translation => "translation",
            gltf::animation::Property::Rotation => "rotation",
            gltf::animation::Property::Scale => "scale",
            gltf::animation::Property::MorphTargetWeights => continue,
        };
        let comps = match path {
            "rotation" => 4,
            _ => 3,
        };
        let values: Vec<f32> = match reader.read_outputs() {
            Some(gltf::animation::util::ReadOutputs::Translations(it)) => {
                it.flat_map(|v| v.into_iter()).collect()
            }
            Some(gltf::animation::util::ReadOutputs::Rotations(it)) => {
                it.into_f32().flat_map(|v| v.into_iter()).collect()
            }
            Some(gltf::animation::util::ReadOutputs::Scales(it)) => {
                it.flat_map(|v| v.into_iter()).collect()
            }
            _ => continue,
        };
        let interpolation = match ch.sampler().interpolation() {
            gltf::animation::Interpolation::Linear => "LINEAR",
            gltf::animation::Interpolation::Step => "STEP",
            gltf::animation::Interpolation::CubicSpline => "CUBICSPLINE",
        };
        // The output accessor is read raw: LINEAR / STEP hold one sample per
        // time, CUBICSPLINE three (in-tangent, value, out-tangent). Anything
        // else is a channel we cannot play or write. A shared time accessor
        // with mixed key counts is what crashed Blender
        // (`assign sequence of size 33 to slice of size 2`).
        let n_times = times.len();
        let n_samples = values.len() / comps;
        let interpolation = if n_samples == n_times {
            if interpolation == "CUBICSPLINE" {
                "LINEAR"
            } else {
                interpolation
            }
        } else if interpolation == "CUBICSPLINE" && n_samples == n_times * 3 {
            "CUBICSPLINE"
        } else {
            return Err(BindError::Failed(format!(
                "animation '{name}' channel {path} has {n_times} times and {n_samples} samples"
            )));
        };
        channels.push(AnimChannel {
            node: ch.target().node().index(),
            path,
            times,
            values,
            interpolation,
        });
    }
    if channels.is_empty() {
        return Err(BindError::Failed(format!(
            "animation '{name}' has no TRS channels"
        )));
    }
    Ok(AnimData { channels })
}

/// Pack-space clip → fitted rest: shift translations, keep fitted scales.
/// Clips resolved and rebased onto a fitted armature, ready to write.
struct PreparedClips {
    anims: Vec<(AnimData, String)>,
    packs: Vec<String>,
}

impl PreparedClips {
    fn refs(&self) -> Vec<(&AnimData, &str)> {
        self.anims.iter().map(|(a, n)| (a, n.as_str())).collect()
    }

    fn names(&self) -> Vec<String> {
        self.anims.iter().map(|(_, n)| n.clone()).collect()
    }

    /// Provenance for a bake that may have drawn on several libraries.
    fn pack_id(&self) -> String {
        self.packs.join(PACK_ID_SEPARATOR)
    }
}

/// Joins the pack ids behind a multi-library bake, e.g. `ual1+ual2`.
const PACK_ID_SEPARATOR: &str = "+";

/// Resolve and rebase every clip in `clips` onto `fitted`.
///
/// Shared by the fit-then-bake path and the bake-onto-an-existing-rest path,
/// which differ only in where the rest armature comes from.
fn prepare_clips(
    clips: &[(ClipPack, String)],
    canon_rest: &Armature,
    fitted: &Armature,
) -> Result<PreparedClips, BindError> {
    let mut anims = Vec::with_capacity(clips.len());
    let mut packs: Vec<String> = Vec::new();
    for (pack, clip) in clips {
        let (pack_doc, pack_buffers) = skeleton::load_document(&pack.gltf_path)?;
        let (anim, name) = prepare_clip_anim(&pack_doc, &pack_buffers, clip, canon_rest, fitted)?;
        if !packs.contains(&pack.id) {
            packs.push(pack.id.clone());
        }
        anims.push((anim, name));
    }
    Ok(PreparedClips { anims, packs })
}

/// Extract `clip` from a pack and express it in the **canonical** armature's
/// node-index space, retargeted onto `fitted`.
///
/// The index rebase is the load-bearing step. A pack's channels index that
/// pack's nodes (67 for the Quaternius libraries), while everything we write
/// indexes the canonical armature (54). Passing pack indices to a
/// canonical-indexed writer silently lands every key on whatever joint happens
/// to share that number — the animation plays, and it is scrambled. Rebasing
/// by name here is also what lets clips from different packs land in one file.
fn prepare_clip_anim(
    doc: &gltf::Document,
    buffers: &[gltf::buffer::Data],
    clip: &str,
    canon_rest: &Armature,
    fitted: &Armature,
) -> Result<(AnimData, String), BindError> {
    let clip_name = resolve_clip_name(doc, clip)?;
    let mut anim = extract_animation(doc, buffers, &clip_name)?;
    let pack_arm = skeleton::load_from_gltf(doc)?;
    rebase_channels(&mut anim, &pack_arm, canon_rest);
    // Translation keys are authored against the *pack's* rest, so that is
    // what they are shifted from. Expressed in canonical index space so the
    // delta lines up with the rebased channels. Preview and Bake both come
    // through here with `canon_rest` = `canon::armature()`, which is what
    // makes them agree pose for pose even on a pack whose rest differs from
    // the embedded one.
    let mut pack_rest = canon_rest.clone();
    overlay_trs_by_name(&mut pack_rest, &pack_arm);
    retarget_translations(&pack_rest, fitted, &mut anim);
    drop_scale_channels(&mut anim);
    if anim.channels.is_empty() {
        return Err(BindError::Failed(format!(
            "animation '{clip_name}' has no rotation/translation channels"
        )));
    }
    Ok((anim, clip_name))
}

/// Move channel node indices from `src`'s space into `dst`'s, matching by name.
/// Channels targeting nodes `dst` does not have (leaf tips, wrappers) are
/// dropped — they drive nothing.
fn rebase_channels(anim: &mut AnimData, src: &Armature, dst: &Armature) {
    let map: HashMap<usize, usize> = src
        .joints
        .iter()
        .enumerate()
        .filter_map(|(i, j)| dst.name_to_index.get(&j.name).map(|&d| (i, d)))
        .collect();
    anim.channels.retain_mut(|ch| match map.get(&ch.node) {
        Some(&dst_node) => {
            ch.node = dst_node;
            true
        }
        None => false,
    });
}

/// Shift translation keys so they sit on the fitted rest, not the pack rest.
///
/// A CUBICSPLINE channel stores `[in-tangent, value, out-tangent]` per key.
/// Only the value is a position; the tangents are derivatives and shifting
/// them would bend every curve.
fn retarget_translations(src: &Armature, dst: &Armature, anim: &mut AnimData) {
    for ch in &mut anim.channels {
        if ch.path != "translation" || ch.node >= src.joints.len() {
            continue;
        }
        let delta = dst.joints[ch.node].translation - src.joints[ch.node].translation;
        let (stride, at) = if ch.interpolation == "CUBICSPLINE" {
            (9, 3)
        } else {
            (3, 0)
        };
        for key in ch.values.chunks_mut(stride) {
            if let Some(sample) = key.get_mut(at..at + 3) {
                sample[0] += delta.x;
                sample[1] += delta.y;
                sample[2] += delta.z;
            }
        }
    }
}

/// Pack scale tracks are authored for the unfitted mannequin. Playing them on
/// a fitted rest (root ~0.63, aimed limbs) overwrites those scales and the
/// mesh looks skinny. Rotation + retargeted translation is the clip; rest
/// scale stays on the node.
fn drop_scale_channels(anim: &mut AnimData) {
    anim.channels.retain(|ch| ch.path != "scale");
}

/// Animation-only GLB retargeted onto a fitted rest (same space as Bake).
///
/// The viewer overlays this via `SkinnedClip` without rewriting `model.glb`.
///
/// Built exactly the way [`apply_clips_and_write`] builds a bake: the
/// canonical armature overlaid with the rest GLB's joints, and the clip
/// prepared against it. It used to start from the pack's own armature
/// instead, which agreed with Bake only as long as the pack's rest matched
/// the embedded one; `preview_and_bake_agree_on_a_pack_whose_rest_differs`
/// is the guard.
pub fn animation_overlay_for_rest(
    source: &Path,
    clip: &str,
    rest_glb: &Path,
) -> Result<Vec<u8>, BindError> {
    let (src_doc, src_buffers) = skeleton::load_document(source)?;
    let rest_doc = skeleton::import_json_only(rest_glb)?;
    let canon_rest = canon::armature();
    let mut fitted = canon_rest.clone();
    overlay_trs_by_name(&mut fitted, &skeleton::load_from_gltf(&rest_doc)?);
    let (anim, clip_name) = prepare_clip_anim(&src_doc, &src_buffers, clip, &canon_rest, &fitted)?;
    write::write_animation_glb(&fitted, &anim, &clip_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// No vertex may end up weighted to the armature wrapper.
    ///
    /// The rule lives in `is_bind_bone`, but this is the consequence that
    /// matters, and it is measured where it went wrong: around the pelvis. The
    /// root's bone segment runs from the origin up to `hips`, so before the
    /// wrapper was excluded these points were nearer to it than to any real
    /// bone and it took the bind. Root is animated by nothing, so they stayed
    /// at bind pose while the body moved.
    #[test]
    fn nothing_binds_to_the_armature_wrapper() {
        let arm = canon::armature();
        let root_slot = arm
            .skin
            .iter()
            .position(|&ji| arm.joints[ji].name == canon::ROOT_NAME)
            .expect("the wrapper is in the skin, which is why this can go wrong");

        // A column of points through the pelvis and down the legs: exactly the
        // region the root segment passes through.
        let mut positions = Vec::new();
        for i in 0..40 {
            let y = 0.05 + i as f32 * 0.03;
            for x in [-0.09f32, 0.0, 0.09] {
                positions.push(Vec3::new(x, y, 0.0));
            }
        }

        let (skin, _) = weight_armature(&arm, &positions);
        for (v, (js, ws)) in skin.joints.iter().zip(skin.weights.iter()).enumerate() {
            for (j, w) in js.iter().zip(ws.iter()) {
                assert!(
                    *j as usize != root_slot || *w <= f32::EPSILON,
                    "vertex {v} at {:?} put {w} on the wrapper",
                    positions[v]
                );
            }
        }
    }

    /// Rig has to open on anything, including the assets auto-fit refuses.
    ///
    /// The landmark solve behind [`seed_bind_markers`] fails on anything it
    /// cannot read as a body, and that failure used to leave the workbench with
    /// no joints at all and no way to summon any: a monitor opened Rig empty
    /// with Bind greyed out. Placement is the author's job, so the starting
    /// skeleton must never depend on recognizing the mesh.
    #[test]
    fn the_default_skeleton_lands_inside_any_mesh() {
        // A slab: exactly the shape the landmark solve has no business on.
        let mut vs: Vec<[f32; 3]> = Vec::new();
        for i in 0..20 {
            for k in 0..20 {
                let (u, v) = (i as f32 / 19.0, k as f32 / 19.0);
                for z in [-0.05f32, 0.05] {
                    vs.push([u * 2.0 - 1.0, v * 3.0, z]);
                }
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("slab.glb");
        std::fs::write(&path, slab_glb(&vs)).unwrap();

        let markers = default_bind_markers(&path).expect("Rig must open on a slab");
        assert!(!markers.is_empty(), "a starting skeleton is always offered");

        // Every joint sits within the mesh's own bounds, so the author starts
        // from something on the model rather than beside it.
        for m in &markers {
            assert!(
                (-1.0..=1.0).contains(&m.world[0]) && (0.0..=3.0).contains(&m.world[1]),
                "{} landed outside the mesh at {:?}",
                m.name,
                m.world
            );
        }
        // Feet low, head high: the chain is the right way up and spans the mesh.
        let y = |n: &str| markers.iter().find(|m| m.name == n).map(|m| m.world[1]);
        let (head, foot) = (y("head").unwrap(), y("leftFoot").unwrap());
        assert!(head > foot, "head {head} should sit above foot {foot}");
        assert!(foot > 0.0 && head < 3.0, "the chain insets from both ends");
    }

    fn slab_glb(v: &[[f32; 3]]) -> Vec<u8> {
        let mut bin: Vec<u8> = Vec::new();
        for p in v {
            for c in p {
                bin.extend_from_slice(&c.to_le_bytes());
            }
        }
        let pos_len = bin.len();
        let idx_off = bin.len();
        let idx: Vec<u16> = (0..v.len() as u16).collect();
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
            "nodes": [{"mesh": 0, "name": "slab"}],
            "meshes": [{"primitives": [{"attributes": {"POSITION": 0}, "indices": 1}]}],
            "accessors": [
                {"bufferView": 0, "componentType": 5126, "count": v.len(), "type": "VEC3",
                 "min": [-1.0, 0.0, -0.05], "max": [1.0, 3.0, 0.05]},
                {"bufferView": 1, "componentType": 5123, "count": idx.len(), "type": "SCALAR"}
            ],
            "bufferViews": [
                {"buffer": 0, "byteOffset": 0, "byteLength": pos_len},
                {"buffer": 0, "byteOffset": idx_off, "byteLength": idx_len}
            ],
            "buffers": [{"byteLength": bin.len()}]
        });
        crate::test_support::glb(&serde_json::to_vec(&json).unwrap(), Some(&bin))
    }
    use crate::rig::skeleton::Joint;

    fn joint(name: &str, translation: Vec3, scale: Vec3) -> Joint {
        Joint {
            name: name.into(),
            parent: None,
            translation,
            rotation: Quat::IDENTITY,
            scale,
        }
    }

    fn arm(t: Vec3, s: Vec3) -> Armature {
        let hips = joint("hips", t, s);
        Armature {
            joints: vec![hips],
            skin: vec![0],
            name_to_index: [("hips".into(), 0)].into_iter().collect(),
        }
    }

    #[test]
    fn retarget_shifts_translation_keys_by_rest_delta() {
        let pack = arm(Vec3::new(0.0, 1.0, 0.0), Vec3::ONE);
        let fitted = arm(Vec3::new(0.0, 0.6, 0.0), Vec3::splat(0.63));
        let mut anim = AnimData {
            channels: vec![AnimChannel {
                node: 0,
                path: "translation",
                times: vec![0.0, 1.0],
                values: vec![0.0, 1.0, 0.1, 0.0, 1.0, -0.1],
                interpolation: "LINEAR",
            }],
        };
        retarget_translations(&pack, &fitted, &mut anim);
        assert!((anim.channels[0].values[1] - 0.6).abs() < 1e-5);
        assert!((anim.channels[0].values[4] - 0.6).abs() < 1e-5);
        assert!((anim.channels[0].values[2] - 0.1).abs() < 1e-5);
    }

    fn named_arm(names: &[&str]) -> Armature {
        let joints = names
            .iter()
            .map(|n| crate::rig::skeleton::Joint {
                name: (*n).into(),
                parent: None,
                translation: Vec3::ZERO,
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
            })
            .collect();
        Armature {
            joints,
            skin: (0..names.len()).collect(),
            name_to_index: names
                .iter()
                .enumerate()
                .map(|(i, n)| ((*n).to_string(), i))
                .collect(),
        }
    }

    fn channel(node: usize) -> AnimChannel {
        AnimChannel {
            node,
            path: "rotation",
            times: vec![0.0],
            values: vec![0.0, 0.0, 0.0, 1.0],
            interpolation: "LINEAR",
        }
    }

    #[test]
    fn rebase_moves_channels_by_name_not_by_index() {
        // A pack indexes its own nodes; we write against the canonical
        // armature. The two disagree on both order and length, so passing
        // pack indices straight through lands every key on whatever joint
        // shares that number — animation that plays and is scrambled.
        let pack = named_arm(&["head", "neck", "leaf_tip", "hips"]);
        let canonical = named_arm(&["Rig", "root", "hips", "neck", "head"]);
        let mut anim = AnimData {
            channels: vec![channel(0), channel(1), channel(2), channel(3)],
        };
        rebase_channels(&mut anim, &pack, &canonical);

        // `leaf_tip` has no canonical joint and is dropped, not remapped.
        assert_eq!(anim.channels.len(), 3);
        let landed: Vec<usize> = anim.channels.iter().map(|c| c.node).collect();
        assert_eq!(
            landed,
            vec![
                canonical.name_to_index["head"],
                canonical.name_to_index["neck"],
                canonical.name_to_index["hips"],
            ]
        );
        // The bug this guards: index 0 meant `head` in the pack and `Rig`
        // in the canonical armature.
        assert_ne!(anim.channels[0].node, 0);
    }

    #[test]
    fn bake_rejects_morph_targets() {
        let json = serde_json::json!({
            "asset": { "version": "2.0" },
            "scene": 0,
            "scenes": [{ "nodes": [0] }],
            "nodes": [{ "mesh": 0 }],
            "meshes": [{
                "primitives": [{
                    "attributes": { "POSITION": 0 },
                    "targets": [{ "POSITION": 0 }]
                }]
            }],
            "accessors": [{
                "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
                "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0]
            }],
            "bufferViews": [{ "buffer": 0, "byteOffset": 0, "byteLength": 36 }],
            "buffers": [{ "byteLength": 36 }]
        });
        let mut bin = Vec::new();
        for v in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
            bin.extend_from_slice(&v.to_le_bytes());
        }
        let js = serde_json::to_vec(&json).unwrap();
        let glb = crate::test_support::glb(&js, Some(&bin));
        let (doc, buffers, _) = gltf::import_slice(&glb).unwrap();
        let err = bake_mesh(&doc, &buffers).unwrap_err();
        assert!(err.to_string().contains("morph"), "unexpected error: {err}");
    }

    #[test]
    fn overlay_drops_pack_scale_channels() {
        let mut anim = AnimData {
            channels: vec![
                AnimChannel {
                    node: 0,
                    path: "rotation",
                    times: vec![0.0],
                    values: vec![0.0, 0.0, 0.0, 1.0],
                    interpolation: "LINEAR",
                },
                AnimChannel {
                    node: 0,
                    path: "scale",
                    times: vec![0.0],
                    values: vec![1.0, 1.0, 1.0],
                    interpolation: "LINEAR",
                },
            ],
        };
        drop_scale_channels(&mut anim);
        assert_eq!(anim.channels.len(), 1);
        assert_eq!(anim.channels[0].path, "rotation");
    }
}
