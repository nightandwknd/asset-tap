# Changelog

All notable changes to Asset Tap are documented here.

## v26.8.12 — 2026-08-20

### Bug Fixes

- FBX conversion is opt-in — GLB-only by default on every surface ([#60](https://github.com/nightandwknd/asset-tap/pull/60)) _(cli)_

## v26.8.11 — 2026-08-20

### Chores

- bump h2 0.4.13 -> 0.4.17 (RUSTSEC-2026-0258) ([#59](https://github.com/nightandwknd/asset-tap/pull/59)) _(deps)_

### Documentation

- document the `atap` alias in install and CLI docs (README, AGENTS.md, site) ([#57](https://github.com/nightandwknd/asset-tap/pull/57))

### Features

- Meshy v7 via fal, Smart Topology, remove_background ([#58](https://github.com/nightandwknd/asset-tap/pull/58)) _(providers)_

## v26.8.10 — 2026-08-18

### Features

- `asset-tap mcp` — Model Context Protocol server over stdio, a thin front door over the CLI's own code paths ([#56](https://github.com/nightandwknd/asset-tap/pull/56)) _(cli)_

## v26.8.9 — 2026-08-16

### Features

- agent-ergonomics pass — auth list --json, feature-gated --mock example, AGENTS.md ([#55](https://github.com/nightandwknd/asset-tap/pull/55)) _(cli)_

## v26.8.8 — 2026-08-16

### Chores

- bump Rust toolchain from 1.94.1 to 1.97.1 ([#54](https://github.com/nightandwknd/asset-tap/pull/54))

## v26.8.7 — 2026-08-13

### Bug Fixes

- pin zola-deploy-action to v0.22.1 until zap theme supports Zola 0.23 _(site)_

### CI/CD

- harden release publish against stale reruns _(ci)_

### Chores

- bump the rust-dependencies group across 1 directory with 2 updates ([#53](https://github.com/nightandwknd/asset-tap/pull/53)) _(deps)_
- bump shalzz/zola-deploy-action ([#52](https://github.com/nightandwknd/asset-tap/pull/52)) _(deps)_

## v26.8.6 — 2026-08-13

### CI/CD

- lock-consistency guard + Dependabot grouping fixes _(ci)_

### Chores

- update webbrowser to 1.2.4 (RUSTSEC-2026-0257) _(deps)_

## v26.8.5 — 2026-08-12

### Features

- serve downloaded demo bundle assets when repo checkout is absent _(mock)_
- ASSET_TAP_MOCK_DEMO_DIR points mock at an external demo bundle dir _(mock)_
- `demo download` subcommand _(cli)_
- ship `atap` alias alongside the CLI in all release archives _(packaging)_

## v26.8.4 — 2026-08-12

### Bug Fixes

- mock mode panics in released binaries (missing demo assets) ([#48](https://github.com/nightandwknd/asset-tap/pull/48))

## v26.8.3 — 2026-08-11

### Chores

- bump open in the rust-dependencies group ([#46](https://github.com/nightandwknd/asset-tap/pull/46)) _(deps)_

### Features

- add Meshy v7 image-to-3D with ultra_mode support ([#47](https://github.com/nightandwknd/asset-tap/pull/47)) _(providers)_

## v26.8.2 — 2026-08-02

### Bug Fixes

- report conflicting generation settings instead of running anyway ([#45](https://github.com/nightandwknd/asset-tap/pull/45)) _(gui)_

### Documentation

- correct inaccurate reference material and document tunable parameters _(site)_

## v26.8.1 — 2026-08-01

### Bug Fixes

- correct CLI validation scope, exit codes, and provider catalog ([#44](https://github.com/nightandwknd/asset-tap/pull/44))

### Chores

- bump serde_json in the rust-dependencies group ([#43](https://github.com/nightandwknd/asset-tap/pull/43)) _(deps)_

## v26.7.3 — 2026-07-23

### Chores

- bump the rust-dependencies group across 1 directory with 2 updates ([#40](https://github.com/nightandwknd/asset-tap/pull/40)) _(deps)_
- bump the rust-dependencies group with 4 updates ([#42](https://github.com/nightandwknd/asset-tap/pull/42)) _(deps)_
- move 3D stack to crates.io releases (egui 0.34, three-d 0.19) ([#41](https://github.com/nightandwknd/asset-tap/pull/41)) _(deps)_

## v26.7.2 — 2026-07-14

### Features

- machine-readable CLI interface (--json) for external tools ([#39](https://github.com/nightandwknd/asset-tap/pull/39))

## v26.7.1 — 2026-07-07

### Bug Fixes

- security hardening, robustness, and parallel test suite ([#36](https://github.com/nightandwknd/asset-tap/pull/36))

### Chores

- bump rpassword in the rust-dependencies group ([#27](https://github.com/nightandwknd/asset-tap/pull/27)) _(deps)_
- bump the rust-dependencies group across 1 directory with 3 updates ([#30](https://github.com/nightandwknd/asset-tap/pull/30)) _(deps)_
- bump the rust-dependencies group with 2 updates ([#31](https://github.com/nightandwknd/asset-tap/pull/31)) _(deps)_
- bump actions/checkout from 6 to 7 ([#34](https://github.com/nightandwknd/asset-tap/pull/34)) _(deps)_
- bump the rust-dependencies group across 1 directory with 2 updates ([#33](https://github.com/nightandwknd/asset-tap/pull/33)) _(deps)_
- bump the rust-dependencies group with 2 updates ([#35](https://github.com/nightandwknd/asset-tap/pull/35)) _(deps)_
- bump open in the rust-dependencies group ([#37](https://github.com/nightandwknd/asset-tap/pull/37)) _(deps)_

## v26.4.18 — 2026-04-28

### Chores

- doc site updates

### Features

- provider parameter and widget audit ([#26](https://github.com/nightandwknd/asset-tap/pull/26))

## v26.4.17 — 2026-04-21

### Chores

- lint/format hardening + preserve gui entitlements on bundle re-sign ([#25](https://github.com/nightandwknd/asset-tap/pull/25))

## v26.4.16 — 2026-04-21

### Bug Fixes

- add workflow timeouts and fix cli notarization format

### Chores

- bump the rust-dependencies group with 3 updates ([#24](https://github.com/nightandwknd/asset-tap/pull/24)) _(deps)_

### Features

- sign and notarize macos releases with developer id ([#23](https://github.com/nightandwknd/asset-tap/pull/23))

## v26.4.15 — 2026-04-20

### Features

- image-only mode and library generation shortcuts ([#22](https://github.com/nightandwknd/asset-tap/pull/22))

## v26.4.14 — 2026-04-19

### Features

- add cli auth subcommand and persist effective model params ([#21](https://github.com/nightandwknd/asset-tap/pull/21))

## v26.4.13 — 2026-04-15

### Bug Fixes

- align provider defaults and persist model params ([#20](https://github.com/nightandwknd/asset-tap/pull/20))

### Chores

- bump indexmap in the rust-dependencies group ([#18](https://github.com/nightandwknd/asset-tap/pull/18)) _(deps)_

## v26.4.12 — 2026-04-14

### Features

- add meshy provider config ([#19](https://github.com/nightandwknd/asset-tap/pull/19))

## v26.4.11 — 2026-04-12

### Chores

- update dependabot config

### Features

- settings hardening, content-compare sync, mock test speedup ([#17](https://github.com/nightandwknd/asset-tap/pull/17))

## v26.4.10 — 2026-04-04

### Chores

- bump indexmap in the rust-minor-patch group ([#15](https://github.com/nightandwknd/asset-tap/pull/15)) _(deps)_

## v26.4.9 — 2026-04-04

### Bug Fixes

- release workflow bundle.json

### Features

- versioned demo bundles, bundle importer, delete, integrity checks ([#16](https://github.com/nightandwknd/asset-tap/pull/16))

## v26.4.7 — 2026-04-03

### Features

- strip demo assets from binary; download on demand ([#14](https://github.com/nightandwknd/asset-tap/pull/14))

## v26.4.6 — 2026-04-02

### Chores

- bump the rust-minor-patch group with 5 updates ([#13](https://github.com/nightandwknd/asset-tap/pull/13)) _(deps)_

## v26.4.5 — 2026-04-02

### Chores

- bump tracing-subscriber in the tracing group ([#12](https://github.com/nightandwknd/asset-tap/pull/12)) _(deps)_

## v26.4.4 — 2026-04-02

### Chores

- bump tokio from 1.49.0 to 1.50.0 in the tokio group ([#11](https://github.com/nightandwknd/asset-tap/pull/11)) _(deps)_

### Other

- ignore rust-toolchain in dependabot

## v26.4.3 — 2026-04-02

### Other

- add dependabot config; simplify workflow needs chains ([#9](https://github.com/nightandwknd/asset-tap/pull/9))

## v26.4.2 — 2026-04-02

### Features

- model tunable parameters, param cli flag, bundle improvements, rust toolchain pin ([#7](https://github.com/nightandwknd/asset-tap/pull/7))

## v26.4.1 — 2026-04-01

### Features

- strip mock mode from release builds ([#8](https://github.com/nightandwknd/asset-tap/pull/8))

## v26.3.6 — 2026-03-28

### Documentation

- add macos gatekeeper workaround

### Features

- upgrade egui to 0.33 ([#6](https://github.com/nightandwknd/asset-tap/pull/6))

## v26.3.5 — 2026-03-28

### Features

- upgrade egui to 0.32 + three-d to git rev ([#5](https://github.com/nightandwknd/asset-tap/pull/5))

## v26.3.4 — 2026-03-25

### Bug Fixes

- ref image metadata, changelog fmt, approval progress, dmg packaging ([#4](https://github.com/nightandwknd/asset-tap/pull/4))

## v26.3.3 — 2026-03-25

### Bug Fixes

- add meta tags for x link previews

### Features

- post-pipeline fbx conversion for gui and cli ([#3](https://github.com/nightandwknd/asset-tap/pull/3))

## v26.3.2 — 2026-03-22

### Bug Fixes

- site links to release artifacts
- codesign, binary size, and macOS install docs ([#2](https://github.com/nightandwknd/asset-tap/pull/2))

## v26.3.1 — 2026-03-22

### Chores

- release fixes and updates

### Features

- build asset tap ([#1](https://github.com/nightandwknd/asset-tap/pull/1))
