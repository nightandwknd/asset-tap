# Asset Tap CLI Machine Interface — Spec v1

Status: **implemented in asset-tap** (`--json`, interface version 1.0) · Last updated: 2026-07-11
Consumers: first-party editor integrations, and any future external tooling.

Implementation: the wire format lives in [cli/src/machine.rs](../cli/src/machine.rs);
golden fixtures for both repos are in
[cli/tests/fixtures/machine-interface/](../cli/tests/fixtures/machine-interface/)
and validated by [cli/tests/json_interface.rs](../cli/tests/json_interface.rs).

This spec defines a machine-readable interface for the `asset-tap` CLI so external tools can drive generation as a subprocess without screen-scraping human output. It is a **wire-format contract**: it deliberately does not reference asset-tap's internal Rust types, so either side can change internals freely. Consumers' process bridges and their test fixtures are written against this document.

## Scope

In scope (v1):

1. `--json` output mode: NDJSON events on stdout.
2. A structured final result event (success / error / canceled) carrying the bundle path.
3. Differentiated process exit codes.
4. Machine-readable model/template catalog (`--list-providers --json`, `--list --json`).
5. Graceful cancellation semantics.
6. Release-binary distribution requirements.

Out of scope (explicit non-goals for v1): a long-running `serve` mode, FFI/C-ABI, hosted generation, image-approval interactivity under `--json` (see below).

## Versioning

The `interface` field (in the `start` event and in catalog documents) is a
`"MAJOR.MINOR"` string, following Terraform's `format_version` convention:

- **MAJOR** bumps on breaking wire-format changes — a field removed, a
  field's type or meaning changed, a required shape changed. Consumers
  **must reject** a `start`/catalog whose MAJOR they don't recognize rather
  than guess at the shape.
- **MINOR** bumps on additive, backward-compatible changes — a new event
  variant, a new optional field, a new catalog array. Consumers should
  **tolerate** a MINOR higher than the one they were built against, and must
  **ignore unknown fields** on known events/documents regardless of MINOR.

Current version: `"1.0"`.

## 1. `--json` mode

- New global flag `--json`. When set:
  - **stdout carries NDJSON only**: one JSON object per line, UTF-8, `\n` terminated, no ANSI codes, no banners, no emoji.
  - All human-facing diagnostics/logs go to **stderr** (format unspecified; consumers must not parse stderr).
  - `--json` in v1 requires non-interactive operation: it implies `--yes`, and combining it with `--approve` is a usage error (exit 2). Approval events may be added in a later interface version.
- Every event object has an `event` field (snake_case string). All other fields are event-specific.
- Field/enum naming is `snake_case` throughout.
- **On the wire every event is a single compact line** (as in the `progress` block below). Some single-object examples in this doc are shown with spaces for readability, but the CLI emits them minified, one per `\n`-terminated line.

### Event: `start`

First line emitted. Declares the interface version and generator.

```json
{ "event": "start", "interface": "1.0", "generator": "asset-tap/26.4.18" }
```

