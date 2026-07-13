# Machine-interface golden fixtures

Wire-format contract samples for `asset-tap --json`, defined by
[docs/CLI_MACHINE_INTERFACE.md](../../../../docs/CLI_MACHINE_INTERFACE.md).

**These files are the drift alarm.** They are vendored **identically** in both
`asset-tap` and its downstream consumers. asset-tap's output tests
([../../json_interface.rs](../../json_interface.rs)) and each consumer's
parser tests run against the same bytes. If either side changes the wire format
without updating these files, its test suite breaks — that's the point.

When the format changes intentionally, regenerate these files and copy them to
consumers in the same change.

## Files

| File                        | What it exercises                                                               |
| --------------------------- | ------------------------------------------------------------------------------- |
| `success.ndjson`            | A full successful run: `start` → progress across all stages → `result` success. |
| `provider_error.ndjson`     | A non-retryable provider error (invalid API key) surfaced as a `result` error.  |
| `rate_limited_retry.ndjson` | A `retrying` progress event (rate limit) followed by eventual success.          |
| `canceled.ndjson`           | A run interrupted mid-3D-generation, ending in a `result` canceled.             |
| `catalog.json`              | A representative `--list --json` document (single JSON object, not NDJSON).     |

The `.ndjson` files are newline-delimited JSON: one event object per line.
`catalog.json` is a single pretty-printed JSON document.

Paths (`bundle_dir`) in `success.ndjson` are illustrative absolute paths; a
consumer reads the real path from the live `result` event, not from the fixture.
