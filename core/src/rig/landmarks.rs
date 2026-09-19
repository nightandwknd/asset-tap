//! Body landmarks from a T-pose mesh. Up is the axis of greatest extent
//! (glTF Y after the Trellis +90° X bake).

use glam::Vec3;

/// Named points the fitter aims at. Coordinates are mesh **world** space.
#[derive(Debug, Clone, PartialEq)]
pub struct Landmarks {
    pub hips: Vec3,
    pub head: Vec3,
    pub hand_l: Vec3,
    pub hand_r: Vec3,
    pub shoulder_l: Vec3,
    pub shoulder_r: Vec3,
    pub hip_l: Vec3,
    pub hip_r: Vec3,
    pub ankle_l: Vec3,
    pub ankle_r: Vec3,
    pub toe_l: Vec3,
    pub toe_r: Vec3,
    pub toe_tip_l: Vec3,
    pub toe_tip_r: Vec3,
    /// Axis of greatest AABB extent (0=X, 1=Y, 2=Z).
    pub up: usize,
}

impl Landmarks {
    pub fn height(&self) -> f32 {
        (self.head - self.ankle_l).dot(up_vec(self.up)).abs()
    }
}

pub fn up_vec(up: usize) -> Vec3 {
    match up {
        0 => Vec3::X,
        2 => Vec3::Z,
        _ => Vec3::Y,
    }
}

/// Sample a standing T-pose cloud. `positions` are already in world space.
pub fn mesh_landmarks(positions: &[Vec3]) -> Result<Landmarks, String> {
    if positions.len() < 32 {
        return Err("mesh has too few vertices to landmark".into());
    }
    let mut mins = Vec3::splat(f32::INFINITY);
    let mut maxs = Vec3::splat(f32::NEG_INFINITY);
    for &p in positions {
        mins = mins.min(p);
        maxs = maxs.max(p);
    }
    let extent = maxs - mins;
    let up = pick_up_axis(extent, mins);
    let height = extent[up].max(1e-6);
    let side = match up {
        1 | 2 => 0,
        _ => 1,
    };
    let fwd = 3 - up - side;

    let band = |lo: f32, hi: f32, pred: &dyn Fn(Vec3) -> bool| -> Vec<Vec3> {
        let a = mins[up] + lo * height;
        let b = mins[up] + hi * height;
        positions
            .iter()
            .copied()
            .filter(|&p| p[up] >= a && p[up] <= b && pred(p))
            .collect()
    };
    let centroid = |vs: &[Vec3]| -> Option<Vec3> {
        if vs.is_empty() {
            return None;
        }
        Some(vs.iter().copied().sum::<Vec3>() / vs.len() as f32)
    };
    let side_span = extent[side].max(1e-6);

    let hips = centroid(&band(0.48, 0.58, &|p| {
        p[side].abs() < 0.20 * side_span && p[fwd].abs() < 0.18 * side_span.max(0.15)
    }))
    .ok_or_else(|| "could not find hips".to_string())?;

    let head = centroid(&band(0.78, 0.88, &|p| p[side].abs() < 0.16 * side_span))
        .ok_or_else(|| "could not find head".to_string())?;

    // Outer 10% of each side in the shoulder band — not a single max-X
    // vertex (sword/cape) and not a cowboy-tuned 0.76 shrink (that put
    // wrists inside the elbows on viking/punzel).
    let arm = band(0.60, 0.86, &|_| true);
    let fwd_lim = 0.22 * height.max(0.3);
    let hand_l = outer_hand(&arm, side, 1.0, fwd, fwd_lim)
        .ok_or_else(|| "could not find hands".to_string())?;
    let hand_r = outer_hand(&arm, side, -1.0, fwd, fwd_lim)
        .ok_or_else(|| "could not find hands".to_string())?;

    let sh = band(0.70, 0.80, &|p| {
        let ax = p[side].abs();
        ax > 0.10 * side_span && ax < 0.28 * side_span
    });
    let shoulder_l = centroid(
        &sh.iter()
            .copied()
            .filter(|p| p[side] > 0.0)
            .collect::<Vec<_>>(),
    )
    .unwrap_or(Vec3::new(0.19, hips[1], hips[2]).with_axis(up, head[up] * 0.88));
    let shoulder_r = centroid(
        &sh.iter()
            .copied()
            .filter(|p| p[side] < 0.0)
            .collect::<Vec<_>>(),
    )
    .unwrap_or({
        let mut s = shoulder_l;
        s[side] = -s[side];
        s
    });

    let hip_l = centroid(&band(0.50, 0.58, &|p| {
        p[side] > 0.08 * side_span && p[side] < 0.22 * side_span && p[fwd].abs() < 0.12
    }))
    .unwrap_or(Vec3::new(0.12, hips[1], hips[2]).with_axis(up, hips[up]));
    let hip_r = centroid(&band(0.50, 0.58, &|p| {
        p[side] < -0.08 * side_span && p[side] > -0.22 * side_span && p[fwd].abs() < 0.12
    }))
    .unwrap_or({
        let mut h = hip_l;
        h[side] = -h[side];
        h
    });

    let ankles_l = band(0.04, 0.10, &|p| p[side] > 0.08 * side_span);
    let ankles_r = band(0.04, 0.10, &|p| p[side] < -0.08 * side_span);
    let ankle_l = centroid(&ankles_l).ok_or_else(|| "could not find left ankle".to_string())?;
    let ankle_r = centroid(&ankles_r).ok_or_else(|| "could not find right ankle".to_string())?;

    let toes_l = band(0.00, 0.05, &|p| p[side] > 0.08 * side_span);
    let toes_r = band(0.00, 0.05, &|p| p[side] < -0.08 * side_span);
    let toe_l = centroid(&toes_l).unwrap_or(ankle_l);
    let toe_r = centroid(&toes_r).unwrap_or(ankle_r);
    let tip = |toes: &[Vec3], mid: Vec3| {
        if toes.is_empty() {
            return mid;
        }
        let mut ys: Vec<f32> = toes.iter().map(|p| p[fwd]).collect();
        ys.sort_by(f32::total_cmp);
        let cut = ys[ys.len() / 4];
        let front: Vec<Vec3> = toes.iter().copied().filter(|p| p[fwd] <= cut).collect();
        centroid(&front).unwrap_or(mid)
    };

    Ok(Landmarks {
        hips,
        head,
        hand_l,
        hand_r,
        shoulder_l,
        shoulder_r,
        hip_l,
        hip_r,
        ankle_l,
        ankle_r,
        toe_l,
        toe_r,
        toe_tip_l: tip(&toes_l, toe_l),
        toe_tip_r: tip(&toes_r, toe_r),
        up,
    })
}

