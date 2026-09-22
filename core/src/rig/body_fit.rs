//! Body placement in mesh world space, independent of the reference T-pose.
//!
//! Arms are sampled outside the torso in lateral slices. Each slice contributes
//! once, so dense hands or clothing do not dominate the direction estimate.
use super::{canon::HumanBone, landmarks::Landmarks, skeleton::Armature};
use glam::Vec3;

struct Body<'a> {
    points: &'a [Vec3],
    up: usize,
    side: usize,
    depth: usize,
    lo: Vec3,
    height: f32,
    center: f32,
}

impl<'a> Body<'a> {
    fn new(points: &'a [Vec3], up: usize) -> Self {
        let mut lo = Vec3::splat(f32::INFINITY);
        let mut hi = Vec3::splat(f32::NEG_INFINITY);
        for &p in points {
            lo = lo.min(p);
            hi = hi.max(p);
        }
        let side = if up == 0 { 1 } else { 0 };
        Self {
            points,
            up,
            side,
            depth: 3 - up - side,
            lo,
            height: (hi[up] - lo[up]).max(1e-6),
            center: (lo[side] + hi[side]) * 0.5,
        }
    }

    fn arm(&self, sign: f32) -> Option<(Vec3, Vec3)> {
        let h = self.height;
        let lateral = |p: Vec3| sign * (p[self.side] - self.center);
        let outer: Vec<Vec3> = self
            .points
            .iter()
            .copied()
            .filter(|p| {
                lateral(*p) > 0.15 * h
                    && p[self.up] > self.lo[self.up] + 0.35 * h
                    && p[self.up] < self.lo[self.up] + 0.86 * h
            })
            .collect();
        let reach = quantile(outer.iter().map(|p| lateral(*p)).collect(), 0.98)?;
        let mut samples = Vec::new();
        let mut x = 0.14 * h;
        while x < reach {
            let slice: Vec<Vec3> = outer
                .iter()
                .copied()
                .filter(|p| (lateral(*p) - x).abs() < 0.009 * h)
                .collect();
            if slice.len() >= 8 {
                let top = quantile(slice.iter().map(|p| p[self.up]).collect(), 0.95)?;
                // Hair, a coat or a cape can continue below the arm. Follow
                // the upper limb, not the centroid of that entire curtain.
                let limb: Vec<Vec3> = slice
                    .into_iter()
                    .filter(|p| p[self.up] > top - 0.06 * h)
                    .collect();
                if let Some(mid) = median_point(&limb) {
                    samples.push(mid);
                }
            }
            x += 0.012 * h;
        }
        if samples.len() < 4 {
            return None;
        }
        let center = samples.iter().copied().sum::<Vec3>() / samples.len() as f32;
        let variance: f32 = samples
            .iter()
            .map(|p| (p[self.side] - center[self.side]).powi(2))
            .sum();
        if variance < h * h * 1e-5 {
            return None;
        }
        let mut direction = Vec3::ZERO;
        direction[self.side] = 1.0;
        for axis in [self.up, self.depth] {
            direction[axis] = samples
                .iter()
                .map(|p| (p[self.side] - center[self.side]) * (p[axis] - center[axis]))
                .sum::<f32>()
                / variance;
        }
        direction = direction.normalize() * sign;
        let mut perpendicular = Vec3::ZERO;
        perpendicular[self.side] = -direction[self.up];
        perpendicular[self.up] = direction[self.side];
        let along: Vec<f32> = outer
            .iter()
            .filter(|p| ((**p - center).dot(perpendicular)).abs() < 0.06 * h)
            .map(|p| (*p - center).dot(direction))
            .collect();
        // Hand joint is the wrist, not the outermost fingertip.
        let hand = center + direction * (quantile(along, 0.98)? - 0.055 * h);
        let torso: Vec<f32> = self
            .points
            .iter()
            .filter(|p| {
                (p[self.up] - (self.lo[self.up] + 0.70 * h)).abs() < 0.012 * h
                    && (p[self.side] - self.center).abs() < 0.14 * h
            })
            .map(|p| lateral(*p))
            .collect();
        let half_width = quantile(torso, 0.85)?.clamp(0.075 * h, 0.13 * h);
        let shoulder = center
            + direction
                * ((self.center + sign * half_width - center[self.side]) / direction[self.side]);
        let shoulder_height = (shoulder[self.up] - self.lo[self.up]) / h;
        let length = shoulder.distance(hand) / h;
        // Outside a standing A/T pose, retain the existing landmark estimate.
        // In particular, do not extrapolate a near-vertical coat edge as an arm.
        if !(0.65..0.88).contains(&shoulder_height)
            || !(0.15..0.45).contains(&length)
            || direction[self.up] > 0.3
            || direction[self.side].abs() < 0.25
        {
            return None;
        }
        Some((shoulder, hand))
    }

