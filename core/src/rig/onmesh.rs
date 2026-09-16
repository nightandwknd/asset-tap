//! Are the arranged joint heads on the mesh?
//!
//! Meshy places rig markers by clicking the surface, so an off-mesh joint
//! cannot happen there. Ours drag on a camera plane, so Bind has to check.
//! A bone with no mesh near it gets ~0 inverse-distance weight and drives
//! nothing; the author reads that as "the rig did nothing". Refuse the bind
//! and name the joint instead.
//!
//! "On the mesh" is: inside the shell (an axis ray in at least five of six
//! directions hits a triangle — tolerant of the holes generated meshes
//! have), or within a small skin tolerance of a vertex (chin, wrists, ankles
//! sit near the surface).

use super::export::BakedMesh;
use glam::Vec3;
use rayon::prelude::*;

pub(super) struct Shell {
    tris: Vec<[Vec3; 3]>,
    positions: Vec<Vec3>,
    min: Vec3,
    max: Vec3,
}

/// Skin tolerance as a fraction of the mesh's largest extent (3% ≈ 5 cm on
/// a 1.8 m character).
const SKIN_FRACTION: f32 = 0.03;

impl Shell {
    pub(super) fn new(positions: Vec<Vec3>, tris: Vec<[Vec3; 3]>) -> Self {
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for p in &positions {
            min = min.min(*p);
            max = max.max(*p);
        }
        Self {
            tris,
            positions,
            min,
            max,
        }
    }