/// How tall a mesh must be, against its longest axis, for glTF's own +Y to be
/// taken as up. Below this it is treated as an asset in some other convention
/// and the longest axis wins instead.
const MIN_UP_FRACTION: f32 = 0.5;

/// glTF Y, unless the mesh is plainly not Y-up.
///
/// This used to take the longest AABB axis, then hand a near-tie to whichever
/// axis had the smaller `mins[axis].abs()`, reading that as "closer to a ground
/// plane at 0". Generated meshes are **centered on the origin**, not grounded,
/// so `mins[axis].abs()` is just half the extent and the rule collapsed into
/// "prefer the narrower of two similar axes" - a coin flip, and backwards.
///
/// It matters because a T-pose has an arm span within a few percent of its
/// height, so X and Y are always a near-tie. Measured on four real characters,
/// the old rule got two right and two wrong, and the two it got right were
/// decided by which half-extent happened to be smaller. A wrong up-axis is not
/// a subtle failure: every band is a fraction of it, so the fitter goes looking
/// for a head 80% of the way along the arm span and reports "could not find
/// head".
///
/// glTF defines +Y as up and every provider emits glTF, so Y is the answer
/// whenever it is credible. `MIN_UP_FRACTION` is what "credible" means: a
/// genuinely Z-up import (Blender, unbaked) has a Y extent far below its
/// longest axis and still falls through to the longest.
fn pick_up_axis(extent: Vec3, _mins: Vec3) -> usize {
    let longest = if extent.x >= extent.y && extent.x >= extent.z {
        0
    } else if extent.z >= extent.y && extent.z >= extent.x {
        2
    } else {
        1
    };
    if extent.y >= extent[longest] * MIN_UP_FRACTION {
        return 1;
    }
    longest
}

fn outer_hand(band: &[Vec3], side: usize, sign: f32, fwd: usize, fwd_lim: f32) -> Option<Vec3> {
    let mut pts: Vec<Vec3> = band
        .iter()
        .copied()
        .filter(|p| p[side] * sign > 0.0 && p[fwd].abs() <= fwd_lim)
        .collect();
    if pts.is_empty() {
        pts = band
            .iter()
            .copied()
            .filter(|p| p[side] * sign > 0.0)
            .collect();
    }
    if pts.is_empty() {
        return None;
    }
    pts.sort_by(|a, b| (a[side] * sign).total_cmp(&(b[side] * sign)));
    let cut = (pts.len() * 90 / 100).min(pts.len() - 1);
    let outer = &pts[cut..];
    Some(outer.iter().copied().sum::<Vec3>() / outer.len() as f32)
}

trait WithAxis {
    fn with_axis(self, axis: usize, v: f32) -> Self;
}

