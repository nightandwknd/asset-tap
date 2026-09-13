//! Inverse-distance weights to bone segments. Four influences, glTF-normalized.

use crate::rig::canon::HumanBone;
use glam::Vec3;

/// One bind-pose bone as a segment in world space.
#[derive(Debug, Clone, Copy)]
pub struct BoneSeg {
    pub joint: u16,
    pub a: Vec3,
    pub b: Vec3,
}

/// Per-vertex joints + weights (glTF `JOINTS_0` / `WEIGHTS_0`).
#[derive(Debug, Clone)]
pub struct Skinning {
    pub joints: Vec<[u16; 4]>,
    pub weights: Vec<[f32; 4]>,
}

/// Major deformers only. Finger bones stay in the exported skin (clips
/// target them) but must not receive weights — generated mitts/sleeves
/// explode when 20 tiny finger segments steal the bind.
pub fn is_bind_bone(name: &str) -> bool {
    if let Some(bone) = HumanBone::parse(name) {
        return !bone.is_finger();
    }
    // `root` and friends are transform parents, not deformers, and no clip
    // animates them. They are in the skin because glTF wants the whole chain,
    // and letting them take weights is a disaster: `root` sits at the origin
    // and its bone segment runs from there up to `hips`, straight through the
    // pelvis, so it wins the inverse-distance bind for everything around the
    // crotch. On a real character that was 39.7% of the mesh anchored to a
    // joint that never moves. It smears every clip and erupts as a spike out
    // of the hips in anything that turns the body far from bind pose.
    if is_armature_wrapper(name) {
        return false;
    }
    // No canonical joint: an armature we could not classify, or a pack's
    // leaf/tip bones. Neither may take weights — 20 tiny finger segments
    // stealing the bind is what explodes generated mitts and sleeves.
    let n = name.to_ascii_lowercase();
    !(n.contains("leaf")
        || n.contains("index")
        || n.contains("middle")
        || n.contains("ring")
        || n.contains("pinky")
        || n.contains("thumb"))
}

/// Joints the workbench may show, drag, or write. Excludes fingers, armature
/// wrappers (`Rig`, `Mannequin`), and mesh nodes named `mesh` / `Mesh0` that
/// the writer appends as scene siblings — those are not joints.
pub fn is_placeable_joint(name: &str) -> bool {
    if let Some(bone) = HumanBone::parse(name) {
        return bone.is_placeable();
    }
    !name.is_empty() && is_bind_bone(name) && !is_armature_wrapper(name) && !is_mesh_node_name(name)
}

fn is_armature_wrapper(name: &str) -> bool {
    matches!(
        name,
        "Rig" | "root" | "Root" | "Armature" | "Mannequin" | "Skeleton"
    )
}

fn is_mesh_node_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    n == "mesh" || n.starts_with("mesh")
}

/// Assign up to four influences per vertex. `falloff` 2.0 is Mixamo-ish.
pub fn smooth_bind(positions: &[Vec3], segs: &[BoneSeg], falloff: f32) -> Skinning {
    assert!(!segs.is_empty(), "smooth_bind needs bone segments");
    let falloff = falloff.clamp(0.5, 8.0);
    let mut joints = Vec::with_capacity(positions.len());
    let mut weights = Vec::with_capacity(positions.len());
    let mut dists = Vec::with_capacity(segs.len());
    for &p in positions {
        dists.clear();
        for s in segs {
            dists.push((seg_distance(p, s.a, s.b), s.joint));
        }
        dists.sort_by(|x, y| x.0.partial_cmp(&y.0).unwrap().then(x.1.cmp(&y.1)));
        let take = dists.len().min(4);
        let mut js = [0u16; 4];
        let mut ws = [0f32; 4];
        let mut sum = 0.0;
        for (i, &(d, j)) in dists[..take].iter().enumerate() {
            let w = 1.0 / (d + 1e-4).powf(falloff);
            js[i] = j;
            ws[i] = w;
            sum += w;
        }
        if sum > 0.0 {
            for w in &mut ws {
                *w /= sum;
            }
        } else {
            ws[0] = 1.0;
        }
        joints.push(js);
        weights.push(ws);
    }
    Skinning { joints, weights }
}

