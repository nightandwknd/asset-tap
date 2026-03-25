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

# Helper function to run a test
run_test() {
    local test_name="$1"
    local test_cmd="$2"
    local expected_exit_code="${3:-0}"
    local test_input="${4:-}"

    TOTAL=$((TOTAL + 1))
    echo -e "${BLUE}TEST $TOTAL: $test_name${NC}" | tee -a "$LOG_FILE"
    echo "Command: $test_cmd" >> "$LOG_FILE"

    # Run the command (use bash -c instead of eval for safety)
    set +e  # Don't exit on error for this command
    if [ -n "$test_input" ]; then
        echo "$test_input" | bash -c "$test_cmd" >> "$LOG_FILE" 2>&1
    else
        bash -c "$test_cmd" >> "$LOG_FILE" 2>&1
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

    # Small delay between tests
    sleep 0.5
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

run_test "Empty prompt without image (should fail)" \
    "$CLI --mock -y" 1

run_test "Empty string prompt (should fail)" \
    "$CLI --mock -y ''" 1

run_test "Whitespace-only prompt (should fail)" \
    "$CLI --mock -y '   '" 1

run_test "Empty prompt without image in non-interactive mode" \
    "echo '' | $CLI --mock" 1

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
