//! CPU-evaluate a skinned, animated GLB. The native `three-d` viewer plays
//! clips with this; it is also the contract test for what we write.

use super::skeleton::{Armature, Joint};
use glam::{Mat4, Quat, Vec3};
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::Path;

/// Rest-pose bind-bone head for Place. `parent_name` is the next bind ancestor.
#[derive(Debug, Clone)]
pub struct BindMarker {
    pub name: String,
    pub parent_name: String,
    pub world: [f32; 3],
}

/// A loaded skinned clip: bind-pose mesh + joint rest + one animation.
#[derive(Debug, Clone)]
pub struct SkinnedClip {
    pub name: String,
    pub duration: f32,
    bind_positions: Vec<Vec3>,
    bind_normals: Option<Vec<Vec3>>,
    joints: Vec<[u16; 4]>,
    weights: Vec<[f32; 4]>,
    /// Skin slot → node index.
    skin_nodes: Vec<usize>,
    ibms: Vec<Mat4>,
    nodes: Vec<PlayNode>,
    channels: Vec<PlayChannel>,
    /// Triangle index buffer (empty if the primitive is unindexed).
    indices: Vec<u32>,
    /// Document-order spans into the concatenated bind buffers.
    primitives: Vec<PrimGeom>,
}

#[derive(Debug, Clone)]
struct PrimGeom {
    vertex_start: usize,
    vertex_count: usize,
    indices: Vec<u32>,
}

#[derive(Debug, Clone)]
struct PlayNode {
    name: String,
    parent: Option<usize>,
    translation: Vec3,
    rotation: Quat,
    scale: Vec3,
}

#[derive(Debug, Clone)]
struct PlayChannel {
    node: usize,
    path: ChannelPath,
    times: Vec<f32>,
    values: Vec<f32>,
    interpolation: Interp,
}

#[derive(Debug, Clone, Copy)]
enum ChannelPath {
    Translation,
    Rotation,
    Scale,
}

#[derive(Debug, Clone, Copy)]
enum Interp {
    Linear,
    Step,
}

impl SkinnedClip {
    pub fn from_glb(path: &Path) -> Result<Self, String> {
        Self::from_glb_named(path, None)
    }

    /// Load `path`, playing the animation called `want`.
    ///
    /// `None` takes the first animation, which is all a single-clip file has.
    /// A baked model now holds a *set*, so picking by name is what lets the
    /// viewer show anything but the first one.
    pub fn from_glb_named(path: &Path, want: Option<&str>) -> Result<Self, String> {
        let (doc, buffers) =
            crate::rig::skeleton::import_no_images(path).map_err(|e| e.to_string())?;
        Self::from_gltf_named(&doc, &buffers, want)
    }

    pub fn from_gltf(doc: &gltf::Document, buffers: &[gltf::buffer::Data]) -> Result<Self, String> {
        Self::from_gltf_named(doc, buffers, None)
    }

    /// The mesh for the Rig step, whether or not it carries a skin.
    ///
    /// A freshly generated model has no `skin`, no `JOINTS_0` and no
    /// `WEIGHTS_0`, so [`from_glb`](Self::from_glb) refuses it with "GLB has no
    /// skin". That is exactly the model Rig exists to put a skeleton on, and
    /// refusing it made the workbench unopenable on anything unrigged.
    ///
    /// Geometry is all Rig needs: joints are dragged onto the mesh by
    /// raycasting the bind positions, and the marker set comes from
    /// `seed_bind_markers`, not from a skin. An unskinned load leaves the
    /// skinning fields empty, so [`has_bind_bones`](Self::has_bind_bones) is
    /// false and the workbench correctly reads the mesh as not yet fitted.
    pub fn for_rig(path: &Path) -> Result<Self, String> {
        let (doc, buffers) =
            crate::rig::skeleton::import_no_images(path).map_err(|e| e.to_string())?;
        if doc.skins().next().is_some() {
            return Self::from_gltf_named(&doc, &buffers, None);
        }
        Self::unskinned(&doc, &buffers)
    }

    /// Geometry only: every primitive's positions, normals and indices, with
    /// no skin, no joints and no animation.
    fn unskinned(doc: &gltf::Document, buffers: &[gltf::buffer::Data]) -> Result<Self, String> {
        let reader = |b: gltf::Buffer| Some(buffers.get(b.index())?.0.as_slice());
        let mut bind_positions: Vec<Vec3> = Vec::new();
        let mut bind_normals: Vec<Vec3> = Vec::new();
        let mut indices: Vec<u32> = Vec::new();
        let mut primitives: Vec<PrimGeom> = Vec::new();
        let mut have_normals = true;
        for mesh in doc.meshes() {
            for prim in mesh.primitives() {
                let pr = prim.reader(reader);
                let pos: Vec<Vec3> = pr
                    .read_positions()
                    .ok_or("missing POSITION")?
                    .map(Vec3::from_array)
                    .collect();
                if let Some(n) = pr.read_normals() {
                    bind_normals.extend(n.map(Vec3::from_array));
                } else {
                    have_normals = false;
                }
                let prim_indices: Vec<u32> = pr
                    .read_indices()
                    .map(|i| i.into_u32().collect())
                    .unwrap_or_default();
                let vertex_start = bind_positions.len();
                primitives.push(PrimGeom {
                    vertex_start,
                    vertex_count: pos.len(),
                    indices: prim_indices.clone(),
                });
                let base = vertex_start as u32;
                if prim_indices.is_empty() {
                    indices.extend(base..base + pos.len() as u32);
                } else {
                    indices.extend(prim_indices.iter().map(|i| i + base));
                }
                bind_positions.extend(pos);
            }
        }
        if primitives.is_empty() {
            return Err("GLB has no mesh primitives".into());
        }
        Ok(Self {
            name: "rest".into(),
            duration: 1e-3,
            bind_positions,
            bind_normals: have_normals.then_some(bind_normals),
            joints: Vec::new(),
            weights: Vec::new(),
            skin_nodes: Vec::new(),
            ibms: Vec::new(),
            nodes: Vec::new(),
            channels: Vec::new(),
            indices,
            primitives,
        })
    }

