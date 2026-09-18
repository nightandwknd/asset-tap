//! Load the pack armature, fit joint locals to mesh landmarks.

use super::landmarks::{Landmarks, up_vec};
use crate::rig::BindError;
use crate::rig::canon::{BoneScheme, HumanBone};
use glam::{Mat4, Quat, Vec3};
use std::collections::HashMap;
use std::path::Path;

/// One node in the pack armature (joints + Rig/root wrappers).
#[derive(Debug, Clone)]
pub struct Joint {
    pub name: String,
    pub parent: Option<usize>,
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

#[derive(Debug, Clone)]
pub struct Armature {
    pub joints: Vec<Joint>,
    /// Skin joint order → index into `joints`.
    pub skin: Vec<usize>,
    pub name_to_index: HashMap<String, usize>,
}

impl Armature {
    pub fn local_matrix(&self, i: usize) -> Mat4 {
        let j = &self.joints[i];
        Mat4::from_scale_rotation_translation(j.scale, j.rotation, j.translation)
    }

    pub fn world_matrices(&self) -> Vec<Mat4> {
        let n = self.joints.len();
        let mut worlds = vec![Mat4::IDENTITY; n];
        let mut done = vec![false; n];
        for i in 0..n {
            compute_world(self, i, &mut worlds, &mut done);
        }
        worlds
    }

    pub fn world_heads(&self) -> Vec<Vec3> {
        self.world_matrices()
            .iter()
            .map(|m| m.transform_point3(Vec3::ZERO))
            .collect()
    }

    pub fn children(&self) -> Vec<Vec<usize>> {
        let mut ch = vec![Vec::new(); self.joints.len()];
        for (i, j) in self.joints.iter().enumerate() {
            if let Some(p) = j.parent {
                ch[p].push(i);
            }
        }
        ch
    }

    /// Canonical joint lookup. Armatures are canonicalized on load, so every
    /// name here is a [`HumanBone`] name.
    pub fn joint_index(&self, name: &str) -> Option<usize> {
        self.name_to_index.get(name).copied()
    }

    pub fn set_world_head(&mut self, name: &str, target: Vec3) {
        let Some(i) = self.joint_index(name) else {
            return;
        };
        let worlds = self.world_matrices();
        let parent_world = self.joints[i]
            .parent
            .map(|p| worlds[p])
            .unwrap_or(Mat4::IDENTITY);
        let inv = parent_world.inverse();
        self.joints[i].translation = inv.transform_point3(target);
    }

    /// Pull any joint whose head sits off the mesh back inside, along the bone
    /// toward its parent.
    ///
    /// Auto-fit's contract is a pose the author can Bind, and Bind refuses a
    /// head off the mesh because inverse-distance weights are taken from the
    /// heads: an outside head gets ~0 weight and drives nothing. A fit that
    /// emits one has produced a pose that cannot be used, and the author has to
    /// find and drag the joint themselves with no clue which way to go.
    ///
    /// Shoulders are where it happens. Their landmark is the centroid of a band
    /// that also catches sleeve, cape and pauldron geometry, so on a broad or
    /// heavily dressed character the point lands outside the body.
    ///
    /// Toward the parent, because that is inboard for every joint that runs
    /// off: a shoulder retreats toward the neck, a hand up the arm. Parent-first
    /// so a joint is tested against a parent that has already been corrected.
    pub fn pull_heads_onto_mesh(&mut self, on_mesh: &dyn Fn(Vec3) -> bool) -> Vec<String> {
        /// Fractions of the way to the parent to try. Fine near the joint so a
        /// head barely outside barely moves.
        const STEPS: [f32; 8] = [0.08, 0.16, 0.25, 0.35, 0.45, 0.6, 0.75, 0.9];

        let mut moved = Vec::new();
        for i in self.parent_first_order() {
            if !crate::rig::weights::is_placeable_joint(&self.joints[i].name) {
                continue;
            }
            let worlds = self.world_matrices();
            let head = worlds[i].transform_point3(Vec3::ZERO);
            if on_mesh(head) {
                continue;
            }
            let Some(p) = self.joints[i].parent else {
                continue;
            };
            let anchor = worlds[p].transform_point3(Vec3::ZERO);
            // A parent that is itself outside offers nothing to retreat to.
            if !on_mesh(anchor) {
                continue;
            }
            if let Some(fixed) = STEPS
                .iter()
                .map(|t| head.lerp(anchor, *t))
                .find(|c| on_mesh(*c))
            {
                let name = self.joints[i].name.clone();
                self.set_world_head(&name, fixed);
                moved.push(name);
            }
        }
        moved
    }