    fn center_at(&self, height: f32, side: f32, fallback: Vec3) -> Vec3 {
        let depths: Vec<f32> = self
            .points
            .iter()
            .filter(|p| {
                (p[self.up] - height).abs() < TORSO_SLICE * self.height
                    && (p[self.side] - side).abs() < TORSO_SLAB * self.height
            })
            .map(|p| p[self.depth])
            .collect();
        let mut point = fallback;
        point[self.up] = height;
        point[self.side] = side;
        if depths.len() >= MIN_SLICE_SAMPLES {
            // Midway between the two skins, not a vertex centroid: buttons,
            // lapels and dense front-face tessellation must not pull the spine
            // out of the torso.
            //
            // Take the extrema rather than inner quantiles. A torso slice is
            // bimodal — a back shell and a front shell with the body's hollow
            // between them — and the two are rarely sampled evenly. On a real
            // A-posed character the chest slice carries ~90% of its vertices on
            // the front, so a 0.1 quantile lands *inside the front cluster* and
            // the midpoint of the two quantiles sits on the front skin. The
            // outermost samples are the two surfaces by construction, whatever
            // the density between them. Measured on that character: the quantile
            // midpoint put `chest` at 0.87 of the depth extent, the extrema
            // midpoint at 0.47, and the two agree within millimetres at every
            // height where the old estimator was already correct.
            let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
            for d in depths {
                lo = lo.min(d);
                hi = hi.max(d);
            }
            point[self.depth] = (lo + hi) * 0.5;
        }
        point
    }
}

/// Half-width of the lateral slice `center_at` reads the torso's depth from,
/// as a fraction of body height.
///
/// Narrow on purpose. A slab only has to be wide enough to hold a few vertices
/// of each skin; widening it reaches geometry that is not the torso. At 0.04 —
/// ±7.6 cm on a 1.9-unit character — it caught the upper arms of a close A-pose.
/// At 0.022 the arms of every character measured stay outside it, which is why
/// no explicit arm exclusion is needed: an earlier capsule test rejected nothing
/// on ten of eleven real meshes, and on a folded arm its radius (0.085 of height)
/// exceeded the torso's own half-depth, swallowing the front skin and silently
/// falling back. Geometry does the job the capsule was written for.
const TORSO_SLAB: f32 = 0.022;

/// Half-thickness of that slice along the up axis, as a fraction of height.
/// Thin enough that a joint's depth is local to its own height, thick enough
/// to catch vertices between tessellation rows.
const TORSO_SLICE: f32 = 0.012;

/// Below this many vertices a slice says nothing about where the skins are, so
/// `center_at` retries without the arm exclusion and otherwise keeps the
/// caller's fallback.
const MIN_SLICE_SAMPLES: usize = 4;

fn quantile(mut values: Vec<f32>, fraction: f32) -> Option<f32> {
    if values.is_empty() {
        return None;
    }
    values.sort_unstable_by(f32::total_cmp);
    let index = fraction * (values.len() - 1) as f32;
    let lo = index.floor() as usize;
    let hi = index.ceil() as usize;
    Some(values[lo] + (values[hi] - values[lo]) * index.fract())
}

fn median_point(points: &[Vec3]) -> Option<Vec3> {
    Some(Vec3::new(
        quantile(points.iter().map(|p| p.x).collect(), 0.5)?,
        quantile(points.iter().map(|p| p.y).collect(), 0.5)?,
        quantile(points.iter().map(|p| p.z).collect(), 0.5)?,
    ))
}

