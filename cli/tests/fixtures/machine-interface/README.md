# Machine-interface golden fixtures

Wire-format contract samples for `asset-tap --json`, defined by
[docs/CLI_MACHINE_INTERFACE.md](../../../../docs/CLI_MACHINE_INTERFACE.md).

**These files are the drift alarm.** They are vendored **identically** in both
`asset-tap` and its downstream consumers. asset-tap's output tests
([../../json_interface.rs](../../json_interface.rs)) and each consumer's
parser tests run against the same bytes. If either side changes the wire format
without updating these files, its test suite breaks — that's the point.

When the format changes intentionally, regenerate these files, bump
`machine::INTERFACE_VERSION` (MINOR for additive changes), and copy them to
consumers in the same change. Every fixture that carries `interface` states
the current version verbatim (`1.2`), and `json_interface.rs` asserts exact
equality with the constant, not just the MAJOR.

## Files

| File                                | What it exercises                                                                                                                      |
| ----------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------- |
| `success.ndjson`                    | A full successful run: `start` → progress across all stages → `result` success.                                                        |
| `provider_error.ndjson`             | A non-retryable provider error (invalid API key) surfaced as a `result` error.                                                         |
| `rate_limited_retry.ndjson`         | A `retrying` progress event (rate limit) followed by eventual success.                                                                 |
| `canceled.ndjson`                   | A run interrupted mid-3D-generation, ending in a `result` canceled.                                                                    |
| `catalog.json`                      | A representative `--list --json` document (single object): providers, `templates`, `clips`.                                            |
| `auth_catalog.json`                 | A representative `auth list --json` document: `stored`/`env`/`missing`, no keys.                                                       |
| `clip_download.json`                | `--json clip download` after installing missing packs (single object, not NDJSON).                                                     |
| `clip_download_already_exists.json` | `--json clip download` when every release pack id is already present.                                                                  |
| `clip_download_error.json`          | `--json clip download` fail-closed on a manifest with no `sha256`. Same object `clip list --json` writes when `--model` is unreadable. |
| `bind_success.ndjson`               | `bind --json`: the bind result shape (`model`, `joints`, `vertices`, `clips`).                                                         |
| `bind_error.ndjson`                 | `bind --json` refusing a mesh whose joints sit off the body.                                                                           |

The `.ndjson` files are newline-delimited JSON: one event object per line.
The `.json` files are single pretty-printed JSON documents. This table is the
complete set; spec §6 lists the same files as what a consumer vendors.

## Getting these files

Every GitHub Release attaches `machine-interface-fixtures.zip` (these files,
flat) and `machine-interface-fixtures-manifest.json` (the interface version, a
SHA-256 per file, and one for the zip), at
`releases/latest/download/<name>`. Both are built by
[scripts/machine-interface-fixtures.sh](../../../../scripts/machine-interface-fixtures.sh).
Consumers should fetch them in CI, verify the zip against the manifest's
`sha256`, and diff against their vendored copy — not copy from a sibling
checkout, which pins whatever that working tree happened to hold.

Paths (`bundle_dir`) in `success.ndjson` are illustrative absolute paths; a
consumer reads the real path from the live `result` event, not from the fixture.