    /// Parent-first so each child's world is solved against the *new* parent.
    /// Bind-bone heads in `heads` become the committed worlds; omitted joints stay.
    pub fn apply_world_heads_parent_first(&mut self, heads: &[(String, [f32; 3])]) {
        let mut by_name: HashMap<String, Vec3> = HashMap::new();
        for (name, xyz) in heads {
            by_name.insert(name.clone(), Vec3::from_array(*xyz));
        }
        for i in self.parent_first_order() {
            let name = self.joints[i].name.clone();
            if !crate::rig::weights::is_placeable_joint(&name) {
                continue;
            }
            let Some(target) = by_name.get(&name).copied() else {
                continue;
            };
            self.set_world_head(&name, target);
        }
    }

    fn parent_first_order(&self) -> Vec<usize> {
        let n = self.joints.len();
        let mut out = Vec::with_capacity(n);
        let mut seen = vec![false; n];
        fn walk(arm: &Armature, i: usize, seen: &mut [bool], out: &mut Vec<usize>) {
            if seen[i] {
                return;
            }
            if let Some(p) = arm.joints[i].parent {
                walk(arm, p, seen, out);
            }
            seen[i] = true;
            out.push(i);
        }
        for i in 0..n {
            walk(self, i, &mut seen, &mut out);
        }
        out
    }
}

fn compute_world(arm: &Armature, i: usize, worlds: &mut [Mat4], done: &mut [bool]) {
    if done[i] {
        return;
    }
    let local = arm.local_matrix(i);
    worlds[i] = match arm.joints[i].parent {
        Some(p) => {
            compute_world(arm, p, worlds, done);
            worlds[p] * local
        }
        None => local,
    };
    done[i] = true;
}

/// Load every node from the pack (we keep hierarchy so clip channels remap).
pub fn load_from_gltf(doc: &gltf::Document) -> Result<Armature, BindError> {
    let nodes: Vec<_> = doc.nodes().collect();
    let mut joints = Vec::with_capacity(nodes.len());
    let mut name_to_index = HashMap::new();
    for (i, node) in nodes.iter().enumerate() {
        let (t, r, s) = node.transform().decomposed();
        let name = node
            .name()
            .map(|n| n.to_string())
            .unwrap_or_else(|| format!("node_{i}"));
        name_to_index.insert(name.clone(), i);
        joints.push(Joint {
            name,
            parent: None,
            translation: Vec3::from_array(t),
            rotation: Quat::from_array(r),
            scale: Vec3::from_array(s),
        });
    }
    for node in &nodes {
        let pi = node.index();
        for child in node.children() {
            joints[child.index()].parent = Some(pi);
        }
    }
    let skin = doc
        .skins()
        .next()
        .ok_or_else(|| BindError::Failed("clip pack has no skin / armature".into()))?;
    let skin: Vec<usize> = skin.joints().map(|n| n.index()).collect();
    let mut arm = Armature {
        joints,
        skin,
        name_to_index,
    };
    canonicalize(&mut arm);
    Ok(arm)
}

/// Rename a loaded armature's bones to canonical ([`HumanBone`]) names.
///
/// Everything downstream is keyed by name — `overlay_trs_by_name`, weighting,
/// and the writer's pack-index-to-rest-node remap — so canonicalizing here is
/// what lets one code path serve packs that disagree on naming, and lets an
/// asset fitted before this change still load. Bones with no canonical joint
/// (leaf tips, armature wrappers) keep their names and simply match nothing.
///
/// Returns the detected scheme, or `None` when the nodes are not a humanoid
/// rig, in which case names are left untouched.
pub fn canonicalize(arm: &mut Armature) -> Option<BoneScheme> {
    let scheme = BoneScheme::detect(arm.joints.iter().map(|j| j.name.as_str()))?;
    if scheme != BoneScheme::Vrm {
        for joint in &mut arm.joints {
            if let Some(bone) = scheme.to_canonical(&joint.name) {
                joint.name = bone.as_str().to_string();
            }
        }
        arm.name_to_index = arm
            .joints
            .iter()
            .enumerate()
            .map(|(i, j)| (j.name.clone(), i))
            .collect();
    }
    Some(scheme)
}

/// Place hips/feet by root scale+translate, then snap named bones to landmarks.
pub fn fit(arm: &mut Armature, lm: &Landmarks) {
    let hips_name = first_present(arm, &[HumanBone::Hips.as_str()]);
    let foot_l = first_present(arm, &[HumanBone::LeftFoot.as_str()]);
    let foot_r = first_present(arm, &[HumanBone::RightFoot.as_str()]);
    let Some(hips_name) = hips_name else {
        return;
    };

    let heads = arm.world_heads();
    let hips_i = arm.name_to_index[&hips_name];
    let up = up_vec(lm.up);
    let src_hips = heads[hips_i];
    let src_foot = match (foot_l.as_ref(), foot_r.as_ref()) {
        (Some(l), Some(r)) => 0.5 * (heads[arm.name_to_index[l]] + heads[arm.name_to_index[r]]),
        _ => src_hips - up * 0.9,
    };
    let src_leg = (src_hips - src_foot).dot(up).abs().max(1e-6);
    let dst_leg = (lm.hips - 0.5 * (lm.ankle_l + lm.ankle_r))
        .dot(up)
        .abs()
        .max(1e-6);
    let scale = dst_leg / src_leg;

    // Scale + translate the top-most node so the whole rest pose resizes.
    if let Some(root) = root_index(arm) {
        arm.joints[root].scale *= scale;
        arm.joints[root].translation *= scale;
    }
    let heads = arm.world_heads();
    let src_hips = heads[hips_i];
    if let Some(root) = root_index(arm) {
        arm.joints[root].translation += lm.hips - src_hips;
    }

    arm.set_world_head(&hips_name, lm.hips);
    // Do not teleport `head`. The 78–88% landmark is the head *volume*
    // centroid (beard/hair), which sat below `neck` and folded the bone
    // back on itself. After root scale the pack neck→head chain is enough.
    if let Some(n) = first_present(arm, &[HumanBone::LeftUpperArm.as_str()]) {
        arm.set_world_head(&n, lm.shoulder_l);
        if let Some(hand) = first_present(arm, &[HumanBone::LeftHand.as_str()]) {
            reach_hand(arm, &n, &hand, lm.hand_l);
        }
    }
    if let Some(n) = first_present(arm, &[HumanBone::RightUpperArm.as_str()]) {
        arm.set_world_head(&n, lm.shoulder_r);
        if let Some(hand) = first_present(arm, &[HumanBone::RightHand.as_str()]) {
            reach_hand(arm, &n, &hand, lm.hand_r);
        }
    }
    if let Some(n) = first_present(arm, &[HumanBone::LeftUpperLeg.as_str()]) {
        arm.set_world_head(&n, lm.hip_l);
    }
    if let Some(n) = first_present(arm, &[HumanBone::RightUpperLeg.as_str()]) {
        arm.set_world_head(&n, lm.hip_r);
    }
    if let Some(n) = foot_l {
        arm.set_world_head(&n, lm.ankle_l);
    }
    if let Some(n) = foot_r {
        arm.set_world_head(&n, lm.ankle_r);
    }
    if let Some(n) = first_present(arm, &[HumanBone::LeftToes.as_str()]) {
        arm.set_world_head(&n, lm.toe_l);
    }
    if let Some(n) = first_present(arm, &[HumanBone::RightToes.as_str()]) {
        arm.set_world_head(&n, lm.toe_r);
    }
}

/// Aim + scale the arm so the hand reaches `target` without breaking the
/// chain. Teleporting `hand` via [`Armature::set_world_head`] parked
/// wrists inside the elbows on viking/punzel.
fn reach_hand(arm: &mut Armature, upper_name: &str, hand_name: &str, target: Vec3) {
    let Some(&ui) = arm.name_to_index.get(upper_name) else {
        return;
    };
    let Some(&hi) = arm.name_to_index.get(hand_name) else {
        return;
    };
    let heads = arm.world_heads();
    let shoulder = heads[ui];
    let hand = heads[hi];
    let cur = hand - shoulder;
    let des = target - shoulder;
    let cur_len = cur.length();
    let des_len = des.length();
    if cur_len < 1e-5 || des_len < 1e-5 || cur.dot(des) <= 0.0 {
        return;
    }
    aim_joint(arm, ui, cur, des);
    let s = (des_len / cur_len).clamp(0.55, 1.45);
    for i in descendants(arm, ui) {
        arm.joints[i].translation *= s;
    }
}

fn aim_joint(arm: &mut Armature, i: usize, from: Vec3, to: Vec3) {
    let from_n = from.normalize_or_zero();
    let to_n = to.normalize_or_zero();
    if from_n.length_squared() < 0.5 || to_n.length_squared() < 0.5 {
        return;
    }
    let q = Quat::from_rotation_arc(from_n, to_n);
    let worlds = arm.world_matrices();
    let parent_rot = match arm.joints[i].parent {
        Some(p) => worlds[p].to_scale_rotation_translation().1,
        None => Quat::IDENTITY,
    };
    let local = arm.joints[i].rotation;
    arm.joints[i].rotation = (parent_rot.inverse() * q * parent_rot * local).normalize();
}

fn descendants(arm: &Armature, root: usize) -> Vec<usize> {
    let ch = arm.children();
    let mut out = Vec::new();
    let mut stack = ch[root].clone();
    while let Some(i) = stack.pop() {
        out.push(i);
        stack.extend_from_slice(&ch[i]);
    }
    out
}

fn first_present(arm: &Armature, names: &[&str]) -> Option<String> {
    names
        .iter()
        .find(|n| arm.name_to_index.contains_key(**n))
        .map(|s| (*s).to_string())
}

/// The node to scale and translate so the whole rest pose resizes: the
/// armature wrapper, else the skeleton root, else whatever has no parent.
/// Both names are canonical constants; the armature is always
/// [`crate::rig::canon::armature`] or a fitted copy of it.
fn root_index(arm: &Armature) -> Option<usize> {
    use crate::rig::canon::{ARMATURE_NAME, ROOT_NAME};
    [ARMATURE_NAME, ROOT_NAME]
        .iter()
        .find_map(|n| arm.joint_index(n))
        .or_else(|| arm.joints.iter().position(|j| j.parent.is_none()))
}

pub fn load_document(path: &Path) -> Result<(gltf::Document, Vec<gltf::buffer::Data>), BindError> {
    import_no_images(path)
}

/// Import a glTF's document and buffers, **without decoding images**.
///
/// `gltf::import` eagerly decodes every texture into an `image::DynamicImage`.
/// Nothing in the rig path reads decoded pixels — the writer copies raw image
/// bytes straight out of the source JSON — so on a textured multi-megabyte
/// asset that decode is pure latency. It was being paid three times over when
/// the Animate panel opened, on the UI thread.
pub fn import_no_images(
    path: &Path,
) -> Result<(gltf::Document, Vec<gltf::buffer::Data>), BindError> {
    let gltf::Gltf { document, blob } = read_gltf(path)?;
    let buffers =
        gltf::import_buffers(&document, path.parent(), blob).map_err(|e| BindError::Gltf {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?;
    Ok((document, buffers))
}

/// Parse only the glTF JSON — no buffers, no images.
///
/// Enough to answer questions about structure (which animations exist, what
/// the nodes are called) without touching the geometry.
pub fn import_json_only(path: &Path) -> Result<gltf::Document, BindError> {
    Ok(read_gltf(path)?.document)
}

fn read_gltf(path: &Path) -> Result<gltf::Gltf, BindError> {
    let bytes = std::fs::read(path).map_err(|e| BindError::Io {
        path: path.to_path_buf(),
        source: e,
    })?;
    // Without validation on purpose. The crate refuses any document whose
    // `extensionsRequired` it does not implement, and provider output routinely
    // requires `EXT_texture_webp` (fal's trellis-2 does, today, by default).
    // Such a file is perfectly good geometry: the texture bytes are copied
    // through verbatim and nothing on this path reads a pixel or resolves a
    // texture, so an image extension is not ours to have an opinion about.
    // Structural problems that *would* matter are checked right after, by
    // `check_indices`: skipping the crate's validation also skips its
    // index-range checks, and the crate `unwrap()`s an out-of-range index the
    // moment the node, primitive or skin is touched.
    let gltf = gltf::Gltf::from_slice_without_validation(&bytes).map_err(|e| BindError::Gltf {
        path: path.to_path_buf(),
        message: e.to_string(),
    })?;
    check_indices(&gltf.document).map_err(|message| BindError::Gltf {
        path: path.to_path_buf(),
        message,
    })?;
    Ok(gltf)
}

/// Run the crate's document validation, admitting only the errors this path
/// deliberately tolerates.
///
/// Admitted: `Unsupported` (an `extensionsRequired` entry the crate does not
/// implement, such as `EXT_texture_webp`) and a `Missing` core
/// `textures[].source`, which omitting the fallback under a required texture
/// extension legally implies. Everything else, and in particular every
/// `IndexOutOfBounds`, is returned as the error message: a material, mesh,
/// accessor or skin-joint index past the end of its array is `unwrap()`ed by
/// the crate on first use, which took the GUI down from a malformed drop.
pub(crate) fn check_indices(doc: &gltf::Document) -> Result<(), String> {
    use gltf::json::validation::{Error, Validate};
    let root = doc.as_json();
    // The crate's own validator indexes `root.accessors[POSITION]` directly
    // while checking a primitive's `min`/`max`, so that one index has to be
    // range-checked before the validator may run at all.
    for (mi, mesh) in root.meshes.iter().enumerate() {
        for (pi, prim) in mesh.primitives.iter().enumerate() {
            let attrs = prim
                .attributes
                .values()
                .chain(prim.targets.iter().flatten().flat_map(|t| {
                    [
                        t.positions.as_ref(),
                        t.normals.as_ref(),
                        t.tangents.as_ref(),
                    ]
                    .into_iter()
                    .flatten()
                }));
            for acc in attrs {
                if acc.value() >= root.accessors.len() {
                    return Err(format!(
                        "meshes[{mi}].primitives[{pi}]: accessor {} out of range ({} accessors)",
                        acc.value(),
                        root.accessors.len()
                    ));
                }
            }
        }
    }
    let mut fatal: Vec<String> = Vec::new();
    root.validate(root, gltf::json::Path::new, &mut |path, error| {
        let path = path();
        let admitted = match error {
            Error::Unsupported => true,
            Error::Missing => path.0.starts_with("textures[") && path.0.ends_with(".source"),
            _ => false,
        };
        if !admitted {
            fatal.push(format!("{path}: {error}"));
        }
    });
    if fatal.is_empty() {
        Ok(())
    } else {
        Err(fatal.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A joint fitted outside the body is pulled back in, along the bone.
    ///
    /// Auto-fit's contract is a pose Bind will take. Landmarks are centroids of
    /// bands, so sleeve or pauldron geometry can put a shoulder outside the
    /// body: on a real long-coated character `rightShoulder` landed 3.2 cm out
    /// and Bind refused the fit the author had just asked for, naming a joint
    /// with no hint of which way to drag it.
    #[test]
    fn an_off_mesh_head_retreats_toward_its_parent() {
        let mut arm = crate::rig::canon::armature();
        let name = crate::rig::canon::HumanBone::LeftUpperArm.as_str();
        let i = arm.joint_index(name).expect("canonical joint");
        let parent = arm.joints[i].parent.expect("upper arm has a parent");

        let before = arm.world_heads();
        let anchor = before[parent];
        let stray = before[i] + Vec3::new(0.6, 0.0, 0.0);
        arm.set_world_head(name, stray);

        // "On mesh" here is a ball around the parent: everything within reach
        // of the body, nothing out where the stray head went.
        let radius = (before[i] - anchor).length() * 1.05;
        let on_mesh = move |p: Vec3| (p - anchor).length() <= radius;
        assert!(!on_mesh(stray), "the setup must actually be off-mesh");

        let moved = arm.pull_heads_onto_mesh(&on_mesh);
        assert!(
            moved.iter().any(|n| n == name),
            "the stray joint should be reported as moved, got {moved:?}"
        );

        let after = arm.world_heads()[i];
        assert!(on_mesh(after), "it must end up inside: {after:?}");
        // Toward the parent, not to it: a head barely outside barely moves.
        assert!(
            (after - anchor).length() > 0.1 * radius,
            "pulled all the way onto its parent: {after:?}"
        );
    }

    /// A joint already inside is left exactly where the fit put it.
    #[test]
    fn heads_on_the_mesh_are_not_disturbed() {
        let mut arm = crate::rig::canon::armature();
        let before = arm.world_heads();
        let moved = arm.pull_heads_onto_mesh(&|_| true);
        assert!(moved.is_empty(), "nothing was off-mesh, got {moved:?}");
        for (i, b) in before.iter().enumerate() {
            assert!(
                (arm.world_heads()[i] - *b).length() < 1e-6,
                "joint {i} drifted"
            );
        }
    }

    #[test]
    fn set_world_head_respects_parent() {
        let mut arm = Armature {
            joints: vec![
                Joint {
                    name: "root".into(),
                    parent: None,
                    translation: Vec3::ZERO,
                    rotation: Quat::IDENTITY,
                    scale: Vec3::ONE,
                },
                Joint {
                    name: "hips".into(),
                    parent: Some(0),
                    translation: Vec3::Y,
                    rotation: Quat::IDENTITY,
                    scale: Vec3::ONE,
                },
            ],
            skin: vec![1],
            name_to_index: [("root".into(), 0), ("hips".into(), 1)]
                .into_iter()
                .collect(),
        };
        arm.set_world_head("hips", Vec3::new(0.0, 0.6, 0.0));
        let h = arm.world_heads();
        assert!((h[1] - Vec3::new(0.0, 0.6, 0.0)).length() < 1e-5);
    }

    fn tpose_arm() -> Armature {
        let names = ["root", "leftUpperArm", "leftLowerArm", "leftHand"];
        let joints = vec![
            Joint {
                name: names[0].into(),
                parent: None,
                translation: Vec3::ZERO,
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
            },
            Joint {
                name: names[1].into(),
                parent: Some(0),
                translation: Vec3::new(0.15, 1.3, 0.0),
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
            },
            Joint {
                name: names[2].into(),
                parent: Some(1),
                translation: Vec3::new(0.25, 0.0, 0.0),
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
            },
            Joint {
                name: names[3].into(),
                parent: Some(2),
                translation: Vec3::new(0.22, 0.0, 0.0),
                rotation: Quat::IDENTITY,
                scale: Vec3::ONE,
            },
        ];
        Armature {
            joints,
            skin: vec![1, 2, 3],
            name_to_index: names
                .iter()
                .enumerate()
                .map(|(i, n)| ((*n).to_string(), i))
                .collect(),
        }
    }

    #[test]
    fn reach_hand_keeps_chain_outboard() {
        let mut arm = tpose_arm();
        reach_hand(
            &mut arm,
            "leftUpperArm",
            "leftHand",
            Vec3::new(0.80, 1.3, 0.0),
        );
        let h = arm.world_heads();
        assert!(
            h[1].x < h[2].x && h[2].x < h[3].x,
            "shoulder {} elbow {} hand {}",
            h[1].x,
            h[2].x,
            h[3].x
        );
        assert!((h[3].x - 0.80).abs() < 0.05, "hand {}", h[3]);
    }

    #[test]
    fn parent_first_heads_match_arranged_markers() {
        let mut arm = Armature {
            joints: vec![
                Joint {
                    name: "hips".into(),
                    parent: None,
                    translation: Vec3::new(0.0, 1.0, 0.0),
                    rotation: Quat::IDENTITY,
                    scale: Vec3::ONE,
                },
                Joint {
                    name: "spine".into(),
                    parent: Some(0),
                    translation: Vec3::new(0.0, 0.2, 0.0),
                    rotation: Quat::IDENTITY,
                    scale: Vec3::ONE,
                },
            ],
            skin: vec![0, 1],
            name_to_index: [("hips".into(), 0), ("spine".into(), 1)]
                .into_iter()
                .collect(),
        };
        // Independent markers: hips moved +X, spine stays at its old world.
        let before = arm.world_heads();
        let arranged = [
            ("hips".into(), [0.3, 1.0, 0.0]),
            ("spine".into(), before[1].to_array()),
        ];
        arm.apply_world_heads_parent_first(&arranged);
        let after = arm.world_heads();
        assert!((after[0] - Vec3::new(0.3, 1.0, 0.0)).length() < 1e-5);
        assert!(
            (after[1] - before[1]).length() < 1e-5,
            "spine should stay at arranged world {}, got {}",
            before[1],
            after[1]
        );
    }
}
