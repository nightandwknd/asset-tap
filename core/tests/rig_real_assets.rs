//! Asset-preservation checks against real provider output.
//!
//! Opt-in: set `ASSET_TAP_RIG_FIXTURES` to a directory of `.glb` files. These
//! assets are tens of megabytes each and cannot be checked in, so the suite
//! skips when the variable is unset rather than failing.
//!
//! ```bash
//! ASSET_TAP_RIG_FIXTURES=/path/to/meshes \
//!   ASSET_TAP_CLIPS_DIR=$PWD/.dev/clips \
//!   cargo test -p asset-tap-core --test rig_real_assets -- --nocapture
//! ```
//!
//! This is VIEWER_ANIMATE.md's Phase 2 exit criterion made executable: fit and
//! bake must preserve geometry and image bytes, a bake must land exactly the
//! set it was given, and re-baking must not grow the file.

use asset_tap_core::{
    BindOptions, SkinnedClip, apply_clip, bind_mesh, extract_model_info, fit_mesh,
    fit_mesh_from_heads, heads_off_mesh, seed_bind_markers,
};
use std::path::{Path, PathBuf};

/// Slack for JSON text length changing without the asset changing.
///
/// A leaked clip is tens of kilobytes, so this is nowhere near wide enough to
/// hide accumulation; it only absorbs float-formatting jitter.
const JSON_TEXT_JITTER: u64 = 256;

/// Clips used for the multi-clip checks. Present in the Quaternius libraries.
const CLIP_A: &str = "Walk_Loop";
const CLIP_B: &str = "Sword_Attack";

fn fixtures() -> Vec<PathBuf> {
    let Ok(dir) = std::env::var("ASSET_TAP_RIG_FIXTURES") else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("glb"))
        .collect();
    out.sort();
    out
}

/// The glTF JSON chunk of a GLB, without touching buffers.
fn glb_json(path: &Path) -> serde_json::Value {
    let bytes = std::fs::read(path).expect("read glb");
    let mut off = 12;
    while off + 8 <= bytes.len() {
        let len = u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap()) as usize;
        let kind = u32::from_le_bytes(bytes[off + 4..off + 8].try_into().unwrap());
        if kind == 0x4E4F_534A {
            return serde_json::from_slice(&bytes[off + 8..off + 8 + len]).expect("glb json");
        }
        off += 8 + len;
    }
    panic!("no JSON chunk in {}", path.display());
}

/// Raw bytes of every embedded image, in document order.
///
/// The writer is supposed to copy these through untouched; re-encoding a
/// texture is silent quality loss on an asset the user paid to generate.
fn image_bytes(path: &Path) -> Vec<Vec<u8>> {
    let bytes = std::fs::read(path).expect("read glb");
    let doc = glb_json(path);
    // Locate the BIN chunk so bufferView offsets can be resolved.
    let mut off = 12;
    let mut bin = &bytes[0..0];
    while off + 8 <= bytes.len() {
        let len = u32::from_le_bytes(bytes[off..off + 4].try_into().unwrap()) as usize;
        let kind = u32::from_le_bytes(bytes[off + 4..off + 8].try_into().unwrap());
        if kind == 0x004E_4942 {
            bin = &bytes[off + 8..off + 8 + len];
        }
        off += 8 + len;
    }
    let views = doc["bufferViews"].as_array().cloned().unwrap_or_default();
    doc["images"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|img| img["bufferView"].as_u64())
        .map(|v| {
            let view = &views[v as usize];
            let start = view["byteOffset"].as_u64().unwrap_or(0) as usize;
            let len = view["byteLength"].as_u64().expect("byteLength") as usize;
            bin[start..start + len].to_vec()
        })
        .collect()
}