    pub fn from_gltf_named(
        doc: &gltf::Document,
        buffers: &[gltf::buffer::Data],
        want: Option<&str>,
    ) -> Result<Self, String> {
        let skin = doc.skins().next().ok_or("GLB has no skin")?;
        let reader = |b: gltf::Buffer| Some(buffers.get(b.index())?.0.as_slice());
        let ibm_iter = skin
            .reader(reader)
            .read_inverse_bind_matrices()
            .ok_or("skin missing inverseBindMatrices")?;
        let ibms: Vec<Mat4> = ibm_iter
            .map(|m| Mat4::from_cols_array(&flatten_mat4(m)))
            .collect();
        let skin_nodes: Vec<usize> = skin.joints().map(|n| n.index()).collect();
        if ibms.len() != skin_nodes.len() {
            return Err(format!(
                "IBM count {} != joint count {}",
                ibms.len(),
                skin_nodes.len()
            ));
        }

        let mut bind_positions = Vec::new();
        let mut bind_normals = Vec::new();
        let mut joints = Vec::new();
        let mut weights = Vec::new();
        let mut indices = Vec::new();
        let mut primitives = Vec::new();
        let mut have_normals = true;
        for mesh in doc.meshes() {
            for prim in mesh.primitives() {
                if prim.get(&gltf::Semantic::Joints(0)).is_none()
                    || prim.get(&gltf::Semantic::Weights(0)).is_none()
                {
                    continue;
                }
                let pr = prim.reader(reader);
                let pos: Vec<Vec3> = pr
                    .read_positions()
                    .ok_or("missing POSITION")?
                    .map(Vec3::from_array)
                    .collect();
                let jnt: Vec<[u16; 4]> = pr
                    .read_joints(0)
                    .ok_or("missing JOINTS_0")?
                    .into_u16()
                    .collect();
                let wgt: Vec<[f32; 4]> = pr
                    .read_weights(0)
                    .ok_or("missing WEIGHTS_0")?
                    .into_f32()
                    .collect();
                if pos.len() != jnt.len() || jnt.len() != wgt.len() {
                    return Err("POSITION/JOINTS/WEIGHTS length mismatch".into());
                }
                if let Some(n) = pr.read_normals() {
                    bind_normals.extend(n.map(Vec3::from_array));
                } else {
                    have_normals = false;
                }
                let prim_indices: Vec<u32> = pr
                    .read_indices()
                    .map(|i| i.into_u32().collect())
                    .unwrap_or_default();
                let vertex_start = bind_positions.len();
                primitives.push(PrimGeom {
                    vertex_start,
                    vertex_count: pos.len(),
                    indices: prim_indices.clone(),
                });
                let base = vertex_start as u32;
                if prim_indices.is_empty() {
                    indices.extend(base..base + pos.len() as u32);
                } else {
                    indices.extend(prim_indices.iter().map(|i| i + base));
                }
                bind_positions.extend(pos);
                joints.extend(jnt);
                weights.extend(wgt);
            }
        }
        if primitives.is_empty() {
            return Err("mesh has no JOINTS_0/WEIGHTS_0".into());
        }
        let bind_normals = have_normals.then_some(bind_normals);

        let nodes: Vec<PlayNode> = doc
            .nodes()
            .map(|n| {
                let (t, r, s) = n.transform().decomposed();
                PlayNode {
                    name: n.name().unwrap_or("").to_string(),
                    parent: None,
                    translation: Vec3::from_array(t),
                    rotation: Quat::from_array(r),
                    scale: Vec3::from_array(s),
                }
            })
            .collect();
        let mut nodes = nodes;
        for n in doc.nodes() {
            for c in n.children() {
                nodes[c.index()].parent = Some(n.index());
            }
        }

        let chosen = match want {
            Some(want) => doc
                .animations()
                .find(|a| a.name().is_some_and(|n| n.eq_ignore_ascii_case(want)))
                .ok_or_else(|| format!("animation '{want}' is not in this model"))
                .map(Some)?,
            None => doc.animations().next(),
        };
        let (name, duration, channels) = match chosen {
            Some(anim) => read_animation(anim, reader)?,
            None => ("rest".into(), 0.0, Vec::new()),
        };

        Ok(Self {
            name,
            duration: duration.max(1e-3),
            bind_positions,
            bind_normals,
            joints,
            weights,
            skin_nodes,
            ibms,
            nodes,
            channels,
            indices,
            primitives,
        })
    }

    pub fn has_animation(&self) -> bool {
        !self.channels.is_empty() && self.duration > 0.0
    }

    pub fn has_bind_bones(&self) -> bool {
        self.nodes
            .iter()
            .any(|n| crate::rig::weights::is_placeable_joint(&n.name))
    }

    /// Where a joint should sit for a pointer ray aimed at the mesh.
    ///
    /// Returns the **midline** of the limb under the ray, not the surface
    /// point: the midpoint between where the ray enters the mesh and where it
    /// leaves. A joint parked on the skin is only marginally better than one
    /// in the air, because inverse-distance weighting wants the bone inside
    /// the volume it drives.
    ///
    /// `None` when the ray misses, which the caller should treat as "keep the
    /// marker where it was" rather than moving it somewhere arbitrary.
    ///
    /// Tested against the rest pose, which is what Rig shows: the mesh is
    /// frozen while markers are arranged.
    /// Takes and returns plain arrays: markers are `[f32; 3]` and a caller
    /// should not have to adopt this crate's math library to place one.
    pub fn ray_midline(&self, origin: [f32; 3], dir: [f32; 3]) -> Option<[f32; 3]> {
        let origin = Vec3::from_array(origin);
        let dir = Vec3::from_array(dir).normalize_or_zero();
        if dir.length_squared() < 0.5 {
            return None;
        }
        let mut hits = self.ray_hits(origin, dir);
        if hits.is_empty() {
            return None;
        }
        hits.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        // A ray crossing the edge two triangles share registers on both. Left
        // alone, that duplicate becomes the "exit" and the midline collapses
        // back onto the surface, which is the bug this whole function exists
        // to avoid.
        hits.dedup_by(|a, b| (*a - *b).abs() < SURFACE_EPSILON);
        let entry = hits[0];
        // A closed limb gives an exit too; a single hit means we grazed an
        // open edge, so the surface point is the best answer available.
        let exit = hits.get(1).copied().unwrap_or(entry);
        Some((origin + dir * (0.5 * (entry + exit))).to_array())
    }

