//! What the rig does with input auto-fit cannot read.
//!
//! Auto-fit must name incompatible input. It is landmark-driven, so a prop
//! fails at the landmark stage rather than silently producing a skeleton in
//! the wrong place. These pin the messages, because "could not find hands"
//! is the difference between a user knowing the asset is wrong and thinking
//! the app is broken.
//!
//! They also pin the other half: refusing to *guess* a pose is not refusing to
//! rig. Rig opens with the shipped skeleton scaled to the mesh, so every asset
//! auto-fit turns away is still riggable by hand.

use asset_tap_core::{default_bind_markers, seed_bind_markers, test_support::glb};

/// Minimal single-mesh GLB from positions + indices.
fn box_glb(w: f32, h: f32, d: f32) -> Vec<u8> {
    let (x, y, z) = (w / 2.0, h, d / 2.0);
    let v: Vec<[f32; 3]> = vec![
        [-x, 0.0, -z],
        [x, 0.0, -z],
        [x, y, -z],
        [-x, y, -z],
        [-x, 0.0, z],
        [x, 0.0, z],
        [x, y, z],
        [-x, y, z],
    ];
    let idx: Vec<u16> = vec![
        0, 1, 2, 0, 2, 3, 4, 6, 5, 4, 7, 6, 0, 4, 5, 0, 5, 1, 3, 2, 6, 3, 6, 7, 0, 3, 7, 0, 7, 4,
        1, 5, 6, 1, 6, 2,
    ];
    let mut bin: Vec<u8> = Vec::new();
    for p in &v {
        for c in p {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let pos_len = bin.len();
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let idx_off = bin.len();
    for i in &idx {
        bin.extend_from_slice(&i.to_le_bytes());
    }
    let idx_len = bin.len() - idx_off;
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let mn = [-x, 0.0f32, -z];
    let mx = [x, y, z];
    let json = serde_json::json!({
        "asset": {"version": "2.0"},
        "scene": 0,
        "scenes": [{"nodes": [0]}],
        "nodes": [{"mesh": 0, "name": "Prop"}],
        "meshes": [{"primitives": [{"attributes": {"POSITION": 0}, "indices": 1}]}],
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": v.len(), "type": "VEC3", "min": mn, "max": mx},
            {"bufferView": 1, "componentType": 5123, "count": idx.len(), "type": "SCALAR"}
        ],
        "bufferViews": [
            {"buffer": 0, "byteOffset": 0, "byteLength": pos_len},
            {"buffer": 0, "byteOffset": idx_off, "byteLength": idx_len}
        ],
        "buffers": [{"byteLength": bin.len()}]
    });
    glb(&serde_json::to_vec(&json).unwrap(), Some(&bin))
}

/// A dense axis-aligned box: a prop with enough vertices to clear the
/// 32-vertex landmark gate, the way a real generated chest or crate would.
fn dense_box_glb(w: f32, h: f32, d: f32, n: usize) -> Vec<u8> {
    let (x, y, z) = (w / 2.0, h, d / 2.0);
    let mut v: Vec<[f32; 3]> = Vec::new();
    let mut idx: Vec<u16> = Vec::new();
    // Six faces, each an n x n grid.
    for face in 0..6 {
        let base = v.len() as u16;
        for i in 0..=n {
            for j in 0..=n {
                let (a, b) = (i as f32 / n as f32, j as f32 / n as f32);
                let p = match face {
                    0 => [-x + 2.0 * x * a, y * b, -z],
                    1 => [-x + 2.0 * x * a, y * b, z],
                    2 => [-x + 2.0 * x * a, 0.0, -z + 2.0 * z * b],
                    3 => [-x + 2.0 * x * a, y, -z + 2.0 * z * b],
                    4 => [-x, y * a, -z + 2.0 * z * b],
                    _ => [x, y * a, -z + 2.0 * z * b],
                };
                v.push(p);
            }
        }
        let stride = (n + 1) as u16;
        for i in 0..n as u16 {
            for j in 0..n as u16 {
                let a = base + i * stride + j;
                idx.extend_from_slice(&[a, a + 1, a + stride, a + 1, a + stride + 1, a + stride]);
            }
        }
    }
    pack_glb(&v, &idx, [-x, 0.0, -z], [x, y, z])
}

fn pack_glb(v: &[[f32; 3]], idx: &[u16], mn: [f32; 3], mx: [f32; 3]) -> Vec<u8> {
    let mut bin: Vec<u8> = Vec::new();
    for p in v {
        for c in p {
            bin.extend_from_slice(&c.to_le_bytes());
        }
    }
    let pos_len = bin.len();
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
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
        "nodes": [{"mesh": 0, "name": "Prop"}],
        "meshes": [{"primitives": [{"attributes": {"POSITION": 0}, "indices": 1}]}],
        "accessors": [
            {"bufferView": 0, "componentType": 5126, "count": v.len(), "type": "VEC3", "min": mn, "max": mx},
            {"bufferView": 1, "componentType": 5123, "count": idx.len(), "type": "SCALAR"}
        ],
        "bufferViews": [
            {"buffer": 0, "byteOffset": 0, "byteLength": pos_len},
            {"buffer": 0, "byteOffset": idx_off, "byteLength": idx_len}
        ],
        "buffers": [{"byteLength": bin.len()}]
    });
    glb(&serde_json::to_vec(&json).unwrap(), Some(&bin))
}

