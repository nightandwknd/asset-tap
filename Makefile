.PHONY: help lock-check build build-debug build-cli \
	cli gui dev mock mock-delay mock-gui mock-gui-delay refresh-models \
	test test-core test-cli test-gui test-unit test-integration test-mock test-cli-comprehensive bench \
	coverage coverage-html check clippy clippy-fix fmt fmt-check audit udeps udeps-ci \
	lint-workflows lint-shell \
	doc doc-open install watch watch-gui verify ci clean \
	package-macos package-macos-universal package-windows package-linux install-packager \
	site-serve site-build site-check

# Dependency check helpers
CHECK_DPRINT := $(shell command -v dprint 2> /dev/null)
CHECK_LLVM_COV := $(shell cargo --list 2> /dev/null | grep -q llvm-cov && echo "yes")
CHECK_AUDIT := $(shell cargo --list 2> /dev/null | grep -q audit && echo "yes")
CHECK_UDEPS := $(shell cargo --list 2> /dev/null | grep -q udeps && echo "yes")
CHECK_NEXTEST := $(shell cargo --list 2> /dev/null | grep -q nextest && echo "yes")
CHECK_WATCH := $(shell cargo --list 2> /dev/null | grep -q watch && echo "yes")
CHECK_PACKAGER := $(shell command -v cargo-packager 2> /dev/null)
CHECK_ZOLA := $(shell command -v zola 2> /dev/null)
CHECK_EC := $(shell command -v editorconfig-checker 2> /dev/null)
CHECK_ACTIONLINT := $(shell command -v actionlint 2> /dev/null)
CHECK_SHELLCHECK := $(shell command -v shellcheck 2> /dev/null)