    #[doc(hidden)]
    pub fn ray_hits_debug(&self, o: [f32; 3], d: [f32; 3]) -> Vec<f32> {
        let mut h = self.ray_hits(Vec3::from_array(o), Vec3::from_array(d).normalize());
        h.sort_by(|a, b| a.partial_cmp(b).unwrap());
        h
    }

    /// Distances along `dir` at which the ray crosses a triangle.
    fn ray_hits(&self, origin: Vec3, dir: Vec3) -> Vec<f32> {
        let mut out = Vec::new();
        let verts = &self.bind_positions;
        for tri in self.indices.chunks_exact(3) {
            let (a, b, c) = (tri[0] as usize, tri[1] as usize, tri[2] as usize);
            let (Some(&p0), Some(&p1), Some(&p2)) = (verts.get(a), verts.get(b), verts.get(c))
            else {
                continue;
            };
            if let Some(t) = ray_triangle(origin, dir, p0, p1, p2) {
                out.push(t);
            }
        }
        out
    }

    /// Snapshot of rest locals so Place can restore on Cancel.
    pub fn rest_translations(&self) -> Vec<[f32; 3]> {
        self.nodes
            .iter()
            .map(|n| n.translation.to_array())
            .collect()
    }

    pub fn restore_translations(&mut self, locals: &[[f32; 3]]) {
        for (node, t) in self.nodes.iter_mut().zip(locals.iter()) {
            node.translation = Vec3::from_array(*t);
        }
    }

    /// Move bind-bone heads for a live Place preview. IBMs stay so the mesh
    /// follows the markers (Mixamo-style). Done writes new IBMs afterward.
    pub fn apply_world_heads(&mut self, heads: &[(String, [f32; 3])]) {
        let mut arm = self.to_armature();
        arm.apply_world_heads_parent_first(heads);
        for (node, joint) in self.nodes.iter_mut().zip(arm.joints.iter()) {
            node.translation = joint.translation;
        }
    }

    fn to_armature(&self) -> Armature {
        let mut name_to_index = HashMap::new();
        let joints: Vec<Joint> = self
            .nodes
            .iter()
            .enumerate()
            .map(|(i, n)| {
                if !n.name.is_empty() {
                    name_to_index.insert(n.name.clone(), i);
                }
                Joint {
                    name: n.name.clone(),
                    parent: n.parent,
                    translation: n.translation,
                    rotation: n.rotation,
                    scale: n.scale,
                }
            })
            .collect();
        Armature {
            joints,
            skin: self.skin_nodes.clone(),
            name_to_index,
        }
    }

    /// Bind-bone heads at rest (current locals, not clip time). Sticks skip excluded ancestors.
    pub fn bind_markers(&self) -> Vec<BindMarker> {
        let worlds = self.joint_worlds_from(&self.locals_rest());
        let mut out = Vec::new();
        for &ji in &self.skin_nodes {
            let Some(node) = self.nodes.get(ji) else {
                continue;
            };
            if !crate::rig::weights::is_placeable_joint(&node.name) {
                continue;
            }
            if ji >= worlds.len() {
                continue;
            }
            let w = worlds[ji].transform_point3(Vec3::ZERO);
            out.push(BindMarker {
                name: node.name.clone(),
                parent_name: bind_ancestor_name(&self.nodes, node.parent),
                world: [w.x, w.y, w.z],
            });
        }
        out
    }