    pub(super) fn from_baked(baked: &BakedMesh) -> Self {
        let mut positions = Vec::with_capacity(baked.vertex_count());
        let mut tris = Vec::new();
        for prim in baked.parts.iter().flat_map(|p| p.primitives.iter()) {
            positions.extend_from_slice(&prim.positions);
            let idx = &prim.indices;
            let at = |i: u32| prim.positions.get(i as usize).copied();
            match prim.mode {
                // TRIANGLES
                4 => {
                    for t in idx.chunks_exact(3) {
                        if let (Some(a), Some(b), Some(c)) = (at(t[0]), at(t[1]), at(t[2])) {
                            tris.push([a, b, c]);
                        }
                    }
                }
                // TRIANGLE_STRIP
                5 => {
                    for w in idx.windows(3) {
                        if let (Some(a), Some(b), Some(c)) = (at(w[0]), at(w[1]), at(w[2])) {
                            tris.push([a, b, c]);
                        }
                    }
                }
                // TRIANGLE_FAN
                6 => {
                    if let Some(&first) = idx.first() {
                        for w in idx[1..].windows(2) {
                            if let (Some(a), Some(b), Some(c)) = (at(first), at(w[0]), at(w[1])) {
                                tris.push([a, b, c]);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        Self::new(positions, tris)
    }

    pub(super) fn skin_tolerance(&self) -> f32 {
        let extent = (self.max - self.min).max_element();
        if extent.is_finite() && extent > 0.0 {
            extent * SKIN_FRACTION
        } else {
            0.05
        }
    }

    /// Distance to the nearest point on the mesh *surface*.
    ///
    /// Against triangles, not vertices. A vertex metric answers "how far to the
    /// nearest corner", which on coarse geometry is a different and larger
    /// number than "how far outside the mesh": a point a millimetre off the
    /// middle of a big triangle is nowhere near any of its corners. That number
    /// reaches the author, in a refusal that tells them how far a joint has to
    /// move, so being wrong about it sends them the wrong distance.
    pub(super) fn nearest_surface(&self, p: Vec3) -> f32 {
        let tris = self
            .tris
            .par_iter()
            .map(|t| point_triangle_distance_sq(p, t))
            .min_by(|a, b| a.total_cmp(b))
            .unwrap_or(f32::INFINITY);
        // A mesh whose primitives are unindexed points has no triangles; fall
        // back so the check still reports something finite.
        if tris.is_finite() {
            return tris.sqrt();
        }
        self.positions
            .par_iter()
            .map(|q| (*q - p).length_squared())
            .min_by(|a, b| a.total_cmp(b))
            .unwrap_or(f32::INFINITY)
            .sqrt()
    }

    /// Axis rays from `p` in ±X, ±Y, ±Z; how many hit a triangle.
    fn axis_hits(&self, p: Vec3) -> u8 {
        let mut hits = 0u8;
        for axis in 0..3usize {
            let (u, v) = match axis {
                0 => (1, 2),
                1 => (0, 2),
                _ => (0, 1),
            };
            let (pos, neg) = self
                .tris
                .par_iter()
                .map(|t| {
                    let (mut lo_u, mut hi_u) = (t[0][u], t[0][u]);
                    let (mut lo_v, mut hi_v) = (t[0][v], t[0][v]);
                    for q in &t[1..] {
                        lo_u = lo_u.min(q[u]);
                        hi_u = hi_u.max(q[u]);
                        lo_v = lo_v.min(q[v]);
                        hi_v = hi_v.max(q[v]);
                    }
                    if p[u] < lo_u || p[u] > hi_u || p[v] < lo_v || p[v] > hi_v {
                        return (false, false);
                    }
                    // Barycentric in the (u, v) projection.
                    let (x0, y0) = (t[0][u], t[0][v]);
                    let (x1, y1) = (t[1][u], t[1][v]);
                    let (x2, y2) = (t[2][u], t[2][v]);
                    let det = (y1 - y2) * (x0 - x2) + (x2 - x1) * (y0 - y2);
                    if det.abs() < 1e-12 {
                        return (false, false);
                    }
                    let l0 = ((y1 - y2) * (p[u] - x2) + (x2 - x1) * (p[v] - y2)) / det;
                    let l1 = ((y2 - y0) * (p[u] - x2) + (x0 - x2) * (p[v] - y2)) / det;
                    let l2 = 1.0 - l0 - l1;
                    let eps = -1e-6;
                    if l0 < eps || l1 < eps || l2 < eps {
                        return (false, false);
                    }
                    let hit = l0 * t[0][axis] + l1 * t[1][axis] + l2 * t[2][axis];
                    let d = hit - p[axis];
                    (d > 1e-6, d < -1e-6)
                })
                .reduce(|| (false, false), |a, b| (a.0 || b.0, a.1 || b.1));
            hits += u8::from(pos) + u8::from(neg);
        }
        hits
    }

    /// Inside the shell, or hugging the surface.
    pub(super) fn is_on_mesh(&self, p: Vec3) -> bool {
        let tol = self.skin_tolerance();
        if p.cmplt(self.min - tol).any() || p.cmpgt(self.max + tol).any() {
            return false;
        }
        if self.nearest_surface(p) <= tol {
            return true;
        }
        self.axis_hits(p) >= 5
    }
}

/// Squared distance from `p` to the closest point on triangle `t`.
///
/// Project onto the triangle's plane, and if the projection falls outside, take
/// the nearest point on whichever edge is closest. The squared form keeps the
/// per-triangle scan free of square roots.
fn point_triangle_distance_sq(p: Vec3, t: &[Vec3; 3]) -> f32 {
    let (a, b, c) = (t[0], t[1], t[2]);
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let n = ab.cross(ac);
    let n_len2 = n.length_squared();
    if n_len2 < 1e-20 {
        // Degenerate: treat it as its longest edge.
        return segment_distance_sq(p, a, b)
            .min(segment_distance_sq(p, b, c))
            .min(segment_distance_sq(p, a, c));
    }
    // Barycentric coordinates of the plane projection.
    let d = ap.dot(n) / n_len2;
    let proj = p - n * d;
    let pa = proj - a;
    let (d00, d01, d11) = (ab.dot(ab), ab.dot(ac), ac.dot(ac));
    let (d20, d21) = (pa.dot(ab), pa.dot(ac));
    let denom = d00 * d11 - d01 * d01;
    if denom.abs() > 1e-20 {
        let v = (d11 * d20 - d01 * d21) / denom;
        let w = (d00 * d21 - d01 * d20) / denom;
        if v >= 0.0 && w >= 0.0 && v + w <= 1.0 {
            return (p - proj).length_squared();
        }
    }
    segment_distance_sq(p, a, b)
        .min(segment_distance_sq(p, b, c))
        .min(segment_distance_sq(p, a, c))
}

/// Squared distance from `p` to the segment `a`..`b`.
fn segment_distance_sq(p: Vec3, a: Vec3, b: Vec3) -> f32 {
    let ab = b - a;
    let len2 = ab.length_squared();
    if len2 < 1e-20 {
        return (p - a).length_squared();
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    (p - (a + ab * t)).length_squared()
}

/// `(name, metres to the nearest point on the surface)` for every head that is
/// not on the mesh. Empty means Bind may proceed.
pub(super) fn off_mesh(shell: &Shell, heads: &[(String, [f32; 3])]) -> Vec<(String, f32)> {
    let mut out: Vec<(String, f32)> = heads
        .iter()
        .filter_map(|(name, w)| {
            let p = Vec3::from_array(*w);
            if shell.is_on_mesh(p) {
                None
            } else {
                Some((name.clone(), shell.nearest_surface(p)))
            }
        })
        .collect();
    out.sort_by(|a, b| b.1.total_cmp(&a.1));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reported distance is to the surface, not to the nearest corner.
    ///
    /// One big triangle, and a point a few millimetres off the middle of it.
    /// The corners are far away; the surface is not. Measuring to corners tells
    /// the author a joint is centimetres out when it is millimetres, and that
    /// number is the only guidance the refusal gives them.
    #[test]
    fn distance_is_to_the_surface_not_the_nearest_corner() {
        let a = Vec3::new(-1.0, 0.0, -1.0);
        let b = Vec3::new(1.0, 0.0, -1.0);
        let c = Vec3::new(0.0, 0.0, 1.0);
        let shell = Shell::new(vec![a, b, c], vec![[a, b, c]]);

        // Just above the middle of the face.
        let p = Vec3::new(0.0, 0.004, -0.2);
        let surface = shell.nearest_surface(p);
        assert!(
            (surface - 0.004).abs() < 1e-5,
            "should be 4 mm from the face, got {surface}"
        );

        let corner = [a, b, c]
            .iter()
            .map(|q| (*q - p).length())
            .fold(f32::INFINITY, f32::min);
        assert!(
            corner > 0.5,
            "the fixture only means something if the corners are far: {corner}"
        );
    }

    /// A point genuinely outside still reads as outside.
    #[test]
    fn a_point_well_off_the_surface_still_measures_far() {
        let a = Vec3::new(-1.0, 0.0, -1.0);
        let b = Vec3::new(1.0, 0.0, -1.0);
        let c = Vec3::new(0.0, 0.0, 1.0);
        let shell = Shell::new(vec![a, b, c], vec![[a, b, c]]);
        let d = shell.nearest_surface(Vec3::new(0.0, 2.0, 0.0));
        assert!((d - 2.0).abs() < 1e-5, "expected 2 m, got {d}");
    }

    /// Unit cube centered on the origin, 12 triangles, outward-agnostic.
    fn cube() -> Shell {
        let c = |x: f32, y: f32, z: f32| Vec3::new(x, y, z) * 0.5;
        let v = [
            c(-1.0, -1.0, -1.0),
            c(1.0, -1.0, -1.0),
            c(1.0, 1.0, -1.0),
            c(-1.0, 1.0, -1.0),
            c(-1.0, -1.0, 1.0),
            c(1.0, -1.0, 1.0),
            c(1.0, 1.0, 1.0),
            c(-1.0, 1.0, 1.0),
        ];
        let quads = [
            [0, 1, 2, 3],
            [4, 5, 6, 7],
            [0, 1, 5, 4],
            [2, 3, 7, 6],
            [0, 3, 7, 4],
            [1, 2, 6, 5],
        ];
        let mut tris = Vec::new();
        for q in quads {
            tris.push([v[q[0]], v[q[1]], v[q[2]]]);
            tris.push([v[q[0]], v[q[2]], v[q[3]]]);
        }
        Shell::new(v.to_vec(), tris)
    }

    #[test]
    fn center_is_inside() {
        let s = cube();
        assert_eq!(s.axis_hits(Vec3::ZERO), 6);
        assert!(s.is_on_mesh(Vec3::ZERO));
        assert!(s.is_on_mesh(Vec3::new(0.3, -0.2, 0.1)));
    }

    #[test]
    fn far_point_is_off() {
        let s = cube();
        assert!(!s.is_on_mesh(Vec3::new(0.0, 0.0, 1.3)));
        assert!(!s.is_on_mesh(Vec3::new(5.0, 0.0, 0.0)));
    }

    #[test]
    fn skin_tolerance_accepts_surface_hugging_head() {
        let s = cube();
        // 1 cm outside a corner, well inside 3% of the 1 m extent.
        assert!(s.is_on_mesh(Vec3::new(0.505, 0.505, 0.505)));
        // 10 cm outside a face: not inside, not within tolerance.
        assert!(!s.is_on_mesh(Vec3::new(0.0, 0.0, 0.6)));
    }

    #[test]
    fn one_missing_wall_still_counts_as_inside() {
        let mut s = cube();
        // Drop the +Z face (two triangles) — a hole like an open sleeve.
        s.tris
            .retain(|t| !t.iter().all(|p| (p.z - 0.5).abs() < 1e-6));
        assert_eq!(s.axis_hits(Vec3::ZERO), 5);
        assert!(s.is_on_mesh(Vec3::ZERO));
    }

    #[test]
    fn off_mesh_reports_farthest_first() {
        let s = cube();
        let heads = vec![
            ("hips".to_string(), [0.0, 0.0, 0.0]),
            ("leftHand".to_string(), [2.0, 0.0, 0.0]),
            ("rightLowerLeg".to_string(), [0.0, 0.0, 1.0]),
        ];
        let off = off_mesh(&s, &heads);
        assert_eq!(off.len(), 2);
        assert_eq!(off[0].0, "leftHand");
        assert_eq!(off[1].0, "rightLowerLeg");
        assert!(off[1].1 > 0.4 && off[1].1 < 0.9, "{}", off[1].1);
    }
}