#[test]
fn a_prop_is_refused_with_a_reason_naming_what_was_missing() {
    let dir = tempfile::tempdir().unwrap();

    // Too coarse to sample at all.
    let sparse = dir.path().join("sparse.glb");
    std::fs::write(&sparse, box_glb(1.0, 1.0, 1.0)).unwrap();
    let err = seed_bind_markers(&sparse).unwrap_err().to_string();
    assert!(err.contains("too few vertices"), "{err}");

    // Dense enough to sample, but not a body: the failure names the landmark
    // that could not be found rather than producing a skeleton anyway.
    let wide = dir.path().join("chest.glb");
    std::fs::write(&wide, dense_box_glb(1.2, 0.7, 0.8, 8)).unwrap();
    let err = seed_bind_markers(&wide).unwrap_err().to_string();
    assert!(
        err.contains("hips") || err.contains("hands") || err.contains("head"),
        "expected a named landmark, got: {err}"
    );

    // Tall and thin: also not a body, and also named.
    let tall = dir.path().join("pillar.glb");
    std::fs::write(&tall, dense_box_glb(0.3, 2.0, 0.3, 8)).unwrap();
    let err = seed_bind_markers(&tall).unwrap_err().to_string();
    assert!(
        err.contains("hips"),
        "expected a named landmark, got: {err}"
    );

    // Auto-fit refusing must never be the end of the road. Rig opens with the
    // shipped skeleton scaled to whatever the mesh is, so every asset above is
    // still riggable by hand. Auto-fit is an accelerator; refusing to guess is
    // not refusing to rig, and a workbench with no joints in it and no way to
    // summon any is what this pairing exists to prevent.
    for name in ["chest.glb", "pillar.glb"] {
        let markers = default_bind_markers(&dir.path().join(name))
            .unwrap_or_else(|e| panic!("{name}: Rig must open regardless: {e}"));
        assert!(
            !markers.is_empty(),
            "{name}: a starting skeleton is offered"
        );
    }
}

/// Damaged input must come back as an error, never a panic.
///
/// Every one of these entry points runs on the GUI thread or a job feeding it,
/// so a panic here is a lost session rather than a message. Meshes arrive from
/// providers over the network and from whatever the author drags in, so
/// "malformed" is an ordinary case, not an exotic one.
#[test]
fn damaged_input_is_refused_without_panicking() {
    let dir = tempfile::tempdir().unwrap();
    let good = box_glb(1.0, 1.0, 1.0);

    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty", Vec::new()),
        ("not a glb", b"this is not a model".to_vec()),
        ("header only", good[..12].to_vec()),
        ("truncated mid-json", good[..good.len() / 3].to_vec()),
        ("truncated mid-bin", good[..good.len() - 32].to_vec()),
        // Header intact, JSON chunk length claims more than the file holds.
        ("json length lies", {
            let mut b = good.clone();
            b[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
            b
        }),
        // Valid container, JSON that is not glTF.
        ("json is not gltf", {
            asset_tap_core::test_support::glb(br#"{"hello":"world"}"#, None)
        }),
    ];

    for (label, bytes) in cases {
        let path = dir.path().join(format!("{}.glb", label.replace(' ', "_")));
        std::fs::write(&path, &bytes).unwrap();

        // Each of these is reachable from the workbench with author-supplied
        // input. None may unwind.
        assert!(
            asset_tap_core::default_bind_markers(&path).is_err(),
            "{label}: Rig's starting skeleton should refuse it"
        );
        assert!(
            asset_tap_core::seed_bind_markers(&path).is_err(),
            "{label}: auto-fit should refuse it"
        );
        assert!(
            asset_tap_core::SkinnedClip::for_rig(&path).is_err(),
            "{label}: the viewer loader should refuse it"
        );
        assert!(
            asset_tap_core::is_fitted(&path).is_err() || !asset_tap_core::is_fitted(&path).unwrap(),
            "{label}: a damaged file is not a fitted one"
        );
        assert!(
            asset_tap_core::foreign_rig_joints(&path).is_err()
                || asset_tap_core::foreign_rig_joints(&path).unwrap().is_none(),
            "{label}: a damaged file carries no rig we could name"
        );
    }
}
