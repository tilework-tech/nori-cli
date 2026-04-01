#!/usr/bin/env bash
# Integration tests for the root-level justfile shared runner layer.
# These tests validate the standard targets: help, dev, test, doctor.
#
# Usage: bash scripts/test_justfile.sh
#
# Exit codes:
#   0 - all tests passed
#   1 - one or more tests failed

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PASS=0
FAIL=0
FAILURES=""

pass() {
  PASS=$((PASS + 1))
  echo "  PASS: $1"
}

fail() {
  FAIL=$((FAIL + 1))
  FAILURES="${FAILURES}\n  FAIL: $1"
  echo "  FAIL: $1"
}

assert_contains() {
  local output="$1"
  local expected="$2"
  local label="$3"
  if echo "$output" | grep -q "$expected"; then
    pass "$label"
  else
    fail "$label (expected output to contain '$expected')"
  fi
}


echo "=== Shared Runner Layer: justfile integration tests ==="
echo "Repo root: $REPO_ROOT"
echo ""

# ---- just help ----
echo "[just help]"
HELP_OUTPUT="$(cd "$REPO_ROOT" && just help 2>&1)" || true
assert_contains "$HELP_OUTPUT" "nori" "help mentions nori"
assert_contains "$HELP_OUTPUT" "just dev" "help mentions just dev"
assert_contains "$HELP_OUTPUT" "just test" "help mentions just test"
assert_contains "$HELP_OUTPUT" "just doctor" "help mentions just doctor"
assert_contains "$HELP_OUTPUT" "Standard targets" "help has Standard targets section"
echo ""

# ---- just doctor ----
echo "[just doctor]"
DOCTOR_OUTPUT="$(cd "$REPO_ROOT" && just doctor 2>&1)" || true
assert_contains "$DOCTOR_OUTPUT" "cargo" "doctor checks cargo"
assert_contains "$DOCTOR_OUTPUT" "rustup" "doctor checks rustup"
assert_contains "$DOCTOR_OUTPUT" "just" "doctor checks just"
echo ""

# ---- just dev (dry-run check) ----
echo "[just dev]"
# We can't actually run the full dev server, but we can check that the recipe exists
# by running just --summary and verifying dev is listed
SUMMARY="$(cd "$REPO_ROOT" && just --summary 2>&1)" || true
assert_contains "$SUMMARY" "dev" "dev target exists in justfile"
assert_contains "$SUMMARY" "test" "test target exists in justfile"
assert_contains "$SUMMARY" "help" "help target exists in justfile"
assert_contains "$SUMMARY" "doctor" "doctor target exists in justfile"
echo ""

# ---- just test (dry-run check) ----
echo "[just test - target exists]"
# Verify test target exists and supports subtargets by checking --show
TEST_SHOW="$(cd "$REPO_ROOT" && just --show test 2>&1)" || true
assert_contains "$TEST_SHOW" "cargo test" "test recipe delegates to cargo test"
echo ""

# ---- Forwarded targets ----
echo "[forwarded targets]"
assert_contains "$SUMMARY" "fmt" "fmt target is forwarded"
assert_contains "$SUMMARY" "clippy" "clippy target is forwarded"
assert_contains "$SUMMARY" "nextest" "nextest target is forwarded"
echo ""

# ---- Summary ----
echo "=== Results ==="
echo "  Passed: $PASS"
echo "  Failed: $FAIL"
if [ "$FAIL" -gt 0 ]; then
  echo ""
  echo "Failures:"
  echo -e "$FAILURES"
  exit 1
fi

echo ""
echo "All tests passed."
