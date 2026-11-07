//! Native timing/memory probe over the REAL `asset-tap-core` rig API.
//!
//!   native-probe <mesh.glb> <pack-dir> <out-dir>
//!
//! Run one fixture per process: peak RSS (`VmHWM`) is a process high-water
//! mark, so it is only attributable if nothing else ran first. Thread count is
//! whatever `RAYON_NUM_THREADS` says (set it to 1 for the single-thread case).
use asset_tap_core::{BindOptions, SkinnedClip, apply_clip, fit_mesh};
use std::{path::PathBuf, time::Instant};

#[cfg(target_family = "wasm")]
fn vm(_key: &str) -> String {
    // Size of wasm linear memory (pages * 64 KiB) when the program exits. This is NOT
    // resident-set size and NOT a peak-usage measurement: linear memory can only grow, and
    // the allocator may hold pages it is not using, so it is only an upper-bound-style
    // indicator of how much memory the module asked the host for.
    #[cfg(target_arch = "wasm32")]
    return format!("{}", core::arch::wasm32::memory_size(0) * 65536 / 1024);
    #[allow(unreachable_code)]
    "n/a".into()
}

#[cfg(not(target_family = "wasm"))]
fn vm(key: &str) -> String {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|s| s.lines().find(|l| l.starts_with(key)).map(|l| l.split_whitespace().nth(1).unwrap_or("?").to_string()))
        .unwrap_or_else(|| "n/a".into())
}

fn main() {
    // wasm has no threads here (no atomics build): run rayon on the calling thread only.
    #[cfg(target_family = "wasm")]
    if std::env::var("SPIKE_RAYON_NAIVE").is_err() {
        rayon::ThreadPoolBuilder::new().num_threads(1).use_current_thread().build_global().expect("rayon current-thread pool");
    }
    let a: Vec<String> = std::env::args().collect();
    let (mesh, pack, out) = (PathBuf::from(&a[1]), PathBuf::from(&a[2]), PathBuf::from(&a[3]));
    std::fs::create_dir_all(&out).unwrap();
    let rest = out.join("rest.glb");
    let baked = out.join("baked.glb");
    let opts = BindOptions { pack_dir: Some(pack), ..BindOptions::default() };
    println!("threads(rayon)={} input_bytes={}", rayon_threads(), std::fs::metadata(&mesh).unwrap().len());

    let t = Instant::now();
    let r = fit_mesh(&mesh, &rest, &opts).expect("fit_mesh");
    let t_bind = t.elapsed();
    println!("bind(fit+weights, no clip): {:?}  joints={} verts={} out_bytes={}", t_bind, r.joint_count, r.vertex_count, std::fs::metadata(&rest).unwrap().len());

    let t = Instant::now();
    let r = apply_clip(&rest, &baked, &opts).expect("apply_clip");
    let t_bake = t.elapsed();
    println!("bake(clip {:?} onto rest): {:?}  out_bytes={}", r.clips, t_bake, std::fs::metadata(&baked).unwrap().len());

    let t = Instant::now();
    let clip = SkinnedClip::from_glb(&baked).expect("load clip");
    let t_load = t.elapsed();
    let mut buf = Vec::new();
    let (mut n, t) = (0u32, Instant::now());
    while t.elapsed().as_millis() < 1000 { clip.sample_positions_into((n as f32) * 0.016, &mut buf); n += 1; }
    println!("SkinnedClip::from_glb: {:?}  verts={}", t_load, clip.vertex_count());
    println!("skin sample: {} frames/s => {:.2} ms/frame (CPU, this thread count)", n, 1000.0 / n as f64);
    #[cfg(target_family = "wasm")]
    println!("wasm linear memory size at exit={} kB (NOT RSS, NOT peak usage)", vm("VmHWM"));
    #[cfg(not(target_family = "wasm"))]
    println!("peak RSS VmHWM={} kB", vm("VmHWM"));
}

fn rayon_threads() -> String {
    std::env::var("RAYON_NUM_THREADS").unwrap_or_else(|_| format!("default({} cores)", std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0)))
}
