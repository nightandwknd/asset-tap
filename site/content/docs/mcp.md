+++
title = "MCP Server"
description = "Use Asset Tap from Claude Desktop, Cursor, and other MCP hosts — asset-tap mcp serves the Model Context Protocol over stdio."
date = 2026-08-21
weight = 5
in_search_index = true

[extra]
images = []

[taxonomies]
tags = ["reference"]
+++

`asset-tap mcp` serves the [Model Context Protocol](https://modelcontextprotocol.io) over stdio, for agent hosts that don't have a shell -- Claude Desktop, Cursor, IDE agents. It is a **thin front door**: every tool maps 1:1 onto something the CLI already does and returns the same shapes as the `--json` wire format, so the two can't drift. If your agent has a shell (Claude Code, Cursor's terminal agent), the [CLI itself](@/docs/cli-usage.md) is usually the better interface.

## Setup

The binary must be on `PATH` (or use its absolute path) -- see [Installation](@/docs/installation.md). Provider keys are the CLI's: `asset-tap auth set fal.ai` or an env var such as `FAL_KEY` -- the MCP server reads the same settings. Saved keys are loaded when the server starts; if you add one with `auth set` while a server is running, restart it (hosts do this on config change).

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

Env vars can be passed the usual way (`"env": {"FAL_KEY": "..."}`) if the host doesn't inherit your shell's.

## Tools

| Tool             | Arguments                                                                                                                                              | Backed by                    | Returns                                                                                           |
| ---------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------ | ---------------------------- | ------------------------------------------------------------------------------------------------- |
| `list_catalog`   | --                                                                                                                                                     | `asset-tap --list --json`    | providers, models + parameter schemas, templates                                                  |
| `auth_status`    | --                                                                                                                                                     | `asset-tap auth list --json` | per provider `configured`, `source` (`stored`\|`env`\|`missing`), `env_var` -- never key material |
| `inspect_bundle` | `bundle_dir`                                                                                                                                           | reads `bundle.json`          | `{bundle_dir, files[], bundle}`                                                                   |
| `generate`       | `prompt` or `image`; optional `template`, `provider`, `image_model`, `model_3d`, `params{}`, `fbx` (default false), `image_only`, `output_dir`, `name` | the generation run           | `{status: "success", bundle_dir, duration_ms, bundle}`                                            |

Every tool returns **structured content** (JSON) plus the same JSON as text, so hosts that read either work.

## `generate` semantics

- Implemented by building an argv from the arguments and running it through the **same clap parser and the same run path as the CLI**. Validation, model resolution, `--param` routing, and error classification are literally the CLI's.
- **Non-interactive**: a `prompt` or `image` is required (usage error otherwise); no approval steps.
- **Long-running** (tens of seconds to minutes). If the host sends a progress token, progress arrives as `notifications/progress` -- the same stages the CLI streams as NDJSON, in order, all delivered before the tool result. Cancelling the request cancels the pipeline; the result then has `kind: "canceled"`.
- **Errors** are tool errors (`isError: true`) carrying the wire error shape: `kind`, `message`, optional `provider` / `action` / `retryable` / `retry_after_secs`. Retry only when `retryable` is true; on `unauthorized`, ask the human for a key instead of looping.
- `fbx` defaults to **false**: FBX conversion needs Blender; GLB is enough for three.js, Godot, Bevy, and most engines. Pass `fbx: true` to get FBX.
- Prefer a fresh `output_dir` per project; runs never overwrite (timestamped bundle directories). `name` sets `bundle.json`'s `name`, not the directory.

## Not this

- Not a hosted/remote service -- stdio only, runs on your machine with your keys.
- Not a place capability appears first: anything the MCP can do, the CLI can do, by construction.

Deeper reference (wire-format details, zero-cost testing with mock builds): [docs/MCP.md](https://github.com/nightandwknd/asset-tap/blob/main/docs/MCP.md) in the repository.