fn seg_distance(p: Vec3, a: Vec3, b: Vec3) -> f32 {
    let ab = b - a;
    let len2 = ab.length_squared();
    if len2 < 1e-12 {
        return (p - a).length();
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    (p - (a + ab * t)).length()
}

/// Build segments from rest-pose joint worlds. A bone runs from a joint to
/// the mean of its children, or a short stub along `+Y` for leaves.
pub fn segments_from_hierarchy(
    worlds: &[Vec3],
    children: &[Vec<usize>],
    stub: f32,
) -> Vec<BoneSeg> {
    worlds
        .iter()
        .enumerate()
        .map(|(i, &a)| {
            let kids = &children[i];
            let b = if kids.is_empty() {
                a + Vec3::Y * stub
            } else {
                kids.iter().map(|&c| worlds[c]).sum::<Vec3>() / kids.len() as f32
            };
            BoneSeg {
                joint: i as u16,
                a,
                b,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_bone_wins() {
        let segs = [
            BoneSeg {
                joint: 0,
                a: Vec3::ZERO,
                b: Vec3::Y,
            },
            BoneSeg {
                joint: 1,
                a: Vec3::Y,
                b: Vec3::new(1.0, 1.0, 0.0),
            },
        ];
        let skin = smooth_bind(
            &[Vec3::new(0.0, 0.1, 0.0), Vec3::new(1.0, 1.0, 0.0)],
            &segs,
            2.5,
        );
        assert_eq!(skin.joints[0][0], 0);
        assert!(skin.weights[0][0] > 0.5);
        assert_eq!(skin.joints[1][0], 1);
        let sum: f32 = skin.weights[0].iter().sum();
        assert!((sum - 1.0).abs() < 1e-5);
    }

    #[test]
    fn fingers_are_not_bind_bones() {
        assert!(is_bind_bone("leftHand"));
        assert!(is_bind_bone("rightLowerArm"));
        assert!(!is_bind_bone("leftIndexProximal"));
        assert!(!is_bind_bone("rightThumbProximal"));
        // Un-canonicalized pack names still have to be refused: leaf tips
        // have no canonical joint and would otherwise take weights.
        assert!(!is_bind_bone("index_04_leaf_l"));
        assert!(!is_bind_bone("ball_leaf_r"));
        assert!(!is_bind_bone("thumb_01_l"));
    }

    /// The armature wrapper must never take weights.
    ///
    /// `root` is in the skin because glTF wants the whole chain, but no clip
    /// animates it. Its bone segment runs from the origin up to `hips`, right
    /// through the pelvis, so inverse-distance handed it everything around the
    /// crotch: 39.7% of a real character's vertices, anchored to a joint that
    /// never moves. Every clip smeared, and anything that turned the body far
    /// from bind pose tore a spike out of the hips.
    #[test]
    fn the_armature_wrapper_never_takes_weights() {
        for wrapper in ["root", "Root", "Rig", "Armature", "Mannequin", "Skeleton"] {
            assert!(
                !is_bind_bone(wrapper),
                "{wrapper} is a transform parent, not a deformer"
            );
        }
        // The joints that do deform are untouched by that rule.
        assert!(is_bind_bone("hips"));
        assert!(is_bind_bone("spine"));
        assert!(is_bind_bone("leftUpperLeg"));
    }

    #[test]
    fn mesh_and_rig_are_not_placeable() {
        assert!(is_placeable_joint("rightUpperArm"));
        assert!(!is_placeable_joint("mesh"));
        assert!(!is_placeable_joint("Mesh"));
        assert!(!is_placeable_joint("Mesh0"));
        assert!(!is_placeable_joint("Rig"));
        assert!(!is_placeable_joint("Mannequin"));
        assert!(!is_placeable_joint("leftThumbMetacarpal"));
    }

    #[test]
    fn yanked_segment_loses_smooth_bind() {
        // Why Bind refuses off-mesh heads (`onmesh`). A bone with no mesh
        // near it gets ~0 inverse-distance weight, its neighbors take the
        // verts, and the walk looks stock — the author reads that as "the
        // rig did nothing". Weights must follow the heads (Meshy), so the
        // heads must be on the mesh.
        let on_mesh = [BoneSeg {
            joint: 0,
            a: Vec3::ZERO,
            b: Vec3::Y,
        }];
        let yanked = [BoneSeg {
            joint: 0,
            a: Vec3::new(20.0, 0.0, 0.0),
            b: Vec3::new(20.0, 1.0, 0.0),
        }];
        let p = Vec3::new(0.0, 0.5, 0.0);
        let kept = smooth_bind(&[p], &on_mesh, 2.5);
        assert!(kept.weights[0][0] > 0.99);
        let two = [
            yanked[0],
            BoneSeg {
                joint: 1,
                a: Vec3::ZERO,
                b: Vec3::Y,
            },
        ];
        let after = smooth_bind(&[p], &two, 2.5);
        assert_eq!(after.joints[0][0], 1);
        assert!(after.weights[0][0] > 0.99);
    }
}
