//! Preserve-and-patch GLB writes for Fit / Done / Bake.
//!
//! Fit and Done rebuild mesh *attributes* (world-baked positions + skin) but
//! keep primitive/material/image identity. Bake only replaces `animations`.
//! Staged bytes are validated before the destination is replaced.

use super::skeleton::Armature;
use super::{BindError, export::BakedMesh};
use glam::Mat4;
use serde_json::{Value, json};
use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::path::Path;

pub(super) struct SourceGltf {
    json: Value,
    bin: Vec<u8>,
}

impl SourceGltf {
    pub(super) fn load(path: &Path) -> Result<Self, BindError> {
        let bytes = fs::read(path).map_err(|e| BindError::Io {
            path: path.to_path_buf(),
            source: e,
        })?;
        Self::from_slice(&bytes).map_err(|message| BindError::Gltf {
            path: path.to_path_buf(),
            message,
        })
    }

    fn from_slice(bytes: &[u8]) -> Result<Self, String> {
        let glb = gltf::Glb::from_slice(bytes).map_err(|e| e.to_string())?;
        let json: Value = serde_json::from_slice(&glb.json).map_err(|e| e.to_string())?;
        let bin = glb.bin.map(|b| b.into_owned()).unwrap_or_default();
        Ok(Self { json, bin })
    }
}

pub(super) fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> Result<(), BindError> {
    validate_glb(bytes)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| BindError::Io {
            path: parent.to_path_buf(),
            source: e,
        })?;
    }
    let tmp = path.with_file_name(format!(
        ".{}.partial",
        path.file_name()
            .map(|n| n.to_string_lossy())
            .unwrap_or_else(|| "model.glb".into())
    ));
    fs::write(&tmp, bytes).map_err(|e| BindError::Io {
        path: tmp.clone(),
        source: e,
    })?;
    if let Err(e) = replace_file(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(BindError::Io {
            path: path.to_path_buf(),
            source: e,
        });
    }
    Ok(())
}

fn replace_file(tmp: &Path, dest: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    if dest.exists() {
        fs::remove_file(dest)?;
    }
    fs::rename(tmp, dest)
}

fn validate_glb(bytes: &[u8]) -> Result<(), BindError> {
    let glb = gltf::Glb::from_slice(bytes).map_err(|e| BindError::Failed(e.to_string()))?;
    let json: Value =
        serde_json::from_slice(&glb.json).map_err(|e| BindError::Failed(e.to_string()))?;
    let meshes = json
        .get("meshes")
        .and_then(|m| m.as_array())
        .ok_or_else(|| BindError::Failed("staged GLB has no meshes".into()))?;
    if meshes.is_empty() {
        return Err(BindError::Failed("staged GLB has no meshes".into()));
    }
    // Validate structure and buffers, not pixels, and not texture extensions.
    //
    // Two separate refusals used to land here. `gltf::import_slice` decodes
    // every image and the crate cannot decode WebP ("unsupported image
    // encoding"). Plain `from_slice` then still ran document validation, which
    // rejects any `extensionsRequired` the crate does not implement, plus the
    // missing core `textures[].source` that omitting the fallback implies:
    // exactly the shape fal's trellis-2 emits. Both refused a model over its
    // texture encoding, on a path that never reads a pixel and copies the image
    // bytes through verbatim. What we actually stake the write on is below:
    // meshes exist, and every buffer resolves.
    let gltf::Gltf { document, blob } = gltf::Gltf::from_slice_without_validation(bytes)
        .map_err(|e| BindError::Failed(e.to_string()))?;
    gltf::import_buffers(&document, None, blob)
        .map_err(|e| BindError::Failed(format!("staged GLB is invalid: {e}")))?;
    Ok(())
}

/// Fit / Done: world-bake every primitive, attach one skin, copy materials/images.
pub(super) fn write_skinned_glb(
    source: &SourceGltf,
    mesh: &BakedMesh,
    arm: &Armature,
    skin: &super::weights::Skinning,
    ibms: &[Mat4],
    anims: &[(&super::export::AnimData, &str)],
) -> Result<Vec<u8>, BindError> {
    if mesh.vertex_count() == 0 {
        return Err(BindError::Failed("mesh has no POSITION vertices".into()));
    }
    if skin.joints.len() != mesh.vertex_count() || skin.weights.len() != mesh.vertex_count() {
        return Err(BindError::Failed(
            "skin length does not match concatenated primitives".into(),
        ));
    }

    let mut bin = Bin::new();
    let mut cursor = 0usize;
    let mut meshes_json = Vec::new();
    for part in &mesh.parts {
        let mut prims_json = Vec::new();
        for prim in &part.primitives {
            let n = prim.positions.len();
            let joints = &skin.joints[cursor..cursor + n];
            let weights = &skin.weights[cursor..cursor + n];
            cursor += n;
            prims_json.push(write_primitive(&mut bin, prim, joints, weights)?);
        }
        let mut m = json!({ "primitives": prims_json });
        if let Some(name) = &part.name {
            m["name"] = json!(name);
        }
        meshes_json.push(m);
    }

    let ibm_flat: Vec<f32> = ibms.iter().flat_map(|m| m.to_cols_array()).collect();
    let ibm_acc = bin.push_f32(&ibm_flat, "MAT4", None);

    let keep: Vec<usize> = arm
        .joints
        .iter()
        .enumerate()
        .filter(|(_, j)| j.name != "Mannequin")
        .map(|(i, _)| i)
        .collect();
    let remap: HashMap<usize, usize> = keep
        .iter()
        .enumerate()
        .map(|(new, &old)| (old, new))
        .collect();
    let first_mesh_node = keep.len();

    let mut nodes = Vec::new();
    for &old in &keep {
        let j = &arm.joints[old];
        let children: Vec<usize> = arm
            .joints
            .iter()
            .enumerate()
            .filter(|(i, c)| c.parent == Some(old) && remap.contains_key(i))
            .filter_map(|(i, _)| remap.get(&i).copied())
            .collect();
        let mut n = json!({
            "name": j.name,
            "translation": [j.translation.x, j.translation.y, j.translation.z],
            "rotation": [j.rotation.x, j.rotation.y, j.rotation.z, j.rotation.w],
            "scale": [j.scale.x, j.scale.y, j.scale.z],
        });
        if !children.is_empty() {
            n["children"] = json!(children);
        }
        nodes.push(n);
    }
    for (i, part) in mesh.parts.iter().enumerate() {
        nodes.push(json!({
            "name": part.name.clone().unwrap_or_else(|| format!("Mesh{i}")),
            "mesh": i,
            "skin": 0,
        }));
    }

    let skin_joints: Vec<usize> = arm
        .skin
        .iter()
        .filter_map(|i| remap.get(i).copied())
        .collect();

    let (image_json, texture_json, sampler_json, materials) = copy_appearance(source, &mut bin)?;

    let mut animations = Vec::with_capacity(anims.len());
    for (anim, clip_name) in anims {
        let mut samplers = Vec::new();
        let mut channels = Vec::new();
        append_animation(&mut bin, anim, &remap, &mut samplers, &mut channels);
        if !channels.is_empty() {
            animations.push(json!({
                "name": clip_name,
                "samplers": samplers,
                "channels": channels
            }));
        }
    }

    let scene_root = arm
        .name_to_index
        .get("Rig")
        .and_then(|i| remap.get(i).copied())
        .or_else(|| {
            arm.joints
                .iter()
                .enumerate()
                .find(|(i, j)| j.parent.is_none() && remap.contains_key(i))
                .and_then(|(i, _)| remap.get(&i).copied())
        })
        .unwrap_or(0);
    let mut scene_nodes = vec![json!(scene_root)];
    for i in 0..mesh.parts.len() {
        scene_nodes.push(json!(first_mesh_node + i));
    }

    bin.pad();
    let mut json_doc = json!({
        "asset": { "version": "2.0", "generator": "asset-tap" },
        "scene": 0,
        "scenes": [{ "nodes": scene_nodes }],
        "nodes": nodes,
        "meshes": meshes_json,
        "skins": [{
            "name": "Rig",
            "skeleton": scene_root,
            "joints": skin_joints,
            "inverseBindMatrices": ibm_acc
        }],
        "materials": materials,
        "images": image_json,
        "textures": texture_json,
        "accessors": bin.accessors,
        "bufferViews": bin.views,
        "buffers": [{ "byteLength": bin.data.len() }],
    });
    if !sampler_json.is_empty() {
        json_doc["samplers"] = json!(sampler_json);
    }
    copy_safe_extensions(source, &mut json_doc);
    if !animations.is_empty() {
        json_doc["animations"] = json!(animations);
    }
    pack_glb(&json_doc, &bin.data)
}