    /// Replace channels with an animation-only overlay, remapped by node name.
    ///
    /// Used for catalog Preview onto a fitted rest (same retarget as Bake).
    pub fn overlay_from_slice(&mut self, bytes: &[u8]) -> Result<(), String> {
        let (doc, buffers, _) = gltf::import_slice(bytes).map_err(|e| e.to_string())?;
        let reader = |b: gltf::Buffer| Some(buffers.get(b.index())?.0.as_slice());
        let rest_by_name: HashMap<&str, usize> = self
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| !n.name.is_empty())
            .map(|(i, n)| (n.name.as_str(), i))
            .collect();
        let overlay_names: Vec<String> = doc
            .nodes()
            .map(|n| n.name().unwrap_or("").to_string())
            .collect();
        let anim = doc
            .animations()
            .next()
            .ok_or_else(|| "overlay GLB has no animation".to_string())?;
        let (name, duration, raw) = read_animation(anim, reader)?;
        let mut channels = Vec::with_capacity(raw.len());
        for mut ch in raw {
            let Some(src_name) = overlay_names.get(ch.node) else {
                continue;
            };
            let Some(&dst) = rest_by_name.get(src_name.as_str()) else {
                continue;
            };
            ch.node = dst;
            channels.push(ch);
        }
        if channels.is_empty() {
            return Err("overlay animation did not match rest joint names".into());
        }
        self.name = name;
        self.duration = duration.max(1e-3);
        self.channels = channels;
        Ok(())
    }

    pub fn vertex_count(&self) -> usize {
        self.bind_positions.len()
    }

    pub fn indices(&self) -> &[u32] {
        &self.indices
    }

    /// Document-order skinned primitives: `(vertex_start, vertex_count)`.
    pub fn primitive_range(&self, i: usize) -> Option<(usize, usize)> {
        self.primitives
            .get(i)
            .map(|p| (p.vertex_start, p.vertex_count))
    }

    /// Indices local to primitive `i` (empty if that primitive is unindexed).
    pub fn primitive_indices(&self, i: usize) -> Option<&[u32]> {
        self.primitives.get(i).map(|p| p.indices.as_slice())
    }

    pub fn primitive_count(&self) -> usize {
        self.primitives.len()
    }

    /// Scatter unique-vertex values onto the triangle-corner buffer (glTF index order).
    ///
    /// three-d's `Mesh::vertex_count()` returns the index count for indexed
    /// meshes, and some loaders unroll triangles. Either way the GPU buffer
    /// may be `indices.len()` long while [`sample`](Self::sample) stays unique.
    pub fn expand(&self, unique: &[Vec3]) -> Option<Vec<Vec3>> {
        if self.indices.is_empty() {
            return None;
        }
        Some(
            self.indices
                .iter()
                .map(|&i| unique.get(i as usize).copied().unwrap_or(Vec3::ZERO))
                .collect(),
        )
    }

    /// Parent→child segments of **display** bones at time `t` (world space).
    ///
    /// Skips fingers and armature wrappers (`Rig`, `Mannequin`, …). A hip
    /// joint parents under `Rig` at the origin — drawing that segment puts a
    /// spike between the legs.
    pub fn bone_segments(&self, t: f32) -> Vec<(Vec3, Vec3)> {
        self.bone_segments_from(&self.joint_worlds(t))
    }

    pub fn bone_segments_rest(&self) -> Vec<(Vec3, Vec3)> {
        self.bone_segments_from(&self.joint_worlds_from(&self.locals_rest()))
    }

    fn bone_segments_from(&self, worlds: &[Mat4]) -> Vec<(Vec3, Vec3)> {
        let mut segs = Vec::new();
        for &ji in &self.skin_nodes {
            let Some(node) = self.nodes.get(ji) else {
                continue;
            };
            if !crate::rig::weights::is_placeable_joint(&node.name) {
                continue;
            }
            let Some(p) = bind_ancestor_index(&self.nodes, node.parent) else {
                continue;
            };
            if p >= worlds.len() || ji >= worlds.len() {
                continue;
            }
            let a = worlds[p].transform_point3(Vec3::ZERO);
            let b = worlds[ji].transform_point3(Vec3::ZERO);
            if (b - a).length_squared() > 1e-10 {
                segs.push((a, b));
            }
        }
        segs
    }

    /// Joint world matrices at time `t` (seconds, wrapped).
    pub fn joint_worlds(&self, t: f32) -> Vec<Mat4> {
        self.joint_worlds_from(&self.locals_at(t))
    }

    fn locals_rest(&self) -> Vec<(Vec3, Quat, Vec3)> {
        self.nodes
            .iter()
            .map(|n| (n.translation, n.rotation, n.scale))
            .collect()
    }

    fn joint_worlds_from(&self, locals: &[(Vec3, Quat, Vec3)]) -> Vec<Mat4> {
        let n = locals.len();
        let mut worlds = vec![Mat4::IDENTITY; n];
        let mut done = vec![false; n];
        for i in 0..n {
            compute_world(&self.nodes, locals, i, &mut worlds, &mut done);
        }
        worlds
    }

    fn skin_palette_from_worlds(&self, worlds: &[Mat4]) -> Vec<Mat4> {
        let mut palette = vec![Mat4::IDENTITY; self.skin_nodes.len()];
        for (i, &ni) in self.skin_nodes.iter().enumerate() {
            palette[i] = worlds[ni] * self.ibms[i];
        }
        palette
    }

    fn skin_palette(&self, t: f32) -> Vec<Mat4> {
        self.skin_palette_from_worlds(&self.joint_worlds(t))
    }

    fn fill_positions(&self, palette: &[Mat4], out: &mut Vec<[f32; 3]>) {
        let n = self.bind_positions.len();
        out.resize(n, [0.0; 3]);
        let pos = &self.bind_positions;
        let joints = &self.joints;
        let weights = &self.weights;
        if joints.len() != n || weights.len() != n {
            out.par_iter_mut()
                .enumerate()
                .for_each(|(i, dest)| *dest = pos[i].to_array());
            return;
        }
        // An unskinned mesh (the Rig step, before any bind) has positions but
        // no influences, and nothing to deform them with: its posed position is
        // its bind position. Skinning here would index empty JOINTS_0/WEIGHTS_0
        // once per vertex.
        out.par_iter_mut().enumerate().for_each(|(i, dest)| {
            *dest = skin_point(pos[i], joints[i], weights[i], palette);
        });
    }

    /// Skin positions only into `out` (reuses allocation). No normals — preview path.
    pub fn sample_positions_into(&self, t: f32, out: &mut Vec<[f32; 3]>) {
        let palette = self.skin_palette(t);
        self.fill_positions(&palette, out);
    }

    /// Skin using current rest locals, ignoring clip channels. Place preview
    /// uses this so a loaded walk does not snap joints back to t=0.
    pub fn sample_positions_rest_into(&self, out: &mut Vec<[f32; 3]>) {
        let worlds = self.joint_worlds_from(&self.locals_rest());
        let palette = self.skin_palette_from_worlds(&worlds);
        self.fill_positions(&palette, out);
    }

    /// Scatter unique verts through the triangle index buffer into `out`.
    pub fn expand_into(&self, unique: &[[f32; 3]], out: &mut Vec<[f32; 3]>) {
        out.clear();
        if self.indices.is_empty() {
            return;
        }
        out.reserve(self.indices.len());
        for &i in &self.indices {
            out.push(unique.get(i as usize).copied().unwrap_or([0.0; 3]));
        }
    }

    /// Skin the bind mesh at time `t`. Positions always; normals if present.
    pub fn sample(&self, t: f32) -> (Vec<Vec3>, Option<Vec<Vec3>>) {
        let palette = self.skin_palette(t);
        let mut positions = Vec::with_capacity(self.bind_positions.len());
        for (i, &p) in self.bind_positions.iter().enumerate() {
            let [x, y, z] = skin_point(p, self.joints[i], self.weights[i], &palette);
            positions.push(Vec3::new(x, y, z));
        }
        let normals = self.bind_normals.as_ref().map(|ns| {
            ns.iter()
                .enumerate()
                .map(|(i, &n)| {
                    let mut acc = Vec3::ZERO;
                    for k in 0..4 {
                        let w = self.weights[i][k];
                        if w == 0.0 {
                            continue;
                        }
                        let j = self.joints[i][k] as usize;
                        if j >= palette.len() {
                            continue;
                        }
                        let m = glam::Mat3::from_mat4(palette[j]);
                        acc += m * n * w;
                    }
                    acc.normalize_or_zero()
                })
                .collect()
        });
        (positions, normals)
    }

    fn locals_at(&self, t: f32) -> Vec<(Vec3, Quat, Vec3)> {
        let t = wrap(t, self.duration);
        let mut locals: Vec<(Vec3, Quat, Vec3)> = self
            .nodes
            .iter()
            .map(|n| (n.translation, n.rotation, n.scale))
            .collect();
        for ch in &self.channels {
            if ch.node >= locals.len() || ch.times.is_empty() {
                continue;
            }
            match ch.path {
                ChannelPath::Translation => {
                    locals[ch.node].0 = sample_vec3(ch, t);
                }
                ChannelPath::Scale => {
                    locals[ch.node].2 = sample_vec3(ch, t);
                }
                ChannelPath::Rotation => {
                    locals[ch.node].1 = sample_quat(ch, t);
                }
            }
        }
        locals
    }
}