pub(super) fn refine_arms(points: &[Vec3], landmarks: &mut Landmarks) {
    let body = Body::new(points, landmarks.up);
    for (sign, shoulder, hand) in [
        (1.0, &mut landmarks.shoulder_l, &mut landmarks.hand_l),
        (-1.0, &mut landmarks.shoulder_r, &mut landmarks.hand_r),
    ] {
        if let Some((s, h)) = body.arm(sign) {
            *shoulder = s;
            *hand = h;
        } else {
            tracing::debug!(
                side = sign,
                "arm profile unavailable; retaining band landmarks"
            );
        }
    }
}

pub(super) fn fit_torso(arm: &mut Armature, points: &[Vec3], landmarks: &Landmarks) {
    use HumanBone::{Chest, Head, LeftShoulder, Neck, RightShoulder, Spine, UpperChest};

    let body = Body::new(points, landmarks.up);
    let shoulder_height = (landmarks.shoulder_l[body.up] + landmarks.shoulder_r[body.up]) * 0.5;
    let neck_height = shoulder_height + 0.035 * body.height;
    let hips_height = landmarks.hips[body.up];
    for (bone, fraction) in [
        (Spine, 0.22),
        (Chest, 0.46),
        (UpperChest, 0.72),
        (Neck, 1.0),
    ] {
        let height = hips_height + (neck_height - hips_height) * fraction;
        arm.set_world_head(
            bone.as_str(),
            body.center_at(height, body.center, landmarks.hips),
        );
    }
    arm.set_world_head(
        Head.as_str(),
        body.center_at(
            neck_height + 0.05 * body.height,
            body.center,
            landmarks.head,
        ),
    );
    for (bone, sign) in [(LeftShoulder, 1.0), (RightShoulder, -1.0)] {
        arm.set_world_head(
            bone.as_str(),
            body.center_at(
                shoulder_height + 0.01 * body.height,
                body.center + sign * 0.015 * body.height,
                landmarks.hips,
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rig::{canon, landmarks::mesh_landmarks};

    /// Known anatomical centers on a surface cloud, with straight arms at a
    /// chosen declination. Includes a torso, legs and head so this exercises
    /// the complete landmark/fit path rather than only the line estimator.
    fn character(angle: f32) -> Vec<Vec3> {
        let mut points = Vec::new();
        let mut tube = |a: Vec3, b: Vec3, radius: f32| {
            let axis = (b - a).normalize();
            let across = axis.cross(Vec3::Z).normalize();
            for i in 0..=100 {
                let center = a.lerp(b, i as f32 / 100.0);
                for j in 0..32 {
                    let t = j as f32 * std::f32::consts::TAU / 32.0;
                    points.push(center + radius * (across * t.cos() + Vec3::Z * t.sin()));
                }
            }
        };
        tube(Vec3::new(0.0, 0.49, 0.0), Vec3::new(0.0, 0.80, 0.0), 0.10);
        tube(Vec3::new(0.0, 0.83, 0.0), Vec3::new(0.0, 1.0, 0.0), 0.06);
        for sign in [1.0, -1.0] {
            tube(
                Vec3::new(sign * 0.09, 0.0, 0.0),
                Vec3::new(sign * 0.09, 0.53, 0.0),
                0.04,
            );
            let shoulder = Vec3::new(sign * 0.10, 0.79, 0.0);
            let dir = Vec3::new(
                sign * angle.to_radians().cos(),
                -angle.to_radians().sin(),
                0.0,
            );
            tube(shoulder, shoulder + dir * 0.35, 0.025);
        }
        points
    }

    #[test]
    fn a_and_t_pose_wrists_follow_the_limbs_not_a_height_band() {
        for angle in [0.0, 20.0, 45.0, 60.0, 70.0] {
            let points = character(angle);
            let body = Body::new(&points, 1);
            for sign in [1.0, -1.0] {
                let (shoulder, wrist) = body
                    .arm(sign)
                    .unwrap_or_else(|| panic!("arm not found at {angle} degrees"));
                let expected_shoulder = Vec3::new(sign * 0.10, 0.79, 0.0);
                let dir = Vec3::new(
                    sign * angle.to_radians().cos(),
                    -angle.to_radians().sin(),
                    0.0,
                );
                let expected_wrist = expected_shoulder + dir * (0.35 - 0.055);
                assert!(
                    shoulder.distance(expected_shoulder) < 0.04,
                    "{angle}: shoulder {shoulder}"
                );
                assert!(
                    wrist.distance(expected_wrist) < 0.035,
                    "{angle}: wrist {wrist}, expected {expected_wrist}"
                );
            }
        }
    }

    #[test]
    fn torso_and_clavicles_are_fitted_instead_of_inheriting_reference_depth() {
        for angle in [0.0, 45.0, 70.0] {
            let points = character(angle);
            let mut landmarks = mesh_landmarks(&points).unwrap();
            refine_arms(&points, &mut landmarks);
            let mut arm = canon::armature();
            super::super::skeleton::fit(&mut arm, &landmarks, &points);
            let heads = arm.world_heads();
            let at = |name: &str| heads[arm.joint_index(name).unwrap()];
            for name in [
                "spine",
                "chest",
                "upperChest",
                "neck",
                "head",
                "leftShoulder",
                "rightShoulder",
            ] {
                assert!(
                    at(name).z.abs() < 0.015,
                    "{angle}: {name} floated forward: {}",
                    at(name)
                );
            }
            for side in ["left", "right"] {
                let clavicle = at(&format!("{side}Shoulder"));
                assert!(
                    (clavicle.y - 0.80).abs() < 0.04,
                    "{angle}: clavicle height {}",
                    clavicle.y
                );
                assert!(clavicle.y < at("neck").y);
                assert!(at("neck").y < at("head").y);
            }
        }
    }

    /// A torso whose front face carries far more vertices than its back, the
    /// way a real generated character does: detailing, a jacket front and a
    /// closing seam all tessellate the chest while the back stays coarse.
    ///
    /// The uniform tubes in [`character`] cannot catch a depth estimator that
    /// assumes both skins are sampled evenly — they always are. This one has
    /// the front at ten times the density, which is what put `chest` on the
    /// front skin of `2026-09-20_191246` while `spine` and `neck` stayed right.
    fn front_heavy_torso() -> Vec<Vec3> {
        let mut points = character(35.0);
        // Extra rows on the +z skin only, narrow enough in x to land inside the
        // slab `center_at` samples. The back skin keeps the base tessellation,
        // so the slice becomes two clusters with the front ~10x the denser.
        for i in 0..=400 {
            let y = 0.49 + (0.80 - 0.49) * i as f32 / 400.0;
            for j in 0..10 {
                let x = -0.018 + 0.036 * j as f32 / 9.0;
                let z = (0.10f32 * 0.10 - x * x).max(0.0).sqrt();
                points.push(Vec3::new(x, y, z));
            }
        }
        points
    }

    #[test]
    fn torso_depth_survives_a_front_heavy_mesh() {
        let points = front_heavy_torso();
        let mut landmarks = mesh_landmarks(&points).unwrap();
        refine_arms(&points, &mut landmarks);
        let mut arm = canon::armature();
        super::super::skeleton::fit(&mut arm, &landmarks, &points);
        let heads = arm.world_heads();
        let at = |name: &str| heads[arm.joint_index(name).unwrap()];
        // The torso's own axis is z = 0; a fit that follows vertex density
        // instead of the two skins lands out at the +0.10 front face.
        for name in ["spine", "chest", "upperChest", "neck"] {
            assert!(
                at(name).z.abs() < 0.02,
                "{name} followed the dense front face: {}",
                at(name)
            );
        }
        let (l, r) = (at("leftShoulder").z, at("rightShoulder").z);
        assert!(
            (l - r).abs() < 0.01,
            "clavicles disagree on depth: {l} vs {r}"
        );
    }

    #[test]
    fn arm_profiles_are_scale_translation_and_up_axis_equivariant() {
        let points = character(50.0);
        let baseline = Body::new(&points, 1).arm(1.0).unwrap();
        for scale in [0.01, 1.0, 100.0] {
            let offset = Vec3::new(2.0, -3.0, 5.0) * scale;
            let transform = |v: Vec3| Vec3::new(v.x, v.z, v.y) * scale + offset;
            let moved: Vec<_> = points.iter().copied().map(transform).collect();
            let (shoulder, wrist) = Body::new(&moved, 2).arm(1.0).unwrap();
            assert!(shoulder.distance(transform(baseline.0)) < 0.005 * scale);
            assert!(wrist.distance(transform(baseline.1)) < 0.005 * scale);
        }
    }
}
