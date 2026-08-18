# asset-tap as an MCP server

`asset-tap mcp` serves the [Model Context Protocol](https://modelcontextprotocol.io)
over stdio, for agent hosts that don't have a shell — Claude Desktop, Cursor,
IDE agents. It is a **thin front door**: every tool maps 1:1 onto something the
CLI already does and returns the same shapes as the `--json` wire format
([CLI_MACHINE_INTERFACE.md](CLI_MACHINE_INTERFACE.md)), so the two can't drift.
If your agent has a shell (Claude Code, Cursor's terminal agent), the CLI itself
is usually the better interface — see [AGENTS.md](../AGENTS.md).

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

| Tool             | Arguments                                                                                                                                                | Backed by                               | Returns                                                                                          |
| ---------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- | --------------------------------------- | ------------------------------------------------------------------------------------------------ |
| `list_catalog`   | —                                                                                                                                                        | `asset-tap --list --json`               | providers, models + parameter schemas, templates                                                 |
| `auth_status`    | —                                                                                                                                                        | `asset-tap auth list --json`            | per provider `configured`, `source` (`stored`\|`env`\|`missing`), `env_var` — never key material |
| `inspect_bundle` | `bundle_dir`                                                                                                                                             | reads `bundle.json`                     | `{bundle_dir, files[], bundle}`                                                                  |
| `generate`       | `prompt` or `image`; optional `template`, `provider`, `image_model`, `model_3d`, `params{}`, `no_fbx` (default true), `image_only`, `output_dir`, `name` | the generation run, exactly as `--json` | `{status: "success", bundle_dir, duration_ms, bundle}`                                           |

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
- `no_fbx` defaults to **true**: FBX conversion needs Blender; GLB is enough
  for three.js, Godot, Bevy, and most engines. Pass `no_fbx: false` to get FBX.
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