- `interface` (string, required): a `"MAJOR.MINOR"` version — see [Versioning](#versioning) below.
- `generator` (string, required): same format as `bundle.json`'s `generator` field.

### Event: `progress`

```json
{"event":"progress","stage":"image_generation","state":"started"}
{"event":"progress","stage":"image_generation","state":"queued","position":3}
{"event":"progress","stage":"model_3d_generation","state":"processing","message":"meshing"}
{"event":"progress","stage":"model_3d_generation","state":"retrying","attempt":2,"max_attempts":5,"delay_secs":10,"reason":"rate limited"}
{"event":"progress","stage":"download","state":"downloading","bytes_downloaded":1048576,"total_bytes":36076232}
{"event":"progress","stage":"image_generation","state":"completed"}
```

- `stage` (required): `image_generation` | `model_3d_generation` | `fbx_conversion` | `download`.
- `state` (required): `started` | `queued` | `processing` | `downloading` | `retrying` | `completed` | `failed`.
- State-specific optional fields: `position` (queued), `message` (processing/failed), `bytes_downloaded`/`total_bytes` (downloading; `total_bytes` may be absent), `attempt`/`max_attempts`/`delay_secs`/`reason` (retrying).
- A `failed` progress state is informational; the authoritative outcome is the `result` event.

### Event: `log`

Free-form informational lines that would otherwise be human stdout.

```json
{ "event": "log", "level": "info", "message": "using template: humanoid" }
```

`level`: `info` (currently the only level emitted — core progress carries no severity; treat unknown values as `info`). Consumers may display or discard.

### Event: `result`

Exactly one `result` event, always the **last line** before exit (on any outcome the CLI can control).

Success:

```json
{
  "event": "result",
  "status": "success",
  "bundle_dir": "/abs/path/output/2026-07-06_101500",
  "duration_ms": 184223
}
```

- `bundle_dir` (required): absolute path to the created bundle directory. Everything else about the output (name, models, mesh stats, provenance) is read from `bundle_dir/bundle.json` — this event intentionally does not duplicate it.

Error:

```json
{
  "event": "result",
  "status": "error",
  "kind": "unauthorized",
  "provider": "fal.ai",
  "stage": "image_generation",
  "message": "fal.ai API key is invalid or expired.",
  "action": "Check your fal.ai API key in Settings.",
  "retryable": false
}
```

- `kind` (required), snake_case, one of: `missing_api_key`, `unauthorized`, `payment_required`, `forbidden`, `not_found`, `validation_error`, `rate_limited`, `server_error`, `timeout`, `model_error`, `network_error`, `blender_not_found`, `io_error`, `unknown`. New kinds may be added; consumers must treat unrecognized kinds as `unknown`.
- Optional: `provider`, `stage`, `action` (suggested user remedy), `retryable` (bool), `retry_after_secs` (int).

Canceled:

```json
{ "event": "result", "status": "canceled", "stage": "model_3d_generation" }
```

Cancellation is typed end-to-end: user signals, image rejection, and provider-side cancels (a job canceled server-side) all produce `status: canceled` — never a generic error.

### Consumer rules (forward compatibility)

- Ignore unknown `event` types and unknown fields on known events.
- Any stdout line that fails to parse as JSON must be ignored (treated as noise), not fatal.
- Only the `result` event is authoritative for the run's outcome. If the process exits without a `result` (hard kill, panic), fall back to the exit code, then to the filesystem invariant below.
- **Filesystem invariant (already true today, must be preserved):** `bundle.json` is written last in the pipeline. A bundle directory containing `bundle.json` is complete; a directory without it is partial garbage and safe to delete/ignore.

## 2. Exit codes

| Code | Meaning                      | Typical `result.kind`                                                                                                                       |
| ---- | ---------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------- |
| 0    | Success                      | —                                                                                                                                           |
| 1    | Internal/unexpected error    | `unknown`, `model_error`                                                                                                                    |
| 2    | Usage error (bad args/flags) | — (clap default, plus invalid `--param`; exits before `start`, so no `result` is emitted)                                                   |
| 3    | API key missing or rejected  | `missing_api_key`, `unauthorized`                                                                                                           |
| 4    | Provider/API error           | `payment_required`, `forbidden`, `not_found`, `validation_error`, `rate_limited`, `server_error`                                            |
| 5    | Canceled                     | — (`status: canceled`)                                                                                                                      |
| 6    | Network/timeout              | `network_error`, `timeout`                                                                                                                  |
| 7    | Local environment/filesystem | `io_error` (output dir not writable, etc.); `blender_not_found` is reserved — FBX conversion is currently best-effort and never fails a run |

Exit codes apply in `--json` mode and (where feasible) in human mode, with one deliberate exception: **cancellation in human mode exits 130** (128+SIGINT, the shell convention) rather than 5, so interactive wrappers detecting interruption keep working. Consumers should prefer `result.kind` over the exit code when both are available.

## 3. Catalog output

`asset-tap --list-providers --json` emits a single JSON document (not NDJSON) on stdout:

```json
{
  "interface": "1.0",
  "providers": [
    {
      "id": "fal.ai",
      "name": "fal.ai",
      "description": "Serverless AI model APIs",
      "configured": true,
      "required_env_vars": ["FAL_KEY"],
      "models": [
        {
          "id": "fal-ai/trellis-2",
          "name": "Trellis 2",
          "description": "…",
          "modality": "image_to_3d",
          "is_default": true,
          "parameters": [
            {
              "name": "resolution",
              "label": "Resolution",
              "description": "…",
              "type": "integer",
              "default": 1024,
              "min": 512,
              "max": 1536,
              "step": 512,
              "widget": "slider"
            },
            {
              "name": "texture_size",
              "label": "Texture Size",
              "type": "select",
              "default": 2048,
              "options": [1024, 2048, 4096]
            }
          ]
        }
      ]
    }
  ]
}
```

- `modality`: `text_to_image` | `image_to_3d`.
- Catalog `parameters` are per-model. A given run only accepts the parameters of the models it will actually use — under `--image-only` no image-to-3D parameter is valid, and with `--image` no text-to-image parameter is. Passing one that doesn't apply is a usage error (exit 2) whose message lists the parameters that do.
- `parameters` mirrors the provider-YAML parameter definitions: `name`, `label`, `description`, `type` (`float`|`integer`|`boolean`|`string`|`select`), `default`, `min`, `max`, `step`, `options`, `widget` (`slider`|`input`). Optional fields omitted when unset.
- `description` (string): human-readable provider description (shared with the human `--list-providers` output — both render from one catalog).
- `configured` (bool): whether the provider's API key is present — lets a consumer build its form _and_ its preflight warnings from one call. Key material itself must never appear in output.
- `asset-tap --list --json` additionally includes a `templates` array: `{id, name, description, category, variables: [{name, description, required}], examples}`.

## 4. Cancellation

- On SIGINT/SIGTERM, the CLI should attempt graceful cancel (the core pipeline already has a cancel channel), emit `{"event":"result","status":"canceled",...}`, and exit 5. Best-effort cleanup of the partial bundle dir is desirable but not required — the `bundle.json`-last invariant covers consumers.
- On platforms/paths where graceful handling isn't possible (e.g. Windows hard `TerminateProcess`), consumers rely on the exit code and the filesystem invariant. The spec only requires that a hard kill can never leave a directory containing `bundle.json` that isn't a valid complete bundle.

## 5. Distribution

- GitHub Releases must include prebuilt CLI binaries per platform: `macos-arm64`, `macos-x64` (or a universal binary), `windows-x64`, `linux-x64`, with a single `SHA256SUMS.txt` covering every release asset (verify per-line with `sha256sum -c --ignore-missing`).
- `asset-tap --version` output must remain a stable, parseable single line containing the CalVer version (used by consumers for compatibility pinning against a tested version range). This human-readable form is unchanged by everything in this spec.
- `asset-tap --version --json` emits a single JSON object instead: `{"version":"26.4.18","interface":"1.0"}`. `version` is the same CalVer string as the human line; `interface` is the current wire-interface version (see [Versioning](#versioning)). Useful for a consumer to check both compatibility axes (CLI release, wire format) in one call without parsing human text.

## 6. Fixtures & acceptance

- This repo and its consumers vendor **identical golden fixture files**: sample NDJSON streams for `success`, `provider_error`, `rate_limited_retry`, `canceled`, plus a sample catalog JSON. Consumers' parser tests and asset-tap's output tests run against the same files — that's the drift alarm.
- Acceptance checklist for the asset-tap implementation:
  - [x] `--json` produces valid NDJSON on stdout; nothing else on stdout.
  - [x] `start` is first, `result` is last, exactly one of each per run.
  - [x] `--json` + `--approve` → exit 2.
  - [x] Exit codes match the table for: missing key, invalid key, provider 5xx, network down, SIGTERM mid-generation.
  - [x] `--list-providers --json` round-trips every parameter definition in the bundled provider YAMLs.
  - [x] Fixture files generated (`cli/tests/fixtures/machine-interface/`) — re-vendor in consumers when integrating.
  - [x] Human-mode stdout unchanged. One intentional behavior change: error/cancel **exit codes** in human mode now follow the §2 table (previously always 1) — see note below.
  - [x] Agent-ergonomics acceptance (§7): a fresh agent with only the binary can construct a correct call from `--help` + `--machine-help` + `--list --json`. (Implemented: after_help examples/auth/exit-codes, --json long help with stdout contract, --machine-help embeds this spec via include_str!.)

## 7. Agent ergonomics

Agents (and scripted tooling) learn CLIs at runtime from `--help`, embedded docs,
and exit codes — they can't read the repo. The binary must be self-describing:

- **`--machine-help`**: prints this spec verbatim (embedded via `include_str!` at
  build time). This replaces the dead reference to `docs/CLI_MACHINE_INTERFACE.md`
  in `--json`'s help — an installed binary has no repo checkout. Exit 0, plain
  text on stdout, works with no other flags. `--describe` is a hidden alias for
  `--machine-help` (same behavior, not shown in `--help`) for consumers that
  probe for a conventional "describe yourself" flag.
- **Examples block** in the main `--help` (clap `after_help`), covering at least:
  - `asset-tap "a stylized sci-fi crate"` — basic generation
  - `asset-tap --image ref.png --no-fbx` — image-to-3D, GLB only
  - `asset-tap "a crate" --json --no-fbx -o ./out` — programmatic use (parse NDJSON)
  - `asset-tap --list --json` — machine-readable model/template catalog
  - `asset-tap "test" --mock --json` — zero-cost pipeline test
  - `echo $KEY | asset-tap auth set fal.ai` — key setup (or env var, e.g. `FAL_KEY`)
- **Exit codes** summarized in `--json`'s long help (`--help`, not `-h`) and in the
  examples block footer: `0 ok · 2 usage · 3 auth · 4 provider · 5 canceled ·
  6 network · 7 local/env · 1 other`.
- **Env-var auth documented** in `after_help`: provider keys resolve from settings
  first, then env vars (`FAL_KEY`, …) — `asset-tap auth list` shows the effective
  source. This is a first-class path (integrations inject keys this way).
- **stdout/stderr contract stated** in `--json`'s long help: NDJSON on stdout only;
  all diagnostics on stderr; consumers must never parse stderr.
- Existing strengths to preserve: `--list --json` as the self-describing capability
  catalog, and per-flag one-line help with behavioral notes (e.g. "implies --yes").

## Implementation notes (non-normative)

- Core's `Progress` enum currently derives only `Debug, Clone` — it is **not** serializable today. Recommended approach: define dedicated event structs in the CLI crate that map from `Progress`/`Stage`/`ApiErrorKind`, rather than deriving serde on core types. That keeps this wire format decoupled from core internals, which is the point of the spec.
- `Stage` and `ApiErrorKind` already derive Serialize; if reused directly, add explicit `#[serde(rename_all = "snake_case")]` so the wire names can't drift with Rust naming.
- clap already exits 2 on usage errors; codes 3–7 need a mapping layer at the top of `main` from the error taxonomy to `std::process::exit`.
- **Human-mode exit-code change (implemented):** the §2 exit-code mapping now applies to human mode too, not just `--json`. Previously every non-usage error exited 1 (anyhow's default). Now, for example, an invalid model/provider or empty prompt exits 4 (validation), and rejecting the image at the approval prompt exits 5 (canceled). Human stdout/stderr text is unchanged; only the process exit code is more specific. Scripts that keyed off "exit 1 == any failure" should switch to checking for non-zero.