fn skin_point(p: Vec3, joints: [u16; 4], weights: [f32; 4], palette: &[Mat4]) -> [f32; 3] {
    let mut acc = Vec3::ZERO;
    for k in 0..4 {
        let w = weights[k];
        if w == 0.0 {
            continue;
        }
        let j = joints[k] as usize;
        if j >= palette.len() {
            continue;
        }
        acc += palette[j].transform_point3(p) * w;
    }
    [acc.x, acc.y, acc.z]
}

fn wrap(t: f32, duration: f32) -> f32 {
    if duration <= 0.0 {
        return 0.0;
    }
    let mut x = t % duration;
    if x < 0.0 {
        x += duration;
    }
    x
}

fn read_animation<'a>(
    anim: gltf::Animation<'a>,
    reader: impl Fn(gltf::Buffer<'a>) -> Option<&'a [u8]> + Copy,
) -> Result<(String, f32, Vec<PlayChannel>), String> {
    let name = anim.name().unwrap_or("clip").to_string();
    let mut channels = Vec::new();
    let mut duration = 0.0f32;
    for ch in anim.channels() {
        let cr = ch.reader(reader);
        let times: Vec<f32> = cr.read_inputs().ok_or("channel missing times")?.collect();
        if let Some(&last) = times.last() {
            duration = duration.max(last);
        }
        let path = match ch.target().property() {
            gltf::animation::Property::Translation => ChannelPath::Translation,
            gltf::animation::Property::Rotation => ChannelPath::Rotation,
            gltf::animation::Property::Scale => ChannelPath::Scale,
            gltf::animation::Property::MorphTargetWeights => continue,
        };
        let values: Vec<f32> = match cr.read_outputs() {
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
            gltf::animation::Interpolation::Step => Interp::Step,
            _ => Interp::Linear,
        };
        channels.push(PlayChannel {
            node: ch.target().node().index(),
            path,
            times,
            values,
            interpolation,
        });
    }
    Ok((name, duration, channels))
}

fn bind_ancestor_index(nodes: &[PlayNode], mut parent: Option<usize>) -> Option<usize> {
    while let Some(i) = parent {
        let n = nodes.get(i)?;
        if crate::rig::weights::is_placeable_joint(&n.name) {
            return Some(i);
        }
        parent = n.parent;
    }
    None
}

fn bind_ancestor_name(nodes: &[PlayNode], parent: Option<usize>) -> String {
    bind_ancestor_index(nodes, parent)
        .and_then(|i| nodes.get(i))
        .map(|n| n.name.clone())
        .unwrap_or_default()
}

fn flatten_mat4(m: [[f32; 4]; 4]) -> [f32; 16] {
    let mut out = [0.0; 16];
    for (c, col) in m.iter().enumerate() {
        out[c * 4] = col[0];
        out[c * 4 + 1] = col[1];
        out[c * 4 + 2] = col[2];
        out[c * 4 + 3] = col[3];
    }
    out
}

fn compute_world(
    nodes: &[PlayNode],
    locals: &[(Vec3, Quat, Vec3)],
    i: usize,
    worlds: &mut [Mat4],
    done: &mut [bool],
) {
    if done[i] {
        return;
    }
    let (t, r, s) = locals[i];
    let local = Mat4::from_scale_rotation_translation(s, r, t);
    worlds[i] = match nodes[i].parent {
        Some(p) => {
            compute_world(nodes, locals, p, worlds, done);
            worlds[p] * local
        }
        None => local,
    };
    done[i] = true;
}

fn key_span(times: &[f32], t: f32) -> (usize, usize, f32) {
    if t <= times[0] {
        return (0, 0, 0.0);
    }
    let last = times.len() - 1;
    if t >= times[last] {
        return (last, last, 0.0);
    }
    let mut hi = 1;
    while hi < times.len() && times[hi] < t {
        hi += 1;
    }
    let lo = hi - 1;
    let span = (times[hi] - times[lo]).max(1e-8);
    (lo, hi, (t - times[lo]) / span)
}

