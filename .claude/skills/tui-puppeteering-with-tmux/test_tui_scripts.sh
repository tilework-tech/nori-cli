#!/usr/bin/env bash
# test_tui_scripts.sh: Verify TUI puppeteering scripts work correctly
#
# Run: bash test_tui_scripts.sh
#
# Prerequisites: tmux installed

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SESSION="test-tui-$$"
PASS=0
FAIL=0

cleanup() {
    "$SCRIPT_DIR/tui-stop" "$SESSION" 2>/dev/null || true
    rm -f "${SESSION}_failure.log" /tmp/tui-capture-test.txt
}
trap cleanup EXIT

pass() {
    echo "  ✓ $1"
    ((PASS++))
}

fail() {
    echo "  ✗ $1"
    ((FAIL++))
}

echo "=== TUI Puppeteering Test Suite ==="
echo

# --- tmux-isolated ---
echo "Testing tmux-isolated..."

if "$SCRIPT_DIR/tmux-isolated" -V >/dev/null 2>&1; then
    pass "tmux-isolated runs tmux"
else
    fail "tmux-isolated failed to run"
fi

# --- tui-start ---
echo "Testing tui-start..."

if output=$("$SCRIPT_DIR/tui-start" "$SESSION" "bash" 2>&1); then
    pass "tui-start creates session"
else
    fail "tui-start failed: $output"
fi

if "$SCRIPT_DIR/tmux-isolated" has-session -t "$SESSION" 2>/dev/null; then
    pass "session exists after tui-start"
else
    fail "session not found after tui-start"
fi

# --- tui-capture ---
echo "Testing tui-capture..."

sleep 0.5  # Let bash prompt render

if output=$("$SCRIPT_DIR/tui-capture" "$SESSION" 2>&1); then
    pass "tui-capture returns content"
else
    fail "tui-capture failed: $output"
fi

# --- tui-send (literal text) ---
echo "Testing tui-send..."

if "$SCRIPT_DIR/tui-send" "$SESSION" "echo TESTMARKER123" 2>&1; then
    pass "tui-send accepts text"
else
    fail "tui-send text failed"
fi

# --- tui-send (keys) ---
if "$SCRIPT_DIR/tui-send" "$SESSION" --keys "Enter" 2>&1; then
    pass "tui-send --keys works"
else
    fail "tui-send --keys failed"
fi

sleep 0.3

# --- tui-assert (success case) ---
echo "Testing tui-assert..."

if "$SCRIPT_DIR/tui-assert" "$SESSION" "TESTMARKER123" 3 2>&1; then
    pass "tui-assert finds text"
else
    fail "tui-assert did not find expected text"
fi

# --- tui-assert (timeout case) ---
if ! "$SCRIPT_DIR/tui-assert" "$SESSION" "NONEXISTENT_xyz_999" 1 2>/dev/null; then
    pass "tui-assert times out on missing text"
else
    fail "tui-assert should have timed out"
fi

if [[ -f "${SESSION}_failure.log" ]]; then
    pass "tui-assert creates failure log on timeout"
    rm -f "${SESSION}_failure.log"
else
    fail "tui-assert did not create failure log"
fi

# --- tui-assert with regex ---
echo "Testing tui-assert regex..."

"$SCRIPT_DIR/tui-send" "$SESSION" "echo 'Error: code 42'"
"$SCRIPT_DIR/tui-send" "$SESSION" --keys "Enter"
sleep 0.3

if "$SCRIPT_DIR/tui-assert" "$SESSION" -E "Error:.*42" 3 2>&1; then
    pass "tui-assert -E matches regex"
else
    fail "tui-assert -E regex failed"
fi

# --- tui-capture with -e (ANSI) ---
echo "Testing tui-capture flags..."

"$SCRIPT_DIR/tui-send" "$SESSION" 'echo -e "\033[31mRED\033[0m"'
"$SCRIPT_DIR/tui-send" "$SESSION" --keys "Enter"
sleep 0.3

if output=$("$SCRIPT_DIR/tui-capture" "$SESSION" -e 2>&1) && [[ "$output" == *$'\033'* ]]; then
    pass "tui-capture -e preserves ANSI codes"
else
    fail "tui-capture -e did not preserve ANSI codes"
fi

# --- tui-capture with -S (scrollback) ---
"$SCRIPT_DIR/tui-send" "$SESSION" "for i in {1..5}; do echo LINE\$i; done"
"$SCRIPT_DIR/tui-send" "$SESSION" --keys "Enter"
sleep 0.3

if output=$("$SCRIPT_DIR/tui-capture" "$SESSION" -S 10 2>&1) && [[ "$output" == *"LINE1"* ]]; then
    pass "tui-capture -S captures scrollback"
else
    fail "tui-capture -S scrollback failed"
fi

# --- tui-stop ---
echo "Testing tui-stop..."

if "$SCRIPT_DIR/tui-stop" "$SESSION" 2>&1; then
    pass "tui-stop completes"
else
    fail "tui-stop failed"
fi

if ! "$SCRIPT_DIR/tmux-isolated" has-session -t "$SESSION" 2>/dev/null; then
    pass "session gone after tui-stop"
else
    fail "session still exists after tui-stop"
fi

# --- tui-stop idempotent ---
if "$SCRIPT_DIR/tui-stop" "$SESSION" 2>&1; then
    pass "tui-stop is idempotent"
else
    fail "tui-stop failed on already-stopped session"
fi

# --- Summary ---
echo
echo "=== Results ==="
echo "Passed: $PASS"
echo "Failed: $FAIL"
echo

if [[ $FAIL -eq 0 ]]; then
    echo "All tests passed!"
    exit 0
else
    echo "Some tests failed."
    exit 1
fi
