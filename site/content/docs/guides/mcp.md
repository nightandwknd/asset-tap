+++
title = "MCP Server"
description = "Use Asset Tap from Claude Desktop, Cursor, and other MCP hosts. asset-tap mcp serves the Model Context Protocol over stdio."
date = 2026-08-21
weight = 3
in_search_index = true

[extra]
images = []

[taxonomies]
tags = ["reference"]
+++

`asset-tap mcp` serves the [Model Context Protocol](https://modelcontextprotocol.io) over stdio. It is a **thin wrapper**: every tool maps 1:1 onto something the CLI already does and returns the same shapes as the `--json` wire format, so the two can't drift.

When to use which:

- **Hosts with no shell at all** (Claude Desktop and other chat apps) -- the MCP server is the only way in.
- **Agents that have a shell** (Cursor's agent, IDE agents, Claude Code) -- both work. The MCP gives you discovered, typed tools with structured results, live progress, and clean cancellation, with no `PATH` or output-parsing concerns; the [CLI](@/docs/guides/cli-usage.md) is the more direct interface if your agent already lives in a terminal.
- **Long-running generation** benefits from MCP either way: progress streams as notifications and cancelling the request cancels the pipeline.

## Setup

The binary must be on `PATH` -- see [Installation](@/docs/getting-started/installation.md). **For GUI-launched hosts (Claude Desktop), use the absolute path instead**: apps launched outside a terminal don't inherit your shell's `PATH`, so a bare `asset-tap` command can silently fail to spawn (e.g. use `"command": "/Users/you/.local/bin/asset-tap"`). Provider keys are the CLI's: `asset-tap auth set fal.ai` or an env var such as `FAL_KEY` -- the MCP server reads the same settings. Saved keys are loaded when the server starts; if you add one with `auth set` while a server is running, restart it (hosts do this on config change).

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

Env vars can be passed the usual way (`"env": {"FAL_KEY": "..."}`) if the host doesn't inherit your shell's. After editing a host's config, fully quit and reopen it -- a config reload without a real restart is the most common "server doesn't appear" cause.

## Tools

**`list_catalog`** -- no arguments. Backed by `asset-tap --list --json`.
Returns providers, models with their parameter schemas, and templates.

**`auth_status`** -- no arguments. Backed by `asset-tap auth list --json`.
Returns, per provider: `configured`, `source` (`stored` | `env` | `missing`),
and the `env_var` name -- never key material.

**`inspect_bundle`** -- takes `bundle_dir`. Reads the bundle's `bundle.json`.
Returns `{bundle_dir, files[], bundle}`.

**`generate`** -- takes a `prompt` or an `image` path; optional `template`,
`provider`, `image_model`, `model_3d`, `params{}`, `fbx` (default false),
`image_only`, `output_dir`, `name`. Runs the generation pipeline and returns
`{status: "success", bundle_dir, duration_ms, bundle}`.

Every tool returns **structured content** (JSON) plus the same JSON as text, so hosts that read either work. `list_catalog` and `auth_status` make no API calls -- they're free to use as preflight checks.

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

The tools return the same shapes as the CLI's `--json` output; the full wire-format specification is [CLI_MACHINE_INTERFACE.md](https://github.com/nightandwknd/asset-tap/blob/main/docs/CLI_MACHINE_INTERFACE.md) in the repository.