fn sample_vec3(ch: &PlayChannel, t: f32) -> Vec3 {
    let (lo, hi, u) = key_span(&ch.times, t);
    let a = Vec3::from_slice(&ch.values[lo * 3..lo * 3 + 3]);
    if matches!(ch.interpolation, Interp::Step) || lo == hi {
        return a;
    }
    let b = Vec3::from_slice(&ch.values[hi * 3..hi * 3 + 3]);
    a.lerp(b, u)
}

fn sample_quat(ch: &PlayChannel, t: f32) -> Quat {
    let (lo, hi, u) = key_span(&ch.times, t);
    let a = Quat::from_xyzw(
        ch.values[lo * 4],
        ch.values[lo * 4 + 1],
        ch.values[lo * 4 + 2],
        ch.values[lo * 4 + 3],
    )
    .normalize();
    if matches!(ch.interpolation, Interp::Step) || lo == hi {
        return a;
    }
    let mut b = Quat::from_xyzw(
        ch.values[hi * 4],
        ch.values[hi * 4 + 1],
        ch.values[hi * 4 + 2],
        ch.values[hi * 4 + 3],
    )
    .normalize();
    if a.dot(b) < 0.0 {
        b = -b;
    }
    a.slerp(b, u)
}

#[cfg(test)]
mod raycast_tests {
    use super::*;

    /// A clip carrying only geometry: enough for the raycast, nothing else.
    fn clip_from(bind_positions: Vec<Vec3>, indices: Vec<u32>) -> SkinnedClip {
        let n = bind_positions.len();
        SkinnedClip {
            name: "test".into(),
            duration: 1.0,
            bind_positions,
            bind_normals: None,
            joints: vec![[0; 4]; n],
            weights: vec![[1.0, 0.0, 0.0, 0.0]; n],
            skin_nodes: Vec::new(),
            ibms: Vec::new(),
            nodes: Vec::new(),
            channels: Vec::new(),
            indices,
            primitives: Vec::new(),
        }
    }

    /// An axis-aligned box spanning x in [-1,1], y in [0,2], z in [-1,1].
    fn unit_box() -> (Vec<Vec3>, Vec<u32>) {
        let v = vec![
            Vec3::new(-1.0, 0.0, -1.0),
            Vec3::new(1.0, 0.0, -1.0),
            Vec3::new(1.0, 2.0, -1.0),
            Vec3::new(-1.0, 2.0, -1.0),
            Vec3::new(-1.0, 0.0, 1.0),
            Vec3::new(1.0, 0.0, 1.0),
            Vec3::new(1.0, 2.0, 1.0),
            Vec3::new(-1.0, 2.0, 1.0),
        ];
        let i = vec![
            0, 1, 2, 0, 2, 3, // -z
            4, 6, 5, 4, 7, 6, // +z
            0, 4, 5, 0, 5, 1, // -y
            3, 2, 6, 3, 6, 7, // +y
            0, 3, 7, 0, 7, 4, // -x
            1, 5, 6, 1, 6, 2, // +x
        ];
        (v, i)
    }

    #[test]
    fn a_ray_through_a_box_lands_on_the_midline_not_the_surface() {
        let (verts, indices) = unit_box();
        // Deliberately off the diagonal the two triangles of each face share.
        let origin = Vec3::new(0.3, 0.8, -5.0);
        let dir = Vec3::new(0.0, 0.0, 1.0);
        let mut hits: Vec<f32> = Vec::new();
        for tri in indices.chunks_exact(3) {
            if let Some(t) = ray_triangle(
                origin,
                dir,
                verts[tri[0] as usize],
                verts[tri[1] as usize],
                verts[tri[2] as usize],
            ) {
                hits.push(t);
            }
        }
        hits.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert_eq!(hits.len(), 2, "a closed box gives an entry and an exit");
        let mid = origin + dir * (0.5 * (hits[0] + hits[1]));
        // Surface would be z = -1; the midline is z = 0.
        assert!(mid.z.abs() < 1e-5, "expected the center, got {mid:?}");
    }

    #[test]
    fn a_ray_along_a_shared_edge_still_finds_the_midline() {
        // Aiming exactly at the diagonal two triangles share makes each face
        // register twice. Without deduping, the "exit" is the duplicate of the
        // entry and the marker lands on the skin.
        let (verts, indices) = unit_box();
        let clip = clip_from(verts, indices);
        let mid = clip
            .ray_midline([0.0, 1.0, -5.0], [0.0, 0.0, 1.0])
            .expect("ray hits the box");
        assert!(mid[2].abs() < 1e-4, "expected the center, got {mid:?}");
    }

    #[test]
    fn back_faces_count_or_there_is_no_exit_to_find() {
        // The far wall of a limb faces away from the ray. If the intersection
        // test culled back faces there would be one hit, and the marker would
        // sit on the skin instead of inside the volume.
        let a = Vec3::new(-1.0, -1.0, 1.0);
        let b = Vec3::new(1.0, -1.0, 1.0);
        let c = Vec3::new(0.0, 1.0, 1.0);
        // Wound so its normal points *towards* the ray origin's far side.
        let t = ray_triangle(Vec3::ZERO, Vec3::Z, a, c, b);
        assert!(t.is_some(), "back-facing triangle must still register");
    }

    #[test]
    fn a_ray_that_misses_reports_nothing() {
        let (verts, indices) = unit_box();
        let origin = Vec3::new(10.0, 1.0, -5.0);
        let dir = Vec3::new(0.0, 0.0, 1.0);
        let any = indices.chunks_exact(3).any(|tri| {
            ray_triangle(
                origin,
                dir,
                verts[tri[0] as usize],
                verts[tri[1] as usize],
                verts[tri[2] as usize],
            )
            .is_some()
        });
        assert!(!any, "a miss must not be reported as a hit");
    }

