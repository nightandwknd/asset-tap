use wasm_bindgen::prelude::*;

/// Size of wasm linear memory right now (not RSS, not a peak-usage measurement).
fn mem_kib() -> u64 {
    core::arch::wasm32::memory_size(0) as u64 * 65536 / 1024
}

/// Does rayon's global pool work on wasm32-unknown-unknown (no threads)?
#[wasm_bindgen]
pub fn rayon_probe() -> String {
    std::panic::catch_unwind(|| {
        use rayon::prelude::*;
        let s: u64 = (0..100_000u64).into_par_iter().map(|x| x ^ 1).sum();
        format!("par_iter ok sum={s} current_num_threads={}", rayon::current_num_threads())
    })
    .unwrap_or_else(|_| "par_iter PANICKED".into())
}

/// Minimal repro: the REAL path-based bind API on the browser target.
#[wasm_bindgen]
pub fn path_api_probe(path: &str) -> String {
    let opts = asset_tap_core::BindOptions::default();
    match asset_tap_core::fit_mesh(std::path::Path::new(path), std::path::Path::new("/out.glb"), &opts) {
        Ok(r) => format!("OK joints={}", r.joint_count),
        Err(e) => format!("Err: {e}"),
    }
}

/// REAL SkinnedClip from in-memory bytes; times parse, then CPU skinning for `ms` milliseconds.
#[wasm_bindgen]
pub fn play_probe(glb: &[u8], ms: f64) -> String {
    let now = || js_sys::Date::now();
    let t0 = js_sys::Date::now();
    let parsed = (|| -> Result<asset_tap_core::SkinnedClip, String> {
        let g = gltf::Gltf::from_slice_without_validation(glb).map_err(|e| e.to_string())?;
        let gltf::Gltf { document, blob } = g;
        let buffers = gltf::import_buffers(&document, None, blob).map_err(|e| e.to_string())?;
        asset_tap_core::SkinnedClip::from_gltf(&document, &buffers)
    })();
    let parse_ms = now() - t0;
    let clip = match parsed { Ok(c) => c, Err(e) => return format!("{{\"error\":{e:?}}}") };
    let mut buf = Vec::new();
    let (mut n, start) = (0u32, now());
    while now() - start < ms { clip.sample_positions_into(n as f32 * 0.016 % clip.duration.max(0.01), &mut buf); n += 1; }
    let el = now() - start;
    format!(
        "{{\"parse_ms\":{parse_ms:.1},\"verts\":{},\"has_animation\":{},\"clip_seconds\":{:.3},\"frames\":{n},\"ms_per_frame\":{:.3},\"wasm_linear_memory_kib_at_end\":{}}}",
        clip.vertex_count(), clip.has_animation(), clip.duration, el / n.max(1) as f64, mem_kib()
    )
}