fn write_primitive(
    bin: &mut Bin,
    prim: &super::export::BakedPrimitive,
    joints: &[[u16; 4]],
    weights: &[[f32; 4]],
) -> Result<Value, BindError> {
    let pos: Vec<f32> = prim
        .positions
        .iter()
        .flat_map(|p| [p.x, p.y, p.z])
        .collect();
    let pos_acc = bin.push_f32(&pos, "VEC3", Some(34962));
    let idx_acc = bin.push_u32_indices(&prim.indices);
    let jnt_acc = bin.push_u16_vec4(joints);
    let wgt_flat: Vec<f32> = weights.iter().flat_map(|w| *w).collect();
    let wgt_acc = bin.push_f32(&wgt_flat, "VEC4", Some(34962));
    let mut attributes = json!({
        "POSITION": pos_acc,
        "JOINTS_0": jnt_acc,
        "WEIGHTS_0": wgt_acc,
    });
    if let Some(ns) = &prim.normals {
        let v: Vec<f32> = ns.iter().flat_map(|p| [p.x, p.y, p.z]).collect();
        attributes["NORMAL"] = json!(bin.push_f32(&v, "VEC3", Some(34962)));
    }
    if let Some(uv) = &prim.texcoords0 {
        let v: Vec<f32> = uv.iter().flat_map(|p| [p.x, p.y]).collect();
        attributes["TEXCOORD_0"] = json!(bin.push_f32(&v, "VEC2", Some(34962)));
    }
    if let Some(uv) = &prim.texcoords1 {
        let v: Vec<f32> = uv.iter().flat_map(|p| [p.x, p.y]).collect();
        attributes["TEXCOORD_1"] = json!(bin.push_f32(&v, "VEC2", Some(34962)));
    }
    if let Some(cols) = &prim.colors {
        let v: Vec<f32> = cols.iter().flat_map(|c| *c).collect();
        attributes["COLOR_0"] = json!(bin.push_f32(&v, "VEC4", Some(34962)));
    }
    if let Some(tans) = &prim.tangents {
        let v: Vec<f32> = tans.iter().flat_map(|t| *t).collect();
        attributes["TANGENT"] = json!(bin.push_f32(&v, "VEC4", Some(34962)));
    }
    let mut p = json!({
        "attributes": attributes,
        "indices": idx_acc,
    });
    if let Some(mat) = prim.material {
        p["material"] = json!(mat);
    }
    if prim.mode != 4 {
        p["mode"] = json!(prim.mode);
    }
    Ok(p)
}

/// Bake: append animation accessors onto the existing rest GLB. Mesh bytes stay.
/// Replace a rest GLB's animations with exactly `clips`, keeping mesh and
/// image data byte-identical.
///
/// The write is **declarative**: the file ends up with the set it was given,
/// which is what makes Bake idempotent and lets clips be removed. Because the
/// previous set's accessors are then unreferenced, [`prune_unused`] runs
/// afterwards — without it the GLB would grow by a clip's worth of keyframes
/// on every re-bake and removing a clip would never shrink it.
pub(super) fn patch_animations_glb(
    rest_glb: &Path,
    arm: &Armature,
    clips: &[(&super::export::AnimData, &str)],
) -> Result<Vec<u8>, BindError> {
    let source = SourceGltf::load(rest_glb)?;
    let nodes = source
        .json
        .get("nodes")
        .and_then(|v| v.as_array())
        .ok_or_else(|| BindError::Failed("rest GLB has no nodes".into()))?;
    let mut name_to_rest: HashMap<String, usize> = HashMap::new();
    for (i, n) in nodes.iter().enumerate() {
        if let Some(name) = n.get("name").and_then(|v| v.as_str()) {
            name_to_rest.insert(name.to_string(), i);
        }
    }
    let mut remap = HashMap::new();
    for (i, j) in arm.joints.iter().enumerate() {
        if let Some(&dst) = name_to_rest.get(&j.name) {
            remap.insert(i, dst);
        }
    }

    let mut bin = Bin::from_source(&source)?;
    let mut animations = Vec::with_capacity(clips.len());
    for (anim, clip_name) in clips {
        let mut samplers = Vec::new();
        let mut channels = Vec::new();
        append_animation(&mut bin, anim, &remap, &mut samplers, &mut channels);
        if channels.is_empty() {
            return Err(BindError::Failed(format!(
                "clip '{clip_name}' did not match any rest joint names"
            )));
        }
        animations.push(json!({
            "name": clip_name,
            "samplers": samplers,
            "channels": channels
        }));
    }

    bin.pad();
    let mut json_doc = source.json.clone();
    json_doc["accessors"] = json!(bin.accessors);
    json_doc["bufferViews"] = json!(bin.views);
    if animations.is_empty() {
        json_doc.as_object_mut().map(|o| o.remove("animations"));
    } else {
        json_doc["animations"] = json!(animations);
    }
    let (mut json_doc, data) = prune_unused(json_doc, bin.data)?;
    json_doc["buffers"] = json!([{ "byteLength": data.len() }]);
    if let Some(asset) = json_doc.get_mut("asset").and_then(|a| a.as_object_mut()) {
        asset.insert("generator".into(), json!("asset-tap"));
    }
    pack_glb(&json_doc, &data)
}