    #[test]
    fn geometry_behind_the_pointer_is_ignored() {
        let (verts, indices) = unit_box();
        // Origin past the box, aimed further away: everything is behind.
        let origin = Vec3::new(0.0, 1.0, 5.0);
        let dir = Vec3::new(0.0, 0.0, 1.0);
        let any = indices.chunks_exact(3).any(|tri| {
            ray_triangle(
                origin,
                dir,
                verts[tri[0] as usize],
                verts[tri[1] as usize],
                verts[tri[2] as usize],
            )
            .is_some()
        });
        assert!(!any, "hits behind the ray origin must be rejected");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::Write;

    fn pack_glb(json_doc: &serde_json::Value, bin: &[u8]) -> Vec<u8> {
        crate::test_support::glb(&serde_json::to_vec(json_doc).unwrap(), Some(bin))
    }

    /// One joint, two verts glued to it, translation 0 → +1 X over 1s.
    fn tiny_clip_glb() -> Vec<u8> {
        let mut bin = Vec::new();
        // positions (2×vec3)
        let pos = [0.0f32, 0.0, 0.0, 0.0, 1.0, 0.0];
        let pos_off = bin.len();
        bin.extend_from_slice(&bytemuck_bytes(&pos));
        // joints (2×u16×4)
        let jnt: [u16; 8] = [0, 0, 0, 0, 0, 0, 0, 0];
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        let jnt_off = bin.len();
        for x in jnt {
            bin.extend_from_slice(&x.to_le_bytes());
        }
        // weights
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        let wgt = [1.0f32, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0];
        let wgt_off = bin.len();
        bin.extend_from_slice(&bytemuck_bytes(&wgt));
        // indices
        let idx: [u32; 3] = [0, 1, 0];
        let idx_off = bin.len();
        for x in idx {
            bin.extend_from_slice(&x.to_le_bytes());
        }
        // IBM identity
        let ibm = Mat4::IDENTITY.to_cols_array();
        let ibm_off = bin.len();
        bin.extend_from_slice(&bytemuck_bytes(&ibm));
        // times 0, 1
        let times = [0.0f32, 1.0];
        let time_off = bin.len();
        bin.extend_from_slice(&bytemuck_bytes(&times));
        // translations
        let xlat = [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0];
        let xlat_off = bin.len();
        bin.extend_from_slice(&bytemuck_bytes(&xlat));

        let doc = json!({
            "asset": { "version": "2.0" },
            "scene": 0,
            "scenes": [{ "nodes": [0, 1] }],
            "nodes": [
                { "name": "joint", "translation": [0,0,0] },
                { "name": "Mesh", "mesh": 0, "skin": 0 }
            ],
            "meshes": [{
                "primitives": [{
                    "attributes": { "POSITION": 0, "JOINTS_0": 1, "WEIGHTS_0": 2 },
                    "indices": 3
                }]
            }],
            "skins": [{ "joints": [0], "inverseBindMatrices": 4 }],
            "animations": [{
                "name": "slide",
                "samplers": [{ "input": 5, "output": 6, "interpolation": "LINEAR" }],
                "channels": [{ "sampler": 0, "target": { "node": 0, "path": "translation" } }]
            }],
            "accessors": [
                { "bufferView": 0, "componentType": 5126, "count": 2, "type": "VEC3",
                  "min": [0, 0, 0], "max": [0, 1, 0] },
                { "bufferView": 1, "componentType": 5123, "count": 2, "type": "VEC4" },
                { "bufferView": 2, "componentType": 5126, "count": 2, "type": "VEC4" },
                { "bufferView": 3, "componentType": 5125, "count": 3, "type": "SCALAR" },
                { "bufferView": 4, "componentType": 5126, "count": 1, "type": "MAT4" },
                { "bufferView": 5, "componentType": 5126, "count": 2, "type": "SCALAR" },
                { "bufferView": 6, "componentType": 5126, "count": 2, "type": "VEC3" }
            ],
            "bufferViews": [
                { "buffer": 0, "byteOffset": pos_off, "byteLength": 24 },
                { "buffer": 0, "byteOffset": jnt_off, "byteLength": 16 },
                { "buffer": 0, "byteOffset": wgt_off, "byteLength": 32 },
                { "buffer": 0, "byteOffset": idx_off, "byteLength": 12 },
                { "buffer": 0, "byteOffset": ibm_off, "byteLength": 64 },
                { "buffer": 0, "byteOffset": time_off, "byteLength": 8 },
                { "buffer": 0, "byteOffset": xlat_off, "byteLength": 24 }
            ],
            "buffers": [{ "byteLength": bin.len() }]
        });
        pack_glb(&doc, &bin)
    }

    fn bytemuck_bytes(v: &[f32]) -> Vec<u8> {
        v.iter().flat_map(|f| f.to_le_bytes()).collect()
    }

    /// Rig must open on a mesh that has never been rigged.
    ///
    /// A freshly generated model carries no `skin`, no `JOINTS_0` and no
    /// `WEIGHTS_0`. `from_glb` refuses it with "GLB has no skin", which is
    /// correct for playback and fatal for the workbench: that is the one mesh
    /// the Rig step exists to serve, and the GUI surfaced the refusal as an
    /// error on opening Animate.
    #[test]
    fn rig_opens_on_a_mesh_with_no_skin() {
        let mut bin = Vec::new();
        for v in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
            bin.extend_from_slice(&v.to_le_bytes());
        }
        let pos_len = bin.len();
        for i in [0u16, 1, 2] {
            bin.extend_from_slice(&i.to_le_bytes());
        }
        let idx_len = bin.len() - pos_len;
        let doc = json!({
            "asset": { "version": "2.0" },
            "scene": 0,
            "scenes": [{ "nodes": [0] }],
            "nodes": [{ "mesh": 0, "name": "mesh" }],
            "meshes": [{ "primitives": [
                { "attributes": { "POSITION": 0 }, "indices": 1 }
            ]}],
            "accessors": [
                { "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
                  "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0] },
                { "bufferView": 1, "componentType": 5123, "count": 3, "type": "SCALAR" }
            ],
            "bufferViews": [
                { "buffer": 0, "byteOffset": 0, "byteLength": pos_len },
                { "buffer": 0, "byteOffset": pos_len, "byteLength": idx_len }
            ],
            "buffers": [{ "byteLength": bin.len() }]
        });
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("unrigged.glb");
        std::fs::write(&path, pack_glb(&doc, &bin)).unwrap();

        // The playback loader is right to refuse: there is nothing to play.
        let err = SkinnedClip::from_glb(&path).unwrap_err();
        assert!(err.contains("no skin"), "{err}");

        // The Rig loader takes it, and reports it as not yet fitted.
        let clip = SkinnedClip::for_rig(&path).expect("Rig must open an unrigged mesh");
        assert!(!clip.has_bind_bones(), "an unrigged mesh has no bind bones");
        assert!(!clip.has_animation());
        assert!(clip.bind_markers().is_empty(), "no markers until auto-fit");
        assert!(
            clip.rest_translations().is_empty(),
            "no joints, so no rest translations"
        );
        // The geometry is what Rig actually needs: joints are dragged onto it.
        assert_eq!(clip.vertex_count(), 3);

        // The viewer poses the mesh every frame while Rig is open. With no
        // influences to skin by, that has to fall through to the bind
        // positions: skinning would index empty JOINTS_0/WEIGHTS_0 per vertex
        // and take the GUI down on the first frame.
        let mut posed = Vec::new();
        clip.sample_positions_rest_into(&mut posed);
        assert_eq!(
            posed,
            vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]]
        );
        let mut played = Vec::new();
        clip.sample_positions_into(0.5, &mut played);
        assert_eq!(played, posed, "an unskinned mesh never deforms");

