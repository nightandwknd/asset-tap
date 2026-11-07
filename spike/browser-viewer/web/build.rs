//! Copies the desktop viewer source into OUT_DIR so it can be `include!`d inside an inline module
//! (needed to add a bytes loader that can reach the viewer's private items).
//!
//! The copy is NOT byte-identical to `gui/src/viewer/model.rs`. Exactly three kinds of change are
//! made, nothing else:
//!   0. The file's 4 leading `//!` inner-doc lines become `//` (inner docs are illegal under
//!      `include!`). Comment-only.
//!   1. SPIKE PATCH 1: the one call `three_d_asset::io::load(&[&temp_path])` is replaced by an empty
//!      `RawAssets`, because that path loader does not exist on wasm32. The replaced fn
//!      (`load_cpu_model_data`) is therefore dead on the web; `bytes_ext::load_glb_bytes` is used.
//!   2. SPIKE PATCH 2: `pub type SharedModelViewer = Arc<Mutex<ModelViewer>>` becomes
//!      `Arc<Mutex<SendWrapper<ModelViewer>>>`, because egui's PaintCallback needs `Send + Sync`
//!      and glow's WebGL `Context` is not `Send`.
//! Each patch target must match exactly once or the build fails, so a change upstream cannot
//! silently go unnoticed. Both patches are extraction requirements, not accidents.
use std::{env, fs, path::PathBuf};
fn main() {
    let src = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../../gui/src/viewer/model.rs");
    println!("cargo:rerun-if-changed={}", src.display());
    let text = fs::read_to_string(&src).expect("read gui/src/viewer/model.rs");
    let mut changed = 0;
    let out: String = text
        .split_inclusive('\n')
        .map(|l| if l.starts_with("//!") { changed += 1; format!("//{}", &l[3..]) } else { l.to_string() })
        .collect();
    // The two MINIMUM wasm32 patches. Each must match exactly once (build fails if upstream
    // moves), and each is exactly an extraction requirement:
    //  1. `three_d_asset::io::load(&[path])` does not exist on wasm32 -> bytes loading is mandatory.
    //  2. egui's PaintCallback needs `Send + Sync`, but glow's WebGL `Context` is `RefCell`-based
    //     (!Send) so `Arc<Mutex<ModelViewer>>` can't be built -> wrap in SendWrapper (single-threaded).
    let patches: [(&str, &str); 2] = [
        ("three_d_asset::io::load(&[&temp_path])",
         "Ok::<three_d_asset::io::RawAssets, String>(three_d_asset::io::RawAssets::new()) /* SPIKE PATCH 1 */"),
        ("pub type SharedModelViewer = Arc<Mutex<ModelViewer>>;",
         "pub type SharedModelViewer = Arc<Mutex<::send_wrapper::SendWrapper<ModelViewer>>>; /* SPIKE PATCH 2 */"),
    ];
    let mut out = out;
    for (from, to) in patches {
        assert_eq!(out.matches(from).count(), 1, "patch target changed upstream: {from}");
        out = out.replace(from, to);
    }
    fs::write(PathBuf::from(env::var("OUT_DIR").unwrap()).join("viewer_model.rs"), out).unwrap();
    println!("cargo:warning=viewer_model.rs: {changed} leading `//!` lines -> `//`, plus 2 asserted wasm32 patches (see build.rs); nothing else changed");
}
