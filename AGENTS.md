# AGENTS.md — driving `asset-tap` from a coding agent

This file is for AI agents (Claude Code, Cursor, Codex, …) that either **use
the CLI** to make assets for a project, or **work on this repository**. Humans:
see [README.md](README.md). Everything below is also discoverable from the
binary itself — `asset-tap --help`, `asset-tap --machine-help`, and
`asset-tap --list --json` are the source of truth at runtime.

## Using the CLI (the 60-second version)

```bash
# 0. Not installed? One line (installs to ~/.local/bin, checksum-verified)
curl -fsSL https://assettap.dev/install | bash

# 1. Preflight: which providers have a key? (JSON, never prints keys)
asset-tap auth list --json

# 2. Provide a key if needed — env var or stored setting, both first-class
export FAL_KEY=...              # or: echo "$KEY" | asset-tap auth set fal.ai

# 3. What can it make? Models + templates + parameters, machine-readable
asset-tap --list --json

# 4. Generate — NDJSON events on stdout, one `result` at the end
asset-tap "a low-poly wooden treasure chest" --json -o ./assets/generated

# 5. Read the result: bundle_dir/bundle.json describes everything produced
```

`atap` is a short alias for `asset-tap` in release installs (symlink on
macOS/Linux, `atap.cmd` on Windows) — same binary, same flags; use whichever
is on `PATH`. This file says `asset-tap` for clarity.

Rules an agent should follow:

- **Always pass `--json`.** stdout is then NDJSON only (`start`, `progress`,
  `log`, exactly one `result`); everything human goes to stderr — never parse
  stderr. `--json` implies `--yes` (fully non-interactive) and rejects
  `--approve` (exit 2).
- **A prompt or `--image` is required under `--json`**; there is no
  interactive prompting. Omitting both is a usage error (exit 2, nothing on
  stdout).
- **Read `result.status`** and the exit code — not progress events. Error
  results carry `kind` (`unauthorized`, `rate_limited`, `network`, …),
  `retryable`, and a human `action`. Exit codes: `0` ok · `2` usage · `3`
  auth/key · `4` provider · `5` canceled · `6` network/timeout · `7` local
  environment (Blender, filesystem) · `1` other. Retry only when
  `retryable` is true; on `3`, ask the human for a key rather than looping.
- **The bundle is the product.** `result.bundle_dir` is an absolute path
  containing `bundle.json` (v2: `artifacts` + `pipeline` steps, plus v1
  `config` / mesh stats), the image, `model.glb`, optional `model.fbx`, and
  textures. `bundle.json` is written **last**, so a directory containing it
  is complete. Schema: [docs/guides/BUNDLE_STRUCTURE.md](docs/guides/BUNDLE_STRUCTURE.md).
- **GLB-only is the default; pass `--fbx` only when FBX is needed** — FBX conversion requires
  Blender on the machine (exit 7 if missing). GLB is enough for three.js,
  Godot, Bevy, and most web/engine targets.
- **Two-step pipeline, both steps optional**: text → image
  (`--image-only` stops here) → 3D (`--image PATH_OR_URL` starts here from
  your own image). Pick models with `--image-model` / `--3d-model` from the
  catalog; tune with `--param key=value` (only parameters of the models that
  will actually run are accepted — passing another is exit 2 with the valid
  list in the message).
- **Templates** (`-t humanoid`, `-t vehicle`, … from `--list --json`) turn a
  short description into a well-formed prompt; `--inspect-template NAME`
  shows exactly what it will send.
- **Idempotency**: each run creates a new timestamped bundle directory under
  `-o` (`YYYY-MM-DD_HHMMSS`, with `-1`, `-2`… suffixes on collision) — a run
  never overwrites an earlier one, so re-running the same prompt costs a
  second generation. Check for an existing bundle first if you want to
  reuse. `--name` sets the bundle's `name` in `bundle.json` (needed later
  for `--export-bundle`); it does not change the directory name.
- **Long-running**: a generation takes tens of seconds to a few minutes.
  Stream the NDJSON rather than waiting silently; `progress` events include
  queue position, retries, and download bytes.
- **Cancellation**: SIGINT/SIGTERM → `result.status: canceled`, exit 5
  (`130` in human mode). A hard kill never leaves a `bundle.json` in an
  incomplete directory.

Zero-cost dry runs: builds with the `mock` feature (`make mock ARGS=…`)
accept `--mock`; **release binaries do not have this flag**, and their
`--help` won't show it. Use `--list --json` and `auth list --json` as your
no-cost calls against a release binary.

## No shell? Use the MCP server

MCP hosts (Claude Desktop, Cursor, IDE agents) can add
`asset-tap mcp` as an MCP server — `claude mcp add asset-tap -- asset-tap mcp`
or a `{"command": "asset-tap", "args": ["mcp"]}` entry. Its tools
(`list_catalog`, `auth_status`, `inspect_bundle`, `generate`) return the same
documents as `--list --json`, `auth list --json`, and the `--json` result;
`generate` streams progress as MCP notifications. Details:
[docs/MCP.md](docs/MCP.md). If you _do_ have a shell, prefer the CLI above.

## Working on this repository (contributor agents)

- Start with [CLAUDE.md](CLAUDE.md) (architecture, commands, invariants) and
  [CONTRIBUTING.md](CONTRIBUTING.md) (Conventional Commits, PR checklist).
- `make verify` before a PR; `make ci` to match CI exactly (fmt-check,
  clippy `-D warnings`, workflow/shell lint, doc, audit, tests, the mock CLI
  suite, site build).
- The `--json` wire format is a **contract**
  ([docs/CLI_MACHINE_INTERFACE.md](docs/CLI_MACHINE_INTERFACE.md)):
  changing an existing event/document shape is a MAJOR bump; adding a field
  is MINOR; golden fixtures live in
  `cli/tests/fixtures/machine-interface/` and are vendored by consumers —
  update them in the same PR.
- Providers and templates are YAML (`providers/`, `templates/`), embedded at
  build time and copied to the user's config dir on first run; adding a
  provider needs no Rust changes
  ([docs/guides/PROVIDER_SCHEMA.md](docs/guides/PROVIDER_SCHEMA.md)).
- Tests run in parallel; anything that touches `std::env` or the shared
  templates dir must hold the matching guard from
  `asset_tap_core::test_support` (see CLAUDE.md).
- Public repo, direct-to-main pushes are not used: branch → PR → CI green.