        // Dragging a joint onto the mesh raycasts the bind geometry, which is
        // the whole point of loading an unrigged mesh at all.
        let hit = clip
            .ray_midline([0.25, 0.25, 1.0], [0.0, 0.0, -1.0])
            .expect("the Rig drag must hit an unskinned mesh");
        assert!(
            hit[2].abs() < 1e-4,
            "hit should land on the z=0 face: {hit:?}"
        );

        // The bone overlay has nothing to draw, and must say so rather than
        // reaching into an empty skin.
        assert!(clip.bone_segments_rest().is_empty());
    }

    #[test]
    fn identity_bind_pose_at_t0() {
        let bytes = tiny_clip_glb();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tiny.glb");
        std::fs::File::create(&path)
            .unwrap()
            .write_all(&bytes)
            .unwrap();
        let clip = SkinnedClip::from_glb(&path).expect("load");
        assert_eq!(clip.name, "slide");
        assert_eq!(clip.indices(), &[0, 1, 0]);
        let expanded = clip.expand(&[Vec3::ZERO, Vec3::Y]).expect("indices");
        assert_eq!(expanded, vec![Vec3::ZERO, Vec3::Y, Vec3::ZERO]);
        assert!(clip.bone_segments(0.0).is_empty());
        assert!((clip.duration - 1.0).abs() < 1e-5);
        let (p0, _) = clip.sample(0.0);
        assert!((p0[0] - Vec3::ZERO).length() < 1e-5);
        assert!((p0[1] - Vec3::Y).length() < 1e-5);
        let (pmid, _) = clip.sample(0.5);
        assert!(
            (pmid[0] - Vec3::new(0.5, 0.0, 0.0)).length() < 1e-4,
            "t=0.5 vert0 {}",
            pmid[0]
        );
        let (p1, _) = clip.sample(0.999);
        assert!((p1[0] - Vec3::X).length() < 2e-2, "t=0.999 vert0 {}", p1[0]);
    }

    #[test]
    fn place_heads_move_rest_mesh_not_clip_time() {
        let bytes = tiny_clip_glb();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tiny.glb");
        std::fs::File::create(&path)
            .unwrap()
            .write_all(&bytes)
            .unwrap();
        let mut clip = SkinnedClip::from_glb(&path).expect("load");
        clip.apply_world_heads(&[("joint".into(), [2.0, 0.0, 0.0])]);
        let mut rest = Vec::new();
        clip.sample_positions_rest_into(&mut rest);
        assert!(
            (Vec3::from_array(rest[0]) - Vec3::new(2.0, 0.0, 0.0)).length() < 1e-4,
            "rest vert0 {:?}",
            rest[0]
        );
        let (p0, _) = clip.sample(0.0);
        assert!(
            (p0[0] - Vec3::ZERO).length() < 1e-4,
            "clip t=0 still uses the channel, got {}",
            p0[0]
        );
    }
}

/// Two crossings closer than this are the same surface, hit twice.
///
/// Sized in metres against a human-scale mesh: limb walls are centimetres
/// apart at the thinnest (fingers), so a tenth of a millimetre separates
/// "shared edge" from "genuinely thin".
const SURFACE_EPSILON: f32 = 1e-4;

/// Möller-Trumbore ray/triangle intersection, double-sided.
///
/// Back faces count: the far wall of a limb is one, and finding it is the
/// whole point of [`SkinnedClip::ray_midline`].
fn ray_triangle(origin: Vec3, dir: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Option<f32> {
    const EPS: f32 = 1e-7;
    let e1 = b - a;
    let e2 = c - a;
    let h = dir.cross(e2);
    let det = e1.dot(h);
    if det.abs() < EPS {
        return None;
    }
    let inv = 1.0 / det;
    let s = origin - a;
    let u = inv * s.dot(h);
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = inv * dir.dot(q);
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = inv * e2.dot(q);
    (t > EPS).then_some(t)
}
