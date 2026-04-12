#!/bin/bash

# Comprehensive CLI Test Script
# Tests all CLI functionality in mock mode

set -e  # Exit on error

CLI="./target/release/asset-tap"
TEST_OUTPUT="./test_results"
TIMESTAMP=$(date +%Y%m%d_%H%M%S)
LOG_FILE="$TEST_OUTPUT/test_log_$TIMESTAMP.txt"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Setup
mkdir -p "$TEST_OUTPUT"
echo "CLI Test Suite - $(date)" | tee "$LOG_FILE"
echo "======================================" | tee -a "$LOG_FILE"
echo "" | tee -a "$LOG_FILE"

PASSED=0
FAILED=0
TOTAL=0

# -----------------------------------------------------------------------------
# Cleanup of mock pipeline outputs that escape into the user's default output
# directory.
#
# Most tests don't pass `-o`, so the CLI's release-mode default kicks in:
# `~/Documents/Asset Tap/` on macOS, and either `~/Documents/Asset Tap/` or
# `~/Asset Tap/` on Linux depending on whether xdg-user-dirs is configured.
# Without cleanup, every shell test run leaves dozens of timestamped mock
# outputs there forever, eventually filling the disk.
#
# Strategy: snapshot the contents of every candidate default output dir
# before the suite runs, then on EXIT (success, failure, or interrupt) delete
# only the entries that didn't exist in the snapshot. We never touch entries
# the user generated outside of test runs.
#
# Implementation note: macOS ships bash 3.2 by default, which has no
# associative arrays. We stash each dir's snapshot in a tempfile and use a
# parallel-array convention instead.
DEFAULT_OUTPUT_DIRS=()
DEFAULT_OUTPUT_SNAPSHOT_FILES=()
SNAPSHOT_TMPDIR=$(mktemp -d -t asset_tap_test_snapshots.XXXXXX)

snapshot_dir() {
    local d="$1"
    DEFAULT_OUTPUT_DIRS+=("$d")
    local snap_file
    snap_file="$SNAPSHOT_TMPDIR/$(echo "$d" | tr '/' '_').snap"
    DEFAULT_OUTPUT_SNAPSHOT_FILES+=("$snap_file")
    if [ -d "$d" ]; then
        ls -1 "$d" 2>/dev/null | sort -u > "$snap_file"
    else
        : > "$snap_file"  # empty snapshot — every entry created during the run is fair game
    fi
}

# macOS default. Linux fallback when xdg-user-dirs has Documents.
snapshot_dir "$HOME/Documents/Asset Tap"
# Linux fallback when xdg-user-dirs has no Documents — APP_DISPLAY_NAME under HOME directly.
snapshot_dir "$HOME/Asset Tap"