/// Drop accessors and bufferViews nothing references, and rebuild the buffer.
///
/// Only reachability matters here: an accessor survives if a mesh primitive, a
/// skin, or an animation sampler names it, and a bufferView survives if a
/// surviving accessor or an image names it.
fn prune_unused(mut doc: Value, data: Vec<u8>) -> Result<(Value, Vec<u8>), BindError> {
    let mut used_acc: BTreeSet<usize> = BTreeSet::new();
    let idx = |v: Option<&Value>| v.and_then(|v| v.as_u64()).map(|n| n as usize);

    for mesh in array(&doc, "meshes") {
        for prim in mesh.get("primitives").into_iter().flat_map(as_array) {
            used_acc.extend(idx(prim.get("indices")));
            for group in ["attributes"] {
                if let Some(obj) = prim.get(group).and_then(|v| v.as_object()) {
                    used_acc.extend(obj.values().filter_map(|v| idx(Some(v))));
                }
            }
            for target in prim.get("targets").into_iter().flat_map(as_array) {
                if let Some(obj) = target.as_object() {
                    used_acc.extend(obj.values().filter_map(|v| idx(Some(v))));
                }
            }
        }
    }
    for skin in array(&doc, "skins") {
        used_acc.extend(idx(skin.get("inverseBindMatrices")));
    }
    for anim in array(&doc, "animations") {
        for sampler in anim.get("samplers").into_iter().flat_map(as_array) {
            used_acc.extend(idx(sampler.get("input")));
            used_acc.extend(idx(sampler.get("output")));
        }
    }

    let accessors = array(&doc, "accessors");
    let acc_map: HashMap<usize, usize> =
        used_acc.iter().enumerate().map(|(n, &o)| (o, n)).collect();
    let mut kept_acc: Vec<Value> = Vec::with_capacity(used_acc.len());
    let mut used_views: BTreeSet<usize> = BTreeSet::new();
    for &old in &used_acc {
        let a = accessors
            .get(old)
            .ok_or_else(|| BindError::Failed(format!("accessor {old} out of range")))?;
        used_views.extend(idx(a.get("bufferView")));
        kept_acc.push(a.clone());
    }
    for image in array(&doc, "images") {
        used_views.extend(idx(image.get("bufferView")));
    }

    let views = array(&doc, "bufferViews");
    let view_map: HashMap<usize, usize> = used_views
        .iter()
        .enumerate()
        .map(|(n, &o)| (o, n))
        .collect();
    let mut kept_views = Vec::with_capacity(used_views.len());
    let mut out = Vec::with_capacity(data.len());
    for &old in &used_views {
        let v = views
            .get(old)
            .ok_or_else(|| BindError::Failed(format!("bufferView {old} out of range")))?;
        let off = idx(v.get("byteOffset")).unwrap_or(0);
        let len = idx(v.get("byteLength"))
            .ok_or_else(|| BindError::Failed("bufferView has no byteLength".into()))?;
        let end = off
            .checked_add(len)
            .filter(|e| *e <= data.len())
            .ok_or_else(|| BindError::Failed(format!("bufferView {old} exceeds BIN")))?;
        while !out.len().is_multiple_of(4) {
            out.push(0);
        }
        let mut nv = v.clone();
        nv["byteOffset"] = json!(out.len());
        nv["buffer"] = json!(0);
        out.extend_from_slice(&data[off..end]);
        kept_views.push(nv);
    }

    for a in &mut kept_acc {
        if let Some(bv) = idx(a.get("bufferView")) {
            a["bufferView"] = json!(view_map[&bv]);
        }
    }
    remap_indices(&mut doc, &acc_map, &view_map);
    doc["accessors"] = json!(kept_acc);
    doc["bufferViews"] = json!(kept_views);
    Ok((doc, out))
}

fn array(doc: &Value, key: &str) -> Vec<Value> {
    doc.get(key)
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default()
}

fn as_array(v: &Value) -> Vec<Value> {
    v.as_array().cloned().unwrap_or_default()
}

