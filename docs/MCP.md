# Asset Tap as an MCP server

`asset-tap mcp` serves the [Model Context Protocol](https://modelcontextprotocol.io)
over stdio. It is a **thin front door**: every tool maps 1:1 onto something the
CLI already does and returns the same shapes as the `--json` wire format
([CLI_MACHINE_INTERFACE.md](CLI_MACHINE_INTERFACE.md)), so the two can't drift.
For hosts with no shell (Claude Desktop and other chat apps) it is the only way
in; for agents that have one (Cursor's agent, IDE agents, Claude Code) both
work — MCP gives discovered, typed tools with progress and cancellation, while
the CLI is the more direct interface for terminal-native agents — see
[AGENTS.md](../AGENTS.md).

## Install

The binary must be on `PATH` (or use its absolute path). Provider keys are the
CLI's: `asset-tap auth set fal.ai` or an env var such as `FAL_KEY` — the MCP
server reads the same settings. Saved keys are loaded when the server starts;
if you add one with `auth set` while a server is running, restart it (hosts
do this on config change).

**Claude Code**

```bash
claude mcp add asset-tap -- asset-tap mcp
```

**Claude Desktop** (`claude_desktop_config.json`) / **Cursor** (`.cursor/mcp.json`)

```json
{
  "mcpServers": {
    "asset-tap": { "command": "asset-tap", "args": ["mcp"] }
  }
}
```

Env vars can be passed the usual way (`"env": {"FAL_KEY": "…"}`) if the host
doesn't inherit your shell's.

## Tools

| Tool             | Arguments                                                                                                                                                                     | Backed by                               | Returns                                                                                          |
| ---------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------- | ------------------------------------------------------------------------------------------------ |
| `list_catalog`   | —                                                                                                                                                                             | `asset-tap --list --json`               | providers, models + parameter schemas, templates, `clips` (installed clip ids)                   |
| `auth_status`    | —                                                                                                                                                                             | `asset-tap auth list --json`            | per provider `configured`, `source` (`stored`\|`env`\|`missing`), `env_var` — never key material |
| `inspect_bundle` | `bundle_dir`                                                                                                                                                                  | reads `bundle.json`                     | `{bundle_dir, files[], bundle}`                                                                  |
| `clip_download`  | optional `force`                                                                                                                                                              | `asset-tap --json clip download`        | `{status, installed[], already_exists, packs_version}`                                           |
| `generate`       | `prompt` or `image`; optional `template`, `provider`, `image_model`, `model_3d`, `params{}`, `bind` (default false), `clips[]`, `image_only`, `output_dir`, `name`, `install` | the generation run, exactly as `--json` | `{status: "success", bundle_dir, duration_ms, bundle}`                                           |

Every tool returns **structured content** (JSON) plus the same JSON as text, so
hosts that read either work.

### `generate` semantics

- Implemented by building an argv from the arguments and running it through
  the **same clap parser and the same run path as the CLI**. Validation,
  model resolution, `--param` routing, and error classification are literally
  the CLI's; a usage error is word-for-word what the CLI would print.
- **Non-interactive** (the `--json` contract): a `prompt` or `image` is
  required (usage error otherwise); no approval steps.
- **Long-running** (tens of seconds to minutes). If the host sends a
  progress token, progress arrives as `notifications/progress` — the same
  stages the CLI streams as NDJSON (`image_generation started`, `queued
  position 3`, `download …`), formatted as short messages, in order, all
  delivered before the tool result. Cancelling the request (MCP
  `notifications/cancelled`) cancels the pipeline — the same channel the
  CLI's SIGINT uses; the result then has `kind: "canceled"`.
- **Errors** are tool errors (`isError: true`) whose structured content is
  the wire error shape: `kind`, `message`, optional `provider` / `action` /
  `retryable` / `retry_after_secs`, plus `status: "error"` and the `stage`
  in flight. `kind: "usage"` for argument problems, `"canceled"` on
  cancellation. Retry only when `retryable` is true; on `unauthorized`, ask
  the human for a key instead of looping.
- `install` copies the run's primary artifact (`model.glb`, or `image.png`
  under `image_only`) to a path of your choosing, the same as the CLI's
  `--install`: a path ending in that extension is the file verbatim, a
  directory receives `<name-or-bundle-dir>.<ext>`, and any other extension is
  a usage error raised **before** the run starts. The bundle is written in
  full either way — it is a copy, not a move — and the result document is
  unchanged.
- Output is GLB, which is enough for three.js, Godot, Bevy, and most engines.
  Older clients passing `fbx` or `no_fbx` are ignored: FBX was removed.
- `bind` defaults to **false**. When true the mesh is rigged to the embedded
  canonical humanoid skeleton, which needs nothing installed, and each name in
  `clips[]` is baked in as its own animation (one model, N animations); no
  `clips` means `walk`. Valid names are `list_catalog` → `clips`. A clip
  no installed pack provides is a local error —
  `clip_download` / `asset-tap clip list --json` / `clip install --from PATH`. Packs are
  Quaternius CC0:
  https://quaternius.com/packs/universalanimationlibrary.html and
  https://quaternius.com/packs/universalanimationlibrary2.html
  (free Standard via `asset-tap clip download` from the latest release,
  hash-verified; paid Source via `clip install --from` on a zip from those
  pages).
  The GUI 3D tab plays overlays retargeted onto the fitted rest (same space
  as Bake) without rewriting `model.glb` until Bake.
- Prefer a fresh `output_dir` per project; runs never overwrite (timestamped
  bundle directories). `name` sets `bundle.json`'s `name`, not the directory.

## Zero-cost testing

Builds with the `mock` feature accept `asset-tap --mock mcp`; every `generate`
then runs the mock pipeline (no API calls) — that's how
`cli/tests/mcp_server.rs` drives a real MCP client against a real child
process. Release binaries don't have `--mock`; use `list_catalog` and
`auth_status` as your no-cost calls there.

## Not this

- Not a hosted/remote service — stdio only, runs on your machine with your
  keys. (Streamable HTTP is possible with the same server type if that's
  ever wanted.)
- Not a place capability appears first: anything the MCP can do, the CLI can
  do, by construction.