impl WithAxis for Vec3 {
    fn with_axis(mut self, axis: usize, v: f32) -> Self {
        self[axis] = v;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capsule_humanoid() -> Vec<Vec3> {
        let mut vs = Vec::new();
        // torso
        for y in 0..20 {
            let t = y as f32 / 19.0;
            let h = 0.2 + t * 1.4;
            for i in 0..16 {
                let a = i as f32 * std::f32::consts::TAU / 16.0;
                vs.push(Vec3::new(0.12 * a.cos(), h, 0.08 * a.sin()));
            }
        }
        // head
        for y in 0..8 {
            let h = 1.55 + y as f32 * 0.03;
            for i in 0..12 {
                let a = i as f32 * std::f32::consts::TAU / 12.0;
                vs.push(Vec3::new(0.08 * a.cos(), h, 0.08 * a.sin()));
            }
        }
        // arms
        for xsign in [-1.0, 1.0] {
            for i in 0..12 {
                let x = xsign * (0.15 + i as f32 * 0.03);
                vs.push(Vec3::new(x, 1.35, 0.0));
            }
        }
        // legs / feet
        for xsign in [-1.0, 1.0] {
            for y in 0..12 {
                vs.push(Vec3::new(xsign * 0.12, y as f32 * 0.05, 0.0));
            }
            vs.push(Vec3::new(xsign * 0.12, 0.02, -0.08));
        }
        vs
    }

    #[test]
    fn landmarks_find_up_and_hips() {
        let lm = mesh_landmarks(&capsule_humanoid()).expect("landmarks");
        assert_eq!(lm.up, 1);
        assert!(lm.hips.y > 0.6 && lm.hips.y < 1.1, "hips {}", lm.hips);
        assert!(lm.head.y > lm.hips.y);
        assert!(lm.hand_l.x > 0.0 && lm.hand_r.x < 0.0);
        assert!(
            lm.hand_l.x > 0.35,
            "hands should sit near the T-pose extreme, not a 0.76 shrink ({})",
            lm.hand_l.x
        );
        assert!(lm.ankle_l.y < 0.25 && lm.ankle_r.y < 0.25);
    }

    /// Up-axis choice, against real generated meshes.
    ///
    /// These are measured AABBs from provider output, centered on the origin as
    /// that output actually is. A T-pose puts the arm span within a few percent
    /// of the height, so X and Y are always a near-tie and the old rule broke
    /// the tie on `mins[axis].abs()`, which on a centered mesh is just half the
    /// extent: it got the plague doctor and the first gravekeeper wrong, and
    /// the two it got right only because their Y half-extent happened to be the
    /// smaller number. Every band below is a fraction of this axis, so getting
    /// it wrong means hunting for a head partway along an arm.
    #[test]
    fn real_generated_meshes_all_resolve_to_y_up() {
        // (name, extent, mins) exactly as measured.
        let cases = [
            (
                "plague doctor",
                [0.970, 1.001, 0.270],
                [-0.485, -0.501, -0.137],
            ),
            ("scarecrow", [1.000, 0.952, 0.286], [-0.500, -0.477, -0.151]),
            (
                "gravekeeper",
                [0.984, 0.963, 0.213],
                [-0.492, -0.482, -0.103],
            ),
            (
                "gravekeeper (wide)",
                [1.001, 0.867, 0.231],
                [-0.500, -0.434, -0.106],
            ),
            (
                "jack-o-lantern",
                [1.000, 0.912, 0.999],
                [-0.500, -0.456, -0.500],
            ),
        ];
        for (name, e, m) in cases {
            assert_eq!(
                pick_up_axis(Vec3::from_array(e), Vec3::from_array(m)),
                1,
                "{name}: generated meshes are Y-up"
            );
        }
    }

    /// A genuinely Z-up asset still falls through to its longest axis.
    #[test]
    fn a_z_up_import_is_not_forced_onto_y() {
        // Blender-style export, unbaked: tall in Z, shallow in Y.
        let up = pick_up_axis(Vec3::new(0.6, 0.3, 1.8), Vec3::new(-0.3, -0.15, 0.0));
        assert_eq!(up, 2, "Y is not credible here, so the longest axis wins");
    }

    /// A flat slab must be refused, and depth cannot be what catches it.
    ///
    /// A computer monitor is *thinner* than a person: it cleared the depth gate
    /// at 0.34 and took a full 53-joint skeleton across its screen. What
    /// separates it from a body is that a body seen from the front is mostly
    /// holes (under the arms, between the legs) while a slab fills its own
    /// A roughly spherical solid must be refused, not fitted.
    ///
    /// This is the case the older refusals missed. They catch shapes that are
    /// degenerate in some band (a pillar has nothing at shoulder height, a
    /// slab has no head), but a blob has geometry wherever a band looks, so
    /// every landmark resolved and a real jack-o-lantern took a 53-joint
    /// The gate must not cost us characters that are merely bulky.
    ///
    /// The closest real case measured 0.57 deep for its height, so the limit
    #[test]
    fn standing_axis_wins_when_arms_are_slightly_wider() {
        let extent = Vec3::new(1.04, 1.00, 0.22);
        let mins = Vec3::new(-0.52, 0.0, -0.11);
        assert_eq!(
            pick_up_axis(extent, mins),
            1,
            "feet-on-Y=0 should beat a slightly wider arm span"
        );
    }
}