/// Rewrite every accessor/bufferView reference through the prune maps.
fn remap_indices(doc: &mut Value, acc: &HashMap<usize, usize>, view: &HashMap<usize, usize>) {
    let get = |m: &HashMap<usize, usize>, v: &Value| {
        v.as_u64().and_then(|n| m.get(&(n as usize)).copied())
    };
    if let Some(meshes) = doc.get_mut("meshes").and_then(|v| v.as_array_mut()) {
        for mesh in meshes.iter_mut() {
            let Some(prims) = mesh.get_mut("primitives").and_then(|v| v.as_array_mut()) else {
                continue;
            };
            for prim in prims.iter_mut() {
                if let Some(n) = prim.get("indices").and_then(|v| get(acc, v)) {
                    prim["indices"] = json!(n);
                }
                if let Some(obj) = prim.get_mut("attributes").and_then(|v| v.as_object_mut()) {
                    for v in obj.values_mut() {
                        if let Some(n) = get(acc, v) {
                            *v = json!(n);
                        }
                    }
                }
                if let Some(targets) = prim.get_mut("targets").and_then(|v| v.as_array_mut()) {
                    for t in targets.iter_mut() {
                        if let Some(obj) = t.as_object_mut() {
                            for v in obj.values_mut() {
                                if let Some(n) = get(acc, v) {
                                    *v = json!(n);
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    if let Some(skins) = doc.get_mut("skins").and_then(|v| v.as_array_mut()) {
        for skin in skins.iter_mut() {
            if let Some(n) = skin.get("inverseBindMatrices").and_then(|v| get(acc, v)) {
                skin["inverseBindMatrices"] = json!(n);
            }
        }
    }
    if let Some(anims) = doc.get_mut("animations").and_then(|v| v.as_array_mut()) {
        for anim in anims.iter_mut() {
            let Some(samplers) = anim.get_mut("samplers").and_then(|v| v.as_array_mut()) else {
                continue;
            };
            for s in samplers.iter_mut() {
                for key in ["input", "output"] {
                    if let Some(n) = s.get(key).and_then(|v| get(acc, v)) {
                        s[key] = json!(n);
                    }
                }
            }
        }
    }
    if let Some(images) = doc.get_mut("images").and_then(|v| v.as_array_mut()) {
        for img in images.iter_mut() {
            if let Some(n) = img.get("bufferView").and_then(|v| get(view, v)) {
                img["bufferView"] = json!(n);
            }
        }
    }
}

pub(super) fn write_animation_glb(
    arm: &Armature,
    anim: &super::export::AnimData,
    clip_name: &str,
) -> Result<Vec<u8>, BindError> {
    let mut bin = Bin::new();
    let keep: Vec<usize> = arm
        .joints
        .iter()
        .enumerate()
        .filter(|(_, j)| j.name != "Mannequin")
        .map(|(i, _)| i)
        .collect();
    let remap: HashMap<usize, usize> = keep
        .iter()
        .enumerate()
        .map(|(new, &old)| (old, new))
        .collect();

    let mut nodes = Vec::new();
    for &old in &keep {
        let j = &arm.joints[old];
        let children: Vec<usize> = arm
            .joints
            .iter()
            .enumerate()
            .filter(|(i, c)| c.parent == Some(old) && remap.contains_key(i))
            .filter_map(|(i, _)| remap.get(&i).copied())
            .collect();
        let mut n = json!({
            "name": j.name,
            "translation": [j.translation.x, j.translation.y, j.translation.z],
            "rotation": [j.rotation.x, j.rotation.y, j.rotation.z, j.rotation.w],
            "scale": [j.scale.x, j.scale.y, j.scale.z],
        });
        if !children.is_empty() {
            n["children"] = json!(children);
        }
        nodes.push(n);
    }

    let mut samplers = Vec::new();
    let mut channels = Vec::new();
    append_animation(&mut bin, anim, &remap, &mut samplers, &mut channels);

    let scene_root = arm
        .name_to_index
        .get("Rig")
        .and_then(|i| remap.get(i).copied())
        .unwrap_or(0);
    bin.pad();
    let json_doc = json!({
        "asset": { "version": "2.0", "generator": "asset-tap" },
        "scene": 0,
        "scenes": [{ "nodes": [scene_root] }],
        "nodes": nodes,
        "animations": [{
            "name": clip_name,
            "samplers": samplers,
            "channels": channels
        }],
        "accessors": bin.accessors,
        "bufferViews": bin.views,
        "buffers": [{ "byteLength": bin.data.len() }],
    });
    pack_glb(&json_doc, &bin.data)
}

fn append_animation(
    bin: &mut Bin,
    anim: &super::export::AnimData,
    remap: &HashMap<usize, usize>,
    samplers: &mut Vec<Value>,
    channels: &mut Vec<Value>,
) {
    for ch in &anim.channels {
        let Some(&node) = remap.get(&ch.node) else {
            continue;
        };
        let ty = match ch.path {
            "rotation" => "VEC4",
            _ => "VEC3",
        };
        let time_acc = bin.push_f32(&ch.times, "SCALAR", None);
        let out_acc = bin.push_f32(&ch.values, ty, None);
        let si = samplers.len();
        samplers.push(json!({
            "input": time_acc,
            "output": out_acc,
            "interpolation": ch.interpolation,
        }));
        channels.push(json!({
            "sampler": si,
            "target": { "node": node, "path": ch.path },
        }));
    }
}

fn copy_appearance(
    source: &SourceGltf,
    bin: &mut Bin,
) -> Result<(Vec<Value>, Value, Vec<Value>, Value), BindError> {
    let mut image_json = Vec::new();
    if let Some(images) = source.json.get("images").and_then(|v| v.as_array()) {
        for img in images {
            let mut out = img.clone();
            if let Some(bv) = img.get("bufferView").and_then(|v| v.as_u64()) {
                let bytes = slice_buffer_view(source, bv as usize)?;
                let new_bv = bin.push_bytes(&bytes, None);
                out["bufferView"] = json!(new_bv);
                if let Some(obj) = out.as_object_mut() {
                    obj.remove("uri");
                }
            }
            image_json.push(out);
        }
    }
    let textures = source
        .json
        .get("textures")
        .cloned()
        .unwrap_or_else(|| json!([]));
    let samplers = source
        .json
        .get("samplers")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let materials = source
        .json
        .get("materials")
        .cloned()
        .unwrap_or_else(|| json!([]));
    Ok((image_json, textures, samplers, materials))
}

fn slice_buffer_view(source: &SourceGltf, index: usize) -> Result<Vec<u8>, BindError> {
    let view = source
        .json
        .get("bufferViews")
        .and_then(|v| v.as_array())
        .and_then(|a| a.get(index))
        .ok_or_else(|| BindError::Failed(format!("image bufferView {index} missing")))?;
    let offset = view.get("byteOffset").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
    let length = view
        .get("byteLength")
        .and_then(|v| v.as_u64())
        .ok_or_else(|| BindError::Failed(format!("bufferView {index} missing byteLength")))?
        as usize;
    let end = offset
        .checked_add(length)
        .ok_or_else(|| BindError::Failed("bufferView overflow".into()))?;
    if end > source.bin.len() {
        return Err(BindError::Failed(format!(
            "bufferView {index} exceeds BIN ({end} > {})",
            source.bin.len()
        )));
    }
    Ok(source.bin[offset..end].to_vec())
}

fn copy_safe_extensions(source: &SourceGltf, dest: &mut Value) {
    const DROP: &[&str] = &[
        "KHR_draco_mesh_compression",
        "EXT_meshopt_compression",
        "EXT_mesh_gpu_instancing",
    ];
    let filter = |arr: &Value| -> Vec<Value> {
        arr.as_array()
            .map(|a| {
                a.iter()
                    .filter(|v| v.as_str().is_none_or(|s| !DROP.contains(&s)))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    };
    let used = source
        .json
        .get("extensionsUsed")
        .map(filter)
        .unwrap_or_default();
    if !used.is_empty() {
        dest["extensionsUsed"] = json!(used);
    }
    let required = source
        .json
        .get("extensionsRequired")
        .map(filter)
        .unwrap_or_default();
    if !required.is_empty() {
        dest["extensionsRequired"] = json!(required);
    }
}

struct Bin {
    data: Vec<u8>,
    views: Vec<Value>,
    accessors: Vec<Value>,
}

impl Bin {
    fn new() -> Self {
        Self {
            data: Vec::new(),
            views: Vec::new(),
            accessors: Vec::new(),
        }
    }

    fn from_source(source: &SourceGltf) -> Result<Self, BindError> {
        let views = source
            .json
            .get("bufferViews")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let accessors = source
            .json
            .get("accessors")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        let mut data = source.bin.clone();
        while !data.len().is_multiple_of(4) {
            data.push(0);
        }
        Ok(Self {
            data,
            views,
            accessors,
        })
    }

    fn align(&mut self) {
        while !self.data.len().is_multiple_of(4) {
            self.data.push(0);
        }
    }

    fn pad(&mut self) {
        self.data
            .extend(std::iter::repeat_n(0, (4 - self.data.len() % 4) % 4));
    }

    fn push_bytes(&mut self, bytes: &[u8], target: Option<u32>) -> usize {
        self.align();
        let offset = self.data.len();
        self.data.extend_from_slice(bytes);
        let mut view = json!({
            "buffer": 0,
            "byteOffset": offset,
            "byteLength": bytes.len(),
        });
        if let Some(t) = target {
            view["target"] = json!(t);
        }
        self.views.push(view);
        self.views.len() - 1
    }

    fn push_f32(&mut self, values: &[f32], ty: &str, target: Option<u32>) -> usize {
        let mut bytes = Vec::with_capacity(values.len() * 4);
        for v in values {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        let view = self.push_bytes(&bytes, target);
        let count = match ty {
            "VEC2" => values.len() / 2,
            "VEC3" => values.len() / 3,
            "VEC4" => values.len() / 4,
            "MAT4" => values.len() / 16,
            _ => values.len(),
        };
        let mut acc = json!({
            "bufferView": view,
            "componentType": 5126,
            "count": count,
            "type": ty,
        });
        if ty == "VEC3" && count > 0 {
            let mut min = [f32::INFINITY; 3];
            let mut max = [f32::NEG_INFINITY; 3];
            for c in values.chunks(3) {
                for k in 0..3 {
                    min[k] = min[k].min(c[k]);
                    max[k] = max[k].max(c[k]);
                }
            }
            acc["min"] = json!(min);
            acc["max"] = json!(max);
        }
        if ty == "SCALAR" && count > 0 {
            let (mn, mx) = values
                .iter()
                .fold((f32::INFINITY, f32::NEG_INFINITY), |a, &v| {
                    (a.0.min(v), a.1.max(v))
                });
            acc["min"] = json!([mn]);
            acc["max"] = json!([mx]);
        }
        self.accessors.push(acc);
        self.accessors.len() - 1
    }

    fn push_u16_vec4(&mut self, values: &[[u16; 4]]) -> usize {
        let mut bytes = Vec::with_capacity(values.len() * 8);
        for v in values {
            for c in v {
                bytes.extend_from_slice(&c.to_le_bytes());
            }
        }
        let view = self.push_bytes(&bytes, Some(34962));
        self.accessors.push(json!({
            "bufferView": view,
            "componentType": 5123,
            "count": values.len(),
            "type": "VEC4",
        }));
        self.accessors.len() - 1
    }

    fn push_u32_indices(&mut self, values: &[u32]) -> usize {
        let mut bytes = Vec::with_capacity(values.len() * 4);
        for v in values {
            bytes.extend_from_slice(&v.to_le_bytes());
        }
        let view = self.push_bytes(&bytes, Some(34963));
        self.accessors.push(json!({
            "bufferView": view,
            "componentType": 5125,
            "count": values.len(),
            "type": "SCALAR",
        }));
        self.accessors.len() - 1
    }
}

fn pack_glb(json: &Value, bin: &[u8]) -> Result<Vec<u8>, BindError> {
    let mut json_bytes = serde_json::to_vec(json).map_err(|e| BindError::Failed(e.to_string()))?;
    while !json_bytes.len().is_multiple_of(4) {
        json_bytes.push(b' ');
    }
    let mut bin = bin.to_vec();
    while !bin.len().is_multiple_of(4) {
        bin.push(0);
    }
    let total = 12 + 8 + json_bytes.len() + 8 + bin.len();
    let mut out = Vec::with_capacity(total);
    out.extend_from_slice(b"glTF");
    out.extend_from_slice(&2u32.to_le_bytes());
    out.extend_from_slice(&(total as u32).to_le_bytes());
    out.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x4E4F534Au32.to_le_bytes());
    out.extend_from_slice(&json_bytes);
    out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
    out.extend_from_slice(&0x004E4942u32.to_le_bytes());
    out.extend_from_slice(&bin);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rig::export::{AnimChannel, AnimData, BakedPart, BakedPrimitive};
    use crate::rig::skeleton::Joint;
    use crate::rig::weights::Skinning;
    use glam::{Quat, Vec2, Vec3};
    use image::{ImageFormat, Rgb, RgbImage};
    use std::io::Cursor;

    fn joint(name: &str) -> Joint {
        Joint {
            name: name.into(),
            parent: None,
            translation: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }

    fn tiny_arm() -> Armature {
        let mut hips = joint("hips");
        hips.parent = Some(0);
        let joints = vec![joint("Rig"), hips];
        Armature {
            joints,
            skin: vec![1],
            name_to_index: [("Rig".into(), 0), ("hips".into(), 1)]
                .into_iter()
                .collect(),
        }
    }

    fn png_rgb(r: u8, g: u8, b: u8) -> Vec<u8> {
        let img = RgbImage::from_pixel(1, 1, Rgb([r, g, b]));
        let mut out = Cursor::new(Vec::new());
        img.write_to(&mut out, ImageFormat::Png).unwrap();
        out.into_inner()
    }

    fn two_prim_source(png0: &[u8], png1: &[u8]) -> (SourceGltf, BakedMesh, Skinning, Vec<Mat4>) {
        let mut bin = Vec::new();
        // two triangles
        let pos = [
            0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0, 0.0, 1.0,
            1.0,
        ];
        bin.extend(pos.iter().flat_map(|f| f.to_le_bytes()));
        let idx: [u16; 6] = [0, 1, 2, 0, 1, 2];
        let idx_off = bin.len();
        for i in idx {
            bin.extend_from_slice(&i.to_le_bytes());
        }
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        let img0_off = bin.len();
        bin.extend_from_slice(png0);
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        let img1_off = bin.len();
        bin.extend_from_slice(png1);
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }

        let json = json!({
            "asset": { "version": "2.0" },
            "scene": 0,
            "scenes": [{ "nodes": [0] }],
            "nodes": [{ "mesh": 0 }],
            "meshes": [{
                "name": "Body",
                "primitives": [
                    { "attributes": { "POSITION": 0 }, "indices": 2, "material": 0 },
                    { "attributes": { "POSITION": 1 }, "indices": 2, "material": 1 }
                ]
            }],
            "materials": [
                { "name": "skin", "pbrMetallicRoughness": {
                    "baseColorTexture": { "index": 0 }, "metallicFactor": 0.2, "roughnessFactor": 0.4
                }},
                { "name": "eyes", "pbrMetallicRoughness": {
                    "baseColorTexture": { "index": 1 }, "metallicFactor": 0.8, "roughnessFactor": 0.1
                }}
            ],
            "textures": [{ "source": 0 }, { "source": 1 }],
            "images": [
                { "mimeType": "image/png", "bufferView": 3 },
                { "mimeType": "image/png", "bufferView": 4 }
            ],
            "accessors": [
                { "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
                  "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0] },
                { "bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC3",
                  "min": [0.0, 0.0, 1.0], "max": [1.0, 1.0, 1.0] },
                { "bufferView": 2, "componentType": 5123, "count": 3, "type": "SCALAR" }
            ],
            "bufferViews": [
                { "buffer": 0, "byteOffset": 0, "byteLength": 36 },
                { "buffer": 0, "byteOffset": 36, "byteLength": 36 },
                { "buffer": 0, "byteOffset": idx_off, "byteLength": 6 },
                { "buffer": 0, "byteOffset": img0_off, "byteLength": png0.len() },
                { "buffer": 0, "byteOffset": img1_off, "byteLength": png1.len() }
            ],
            "buffers": [{ "byteLength": bin.len() }]
        });
        let source = SourceGltf { json, bin };
        let tri = |z: f32, material: usize| BakedPrimitive {
            positions: vec![
                Vec3::new(0.0, 0.0, z),
                Vec3::new(1.0, 0.0, z),
                Vec3::new(0.0, 1.0, z),
            ],
            normals: Some(vec![Vec3::Z; 3]),
            texcoords0: Some(vec![Vec2::ZERO, Vec2::X, Vec2::Y]),
            texcoords1: None,
            colors: None,
            tangents: None,
            indices: vec![0, 1, 2],
            material: Some(material),
            mode: 4,
        };
        let baked = BakedMesh {
            parts: vec![BakedPart {
                name: Some("Body".into()),
                primitives: vec![tri(0.0, 0), tri(1.0, 1)],
            }],
        };
        let n = baked.vertex_count();
        let skin = Skinning {
            joints: vec![[0, 0, 0, 0]; n],
            weights: vec![[1.0, 0.0, 0.0, 0.0]; n],
        };
        (source, baked, skin, vec![Mat4::IDENTITY])
    }

    fn webp_rgb(r: u8, g: u8, b: u8) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(4, 4, image::Rgba([r, g, b, 255]));
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut out, image::ImageFormat::WebP)
            .expect("encode webp");
        out.into_inner()
    }

    /// Two meshes, three materials, and the texture kinds a provider actually
    /// emits: a WebP base color behind `EXT_texture_webp`, plus normal and
    /// metallic-roughness maps.
    ///
    /// The plain fixture is one mesh of two primitives with PNG base colors,
    /// which leaves the preservation cases untested: whether the
    /// writer walks *every* mesh, keeps material references that are not
    /// base color, and copies WebP bytes through instead of re-encoding them.
    #[allow(clippy::type_complexity)]
    fn rich_source(imgs: &[Vec<u8>]) -> (SourceGltf, BakedMesh, Skinning, Vec<Mat4>) {
        let mut bin = Vec::new();
        let pos = [
            0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0, 0.0, 1.0,
            1.0,
        ];
        bin.extend(pos.iter().flat_map(|f| f.to_le_bytes()));
        let idx: [u16; 6] = [0, 1, 2, 0, 1, 2];
        let idx_off = bin.len();
        for i in idx {
            bin.extend_from_slice(&i.to_le_bytes());
        }
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        let mut views = vec![
            json!({ "buffer": 0, "byteOffset": 0, "byteLength": 36 }),
            json!({ "buffer": 0, "byteOffset": 36, "byteLength": 36 }),
            json!({ "buffer": 0, "byteOffset": idx_off, "byteLength": 6 }),
        ];
        for img in imgs {
            let off = bin.len();
            bin.extend_from_slice(img);
            while !bin.len().is_multiple_of(4) {
                bin.push(0);
            }
            views.push(json!({ "buffer": 0, "byteOffset": off, "byteLength": img.len() }));
        }

        let json = json!({
            "asset": { "version": "2.0" },
            "extensionsUsed": ["EXT_texture_webp"],
            "scene": 0,
            "scenes": [{ "nodes": [0, 1] }],
            "nodes": [{ "mesh": 0 }, { "mesh": 1 }],
            "meshes": [
                { "name": "Body", "primitives": [
                    { "attributes": { "POSITION": 0 }, "indices": 2, "material": 0 }
                ]},
                { "name": "Cloak", "primitives": [
                    { "attributes": { "POSITION": 1 }, "indices": 2, "material": 1 },
                    { "attributes": { "POSITION": 1 }, "indices": 2, "material": 2 }
                ]}
            ],
            "materials": [
                { "name": "skin", "pbrMetallicRoughness": { "baseColorTexture": { "index": 0 } },
                  "normalTexture": { "index": 1, "scale": 0.8 } },
                { "name": "cloth", "pbrMetallicRoughness": {
                    "baseColorTexture": { "index": 2 },
                    "metallicRoughnessTexture": { "index": 3 } } },
                { "name": "trim", "pbrMetallicRoughness": { "baseColorTexture": { "index": 0 } },
                  "emissiveFactor": [0.1, 0.2, 0.3] }
            ],
            "textures": [
                { "source": 0 },
                { "source": 1 },
                // Spec shape when the extension is not *required*: a core
                // `source` fallback plus the WebP override. Omitting the
                // fallback is legal only under `extensionsRequired`, and the
                // `gltf` crate rejects that at load, which is why
                // `glb_webp` exists to flatten such files to PNG.
                { "source": 0, "extensions": { "EXT_texture_webp": { "source": 2 } } },
                { "source": 3 }
            ],
            "images": [
                { "mimeType": "image/png", "bufferView": 3 },
                { "mimeType": "image/png", "bufferView": 4 },
                { "mimeType": "image/webp", "bufferView": 5 },
                { "mimeType": "image/png", "bufferView": 6 }
            ],
            "accessors": [
                { "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
                  "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0] },
                { "bufferView": 1, "componentType": 5126, "count": 3, "type": "VEC3",
                  "min": [0.0, 0.0, 1.0], "max": [1.0, 1.0, 1.0] },
                { "bufferView": 2, "componentType": 5123, "count": 3, "type": "SCALAR" }
            ],
            "bufferViews": views,
            "buffers": [{ "byteLength": bin.len() }]
        });
        let source = SourceGltf { json, bin };
        let tri = |z: f32, material: usize| BakedPrimitive {
            positions: vec![
                Vec3::new(0.0, 0.0, z),
                Vec3::new(1.0, 0.0, z),
                Vec3::new(0.0, 1.0, z),
            ],
            normals: Some(vec![Vec3::Z; 3]),
            texcoords0: Some(vec![Vec2::ZERO, Vec2::X, Vec2::Y]),
            texcoords1: None,
            colors: None,
            tangents: None,
            indices: vec![0, 1, 2],
            material: Some(material),
            mode: 4,
        };
        let baked = BakedMesh {
            parts: vec![
                BakedPart {
                    name: Some("Body".into()),
                    primitives: vec![tri(0.0, 0)],
                },
                BakedPart {
                    name: Some("Cloak".into()),
                    primitives: vec![tri(1.0, 1), tri(1.5, 2)],
                },
            ],
        };
        let n = baked.vertex_count();
        let skin = Skinning {
            joints: vec![[0, 0, 0, 0]; n],
            weights: vec![[1.0, 0.0, 0.0, 0.0]; n],
        };
        (source, baked, skin, vec![Mat4::IDENTITY])
    }

    #[test]
    fn write_keeps_every_mesh_material_and_texture_kind() {
        let imgs = vec![
            png_rgb(10, 20, 30),
            png_rgb(128, 128, 255), // normal map
            webp_rgb(200, 10, 10),  // base color behind EXT_texture_webp
            png_rgb(0, 255, 0),     // metallic-roughness
        ];
        let (source, baked, skin, ibms) = rich_source(&imgs);
        let arm = tiny_arm();
        let out = write_skinned_glb(&source, &baked, &arm, &skin, &ibms, &[]).unwrap();
        validate_glb(&out).unwrap();
        let after = SourceGltf::from_slice(&out).unwrap();

        // Every mesh, not just the largest.
        let meshes = after.json["meshes"].as_array().unwrap();
        assert_eq!(meshes.len(), 2, "both meshes survive");
        assert_eq!(meshes[0]["name"], "Body");
        assert_eq!(meshes[1]["name"], "Cloak");
        assert_eq!(meshes[1]["primitives"].as_array().unwrap().len(), 2);

        // Materials, including references that are not base color.
        let mats = after.json["materials"].as_array().unwrap();
        assert_eq!(mats.len(), 3);
        assert_eq!(mats[0]["normalTexture"]["index"], 1, "normal map kept");
        assert_eq!(mats[0]["normalTexture"]["scale"], 0.8, "and its scale");
        assert_eq!(
            mats[1]["pbrMetallicRoughness"]["metallicRoughnessTexture"]["index"], 3,
            "metallic-roughness kept"
        );
        assert_eq!(mats[2]["emissiveFactor"][2], 0.3, "emissive kept");

        // The WebP extension survives, still pointing at its image.
        assert_eq!(
            after.json["textures"][2]["extensions"]["EXT_texture_webp"]["source"], 2,
            "EXT_texture_webp reference kept"
        );
        assert!(
            after.json["extensionsUsed"]
                .as_array()
                .is_some_and(|a| a.iter().any(|v| v == "EXT_texture_webp")),
            "extension still declared"
        );

        // Image payloads copied through byte for byte. A re-encode here is
        // silent quality loss on textures the user paid to generate, and for
        // WebP it would also mean decoding a format we never need to read.
        for (i, want) in imgs.iter().enumerate() {
            let bv = after.json["images"][i]["bufferView"].as_u64().unwrap() as usize;
            assert_eq!(
                &slice_buffer_view(&after, bv).unwrap(),
                want,
                "image {i} bytes changed"
            );
        }
        assert_eq!(after.json["images"][2]["mimeType"], "image/webp");
    }

    /// A mesh whose textures *require* `EXT_texture_webp` must still bind.
    ///
    /// This is the shape fal's trellis-2 emits today, and it is stricter than
    /// [`rich_source`]: the extension sits in `extensionsRequired`, so the
    /// textures legally omit the core `source` fallback. Both facts make the
    /// `gltf` crate's document validation reject the file outright
    /// ("Unsupported extension", "textures[0].source: Missing data"), which
    /// refused every freshly generated model on the rig path even though
    /// nothing there reads a pixel. Loading without validation is what fixed
    /// it; this is the guard.
    #[test]
    fn a_mesh_that_requires_the_webp_extension_still_validates() {
        let bin = {
            let mut b = Vec::new();
            for v in [0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0] {
                b.extend_from_slice(&v.to_le_bytes());
            }
            for i in [0u16, 1, 2] {
                b.extend_from_slice(&i.to_le_bytes());
            }
            while !b.len().is_multiple_of(4) {
                b.push(0);
            }
            b
        };
        let webp = webp_rgb(10, 20, 30);
        let img_off = bin.len();
        let mut all = bin.clone();
        all.extend_from_slice(&webp);
        while !all.len().is_multiple_of(4) {
            all.push(0);
        }

        let json = serde_json::json!({
            "asset": { "version": "2.0" },
            // Required, not merely used: the crate refuses what it cannot implement.
            "extensionsUsed": ["EXT_texture_webp"],
            "extensionsRequired": ["EXT_texture_webp"],
            "scene": 0,
            "scenes": [{ "nodes": [0] }],
            "nodes": [{ "mesh": 0 }],
            "meshes": [{ "name": "Body", "primitives": [
                { "attributes": { "POSITION": 0 }, "indices": 1, "material": 0 }
            ]}],
            "materials": [{ "pbrMetallicRoughness": { "baseColorTexture": { "index": 0 } } }],
            // No core `source`: legal only because the extension is required,
            // and exactly what a provider writes.
            "textures": [{ "extensions": { "EXT_texture_webp": { "source": 0 } } }],
            "images": [{ "mimeType": "image/webp", "bufferView": 2 }],
            "accessors": [
                { "bufferView": 0, "componentType": 5126, "count": 3, "type": "VEC3",
                  "min": [0.0, 0.0, 0.0], "max": [1.0, 1.0, 0.0] },
                { "bufferView": 1, "componentType": 5123, "count": 3, "type": "SCALAR" }
            ],
            "bufferViews": [
                { "buffer": 0, "byteOffset": 0, "byteLength": 36 },
                { "buffer": 0, "byteOffset": 36, "byteLength": 6 },
                { "buffer": 0, "byteOffset": img_off, "byteLength": webp.len() }
            ],
            "buffers": [{ "byteLength": all.len() }]
        });

        let glb = crate::test_support::glb(&serde_json::to_vec(&json).unwrap(), Some(&all));
        validate_glb(&glb).expect("a required WebP extension must not refuse the model");

        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("model.glb");
        write_bytes_atomic(&dest, &glb).expect("write must accept it too");
        assert!(dest.is_file());
    }

    #[test]
    fn write_keeps_primitives_materials_and_png_bytes() {
        let png0 = png_rgb(10, 20, 30);
        let png1 = png_rgb(200, 10, 10);
        let (source, baked, skin, ibms) = two_prim_source(&png0, &png1);
        let bytes = write_skinned_glb(&source, &baked, &tiny_arm(), &skin, &ibms, &[]).unwrap();
        validate_glb(&bytes).unwrap();
        let out = SourceGltf::from_slice(&bytes).unwrap();
        let prims = out.json["meshes"][0]["primitives"].as_array().unwrap();
        assert_eq!(prims.len(), 2);
        assert_eq!(prims[0]["material"], 0);
        assert_eq!(prims[1]["material"], 1);
        assert_eq!(out.json["materials"][0]["name"], "skin");
        assert_eq!(
            out.json["materials"][1]["pbrMetallicRoughness"]["metallicFactor"],
            0.8
        );
        assert_eq!(out.json["images"].as_array().unwrap().len(), 2);
        let bv0 = out.json["images"][0]["bufferView"].as_u64().unwrap() as usize;
        let bv1 = out.json["images"][1]["bufferView"].as_u64().unwrap() as usize;
        assert_eq!(slice_buffer_view(&out, bv0).unwrap(), png0);
        assert_eq!(slice_buffer_view(&out, bv1).unwrap(), png1);
    }

    #[test]
    fn atomic_write_leaves_dest_when_bytes_are_invalid() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("model.glb");
        fs::write(&dest, b"KEEP").unwrap();
        let err = write_bytes_atomic(&dest, b"not-a-glb");
        assert!(err.is_err());
        assert_eq!(fs::read(&dest).unwrap(), b"KEEP");
    }

    fn anim_on(node: usize, keys: usize) -> AnimData {
        AnimData {
            channels: vec![AnimChannel {
                node,
                path: "translation",
                times: (0..keys).map(|k| k as f32).collect(),
                values: (0..keys * 3).map(|k| k as f32).collect(),
                interpolation: "LINEAR",
            }],
        }
    }

    fn rest_fixture() -> (tempfile::TempDir, std::path::PathBuf, Armature) {
        let png0 = png_rgb(1, 2, 3);
        let png1 = png_rgb(4, 5, 6);
        let (source, baked, skin, ibms) = two_prim_source(&png0, &png1);
        let arm = tiny_arm();
        let rest = write_skinned_glb(&source, &baked, &arm, &skin, &ibms, &[]).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rest.glb");
        fs::write(&path, &rest).unwrap();
        (dir, path, arm)
    }

    fn anim_names(glb: &[u8]) -> Vec<String> {
        let doc = SourceGltf::from_slice(glb).unwrap();
        doc.json["animations"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|x| x["name"].as_str().unwrap_or_default().to_string())
                    .collect()
            })
            .unwrap_or_default()
    }

    #[test]
    fn skipping_image_decode_yields_the_same_document_and_buffers() {
        // `import_no_images` exists purely for latency: `gltf::import` decodes
        // every texture, and the rig path never reads a decoded pixel. It has
        // to be otherwise indistinguishable, or the speedup is a bug.
        let (_dir, path, _arm) = rest_fixture();

        let (eager_doc, eager_buffers, _images) = gltf::import(&path).unwrap();
        let (lean_doc, lean_buffers) = crate::rig::skeleton::import_no_images(&path).unwrap();

        assert_eq!(lean_doc.nodes().count(), eager_doc.nodes().count());
        assert_eq!(lean_doc.meshes().count(), eager_doc.meshes().count());
        assert_eq!(lean_doc.skins().count(), eager_doc.skins().count());
        assert_eq!(lean_doc.images().count(), eager_doc.images().count());
        let names = |d: &gltf::Document| -> Vec<String> {
            d.nodes()
                .map(|n| n.name().unwrap_or_default().to_string())
                .collect()
        };
        assert_eq!(names(&lean_doc), names(&eager_doc));

        assert_eq!(lean_buffers.len(), eager_buffers.len());
        for (lean, eager) in lean_buffers.iter().zip(eager_buffers.iter()) {
            assert_eq!(lean.0, eager.0, "buffer bytes must be identical");
        }

        // JSON-only goes further and skips buffers entirely, but must still
        // answer structural questions.
        let json_only = crate::rig::skeleton::import_json_only(&path).unwrap();
        assert_eq!(names(&json_only), names(&eager_doc));
    }

    #[test]
    fn bake_writes_every_clip_in_the_set() {
        let (_dir, path, arm) = rest_fixture();
        let (a, b) = (anim_on(1, 4), anim_on(1, 6));
        let out = patch_animations_glb(&path, &arm, &[(&a, "walk"), (&b, "sword")]).unwrap();
        validate_glb(&out).unwrap();
        assert_eq!(anim_names(&out), ["walk", "sword"]);
    }

    #[test]
    fn bake_is_declarative_and_does_not_accumulate() {
        // The set is authoritative: re-baking replaces rather than appends,
        // and the dropped clip's accessors go with it. Without the prune the
        // file would grow on every bake and removing a clip would never
        // shrink it.
        let (_dir, path, arm) = rest_fixture();
        let (a, b) = (anim_on(1, 64), anim_on(1, 64));

        let two = patch_animations_glb(&path, &arm, &[(&a, "walk"), (&b, "sword")]).unwrap();
        fs::write(&path, &two).unwrap();
        assert_eq!(anim_names(&two), ["walk", "sword"]);

        let one = patch_animations_glb(&path, &arm, &[(&a, "walk")]).unwrap();
        validate_glb(&one).unwrap();
        assert_eq!(anim_names(&one), ["walk"], "sword must be gone");
        assert!(
            one.len() < two.len(),
            "dropping a clip must shrink the file: {} -> {}",
            two.len(),
            one.len()
        );

        // Re-baking the same set repeatedly is a fixed point, not growth.
        fs::write(&path, &one).unwrap();
        let again = patch_animations_glb(&path, &arm, &[(&a, "walk")]).unwrap();
        assert_eq!(again.len(), one.len(), "re-bake must be idempotent in size");
    }

    #[test]
    fn baked_names_round_trip_and_playback_picks_by_name() {
        let (_dir, path, arm) = rest_fixture();
        let (a, b) = (anim_on(1, 4), anim_on(1, 6));
        let out = patch_animations_glb(&path, &arm, &[(&a, "walk"), (&b, "sword")]).unwrap();
        fs::write(&path, &out).unwrap();

        // What a reopened model reports is what pre-checks the export set.
        assert_eq!(
            crate::rig::baked_clip_names(&path).unwrap(),
            ["walk", "sword"]
        );

        // Without name selection a multi-clip file only ever plays its first
        // animation, so "sword" would be unreachable in the viewer.
        let second = crate::rig::SkinnedClip::from_glb_named(&path, Some("sword")).unwrap();
        assert_eq!(second.name, "sword");
        let first = crate::rig::SkinnedClip::from_glb(&path).unwrap();
        assert_eq!(first.name, "walk");
        assert!(crate::rig::SkinnedClip::from_glb_named(&path, Some("nope")).is_err());
    }

    #[test]
    fn baking_an_empty_set_strips_animation_but_keeps_the_mesh() {
        let (_dir, path, arm) = rest_fixture();
        let a = anim_on(1, 8);
        let with = patch_animations_glb(&path, &arm, &[(&a, "walk")]).unwrap();
        fs::write(&path, &with).unwrap();

        let bare = patch_animations_glb(&path, &arm, &[]).unwrap();
        validate_glb(&bare).unwrap();
        let doc = SourceGltf::from_slice(&bare).unwrap();
        assert!(doc.json.get("animations").is_none());
        assert_eq!(
            doc.json["meshes"][0]["primitives"]
                .as_array()
                .unwrap()
                .len(),
            2,
            "mesh survives"
        );
        let bv = doc.json["images"][0]["bufferView"].as_u64().unwrap() as usize;
        assert_eq!(slice_buffer_view(&doc, bv).unwrap(), png_rgb(1, 2, 3));
    }

    #[test]
    fn patch_animation_leaves_mesh_and_images() {
        let png0 = png_rgb(1, 2, 3);
        let png1 = png_rgb(4, 5, 6);
        let (source, baked, skin, ibms) = two_prim_source(&png0, &png1);
        let arm = tiny_arm();
        let rest = write_skinned_glb(&source, &baked, &arm, &skin, &ibms, &[]).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rest.glb");
        fs::write(&path, &rest).unwrap();

        let anim = AnimData {
            channels: vec![AnimChannel {
                node: 1,
                path: "translation",
                times: vec![0.0, 1.0],
                values: vec![0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
                interpolation: "LINEAR",
            }],
        };
        let patched = patch_animations_glb(&path, &arm, &[(&anim, "walk")]).unwrap();
        validate_glb(&patched).unwrap();
        let before = SourceGltf::from_slice(&rest).unwrap();
        let after = SourceGltf::from_slice(&patched).unwrap();
        assert_eq!(
            before.json["meshes"][0]["primitives"]
                .as_array()
                .unwrap()
                .len(),
            after.json["meshes"][0]["primitives"]
                .as_array()
                .unwrap()
                .len()
        );
        assert_eq!(after.json["animations"][0]["name"], "walk");
        let bv0 = after.json["images"][0]["bufferView"].as_u64().unwrap() as usize;
        assert_eq!(slice_buffer_view(&after, bv0).unwrap(), png0);
        let pos_acc = after.json["meshes"][0]["primitives"][0]["attributes"]["POSITION"]
            .as_u64()
            .unwrap();
        let view = after.json["accessors"][pos_acc as usize]["bufferView"]
            .as_u64()
            .unwrap() as usize;
        let pos_before = {
            let acc = before.json["meshes"][0]["primitives"][0]["attributes"]["POSITION"]
                .as_u64()
                .unwrap();
            let v = before.json["accessors"][acc as usize]["bufferView"]
                .as_u64()
                .unwrap() as usize;
            slice_buffer_view(&before, v).unwrap()
        };
        assert_eq!(slice_buffer_view(&after, view).unwrap(), pos_before);
    }
}
