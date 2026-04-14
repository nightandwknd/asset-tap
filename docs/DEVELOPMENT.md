# Development Guide

This guide covers local development setup, testing, code standards, and contribution workflow.

## Table of Contents

- [Development Setup](#development-setup)
- [Project Structure](#project-structure)
- [Development Workflow](#development-workflow)
- [Testing](#testing)
- [Code Standards](#code-standards)
- [Adding Features](#adding-features)
- [Submitting Changes](#submitting-changes)

## Development Setup

### Prerequisites

- **Rust 1.82+** - Install via [rustup](https://rustup.rs)
- **Git** - For version control
- **cargo-nextest** - Test runner (auto-installed by `make test` if missing, or `cargo install cargo-nextest --locked`)
- **Blender** (optional) - For testing FBX conversion
- **AI Provider API key** - For testing (or use mock mode)

### Initial Setup

```bash
# 1. Clone the repository
git clone https://github.com/nightandwknd/asset-tap.git
cd asset-tap

# 2. Copy environment template
cp .env.example .env

# 3. Add your API keys to .env (optional - can use mock mode)
# Edit .env and add one or more:
#   FAL_KEY=your-key
#   MESHY_API_KEY=your-key
#
# Add API keys for the providers you want to use.
# Check providers/*.yaml for the required environment variable names.

# 4. Build the project
make build

# 5. Run tests to verify setup
make test
```

### Development Tools

The project uses several development tools that auto-install on first use:

- **dprint** - Code formatter (Rust, TOML, JSON, YAML, Markdown)
- **cargo-watch** - Auto-rebuild on file changes
- **cargo-llvm-cov** - Code coverage reports
- **cargo-audit** - Security vulnerability scanning
- **cargo-udeps** - Unused dependency detection

## Project Structure

```
asset-tap/
├── core/              # Core library
│   ├── src/
│   │   ├── api/       # HTTP clients, mock server
│   │   ├── providers/ # Provider system
│   │   ├── templates/ # Template system
│   │   ├── pipeline.rs
│   │   ├── convert.rs # Blender integration
│   │   └── ...
│   └── tests/         # Integration tests
├── cli/               # Command-line interface
│   └── src/main.rs
├── gui/               # GUI application
│   └── src/
│       ├── app.rs     # Main app state
│       ├── views/     # UI components
│       └── ...
├── providers/         # Provider YAML configs
├── templates/         # Template YAML configs
├── docs/              # Documentation
└── Makefile           # Build commands
```

### Cargo Workspace

This is a Cargo workspace with three members:

- `asset-tap-core` - Core library (reusable logic)
- `asset-tap` - CLI binary
- `asset-tap-gui` - GUI binary

## Development Workflow

### Building

```bash
# Full release build (optimized)
make build

# Debug build (faster compilation, slower runtime)
make build-debug

# Build specific component
make build-cli     # CLI only
```

### Running

```bash
# CLI
make cli ARGS='-y "a cowboy ninja with a leather duster, bandana mask, and dual katanas on the back"'

# GUI (release mode)
make gui

# GUI (debug mode - faster iteration)
make dev

# With auto-rebuild on changes
make watch-gui
```

### Mock Mode (Zero-Cost Development)

Mock mode is an opt-in Cargo feature (`mock`) that is **not included in release builds**. The Makefile targets enable it automatically.

Test without consuming API credits:

```bash
# CLI with instant mock responses
make mock ARGS='-y "test prompt"'

# CLI with realistic delays
make mock ARGS='--mock-delay -y "test"'

# GUI in mock mode
make mock-gui

# Or build with the feature explicitly
cargo run --features mock --bin asset-tap -- --mock -y "test"
cargo run --features mock --bin asset-tap-gui  # then set MOCK_API=1
```

Mock mode redirects all API requests to a local `wiremock` server that returns generic synthetic data (test PNG/GLB files). It validates pipeline and configuration plumbing, but does not test provider-specific response parsing.

### Development Mode Paths

Debug builds use `.dev/` directory for all data:

```
.dev/
├── settings.json       # App settings
├── output/             # Generated bundles
├── providers/          # Custom providers (override embedded)
├── templates/          # Custom templates (override embedded)
└── logs/               # Application logs
```

Release builds use OS-specific config directories (e.g., `~/.config/asset-tap/` on Linux).

## Testing

### Running Tests

```bash
# All tests (MUST use --test-threads=1)
make test

# Specific test suites
make test-core              # Core library tests only
cargo test --test mock_server_tests
cargo test --test file_io_tests
cargo test --test pipeline_execution_tests

# With output
cargo test -- --nocapture --test-threads=1

# Watch mode (auto-run on changes)
cargo watch -x "test -- --test-threads=1"
```

**Important:** Tests must run with `--test-threads=1` due to shared file access in the template system.

### Test Coverage

```bash
# Generate coverage report
make coverage

# Open coverage report in browser
make coverage-html
```

### Test Organization

- `core/src/**/*.rs` - Unit tests (inline with code)
- `core/tests/*.rs` - Integration tests
  - `mock_server_tests.rs` - Mock API infrastructure
  - `file_io_tests.rs` - File operations
  - `pipeline_execution_tests.rs` - End-to-end pipeline
  - `integration_tests.rs` - Cross-module integration
  - `discovery_tests.rs` - Model discovery

### Writing Tests

**Unit tests:**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_function_name() {
        let result = my_function();
        assert_eq!(result, expected_value);
    }
}
```

**Async tests:**

```rust
#[tokio::test]
async fn test_async_function() {
    let result = async_function().await.unwrap();
    assert!(result.is_ok());
}
```

**Mock mode tests:**

```rust
#[tokio::test]
async fn test_with_mock_api() {
    // SAFETY: Called before spawning threads in test setup
    unsafe {
        std::env::set_var("MOCK_API", "1");
        std::env::set_var("FAL_KEY", "test-key");
    }

    // Your test code

    unsafe { std::env::remove_var("MOCK_API"); }
}
```

## Code Standards

### Formatting

```bash
# Auto-format all code
make fmt

# Check formatting (CI mode - no changes)
make fmt-check
```

**Format standards:**

- Rust: 4 spaces (rustfmt)
- TOML/JSON/YAML/Markdown: 2 spaces (dprint)
- UTF-8 encoding, LF line endings

### Linting

```bash
# Run Clippy linter
make clippy

# Auto-fix Clippy warnings
make clippy-fix
```

### Full Verification

```bash
# Auto-fix formatting, linting, run tests
make verify

# CI checks only (no modifications)
make ci
```

### Code Style Guidelines

1. **Error Handling**
   - Use `Result<T, Error>` for fallible operations
   - Provide context with `.context("description")`
   - Don't unwrap in library code (only in binaries)

2. **Documentation**
   - Document public APIs with `///` doc comments
   - Include examples for complex functions
   - Keep docs concise and focused

3. **Naming**
   - `snake_case` for functions and variables
   - `PascalCase` for types and traits
   - `SCREAMING_SNAKE_CASE` for constants

4. **Avoid**
   - Hardcoded paths (use `is_dev_mode()` and path helpers)
   - Blocking operations in async contexts
   - `unwrap()` without justification (prefer `?` or `expect()`)

## Adding Features

### Adding a New Provider

Providers are automatically discovered using the `include_dir!` macro. No code changes needed!

1. Create YAML config:

```bash
# Create provider config
cat > providers/my-provider.yaml <<EOF
provider:
  id: "my-provider"
  name: "My Provider"
  description: "Custom AI provider"
  env_vars: ["MY_API_KEY"]
  base_url: "https://api.example.com"

text_to_image:
  - id: "my-model"
    name: "My Model"
    description: "Model description"
    endpoint: "/generate"
    method: POST
    request:
      headers:
        Authorization: "Bearer \${MY_API_KEY}"
      body:
        prompt: "\${prompt}"
    response:
      response_type: Json
      field: "image_url"
EOF
```

2. Rebuild (automatically embeds all providers/*.yaml files):

```bash
make build
```

3. Test:

```bash
# Set API key
export MY_API_KEY="test-key"

# Verify provider loads
make cli ARGS='--list-providers'

# Verify provider loads in mock mode (validates config parsing, not API responses)
make mock ARGS='-p my-provider -y "test"'

# Test with real API to validate response field extraction
make cli ARGS='-p my-provider -y "test"'
```

**To remove a provider:** Delete or move its YAML file out of `providers/` to exclude it from embedding.

See [Provider Schema](guides/PROVIDER_SCHEMA.md) for complete reference.

### Adding a New Template

Templates are automatically discovered using the `include_dir!` macro. No code changes needed!

1. Create template YAML:

```bash
cat > templates/my-template.yaml <<EOF
id: "my-template"
name: "My Template"
description: "Custom prompt template"
category: "general"
template: "A high-quality render of \${description}, professional lighting, 4K"
variables:
  - name: "description"
    description: "Object description"
    required: true
examples:
  - "a futuristic car"
EOF
```

2. Rebuild (automatically embeds all templates/*.yaml files):

```bash
make build
```

3. Test:

```bash
# List models and templates
make cli ARGS='--list'

# Use template
make cli ARGS='-t my-template -y "a sports car"'
```

**To archive a template:** Move to `templates/archive/` to exclude from embedding.

### Adding GUI Features

GUI uses `egui` for UI and `three-d` for 3D rendering:

```rust
// gui/src/views/my_view.rs
use eframe::egui;

pub fn render(ui: &mut egui::Ui, state: &mut AppState) {
    ui.heading("My Feature");

    if ui.button("Click Me").clicked() {
        // Handle click
    }
}
```

Register in `gui/src/app.rs`:

```rust
impl App {
    fn ui(&mut self, ui: &mut egui::Ui) {
        // ... existing views
        my_view::render(ui, self);
    }
}
```

## Submitting Changes

### Before Submitting

1. **Run all checks:**
   ```bash
   make verify  # Format, lint, test
   ```

2. **Update documentation:**
   - Add/update doc comments for new APIs
   - Update relevant markdown docs
   - Add examples if introducing new features

3. **Write tests:**
   - Unit tests for new functions
   - Integration tests for new features
   - Maintain or improve coverage

### Commit Messages

Use [Conventional Commits](https://www.conventionalcommits.org/) format. These are parsed by [git-cliff](https://git-cliff.org/) (config: `cliff.toml`) to auto-generate the changelog and GitHub Release notes.

```
type(scope): short description

Longer description if needed.

- Bullet points for details
- Multiple lines OK
```

**Types:**

- `feat:` - New feature
- `fix:` - Bug fix
- `docs:` - Documentation only
- `style:` - Formatting, no code change
- `refactor:` - Code restructuring
- `test:` - Adding/updating tests
- `chore:` - Build, dependencies, etc.

**Examples:**

```
feat(providers): add support for custom polling intervals

Allows providers to specify custom polling intervals and max attempts
in their YAML configuration. Improves flexibility for slow APIs.

- Add interval_ms and max_attempts fields
- Update schema validation
- Add tests for custom timing
```

```
fix(gui): prevent crash when Blender not installed

Adds null check before accessing Blender path. Shows user-friendly
message instead of panicking.

Fixes #123
```

### CI Checks

All PRs must pass:

- ✅ Formatting (`make fmt-check` — rustfmt + dprint + editorconfig-checker)
- ✅ Clippy linting (`make clippy`)
- ✅ Type check (`make check`)
- ✅ Tests & coverage (`cargo llvm-cov`)
- ✅ Documentation (`make doc`)
- ✅ Security audit (`cargo audit`)
- ✅ Unused dependencies (`cargo udeps`)
- ✅ Build + package on all platforms via shared action (macOS, Linux, Windows)
- ✅ CLI tests (downloads Linux binary artifact, runs `scripts/test_cli.sh`)

## Building Release Packages

For creating distributable installers (`.dmg`, `.exe`, `.deb`, etc.), see [PACKAGING.md](PACKAGING.md).

Quick reference:

```bash
# Platform-specific packaging
make package-macos     # .dmg and .app
make package-windows   # .exe installer
make package-linux     # .deb and .AppImage
```

## License

By contributing, you agree that your contributions will be licensed under the GNU Affero General Public License v3.0 (AGPL-3.0).