help: ## Show this help message
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-20s\033[0m %s\n", $$1, $$2}'

# =============================================================================
# Build
# =============================================================================

build: ## Build all binaries (release)
	cargo build --release --workspace

build-debug: ## Build all binaries (debug)
	cargo build

build-cli: ## Build CLI only (release)
	cargo build --release -p asset-tap

# =============================================================================
# Run
# =============================================================================

cli: ## Build and run CLI (use ARGS= for arguments)
	cargo run --release --bin asset-tap -- $(ARGS)

gui: ## Build and run GUI (release)
	cargo run --release --bin asset-tap-gui

dev: ## Run GUI in debug mode (faster builds, mock enabled)
	cargo run --features mock --bin asset-tap-gui

# =============================================================================
# Mock Mode (no API costs)
# =============================================================================

mock: ## Run CLI in mock mode (use ARGS= for prompt)
	cargo run --features mock --bin asset-tap -- --mock $(ARGS)

mock-delay: ## Run CLI in mock mode with realistic delays
	cargo run --features mock --bin asset-tap -- --mock --mock-delay $(ARGS)

mock-gui: ## Run GUI in mock mode
	MOCK_API=1 cargo run --features mock --bin asset-tap-gui

mock-gui-delay: ## Run GUI in mock mode with realistic delays
	MOCK_API=1 MOCK_DELAY=1 cargo run --features mock --bin asset-tap-gui

refresh-models: ## [Dev] Refresh provider models from discovery APIs
	cargo run --bin refresh-models -p asset-tap-core

# =============================================================================
# Quality
# =============================================================================

test: ## Run all tests
	@rm -f core/custom_templates.json
ifndef CHECK_NEXTEST
	@echo "Installing cargo-nextest..."
	@cargo install cargo-nextest --locked
endif
	cargo nextest run --workspace --all-features
	@# nextest does not run doctests; run them separately so the rustdoc
	@# examples (e.g. the pipeline quick-start in lib.rs) can't silently rot.
	cargo test --doc --workspace --all-features

test-core: ## Test core library only
	@rm -f core/custom_templates.json
ifndef CHECK_NEXTEST
	@echo "Installing cargo-nextest..."
	@cargo install cargo-nextest --locked
endif
	cargo nextest run -p asset-tap-core --all-features

test-cli: ## Test CLI only
	cargo test -p asset-tap

test-gui: ## Test GUI only
	cargo test -p asset-tap-gui

test-unit: ## Run unit tests only (fast, no integration tests)
	cargo test --workspace --lib

test-integration: ## Run integration tests only
	cargo test -p asset-tap-core --test integration_tests

test-mock: ## Run mock API integration tests
	cargo test -p asset-tap-core --features mock --test mock_server_tests

test-cli-comprehensive: ## Run comprehensive CLI tests in mock mode
	cargo build --release --features mock -p asset-tap
	@echo "Running comprehensive CLI test suite..."
	@./scripts/test_cli.sh

bench: ## Run performance benchmarks (local only)
	cargo bench -p asset-tap-core --bench benchmarks

coverage: ## Run tests with coverage report
ifndef CHECK_LLVM_COV
	@echo "Installing cargo-llvm-cov..."
	@cargo install cargo-llvm-cov
endif
ifndef CHECK_NEXTEST
	@echo "Installing cargo-nextest..."
	@cargo install cargo-nextest --locked
endif
	cargo llvm-cov nextest --workspace --all-features --ignore-filename-regex 'gui/'

coverage-html: ## Generate HTML coverage report
ifndef CHECK_LLVM_COV
	@echo "Installing cargo-llvm-cov..."
	@cargo install cargo-llvm-cov
endif
ifndef CHECK_NEXTEST
	@echo "Installing cargo-nextest..."
	@cargo install cargo-nextest --locked
endif
	cargo llvm-cov nextest --workspace --all-features --ignore-filename-regex 'gui/' --html --open

check: ## Check code (fast compile check)
	cargo check --workspace --all-targets --all-features

lock-check: ## Verify Cargo.lock matches Cargo.toml (catches inert lockfile-only majors)
	@# Without this, a Dependabot major bump under versioning-strategy
	@# lockfile-only rewrites the lock while the manifest still forbids the
	@# new version — cargo silently re-resolves and CI stays green while the
	@# "upgrade" does nothing.
	@cargo metadata --locked --format-version 1 >/dev/null 2>&1 || \
		(echo "Cargo.lock is out of sync with Cargo.toml — run 'cargo update <crate>' and commit." && exit 1)

clippy: ## Run linter (clippy)
	cargo clippy --workspace --all-targets --all-features -- -D warnings

lint-workflows: ## Lint GitHub Actions workflows with actionlint
ifndef CHECK_ACTIONLINT
	@echo "actionlint not found — install: brew install actionlint (or https://github.com/rhysd/actionlint)"
	@exit 1
endif
	actionlint .github/workflows/*.yaml

lint-shell: ## Lint shell scripts with shellcheck (warnings and errors only)
ifndef CHECK_SHELLCHECK
	@echo "shellcheck not found — install: brew install shellcheck"
	@exit 1
endif
	shellcheck -S warning scripts/*.sh

fmt: ## Format all code (Rust + other files)
	cargo fmt --all
ifndef CHECK_DPRINT
	@echo "Installing dprint..."
	@cargo install dprint
endif
	dprint fmt

fmt-check: ## Check formatting (Rust + other files + editorconfig)
	cargo fmt --all -- --check
ifndef CHECK_DPRINT
	@echo "Installing dprint..."
	@cargo install dprint
endif
	dprint check
ifndef CHECK_EC
	@echo "editorconfig-checker not found — skipping (install: https://github.com/editorconfig-checker/editorconfig-checker)"
else
	editorconfig-checker
endif

audit: ## Run security audit
ifndef CHECK_AUDIT
	@echo "Installing cargo-audit..."
	@cargo install cargo-audit
endif
	cargo audit

udeps: ## Check for unused dependencies (requires nightly)
ifndef CHECK_UDEPS
	@echo "Installing cargo-udeps..."
	@cargo install cargo-udeps
endif
	cargo +nightly udeps --workspace --all-features

udeps-ci: ## udeps for `make ci` — skips gracefully if nightly is absent
	@if rustup toolchain list 2>/dev/null | grep -q nightly; then \
		$(MAKE) udeps; \
	else \
		echo "nightly toolchain not installed — skipping udeps (matches CI's nightly job)"; \
	fi

# =============================================================================
# Documentation
# =============================================================================

doc: ## Generate documentation
	RUSTDOCFLAGS="-D warnings --cfg docsrs" cargo doc --workspace --no-deps --all-features

doc-open: ## Generate and open documentation
	RUSTDOCFLAGS="-D warnings --cfg docsrs" cargo doc --workspace --no-deps --all-features --open

# =============================================================================
# Installation
# =============================================================================

install: ## Install CLI and GUI binaries to ~/.cargo/bin
	cargo install --path cli
	cargo install --path gui

# =============================================================================
# Development
# =============================================================================

watch: ## Watch and rebuild on changes
ifndef CHECK_WATCH
	@echo "Installing cargo-watch..."
	@cargo install cargo-watch
endif
	cargo watch -x 'check --workspace'

watch-gui: ## Watch and run GUI on changes
ifndef CHECK_WATCH
	@echo "Installing cargo-watch..."
	@cargo install cargo-watch
endif
	cargo watch -x 'run --bin asset-tap-gui'

# =============================================================================
# Quality Verification
# =============================================================================

clippy-fix: ## Auto-fix clippy warnings where possible
	cargo clippy --workspace --all-targets --all-features --fix --allow-dirty --allow-staged -- -D warnings

verify: fmt clippy-fix lint-workflows lint-shell check test ## Run all quality checks with auto-fixes

ci: lock-check fmt-check clippy lint-workflows lint-shell check doc audit udeps-ci test test-cli-comprehensive site-build ## CI-compatible checks (no modifications)

# =============================================================================
# Utilities
# =============================================================================

clean: ## Clean build artifacts
	cargo clean
	rm -rf dist/

# =============================================================================
# Packaging & Distribution
# =============================================================================

install-packager: ## Install cargo-packager
ifndef CHECK_PACKAGER
	@echo "Installing cargo-packager..."
	@cargo install cargo-packager --locked
else
	@echo "cargo-packager already installed"
endif

package-macos: install-packager build ## Package for macOS (native arch only, fast)
	cd gui && cargo packager --release --formats app
	./scripts/package-macos.sh

package-macos-universal: install-packager ## Package universal macOS binary (arm64 + x86_64)
	./scripts/build-macos-universal.sh

package-windows: install-packager build ## Package for Windows (NSIS installer + CLI archive)
	./scripts/package-windows.sh

package-linux: install-packager build ## Package for Linux (.deb with CLI + AppImage + CLI archive)
	./scripts/package-linux.sh

verify-sign-macos: ## Verify macOS signing, stapling, and Gatekeeper acceptance
	@echo "=== codesign --verify ==="
	codesign --verify --verbose=2 --strict "target/release/Asset Tap.app"
	@echo "=== codesign -dv ==="
	codesign -dv --verbose=4 "target/release/Asset Tap.app" 2>&1 | head -20
	@echo "=== stapler validate ==="
	xcrun stapler validate target/release/AssetTap.dmg
	@echo "=== spctl (Gatekeeper) ==="
	spctl -a -vv -t install target/release/AssetTap.dmg

# =============================================================================
# Website
# =============================================================================
# Zola is PINNED — a system zola (brew ships latest) can be a different major
# with incompatible template syntax (0.23 swapped Tera 1 for Tera 2, which
# breaks the vendored zap theme's macros). The pinned binary is downloaded
# once into site/.bin/ and used by every site target, so local builds match
# the deploy exactly.
# Keep in sync with BOTH pins in .github/workflows/site.yaml
# (taiki-e install-action `zola@…` and the shalzz/zola-deploy-action tag).
ZOLA_VERSION := 0.22.1
ZOLA_BIN := site/.bin/zola-$(ZOLA_VERSION)

UNAME_S := $(shell uname -s)
# uname says arm64 on Apple Silicon; Rust targets (and zola's release asset
# names) say aarch64.
UNAME_M := $(shell uname -m | sed 's/arm64/aarch64/')
ifeq ($(UNAME_S),Darwin)
  ZOLA_TARGET := $(UNAME_M)-apple-darwin
else
  ZOLA_TARGET := $(UNAME_M)-unknown-linux-gnu
endif

$(ZOLA_BIN):
	@echo "Downloading zola $(ZOLA_VERSION) ($(ZOLA_TARGET))..."
	@mkdir -p site/.bin
	@curl -fsSL "https://github.com/getzola/zola/releases/download/v$(ZOLA_VERSION)/zola-v$(ZOLA_VERSION)-$(ZOLA_TARGET).tar.gz" | tar -xz -C site/.bin
	@mv site/.bin/zola $(ZOLA_BIN)

site-serve: $(ZOLA_BIN) ## Serve website locally (hot reload)
	cd site && .bin/zola-$(ZOLA_VERSION) serve

site-build: $(ZOLA_BIN) ## Build website
	cd site && .bin/zola-$(ZOLA_VERSION) build

site-check: $(ZOLA_BIN) ## Check website for broken links
	cd site && .bin/zola-$(ZOLA_VERSION) check