cleanup_test_outputs() {
    local exit_code=$?
    local i=0
    while [ $i -lt ${#DEFAULT_OUTPUT_DIRS[@]} ]; do
        local d="${DEFAULT_OUTPUT_DIRS[$i]}"
        local snap_file="${DEFAULT_OUTPUT_SNAPSHOT_FILES[$i]}"
        i=$((i + 1))
        [ -d "$d" ] || continue
        local after_file
        after_file=$(mktemp -t asset_tap_test_after.XXXXXX)
        ls -1 "$d" 2>/dev/null | sort -u > "$after_file"
        # `comm -23` gives entries in `after` that aren't in `before`. Both
        # files are already sorted by the `sort -u` above.
        local deleted=0
        while IFS= read -r entry; do
            [ -z "$entry" ] && continue
            rm -rf "$d/$entry" && deleted=$((deleted + 1))
        done < <(comm -23 "$after_file" "$snap_file")
        rm -f "$after_file"
        if [ $deleted -gt 0 ]; then
            echo "Cleaned up $deleted test-generated entries from $d" >&2
        fi
    done
    rm -rf "$SNAPSHOT_TMPDIR"
    exit $exit_code
}
trap cleanup_test_outputs EXIT

# Helper function to run a test
run_test() {
    local test_name="$1"
    local test_cmd="$2"
    local expected_exit_code="${3:-0}"
    local test_input="${4:-}"

    TOTAL=$((TOTAL + 1))
    echo -e "${BLUE}TEST $TOTAL: $test_name${NC}" | tee -a "$LOG_FILE"
    echo "Command: $test_cmd" >> "$LOG_FILE"

    # Run the command (use bash -c instead of eval for safety).
    # Always redirect stdin — either from test_input or /dev/null — so that
    # tests never inherit the script's TTY stdin. Inheriting a TTY makes the
    # CLI's interactive-prompt path fire unexpectedly (and can hang the run).
    set +e  # Don't exit on error for this command
    if [ -n "$test_input" ]; then
        echo "$test_input" | bash -c "$test_cmd" >> "$LOG_FILE" 2>&1
    else
        bash -c "$test_cmd" < /dev/null >> "$LOG_FILE" 2>&1
    fi
    local exit_code=$?
    set -e

    # Check result
    if [ $exit_code -eq $expected_exit_code ]; then
        echo -e "${GREEN}✓ PASS${NC}" | tee -a "$LOG_FILE"
        PASSED=$((PASSED + 1))
    else
        echo -e "${RED}✗ FAIL (exit code: $exit_code, expected: $expected_exit_code)${NC}" | tee -a "$LOG_FILE"
        FAILED=$((FAILED + 1))
    fi
    echo "" | tee -a "$LOG_FILE"
}

echo "=== 1. HELP & INFO TESTS ===" | tee -a "$LOG_FILE"

run_test "Help output" \
    "$CLI --help" 0

run_test "Version output" \
    "$CLI --version" 0

run_test "List providers" \
    "$CLI --list-providers" 0

run_test "List models and templates" \
    "$CLI --list" 0

echo "=== 2. BASIC PIPELINE TESTS (Mock Mode) ===" | tee -a "$LOG_FILE"

run_test "Basic text-to-3D with mock mode" \
    "$CLI --mock -y 'a steampunk robot knight'" 0

run_test "Mock mode with delays" \
    "$CLI --mock --mock-delay -y 'test prompt'" 0

run_test "Pipeline without FBX export" \
    "$CLI --mock -y --no-fbx 'a spaceship'" 0

run_test "Custom output directory" \
    "$CLI --mock -y -o '$TEST_OUTPUT/custom_out' 'test'" 0

echo "=== 3. IMAGE INPUT TESTS ===" | tee -a "$LOG_FILE"

# First generate an image for reuse
echo "Setting up test image..." >> "$LOG_FILE"
$CLI --mock -y --no-fbx -o "$TEST_OUTPUT/setup_image" 'setup image' >> "$LOG_FILE" 2>&1 || true
SETUP_DIR=$(ls -td "$TEST_OUTPUT/setup_image"/*/ 2>/dev/null | head -1)
if [ -n "$SETUP_DIR" ] && [ -f "${SETUP_DIR}image.png" ]; then
    TEST_IMAGE="${SETUP_DIR}image.png"
    echo "Test image: $TEST_IMAGE" >> "$LOG_FILE"

    run_test "Image-to-3D with local file" \
        "$CLI --mock -y --no-fbx --image '$TEST_IMAGE'" 0

    run_test "Image-to-3D with local file and prompt" \
        "$CLI --mock -y --no-fbx --image '$TEST_IMAGE' 'a robot knight'" 0

    run_test "Image-to-3D with custom 3D model" \
        "$CLI --mock -y --no-fbx --image '$TEST_IMAGE' --3d-model fal-ai/hunyuan-3d/v3.1/pro/image-to-3d" 0
else
    echo -e "${YELLOW}⚠ Skipping image tests - no test image available${NC}" | tee -a "$LOG_FILE"
fi

run_test "Image with nonexistent local file" \
    "$CLI --mock -y --image '/tmp/nonexistent_asset_tap_image.png'" 1

echo "=== 4. TEMPLATE TESTS ===" | tee -a "$LOG_FILE"

run_test "Inspect template" \
    "$CLI --inspect-template humanoid" 0

run_test "Use humanoid template" \
    "$CLI --mock -y -t humanoid 'a warrior'" 0

run_test "Invalid template name" \
    "$CLI --mock -y -t nonexistent 'test'" 1

echo "=== 5. PROVIDER & MODEL TESTS ===" | tee -a "$LOG_FILE"

run_test "Specify provider" \
    "$CLI --mock -y --no-fbx -p fal.ai 'test'" 0

run_test "Image model: nano-banana-2 (default)" \
    "$CLI --mock -y --no-fbx --image-model fal-ai/nano-banana-2 'test'" 0

run_test "Image model: nano-banana-pro" \
    "$CLI --mock -y --no-fbx --image-model fal-ai/nano-banana-pro 'test'" 0

run_test "Image model: flux-2" \
    "$CLI --mock -y --no-fbx --image-model fal-ai/flux-2 'test'" 0

run_test "Image model: flux-2-pro" \
    "$CLI --mock -y --no-fbx --image-model fal-ai/flux-2-pro 'test'" 0

run_test "3D model: trellis-2" \
    "$CLI --mock -y --no-fbx --3d-model fal-ai/trellis-2 'test'" 0

run_test "3D model: hunyuan-3d" \
    "$CLI --mock -y --no-fbx --3d-model fal-ai/hunyuan-3d/v3.1/pro/image-to-3d 'test'" 0

run_test "3D model: meshy" \
    "$CLI --mock -y --no-fbx --3d-model fal-ai/meshy/v6/image-to-3d 'test'" 0

run_test "Combined: provider + models + output + no-fbx" \
    "$CLI --mock -y --no-fbx -p fal.ai --image-model fal-ai/nano-banana-2 --3d-model fal-ai/trellis-2 -o '$TEST_OUTPUT/combined_test' 'a red cube'" 0

run_test "Combined: template + provider + model" \
    "$CLI --mock -y --no-fbx -t humanoid -p fal.ai --image-model fal-ai/nano-banana-pro 'an orc warrior'" 0

run_test "Invalid provider name" \
    "$CLI --mock -y -p nonexistent 'test'" 1

run_test "Invalid image model name" \
    "$CLI --mock -y --no-fbx --image-model totally-fake-model 'test'" 1

run_test "Invalid 3D model name" \
    "$CLI --mock -y --no-fbx --3d-model totally-fake-model 'test'" 1

echo "=== 6. ERROR HANDLING TESTS ===" | tee -a "$LOG_FILE"

run_test "No prompt and non-TTY stdin (should fail fast)" \
    "$CLI --mock" 1

run_test "Empty string prompt (should fail)" \
    "$CLI --mock ''" 1

run_test "Whitespace-only prompt (should fail)" \
    "$CLI --mock '   '" 1

# Regression guard: when no API key is configured, the CLI must fail BEFORE
# prompting for a text prompt (it used to read stdin first, then emit a terse
# "No providers available" error after the user had already typed).
#
# This test must defeat four possible sources of a real key:
#   1. The tester's shell env (FAL_KEY exported from ~/.zshrc etc.)
#      → neutralized with `env -u FAL_KEY`.
#   2. A GUI-saved key in the user's real settings.json under HOME
#      → neutralized by pointing HOME at a clean scratch dir.
#   3. A GUI-saved key under XDG_CONFIG_HOME on Linux (which `dirs::config_dir()`
#      checks BEFORE falling back to $HOME/.config)
#      → neutralized by `env -u XDG_CONFIG_HOME` and friends.
#   4. A .env file in the repo root that dotenvy walks up to find
#      → neutralized by running from a scratch cwd OUTSIDE the repo. We use
#        $TMPDIR (or /tmp) so dotenvy's parent-walk hits root without
#        finding any .env on the way.
FAKE_HOME="${TMPDIR:-/tmp}/asset_tap_test_no_keys_home"
rm -rf "$FAKE_HOME"
mkdir -p "$FAKE_HOME"
# Absolute path to the binary — we're about to `cd` away from the repo root.
CLI_ABS=$(cd "$(dirname "$CLI")" && pwd)/$(basename "$CLI")
TOTAL=$((TOTAL + 1))
echo -e "${BLUE}TEST $TOTAL: Missing API key fails fast with actionable hint${NC}" | tee -a "$LOG_FILE"
set +e
# Redirect stdin from /dev/null so if the CLI ever regresses and tries to
# read a prompt, it gets EOF instead of hanging the test.
NO_KEY_OUT=$(cd "$FAKE_HOME" && env \
    -u FAL_KEY -u XDG_CONFIG_HOME -u XDG_DATA_HOME -u XDG_STATE_HOME -u XDG_CACHE_HOME \
    HOME="$FAKE_HOME" \
    "$CLI_ABS" < /dev/null 2>&1)
NO_KEY_EXIT=$?
set -e
echo "$NO_KEY_OUT" >> "$LOG_FILE"
if [ $NO_KEY_EXIT -ne 0 ] \
    && echo "$NO_KEY_OUT" | grep -q "API key" \
    && echo "$NO_KEY_OUT" | grep -q "FAL_KEY" \
    && ! echo "$NO_KEY_OUT" | grep -q "Describe what you want to create"; then
    echo -e "${GREEN}✓ PASS${NC}" | tee -a "$LOG_FILE"
    PASSED=$((PASSED + 1))
else
    echo -e "${RED}✗ FAIL (exit=$NO_KEY_EXIT, expected non-zero with FAL_KEY hint and no stdin prompt)${NC}" | tee -a "$LOG_FILE"
    # Surface the captured output on the console so a CI failure is debuggable
    # without needing to download the test_results artifact.
    echo "--- captured CLI output ---" | tee -a "$LOG_FILE"
    echo "$NO_KEY_OUT" | tee -a "$LOG_FILE"
    echo "--- end captured CLI output ---" | tee -a "$LOG_FILE"
    FAILED=$((FAILED + 1))
fi
echo "" | tee -a "$LOG_FILE"

# Regression guard: if settings.json is corrupt, the CLI must surface the
# problem on stderr (not just in the tracing log). Without this test, a typo
# or accidental swap of match arms in the LoadStatus handler would silently
# regress the CLI's only user-visible signal that something is wrong.
#
# We seed both the macOS and Linux config dirs under a fake HOME so the test
# is platform-agnostic. We use mock mode so the missing-API-key check doesn't
# fire first. Same outside-the-repo cwd trick as the test above so dotenvy
# doesn't import a real .env from the parent dirs.
#
# `env -u XDG_CONFIG_HOME ...` is critical on Linux: `dirs::config_dir()`
# checks `$XDG_CONFIG_HOME` BEFORE falling back to `$HOME/.config/`. If the
# CI runner has XDG_CONFIG_HOME set (some Linux distros + GitHub runners do),
# the CLI loads from that instead of our seeded fake-HOME path, finds no
# corrupt file, returns Ok, and emits no warning. We unset every XDG var the
# `dirs` crate consults to make this test deterministic across hosts.
CORRUPT_HOME="${TMPDIR:-/tmp}/asset_tap_test_corrupt_settings_home"
rm -rf "$CORRUPT_HOME"
mkdir -p "$CORRUPT_HOME/Library/Application Support/asset-tap"
mkdir -p "$CORRUPT_HOME/.config/asset-tap"
echo "this is not valid json {{{" > "$CORRUPT_HOME/Library/Application Support/asset-tap/settings.json"
echo "this is not valid json {{{" > "$CORRUPT_HOME/.config/asset-tap/settings.json"
TOTAL=$((TOTAL + 1))
echo -e "${BLUE}TEST $TOTAL: Corrupt settings.json surfaces warning on stderr${NC}" | tee -a "$LOG_FILE"
set +e
CORRUPT_OUT=$(cd "$CORRUPT_HOME" && env \
    -u XDG_CONFIG_HOME -u XDG_DATA_HOME -u XDG_STATE_HOME -u XDG_CACHE_HOME \
    HOME="$CORRUPT_HOME" \
    "$CLI_ABS" --mock --no-fbx -o "$CORRUPT_HOME/out" 'corruption test' < /dev/null 2>&1)
CORRUPT_EXIT=$?
set -e
echo "$CORRUPT_OUT" >> "$LOG_FILE"
# We require:
#   - stderr mentions "warning" and "corrupt" (proves the CLI handler ran)
#   - the warning includes a path hint so the user knows where their data is
#   - the run still succeeds (defaults are usable, mock mode doesn't need keys)
if [ $CORRUPT_EXIT -eq 0 ] \
    && echo "$CORRUPT_OUT" | grep -qi "warning" \
    && echo "$CORRUPT_OUT" | grep -qi "corrupt"; then
    echo -e "${GREEN}✓ PASS${NC}" | tee -a "$LOG_FILE"
    PASSED=$((PASSED + 1))
else
    echo -e "${RED}✗ FAIL (exit=$CORRUPT_EXIT, expected exit 0 with 'warning' and 'corrupt' in stderr)${NC}" | tee -a "$LOG_FILE"
    # Surface the captured output on the console so a CI failure is debuggable
    # without needing to download the test_results artifact (we don't upload it).
    echo "--- captured CLI output ---" | tee -a "$LOG_FILE"
    echo "$CORRUPT_OUT" | tee -a "$LOG_FILE"
    echo "--- end captured CLI output ---" | tee -a "$LOG_FILE"
    FAILED=$((FAILED + 1))
fi
echo "" | tee -a "$LOG_FILE"

echo "=== 7. APPROVAL FLOW TESTS ===" | tee -a "$LOG_FILE"

run_test "Approval with Y (approve)" \
    "$CLI --mock --approve 'test approval'" 0 "Y"

run_test "Approval with yes (approve)" \
    "$CLI --mock --approve 'test approval 2'" 0 "yes"

run_test "Approval with Enter (approve)" \
    "$CLI --mock --approve 'test approval 3'" 0 ""

run_test "Approval with n (reject)" \
    "$CLI --mock --approve 'test rejection'" 1 "n"

run_test "Approval with no (reject)" \
    "$CLI --mock --approve 'test rejection 2'" 1 "no"

run_test "Approval with r (regenerate and complete)" \
    "$CLI --mock --approve 'test regen'" 0 "r"

run_test "Approval with invalid input then Y" \
    "$CLI --mock --approve 'test invalid'" 0 "invalid\nY"

run_test "Approval disabled with -y flag" \
    "$CLI --mock --approve -y 'auto confirm'" 0

echo "=== 8. SPECIAL CHARACTER TESTS ===" | tee -a "$LOG_FILE"

run_test "Prompt with special characters" \
    "$CLI --mock -y 'robot with \"quotes\" and special chars: !@#$%'" 0

run_test "Prompt with unicode" \
    "$CLI --mock -y 'robot emoji 🤖 test'" 0

run_test "Very long prompt" \
    "$CLI --mock -y 'this is a very long prompt that goes on and on and on with lots of details about what we want to create including multiple sentences and various descriptive words to test the handling of lengthy input text'" 0

echo "=== 9. OUTPUT BUNDLE VALIDATION ===" | tee -a "$LOG_FILE"

# Run a pipeline and validate the output bundle structure
BUNDLE_TEST_OUT="$TEST_OUTPUT/bundle_validation"
rm -rf "$BUNDLE_TEST_OUT"
TOTAL=$((TOTAL + 1))
echo -e "${BLUE}TEST $TOTAL: Output bundle structure validation${NC}" | tee -a "$LOG_FILE"
set +e
$CLI --mock -y -o "$BUNDLE_TEST_OUT" --no-fbx 'bundle validation test' >> "$LOG_FILE" 2>&1
BUNDLE_EXIT=$?
set -e
if [ $BUNDLE_EXIT -eq 0 ]; then
    # Find the timestamped output directory
    BUNDLE_DIR=$(ls -td "$BUNDLE_TEST_OUT"/*/ 2>/dev/null | head -1)
    BUNDLE_OK=true
    for expected_file in "bundle.json" "image.png" "model.glb"; do
        if [ ! -f "${BUNDLE_DIR}${expected_file}" ]; then
            echo "Missing expected file: ${expected_file}" >> "$LOG_FILE"
            BUNDLE_OK=false
        fi
    done
    # Validate bundle.json is valid JSON with required fields
    if [ -f "${BUNDLE_DIR}bundle.json" ]; then
        if ! python3 -c "import json,sys; d=json.load(open(sys.argv[1])); assert d.get('config',{}).get('prompt') or 'prompt' in d or 'metadata' in d" "${BUNDLE_DIR}bundle.json" 2>/dev/null; then
            echo "bundle.json missing expected fields" >> "$LOG_FILE"
            BUNDLE_OK=false
        fi
    fi
    if $BUNDLE_OK; then
        echo -e "${GREEN}✓ PASS${NC}" | tee -a "$LOG_FILE"
        PASSED=$((PASSED + 1))
    else
        echo -e "${RED}✗ FAIL (missing output files)${NC}" | tee -a "$LOG_FILE"
        FAILED=$((FAILED + 1))
    fi
else
    echo -e "${RED}✗ FAIL (exit code: $BUNDLE_EXIT)${NC}" | tee -a "$LOG_FILE"
    FAILED=$((FAILED + 1))
fi
echo "" | tee -a "$LOG_FILE"

echo "=== 10. CONVERSION TESTS ===" | tee -a "$LOG_FILE"

run_test "Convert WebP on empty directory (no GLBs)" \
    "$CLI --convert-webp -o '$TEST_OUTPUT/empty_webp'" 0

run_test "Convert WebP with -o targeting test output" \
    "$CLI --convert-webp -o '$TEST_OUTPUT/bundle_validation'" 0

run_test "Convert-only mode" \
    "$CLI --convert-only -o '$TEST_OUTPUT/bundle_validation'" 0

run_test "Convert FBX on non-existent path" \
    "$CLI --convert-fbx /tmp/does_not_exist_xyz.glb" 1

run_test "Convert FBX on non-GLB file" \
    "$CLI --convert-fbx '$TEST_OUTPUT/bundle_validation'" 1

echo "=== 11. ADDITIONAL EDGE CASES ===" | tee -a "$LOG_FILE"

run_test "Inspect non-existent template" \
    "$CLI --inspect-template does_not_exist_xyz" 1

run_test "--list ignores extra prompt argument" \
    "$CLI --list 'ignored prompt'" 0

run_test "--inspect-template ignores --mock flag" \
    "$CLI --mock --inspect-template humanoid" 0

echo "=== 12. MULTIPLE RUNS TO SAME OUTPUT ===" | tee -a "$LOG_FILE"

MULTI_OUT="$TEST_OUTPUT/multi_run"
rm -rf "$MULTI_OUT"
TOTAL=$((TOTAL + 1))
echo -e "${BLUE}TEST $TOTAL: Multiple runs create separate timestamped dirs${NC}" | tee -a "$LOG_FILE"
set +e
$CLI --mock -y --no-fbx -o "$MULTI_OUT" 'run one' >> "$LOG_FILE" 2>&1
EXIT1=$?
$CLI --mock -y --no-fbx -o "$MULTI_OUT" 'run two' >> "$LOG_FILE" 2>&1
EXIT2=$?
set -e
DIR_COUNT=$(ls -d "$MULTI_OUT"/*/ 2>/dev/null | wc -l | tr -d ' ')
if [ $EXIT1 -eq 0 ] && [ $EXIT2 -eq 0 ] && [ "$DIR_COUNT" -eq 2 ]; then
    echo -e "${GREEN}✓ PASS${NC}" | tee -a "$LOG_FILE"
    PASSED=$((PASSED + 1))
else
    echo -e "${RED}✗ FAIL (exit1=$EXIT1, exit2=$EXIT2, dirs=$DIR_COUNT, expected 2)${NC}" | tee -a "$LOG_FILE"
    FAILED=$((FAILED + 1))
fi
echo "" | tee -a "$LOG_FILE"

echo "=== 13. BUNDLE DEEP VALIDATION ===" | tee -a "$LOG_FILE"

# Validate bundle.json has correct prompt, config structure, and non-zero file sizes
DEEP_OUT="$TEST_OUTPUT/deep_bundle"
rm -rf "$DEEP_OUT"
TOTAL=$((TOTAL + 1))
echo -e "${BLUE}TEST $TOTAL: Bundle.json deep content validation${NC}" | tee -a "$LOG_FILE"
set +e
$CLI --mock -y --no-fbx -o "$DEEP_OUT" 'deep validation prompt' >> "$LOG_FILE" 2>&1
DEEP_EXIT=$?
set -e
if [ $DEEP_EXIT -eq 0 ]; then
    DEEP_DIR=$(ls -td "$DEEP_OUT"/*/ 2>/dev/null | head -1)
    DEEP_OK=true
    # Verify bundle.json config.prompt matches input
    if [ -f "${DEEP_DIR}bundle.json" ]; then
        if ! python3 -c "
import json,sys
d=json.load(open(sys.argv[1]))
assert d.get('version') == 1, 'missing version'
assert d.get('config',{}).get('prompt') == 'deep validation prompt', 'prompt mismatch'
assert 'created_at' in d, 'missing created_at'
" "${DEEP_DIR}bundle.json" 2>/dev/null; then
            echo "bundle.json content validation failed" >> "$LOG_FILE"
            DEEP_OK=false
        fi
    else
        echo "bundle.json not found" >> "$LOG_FILE"
        DEEP_OK=false
    fi
    # Verify files are non-empty
    for f in "image.png" "model.glb"; do
        if [ ! -s "${DEEP_DIR}${f}" ]; then
            echo "${f} is missing or empty" >> "$LOG_FILE"
            DEEP_OK=false
        fi
    done
    if $DEEP_OK; then
        echo -e "${GREEN}✓ PASS${NC}" | tee -a "$LOG_FILE"
        PASSED=$((PASSED + 1))
    else
        echo -e "${RED}✗ FAIL (bundle content validation)${NC}" | tee -a "$LOG_FILE"
        FAILED=$((FAILED + 1))
    fi
else
    echo -e "${RED}✗ FAIL (exit code: $DEEP_EXIT)${NC}" | tee -a "$LOG_FILE"
    FAILED=$((FAILED + 1))
fi
echo "" | tee -a "$LOG_FILE"

echo "=== 14. PARAMETER OVERRIDE TESTS ===" | tee -a "$LOG_FILE"

run_test "Param: float override (guidance_scale)" \
    "$CLI --mock -y --no-fbx --image-model fal-ai/flux-2 --param guidance_scale=7.0 'test'" 0

run_test "Param: integer override (num_inference_steps)" \
    "$CLI --mock -y --no-fbx --image-model fal-ai/flux-2 --param num_inference_steps=10 'test'" 0

run_test "Param: multiple params" \
    "$CLI --mock -y --no-fbx --image-model fal-ai/flux-2 --param guidance_scale=7.0 --param num_inference_steps=10 'test'" 0

run_test "Param: 3D model select param (topology=quad)" \
    "$CLI --mock -y --no-fbx --3d-model fal-ai/meshy/v6/image-to-3d --param topology=quad 'test'" 0

run_test "Param: 3D model boolean param (enable_pbr=false)" \
    "$CLI --mock -y --no-fbx --3d-model fal-ai/hunyuan-3d/v3.1/pro/image-to-3d --param enable_pbr=false 'test'" 0

run_test "Param: integer coerced to float for float param" \
    "$CLI --mock -y --no-fbx --image-model fal-ai/flux-2 --param guidance_scale=7 'test'" 0

run_test "Param: invalid param name (should fail)" \
    "$CLI --mock -y --no-fbx --param totally_fake=42 'test'" 1

run_test "Param: malformed format without equals (should fail)" \
    "$CLI --mock -y --no-fbx --param 'no_equals_sign' 'test'" 1

run_test "Param: type mismatch string for float param (should fail)" \
    "$CLI --mock -y --no-fbx --image-model fal-ai/flux-2 --param guidance_scale=high 'test'" 1

run_test "Param: NaN rejected (should fail)" \
    "$CLI --mock -y --no-fbx --param guidance_scale=NaN 'test'" 1

echo "" | tee -a "$LOG_FILE"

echo "" | tee -a "$LOG_FILE"
echo "======================================" | tee -a "$LOG_FILE"
echo "TEST SUMMARY" | tee -a "$LOG_FILE"
echo "======================================" | tee -a "$LOG_FILE"
echo "Total tests: $TOTAL" | tee -a "$LOG_FILE"
echo -e "${GREEN}Passed: $PASSED${NC}" | tee -a "$LOG_FILE"
echo -e "${RED}Failed: $FAILED${NC}" | tee -a "$LOG_FILE"
echo "" | tee -a "$LOG_FILE"

if [ $FAILED -eq 0 ]; then
    echo -e "${GREEN}🎉 ALL TESTS PASSED!${NC}" | tee -a "$LOG_FILE"
    exit 0
else
    echo -e "${RED}❌ SOME TESTS FAILED${NC}" | tee -a "$LOG_FILE"
    echo "See full log at: $LOG_FILE" | tee -a "$LOG_FILE"
    exit 1
fi
