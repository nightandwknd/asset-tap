# Contributing to Asset Tap

Thanks for your interest in contributing! Asset Tap generates 3D models from text prompts
via a data-driven, YAML-based provider system. Bug fixes, new providers/templates, docs,
and features are all welcome.

## Getting Set Up

See [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) for the full development setup (prerequisites,
building, running, mock mode, and testing). The Rust toolchain version is pinned in
[rust-toolchain.toml](rust-toolchain.toml) and installs automatically via rustup — you don't
need to manage it by hand.

Quick start:

```bash
git clone https://github.com/nightandwknd/asset-tap.git
cd asset-tap
make build
make test
```

Many changes need no Rust at all — adding a provider or template is just a new YAML file in
`providers/` or `templates/`. See [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) for details.

## Commit Messages: Conventional Commits Required

This project uses [Conventional Commits](https://www.conventionalcommits.org/). This is not
just style: [git-cliff](https://git-cliff.org/) (config in [cliff.toml](cliff.toml))
generates `CHANGELOG.md` and the GitHub Release notes directly from your commit messages. A
commit that doesn't follow the format won't be grouped correctly in the changelog.

Format:

```
type(scope): short description
```

Examples:

```
feat(providers): add support for custom polling intervals
fix(gui): prevent crash when Blender is not installed
docs: clarify mock mode setup
chore(deps): bump the rust-dependencies group
```

Common types: `feat`, `fix`, `docs`, `refactor`, `test`, `chore`, `perf`, `style`, `build`.
See the [Commit Messages section of DEVELOPMENT.md](docs/DEVELOPMENT.md#commit-messages) for
the full list and longer examples.

## Before You Submit

Run the checks locally so CI passes on the first try:

```bash
make fmt      # Auto-format (Rust + TOML/JSON/YAML/Markdown)
make verify   # Auto-fix formatting & lints, then run checks and tests
```

`make verify` applies fixes as it goes. To reproduce CI exactly without any modifications,
run `make ci`.

If you're adding functionality, please also:

- Add or update tests (see [Testing in DEVELOPMENT.md](docs/DEVELOPMENT.md#testing)).
- Update relevant docs when behavior changes.

## Pull Request Process

1. Fork the repo and create a branch off `main`.
2. Make your change with a Conventional Commit message (or a Conventional Commit PR title).
3. Run `make verify` and confirm it's clean.
4. Open a pull request against `main`.
5. CI must pass — formatting, clippy, type check, tests, docs, audit, and cross-platform
   build/package all run automatically.

Keep PRs focused and reasonably small when you can; it makes review faster.

## Licensing of Contributions

Asset Tap is dual-licensed under the MIT and Apache-2.0 licenses. Unless you explicitly
state otherwise, any contribution intentionally submitted for inclusion in the work by you,
as defined in the Apache-2.0 license, shall be dual-licensed as above, without any
additional terms or conditions.
