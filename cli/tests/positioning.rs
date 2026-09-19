//! Product-positioning copy drift alarm.
//!
//! Asset Tap's positioning lives in exactly one place that code can read:
//! the `APP_*` constants in `core/src/constants/files.rs`. Several surfaces
//! cannot reference a Rust constant — cargo-packager metadata, Markdown docs,
//! the site's TOML front matter — so they repeat the strings by hand. These
//! tests are what keeps those hand copies honest, and what stops a retired
//! phrase from creeping back in.
//!
//! If a test here fails, the fix is to make the surface match the constant,
//! not to loosen the assertion.

use std::path::{Path, PathBuf};

use asset_tap_core::constants::files::{APP_CATEGORY, APP_DESCRIPTION};

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cli/ always has a parent")
        .to_path_buf()
}

/// Read a `key = "value"` string entry from a TOML file.
///
/// Deliberately not a TOML parse: the workspace has no `toml` dependency, and
/// pulling one in for two lookups would cost every consumer a crate to build.
/// `section` is the last `[header]` that must precede the key, so a
/// `description` under `[package.metadata.packager]` is never confused with
/// the one under `[package]`.
fn toml_string(text: &str, section: &str, key: &str) -> Option<String> {
    let mut current = String::new();
    for line in text.lines() {
        let line = line.trim();
        if let Some(header) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            current = header.to_string();
            continue;
        }
        if current != section {
            continue;
        }
        let Some((found, value)) = line.split_once('=') else {
            continue;
        };
        if found.trim() != key {
            continue;
        }
        let value = value.trim();
        return Some(
            value
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .unwrap_or(value)
                .to_string(),
        );
    }
    None
}

/// The installer's own copy is what users read in the OS package manager, and
/// nothing at build time forces it to agree with the app it installs.
#[test]
fn packager_metadata_matches_positioning_constants() {
    let manifest = std::fs::read_to_string(repo_root().join("gui/Cargo.toml"))
        .expect("gui/Cargo.toml is readable");
    let section = "package.metadata.packager";

    assert_eq!(
        toml_string(&manifest, section, "description").as_deref(),
        Some(APP_CATEGORY),
        "cargo-packager `description` must be APP_CATEGORY verbatim"
    );
    assert_eq!(
        toml_string(&manifest, section, "long_description").as_deref(),
        Some(APP_DESCRIPTION),
        "cargo-packager `long_description` must be APP_DESCRIPTION verbatim"
    );
}

/// The MCP server's instructions open with the product description, so an
/// agent's first impression matches every other surface.
#[test]
fn mcp_instructions_open_with_the_description() {
    let mcp = std::fs::read_to_string(repo_root().join("cli/src/mcp.rs"))
        .expect("cli/src/mcp.rs is readable");
    // The constant is private to the binary crate, so assert on the source.
    assert!(
        mcp.contains("pub const INSTRUCTIONS"),
        "INSTRUCTIONS moved; update this test"
    );
    // The literal wraps with `\` line continuations, which Rust resolves to
    // nothing at compile time. Drop the backslashes and all whitespace from
    // both sides so the comparison is about words, not line breaks.
    let squash = |s: &str| -> String {
        s.chars()
            .filter(|c| !c.is_whitespace() && *c != '\\')
            .collect()
    };
    assert!(
        squash(&mcp).contains(&squash(APP_DESCRIPTION)),
        "MCP INSTRUCTIONS must start from APP_DESCRIPTION verbatim (modulo line wrapping)"
    );
}

/// Positioning phrases we have deliberately retired. Provider YAML is exempt:
/// "Meshy 6 — production-ready 3D models" describes a vendor's model, not us.
const RETIRED: &[&str] = &[
    "agent-native",
    "ai-powered",
    "production-ready",
    "state-of-the-art",
    "text-to-3d generation",
    "text-to-3d model generation",
    "the game-asset generation pipeline",
    "prompt in, engine-ready bundle out",
];

/// Surfaces that carry positioning copy, relative to the repo root.
/// Directories are swept for the listed extension.
const COPY_SURFACES: &[&str] = &[
    "README.md",
    "CLAUDE.md",
    "CONTRIBUTING.md",
    "site/config.toml",
    "cli/src/main.rs",
    "cli/src/mcp.rs",
];

fn files_under(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files_under(&path, ext, out);
        } else if path.extension().is_some_and(|e| e == ext) {
            out.push(path);
        }
    }
}

#[test]
fn no_retired_positioning_phrases_in_copy() {
    let root = repo_root();

    let mut targets: Vec<PathBuf> = COPY_SURFACES.iter().map(|p| root.join(p)).collect();
    files_under(&root.join("site/content"), "md", &mut targets);
    files_under(&root.join("gui/src/views"), "rs", &mut targets);

    let mut offenses = Vec::new();
    for path in targets {
        let Ok(text) = std::fs::read_to_string(&path) else {
            panic!("copy surface is unreadable: {}", path.display());
        };
        for (n, line) in text.lines().enumerate() {
            // Markdown table rows mirror the provider YAML's own model
            // descriptions ("Meshy 6 — production-ready 3D"), which describe a
            // vendor's model rather than Asset Tap. Same exemption as the YAML.
            if line.trim_start().starts_with('|') {
                continue;
            }
            let lower = line.to_lowercase();
            for phrase in RETIRED {
                if lower.contains(phrase) {
                    offenses.push(format!(
                        "{}:{}: retired phrase {phrase:?}\n    {}",
                        path.strip_prefix(&root).unwrap_or(&path).display(),
                        n + 1,
                        line.trim()
                    ));
                }
            }
        }
    }

    assert!(
        offenses.is_empty(),
        "retired positioning copy resurfaced:\n{}",
        offenses.join("\n")
    );
}