fn animation_names(path: &Path) -> Vec<String> {
    glb_json(path)["animations"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .filter_map(|a| a["name"].as_str().map(str::to_string))
        .collect()
}

fn primitive_count(path: &Path) -> usize {
    glb_json(path)["meshes"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|m| m["primitives"].as_array().map_or(0, |p| p.len()))
        .sum()
}

fn stage(src: &Path, dir: &Path) -> PathBuf {
    let dest = dir.join("model.glb");
    std::fs::copy(src, &dest).expect("stage fixture");
    dest
}

#[test]
fn fit_and_bake_preserve_real_assets() {
    let fixtures = fixtures();
    if fixtures.is_empty() {
        eprintln!("skipping: set ASSET_TAP_RIG_FIXTURES to a directory of .glb files");
        return;
    }

    for src in fixtures {
        let name = src.file_name().unwrap().to_string_lossy().into_owned();
        let dir = tempfile::tempdir().expect("tempdir");
        let model = stage(&src, dir.path());

        let before = extract_model_info(&model).expect("model info");
        let images_before = image_bytes(&model);
        let prims_before = primitive_count(&model);

        // 1. Fit preserves geometry and appearance.
        fit_mesh(&model, &model, &BindOptions::default())
            .unwrap_or_else(|e| panic!("{name}: fit: {e}"));
        let fitted = extract_model_info(&model).expect("model info");
        assert_eq!(fitted.vertex_count, before.vertex_count, "{name}: vertices");
        assert_eq!(
            fitted.triangle_count, before.triangle_count,
            "{name}: triangles"
        );
        assert_eq!(primitive_count(&model), prims_before, "{name}: primitives");
        assert_eq!(
            image_bytes(&model),
            images_before,
            "{name}: image bytes after fit"
        );
        assert!(
            animation_names(&model).is_empty(),
            "{name}: fit writes no animation"
        );

        // 2. A multi-clip bake lands exactly the set, and still preserves.
        //
        // `apply_clip`, not `bind_mesh`: baking writes animation onto an
        // existing rest, whereas `bind_mesh` re-fits first. That distinction
        // matters below — a re-fit world-bakes the mesh again and recomputes
        // normal bounds, so only the bake-only path is a bit-exact fixed point.
        let two = BindOptions {
            clips: vec![CLIP_A.into(), CLIP_B.into()],
            ..Default::default()
        };
        apply_clip(&model, &model, &two).unwrap_or_else(|e| panic!("{name}: bake 2: {e}"));
        assert_eq!(
            animation_names(&model),
            [CLIP_A, CLIP_B],
            "{name}: baked set"
        );
        let baked = extract_model_info(&model).expect("model info");
        assert_eq!(
            baked.vertex_count, before.vertex_count,
            "{name}: vertices after bake"
        );
        assert_eq!(
            image_bytes(&model),
            images_before,
            "{name}: image bytes after bake"
        );
        let size_two = baked.file_size;

        // 3. Re-baking a smaller set removes, and reclaims the space.
        let one = BindOptions {
            clips: vec![CLIP_A.into()],
            ..Default::default()
        };
        apply_clip(&model, &model, &one).unwrap_or_else(|e| panic!("{name}: bake 1: {e}"));
        assert_eq!(
            animation_names(&model),
            [CLIP_A],
            "{name}: declarative removal"
        );
        let size_one = extract_model_info(&model).expect("model info").file_size;
        assert!(
            size_one < size_two,
            "{name}: dropping a clip must shrink ({size_two} -> {size_one})"
        );

        // 4. Re-baking the same set is a fixed point at the precision glTF
        //    actually stores.
        //
        //    Deliberately not a byte comparison. `serde_json` serializing an
        //    `f32` is not a round-trip fixed point: `json!(f32)` emits 17
        //    significant digits, and re-parsing as `f64` then re-emitting
        //    gives 16. Those two decimal strings are different `f64` values
        //    but decode to the *same* `f32`, which is the precision glTF node
        //    transforms use. So the asset is unchanged and only the JSON text
        //    moves. Asserting byte equality would fail on a difference that
        //    cannot affect a loaded model.
        for round in 0..4 {
            apply_clip(&model, &model, &one)
                .unwrap_or_else(|e| panic!("{name}: rebake {round}: {e}"));
            let size = extract_model_info(&model).expect("model info").file_size;
            let drift = size.abs_diff(size_one);
            assert!(
                drift <= JSON_TEXT_JITTER,
                "{name}: bake {round} drifted {drift} bytes ({size_one} -> {size}); \
                 a leaked clip would be tens of kilobytes"
            );
        }
        let after = extract_model_info(&model).expect("model info");
        assert_eq!(
            animation_names(&model),
            [CLIP_A],
            "{name}: set after rebake"
        );
        assert_eq!(
            after.vertex_count, before.vertex_count,
            "{name}: vertices after rebake"
        );
        assert_eq!(
            after.triangle_count, before.triangle_count,
            "{name}: triangles after rebake"
        );
        assert_eq!(
            primitive_count(&model),
            prims_before,
            "{name}: primitives after rebake"
        );
        assert_eq!(
            image_bytes(&model),
            images_before,
            "{name}: image bytes after rebake"
        );

        eprintln!(
            "ok {name}: {} verts, {} prims, images intact",
            before.vertex_count, prims_before
        );
    }
}

/// A dragged marker always lands *inside* the geometry under the pointer.
///
/// This is what `SkinnedClip::ray_midline` promises, and it is what the Rig
/// drag relies on: the marker follows the surface the user is pointing at
/// instead of a camera-facing plane, so it cannot be dragged into open space.
///
/// Deliberately *not* asserted against `heads_off_mesh`. They are different
/// predicates and both are correct. `ray_midline` answers "is this inside the
/// geometry under the cursor", while `heads_off_mesh` answers "is this inside
/// the *body*" using a 5-of-6 axis-ray containment heuristic plus a
/// nearest-*vertex* tolerance. On a clothed character a point can be squarely
/// inside a cape or armour shell and still fail the body test, and on coarse
/// geometry the nearest-vertex metric over-reports because the surface
/// crossing lies on a large triangle's face rather than near its corners.
/// Bind's check stays the authority on what may be committed.
#[test]
fn a_dragged_marker_lands_inside_the_geometry_under_the_pointer() {
    let fixtures = fixtures();
    if fixtures.is_empty() {
        eprintln!("skipping: set ASSET_TAP_RIG_FIXTURES to a directory of .glb files");
        return;
    }

    for src in fixtures {
        let name = src.file_name().unwrap().to_string_lossy().into_owned();
        let clip = SkinnedClip::from_glb(&src).unwrap_or_else(|e| panic!("{name}: load: {e}"));
        let markers = seed_bind_markers(&src).unwrap_or_else(|e| panic!("{name}: seed: {e}"));
        assert!(!markers.is_empty(), "{name}: auto-fit produced no markers");

        let dirs = [
            [0.0f32, 0.0, 1.0],
            [0.0, 0.0, -1.0],
            [1.0, 0.0, 0.0],
            [-1.0, 0.0, 0.0],
            [0.6, -0.3, 0.8],
        ];
        let mut checked = 0usize;
        for marker in &markers {
            for dir in dirs {
                let n = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
                let unit = [dir[0] / n, dir[1] / n, dir[2] / n];
                let origin = [
                    marker.world[0] - unit[0] * 10.0,
                    marker.world[1] - unit[1] * 10.0,
                    marker.world[2] - unit[2] * 10.0,
                ];
                let Some(hit) = clip.ray_midline(origin, unit) else {
                    continue;
                };
                let hits = clip.ray_hits_debug(origin, unit);
                assert!(hits.len() >= 2, "{name}: a placement needs a solid span");

                // Distance of the placement along the ray.
                let t = (0..3).map(|k| (hit[k] - origin[k]) * unit[k]).sum::<f32>();
                assert!(
                    t > hits[0] - 1e-3 && t < hits[1] + 1e-3,
                    "{name}/{}: placed at {t:.4}, outside the first solid span {:.4}..{:.4}",
                    marker.name,
                    hits[0],
                    hits[1]
                );
                checked += 1;
            }
        }
        assert!(
            checked > 0,
            "{name}: no ray reached the mesh, proving nothing"
        );
        eprintln!("ok {name}: {checked} drag placements, all inside the geometry");
    }
}

/// Adding a clip must not silently undo joints arranged by hand.
///
/// `bind` used to re-fit unconditionally, so posing a rig in the GUI and then
/// running `asset-tap bind --clip walk` threw the pose away and re-weighted to
/// the auto-fit. Nothing said so; the joints simply moved back.
#[test]
fn adding_a_clip_keeps_a_hand_arranged_pose() {
    // Needs a mesh whose auto-fit seeds every joint on the body: Bind refuses
    // an off-mesh head, and some assets seed their shoulders outside (see the
    // Phase 3 note in VIEWER_ANIMATE.md).
    let Some(src) = fixtures().into_iter().find(|f| {
        seed_bind_markers(f).is_ok_and(|markers| {
            let heads: Vec<(String, [f32; 3])> =
                markers.into_iter().map(|m| (m.name, m.world)).collect();
            heads_off_mesh(f, &heads).is_ok_and(|off| off.is_empty())
        })
    }) else {
        eprintln!("skipping: no fixture auto-fits cleanly enough to Bind");
        return;
    };
    let name = src.file_name().unwrap().to_string_lossy().into_owned();
    let dir = tempfile::tempdir().expect("tempdir");
    let model = stage(&src, dir.path());

    let knee = |p: &std::path::Path| -> f32 {
        SkinnedClip::from_glb(p)
            .expect("load")
            .bind_markers()
            .into_iter()
            .find(|m| m.name == "leftLowerLeg")
            .expect("knee")
            .world[1]
    };

    // Establish the canonical rig first: before this the model carries its
    // provider skeleton, which has no `leftLowerLeg` to read.
    fit_mesh(&model, &model, &BindOptions::default())
        .unwrap_or_else(|e| panic!("{name}: fit: {e}"));
    let seeded = knee(&model);

    // Arrange the rig the way Bind does, with one joint nudged on-mesh.
    let mut heads: Vec<(String, [f32; 3])> = seed_bind_markers(&model)
        .unwrap_or_else(|e| panic!("{name}: seed: {e}"))
        .into_iter()
        .map(|m| (m.name, m.world))
        .collect();
    heads
        .iter_mut()
        .find(|(n, _)| n == "leftLowerLeg")
        .expect("knee head")
        .1[1] -= 0.08;
    fit_mesh_from_heads(&model, &model, &heads, &BindOptions::default())
        .unwrap_or_else(|e| panic!("{name}: bind: {e}"));
    let posed = knee(&model);
    assert!(
        (posed - seeded).abs() > 0.05,
        "{name}: the nudge did not take, so the test proves nothing"
    );

    // Adding a clip keeps it.
    let one = BindOptions {
        clips: vec![CLIP_A.into()],
        ..Default::default()
    };
    bind_mesh(&model, &model, &one).unwrap_or_else(|e| panic!("{name}: bind clip: {e}"));
    assert!(
        (knee(&model) - posed).abs() < 1e-3,
        "{name}: adding a clip moved the knee from {posed} to {}",
        knee(&model)
    );
    assert_eq!(
        animation_names(&model),
        [CLIP_A],
        "{name}: clip still written"
    );

    // `--refit` is the explicit opt-out, and does discard the pose.
    let refit = BindOptions {
        clips: vec![CLIP_A.into()],
        refit: true,
        ..Default::default()
    };
    bind_mesh(&model, &model, &refit).unwrap_or_else(|e| panic!("{name}: refit: {e}"));
    assert!(
        (knee(&model) - seeded).abs() < 1e-3,
        "{name}: --refit should return to the auto-fit"
    );
}
