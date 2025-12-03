#!/usr/bin/env bash
# Test script for workflow sanitization logic
# RED phase: This test will fail until we implement the sanitize function

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TEST_DIR="$SCRIPT_DIR/fixtures"
TEMP_DIR="$(mktemp -d)"

trap "rm -rf $TEMP_DIR" EXIT

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
NC='\033[0m' # No Color

failed=0
passed=0

# The sanitize function
sanitize_workflow() {
  local input_file="$1"
  local output_file="$2"

  # Replace on: block with workflow_dispatch only using awk
  # Logic:
  # 1. When we see ^on: (at column 0), enter "in_on" mode, print replacement
  # 2. While in_on, skip lines starting with whitespace (indented on: content)
  # 3. When we see a non-whitespace line at column 0, exit in_on mode
  # 4. Print all other lines normally
  awk '
    /^on:/ {
      in_on = 1
      print "on: workflow_dispatch"
      next
    }
    in_on && /^$/ {
      # Empty line ends the on: block
      in_on = 0
      print
      next
    }
    in_on && /^[^ \t#]/ {
      # Non-indented line ends the on: block
      in_on = 0
    }
    in_on && /^[ \t]/ {
      # Indented content - skip
      next
    }
    in_on && /^#/ {
      # Comment inside on: block - skip
      next
    }
    !in_on { print }
  ' "$input_file" > "$output_file"
}

# Test helper
assert_output() {
  local test_name="$1"
  local input_file="$2"
  local expected_file="$3"

  local actual_file="$TEMP_DIR/actual.yml"
  sanitize_workflow "$input_file" "$actual_file"

  if diff -q "$expected_file" "$actual_file" > /dev/null 2>&1; then
    echo -e "${GREEN}✓ PASS${NC}: $test_name"
    passed=$((passed + 1))
  else
    echo -e "${RED}✗ FAIL${NC}: $test_name"
    echo "  Expected:"
    sed 's/^/    /' "$expected_file"
    echo "  Actual:"
    sed 's/^/    /' "$actual_file"
    echo "  Diff:"
    diff "$expected_file" "$actual_file" | sed 's/^/    /' || true
    failed=$((failed + 1))
  fi
}

echo "Running workflow sanitization tests..."
echo

# Test 1: Multi-line on: block
assert_output "Multi-line on: block" \
  "$TEST_DIR/input-multiline.yml" \
  "$TEST_DIR/expected-multiline.yml"

# Test 2: Single-line on: block
assert_output "Single-line on: block" \
  "$TEST_DIR/input-singleline.yml" \
  "$TEST_DIR/expected-singleline.yml"

# Test 3: Complex on: block with nested structures
assert_output "Complex on: block" \
  "$TEST_DIR/input-complex.yml" \
  "$TEST_DIR/expected-complex.yml"

# Test 4: on: block with comments
assert_output "on: block with comments" \
  "$TEST_DIR/input-comments.yml" \
  "$TEST_DIR/expected-comments.yml"

echo
echo "Results: $passed passed, $failed failed"

if [[ $failed -gt 0 ]]; then
  exit 1
fi
