#!/usr/bin/env bash
# cloud-e2e-test.sh — End-to-end test for nori cloud sessions
#
# Validates the CLI → broker → sprite WebSocket tunnel flow. The test manually
# prepares a sprite (restore checkpoint, push credentials, restart bridge)
# rather than relying on the broker's full provisioning pipeline, which takes
# 20-30 minutes from scratch and is designed for production deployments.
#
# The broker is started with two temporary patches:
#   1. isBaseVersionCurrent() returns true — prevents the broker from deleting
#      production-provisioned sprites due to bootstrap bundle hash mismatch.
#   2. startupRefresh() is a no-op — prevents the broker from gcRefreshing
#      all sprites on startup (which would undo our manual prep).
# Both patches are reverted on cleanup.
#
# Prerequisites:
#   - nori binary built (cargo build --bin nori in nori-rs/)
#   - nori-sessions repo with cli-cloud-session-refactor worktree
#   - Sprites credentials (NORI_SPRITE_ORG + NORI_SPRITE_TOKEN)
#   - Valid cloud-auth.json (browser auth will be triggered if missing/expired)
#   - bun, tmux, curl, jq, python3 installed
#
# Usage:
#   ./scripts/cloud-e2e-test.sh
#
# Environment variables (all have defaults):
#   NORI_SESSIONS_WORKTREE  Path to broker worktree
#   NORI_SPRITE_ORG         Sprites org name
#   NORI_SPRITE_TOKEN       Sprites API token
#   NORI_BINARY             Path to nori binary
#   BROKER_PORT             Broker listen port (default: 19400)
#   SKIP_BROKER_START       Set to 1 to use an already-running broker
#   SKIP_BUILD              Set to 1 to skip cargo build

set -euo pipefail

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------

NORI_SESSIONS_WORKTREE="${NORI_SESSIONS_WORKTREE:-$HOME/code/nori/nori-sessions/.worktrees/cli-cloud-session-refactor}"
NORI_SPRITE_ORG="${NORI_SPRITE_ORG:-amol-kapoor}"
NORI_SPRITE_TOKEN="${NORI_SPRITE_TOKEN:?NORI_SPRITE_TOKEN must be set}"
BROKER_PORT="${BROKER_PORT:-19400}"
BROKER_URL="http://localhost:${BROKER_PORT}"
SKIP_BROKER_START="${SKIP_BROKER_START:-0}"
SKIP_BUILD="${SKIP_BUILD:-0}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
NORI_RS_DIR="$REPO_ROOT/nori-rs"
NORI_BINARY="${NORI_BINARY:-$NORI_RS_DIR/target/debug/nori}"

TMUX_SESSION="nori-cloud-e2e-$$"
BROKER_PID=""
PATCHED_FILES=()

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

log()  { echo "[$(date +%H:%M:%S)] $*"; }
pass() { echo "[$(date +%H:%M:%S)] PASS: $*"; }
fail() { echo "[$(date +%H:%M:%S)] FAIL: $*" >&2; cleanup; exit 1; }

cleanup() {
  log "Cleaning up..."
  tmux kill-session -t "$TMUX_SESSION" 2>/dev/null || true
  tmux kill-session -t "nori-cloud-e2e-reacquire-$$" 2>/dev/null || true
  if [[ -n "$BROKER_PID" ]]; then
    kill "$BROKER_PID" 2>/dev/null || true
    wait "$BROKER_PID" 2>/dev/null || true
  fi
  for backup in "${PATCHED_FILES[@]}"; do
    local orig="${backup%.bak}"
    if [[ -f "$backup" ]]; then
      cp "$backup" "$orig"
      rm -f "$backup"
    fi
  done
  if (( ${#PATCHED_FILES[@]} > 0 )); then
    log "Restored patched broker files"
  fi
  rm -f /tmp/nori-cloud-e2e-broker.log /tmp/nori-cloud-e2e-sprite-helper.ts
}

get_auth_token() {
  python3 -c "import json; print(json.load(open('$HOME/.nori/cli/cloud-auth.json'))['auth_token'])" 2>/dev/null
}

is_token_valid() {
  local token="$1"
  local exp
  exp=$(python3 -c "import json,base64,sys; payload=json.loads(base64.urlsafe_b64decode(sys.argv[1].split(chr(46))[1]+chr(61)*2)); print(payload[chr(101)+chr(120)+chr(112)])" "$token" 2>/dev/null) || return 1
  python3 -c "import time,sys; sys.exit(0 if time.time()<float(sys.argv[1]) else 1)" "$exp" 2>/dev/null
}

sprite_exec() {
  local sprite_name="$1"
  local cmd="$2"
  cat > /tmp/nori-cloud-e2e-sprite-helper.ts << EOFTS
import { SpritesClient } from '@fly/sprites';
const client = new SpritesClient(process.env.NORI_SPRITE_TOKEN!);
const sprite = client.sprite('${sprite_name}');
const result = await sprite.execFile('bash', ['-c', $(printf '%s' "$cmd" | python3 -c "import sys,json; print(json.dumps(sys.stdin.read()))")]);
process.stdout.write(result.stdout ?? '');
if (result.exitCode !== 0) {
  process.stderr.write(result.stderr ?? '');
  process.exit(result.exitCode ?? 1);
}
EOFTS
  cd "$NORI_SESSIONS_WORKTREE/broker/server" && \
    NORI_SPRITE_TOKEN="$NORI_SPRITE_TOKEN" bun run /tmp/nori-cloud-e2e-sprite-helper.ts 2>&1
}

sprite_restore() {
  local sprite_name="$1"
  cat > /tmp/nori-cloud-e2e-sprite-helper.ts << EOFTS
import { SpritesClient } from '@fly/sprites';
const client = new SpritesClient(process.env.NORI_SPRITE_TOKEN!);
await client.sprite('${sprite_name}').restoreCheckpoint('v1');
console.log('restored');
EOFTS
  cd "$NORI_SESSIONS_WORKTREE/broker/server" && \
    NORI_SPRITE_TOKEN="$NORI_SPRITE_TOKEN" bun run /tmp/nori-cloud-e2e-sprite-helper.ts 2>&1
}

sprite_list() {
  curl -sf "https://api.sprites.dev/v1/sprites" \
    -H "Authorization: Bearer $NORI_SPRITE_TOKEN" 2>/dev/null \
    | jq -c '.sprites[]? | {name, status}'
}

tmux_capture() {
  tmux capture-pane -t "$TMUX_SESSION" -p -S - 2>/dev/null
}

tmux_assert() {
  local pattern="$1"
  local timeout="${2:-30}"
  local elapsed=0
  while (( elapsed < timeout )); do
    if tmux_capture | grep -qF "$pattern"; then
      return 0
    fi
    sleep 2
    elapsed=$((elapsed + 2))
  done
  log "Timed out waiting for: $pattern"
  log "Current screen:"
  tmux_capture | tail -20
  return 1
}

# Backup a file and record it for cleanup
backup_file() {
  local file="$1"
  cp "$file" "${file}.bak"
  PATCHED_FILES+=("${file}.bak")
}

# ---------------------------------------------------------------------------
# Step 0: Check prerequisites
# ---------------------------------------------------------------------------

check_prereqs() {
  log "Checking prerequisites..."
  local missing=()
  command -v bun    >/dev/null 2>&1 || missing+=("bun")
  command -v tmux   >/dev/null 2>&1 || missing+=("tmux")
  command -v curl   >/dev/null 2>&1 || missing+=("curl")
  command -v jq     >/dev/null 2>&1 || missing+=("jq")
  command -v python3 >/dev/null 2>&1 || missing+=("python3")

  if (( ${#missing[@]} > 0 )); then
    fail "Missing tools: ${missing[*]}"
  fi

  [[ -d "$NORI_SESSIONS_WORKTREE/broker/server" ]] || \
    fail "Broker worktree not found at $NORI_SESSIONS_WORKTREE"

  [[ -d "$NORI_RS_DIR" ]] || \
    fail "nori-rs directory not found at $NORI_RS_DIR"

  pass "Prerequisites OK"
}

# ---------------------------------------------------------------------------
# Step 1: Build nori binary
# ---------------------------------------------------------------------------

build_nori() {
  if [[ "$SKIP_BUILD" == "1" ]] && [[ -x "$NORI_BINARY" ]]; then
    log "Skipping build (SKIP_BUILD=1, binary exists)"
    return
  fi
  log "Building nori binary..."
  (cd "$NORI_RS_DIR" && cargo build --bin nori 2>&1 | tail -3) || \
    fail "cargo build failed"
  [[ -x "$NORI_BINARY" ]] || fail "Binary not found at $NORI_BINARY"
  pass "nori binary built"
}

# ---------------------------------------------------------------------------
# Step 2: Find and prepare a sprite
# ---------------------------------------------------------------------------

prepare_sprite() {
  log "Looking for a sprite with a checkpoint..."

  local sprites
  sprites=$(sprite_list)
  local sprite_name
  sprite_name=$(echo "$sprites" | jq -r 'select(.name | startswith("nori-")) | .name' | head -1)

  if [[ -z "$sprite_name" ]]; then
    fail "No nori-* sprites found in org $NORI_SPRITE_ORG"
  fi
  log "Using sprite: $sprite_name"

  log "  Restoring checkpoint..."
  if ! sprite_restore "$sprite_name" 2>&1 | grep -q "restored"; then
    fail "Checkpoint restore failed for $sprite_name"
  fi
  sleep 5

  log "  Pushing Claude credentials..."
  if [[ ! -f "$HOME/.claude/.credentials.json" ]]; then
    fail "No local Claude credentials at ~/.claude/.credentials.json"
  fi
  local creds_json
  creds_json=$(cat "$HOME/.claude/.credentials.json")
  sprite_exec "$sprite_name" \
    "mkdir -p /home/sprite/.claude && cat > /home/sprite/.claude/.credentials.json << 'ENDCREDS'
${creds_json}
ENDCREDS
chmod 600 /home/sprite/.claude/.credentials.json" >/dev/null

  log "  Restarting ACP bridge..."
  curl -s -X POST "https://api.sprites.dev/v1/sprites/${sprite_name}/services/nori-acp-bridge/stop" \
    -H "Authorization: Bearer $NORI_SPRITE_TOKEN" >/dev/null 2>&1
  sleep 2
  curl -s -X POST "https://api.sprites.dev/v1/sprites/${sprite_name}/services/nori-acp-bridge/start" \
    -H "Authorization: Bearer $NORI_SPRITE_TOKEN" >/dev/null 2>&1
  sleep 3

  PREPARED_SPRITE="$sprite_name"
  pass "Sprite $sprite_name prepared"
}

# ---------------------------------------------------------------------------
# Step 3: Patch broker for local E2E testing
# ---------------------------------------------------------------------------

patch_broker() {
  if [[ "$SKIP_BROKER_START" == "1" ]]; then
    return
  fi

  local bv_file="$NORI_SESSIONS_WORKTREE/broker/server/src/fleet/base-version.ts"
  local mgr_file="$NORI_SESSIONS_WORKTREE/broker/server/src/features/lifecycle/manager.ts"

  backup_file "$bv_file"
  backup_file "$mgr_file"

  # Patch 1: Skip base version check so broker doesn't delete sprites
  python3 -c "
import re
with open('$bv_file') as f:
    content = f.read()
content = re.sub(
    r'(export const isBaseVersionCurrent = async \()(args)(: \{)',
    r'\1_args\3',
    content,
)
content = re.sub(
    r'(export const isBaseVersionCurrent = async \(_args:.*?\): Promise<boolean> => \{).*?(\n\};)',
    r'\1\n  return true;\2',
    content,
    flags=re.DOTALL,
)
with open('$bv_file', 'w') as f:
    f.write(content)
"

  # Patch 2: Skip startupRefresh gcRefresh loop so broker doesn't undo our prep
  python3 -c "
import re
with open('$mgr_file') as f:
    content = f.read()
# Add early return after discoverExisting() in startupRefresh()
content = content.replace(
    'await this.discoverExisting();\n',
    'await this.discoverExisting();\n    return;\n',
    1,
)
with open('$mgr_file', 'w') as f:
    f.write(content)
"

  pass "Patched broker for local E2E testing"
}

# ---------------------------------------------------------------------------
# Step 4: Start broker
# ---------------------------------------------------------------------------

start_broker() {
  if [[ "$SKIP_BROKER_START" == "1" ]]; then
    log "Skipping broker start (SKIP_BROKER_START=1)"
    if ! curl -sf "${BROKER_URL}/api/health" >/dev/null 2>&1; then
      fail "SKIP_BROKER_START=1 but broker is not responding at $BROKER_URL"
    fi
    pass "Using existing broker"
    return
  fi

  local existing_pid
  existing_pid=$(lsof -ti ":$BROKER_PORT" 2>/dev/null || true)
  if [[ -n "$existing_pid" ]]; then
    log "Killing existing process on port $BROKER_PORT (pid $existing_pid)"
    kill "$existing_pid" 2>/dev/null || true
    sleep 2
  fi

  log "Starting broker on port $BROKER_PORT..."
  (
    cd "$NORI_SESSIONS_WORKTREE/broker/server" && \
    NORI_ORG="local" \
    NORI_SPRITE_ORG="$NORI_SPRITE_ORG" \
    NORI_SPRITE_TOKEN="$NORI_SPRITE_TOKEN" \
    NORI_BROKER_PORT="$BROKER_PORT" \
    exec bun run src/main.ts
  ) > /tmp/nori-cloud-e2e-broker.log 2>&1 &
  BROKER_PID=$!

  local elapsed=0
  while (( elapsed < 30 )); do
    if curl -sf "${BROKER_URL}/api/health" >/dev/null 2>&1; then
      pass "Broker started (pid $BROKER_PID)"
      return
    fi
    sleep 1
    elapsed=$((elapsed + 1))
  done
  log "Broker log tail:"
  tail -20 /tmp/nori-cloud-e2e-broker.log
  fail "Broker did not become healthy within 30s"
}

# ---------------------------------------------------------------------------
# Step 5: Wait for the prepared sprite to be discovered
# ---------------------------------------------------------------------------

wait_for_sprite_ready() {
  log "Waiting for broker to discover sprites and releasing system claims..."
  local token
  token=$(get_auth_token 2>/dev/null || true)
  if [[ -z "$token" ]] || ! is_token_valid "$token"; then
    fail "Need a valid auth token"
  fi

  # Wait for broker to discover sprites
  local elapsed=0
  while (( elapsed < 30 )); do
    local count
    count=$(curl -sf "${BROKER_URL}/api/sessions" -H "Authorization: Bearer $token" 2>/dev/null \
      | jq '[.agents[]] | length' 2>/dev/null || echo "0")
    if (( count > 0 )); then
      break
    fi
    sleep 3
    elapsed=$((elapsed + 3))
  done

  # Release all claimed sprites so one becomes available. Startup discovery
  # marks warm/running sprites as claimed(idle), and stale claims from previous
  # test runs may also persist.
  local agents
  agents=$(curl -sf "${BROKER_URL}/api/sessions" -H "Authorization: Bearer $token" 2>/dev/null)
  local names
  names=$(echo "$agents" | jq -r '.agents[] | select(.lifecycle == "claimed") | .name')
  for name in $names; do
    log "  Releasing system claim on $name"
    curl -sf -X POST "${BROKER_URL}/api/sessions/${name}/release" \
      -H "Authorization: Bearer $token" >/dev/null 2>&1 || true
  done

  # Wait for at least one sprite to be ready
  elapsed=0
  while (( elapsed < 60 )); do
    local ready_count
    ready_count=$(curl -sf "${BROKER_URL}/api/sessions" -H "Authorization: Bearer $token" 2>/dev/null \
      | jq '[.agents[] | select(.lifecycle == "ready")] | length' 2>/dev/null || echo "0")
    if (( ready_count > 0 )); then
      pass "Sprite(s) ready for claiming"
      return
    fi
    sleep 3
    elapsed=$((elapsed + 3))
  done
  local states
  states=$(curl -sf "${BROKER_URL}/api/sessions" -H "Authorization: Bearer $token" 2>/dev/null \
    | jq -r '.agents[] | "\(.name):\(.lifecycle):\(.claimedBy // "none")"')
  log "Final states: $states"
  fail "No sprites became ready within 60s"
}

# ---------------------------------------------------------------------------
# Step 6: Verify authentication
# ---------------------------------------------------------------------------

verify_auth() {
  log "Checking authentication..."
  local token
  token=$(get_auth_token 2>/dev/null || true)

  if [[ -n "$token" ]] && is_token_valid "$token"; then
    pass "Auth token is valid"
    return
  fi

  log "Auth token is missing or expired. Triggering browser auth flow..."
  log ">>> A browser window will open. Please complete the login. <<<"

  local auth_tmux="nori-auth-$$"
  tmux new-session -d -s "$auth_tmux" -x 80 -y 20
  tmux send-keys -t "$auth_tmux" \
    "'$NORI_BINARY' cloud --broker-url '$BROKER_URL' --skip-welcome --skip-trust-directory" Enter

  local elapsed=0
  while (( elapsed < 120 )); do
    token=$(get_auth_token 2>/dev/null || true)
    if [[ -n "$token" ]] && is_token_valid "$token"; then
      tmux kill-session -t "$auth_tmux" 2>/dev/null || true
      pass "Authentication successful"
      return
    fi
    sleep 3
    elapsed=$((elapsed + 3))
  done

  tmux kill-session -t "$auth_tmux" 2>/dev/null || true
  fail "Authentication timed out after 120s"
}

# ---------------------------------------------------------------------------
# Step 7: Run E2E test — two messages via nori cloud
# ---------------------------------------------------------------------------

run_e2e() {
  log "Starting nori cloud in tmux..."
  tmux new-session -d -s "$TMUX_SESSION" -x 160 -y 40
  tmux send-keys -t "$TMUX_SESSION" \
    "cd '$NORI_RS_DIR' && '$NORI_BINARY' cloud --broker-url '$BROKER_URL' --skip-welcome --skip-trust-directory" Enter

  if ! tmux_assert "›" 90; then
    log "Broker log tail:"
    tail -10 /tmp/nori-cloud-e2e-broker.log | jq -r '.message' 2>/dev/null || tail -10 /tmp/nori-cloud-e2e-broker.log
    fail "TUI did not show prompt within 90s"
  fi

  sleep 10

  if tmux_capture | grep -qF "Failed to start agent"; then
    log "TUI output:"
    tmux_capture | tail -30
    fail "Agent failed to start: $(tmux_capture | grep 'Failed to start agent')"
  fi

  if tmux_capture | grep -qF "Cloud session creation failed"; then
    log "TUI output:"
    tmux_capture | tail -30
    fail "Cloud session creation failed: $(tmux_capture | grep 'Cloud session creation failed')"
  fi

  pass "TUI loaded with prompt"

  # --- Message 1 ---
  log "Sending message 1: 'what is 2 plus 2'"
  tmux send-keys -t "$TMUX_SESSION" "what is 2 plus 2"
  sleep 2
  tmux send-keys -t "$TMUX_SESSION" Enter

  if ! tmux_assert "4" 90; then
    fail "No response to message 1 within 90s"
  fi
  pass "Message 1: got response"

  pass "E2E test complete — message sent and received"
}

# ---------------------------------------------------------------------------
# Step 8: Close session and re-acquire — test session lifecycle
# ---------------------------------------------------------------------------

close_session() {
  log "Closing TUI session (Ctrl-C)..."
  tmux send-keys -t "$TMUX_SESSION" C-c
  sleep 3

  # Wait for the process to fully terminate
  local elapsed=0
  while (( elapsed < 15 )); do
    if ! tmux_capture | grep -qF "›"; then
      break
    fi
    sleep 1
    elapsed=$((elapsed + 1))
  done

  # Kill the tmux session to ensure clean state
  tmux kill-session -t "$TMUX_SESSION" 2>/dev/null || true
  sleep 2

  pass "Session closed"
}

wait_for_sprite_available() {
  log "Waiting for sprite to become available after release..."
  local token
  token=$(get_auth_token)

  # Release any claimed sprites from the previous session
  local agents
  agents=$(curl -sf "${BROKER_URL}/api/sessions" -H "Authorization: Bearer $token" 2>/dev/null)
  local names
  names=$(echo "$agents" | jq -r '.agents[] | select(.lifecycle == "claimed") | .name')
  for name in $names; do
    log "  Releasing claim on $name"
    curl -sf -X POST "${BROKER_URL}/api/sessions/${name}/release" \
      -H "Authorization: Bearer $token" >/dev/null 2>&1 || true
  done

  # Wait for at least one sprite to be ready
  local elapsed=0
  while (( elapsed < 60 )); do
    local ready_count
    ready_count=$(curl -sf "${BROKER_URL}/api/sessions" -H "Authorization: Bearer $token" 2>/dev/null \
      | jq '[.agents[] | select(.lifecycle == "ready")] | length' 2>/dev/null || echo "0")
    if (( ready_count > 0 )); then
      pass "Sprite available for re-acquisition"
      return
    fi
    sleep 3
    elapsed=$((elapsed + 3))
  done
  fail "No sprites became ready within 60s after release"
}

run_reacquire_test() {
  log "Re-launching nori cloud for session re-acquisition test..."
  TMUX_SESSION="nori-cloud-e2e-reacquire-$$"
  tmux new-session -d -s "$TMUX_SESSION" -x 160 -y 40
  tmux send-keys -t "$TMUX_SESSION" \
    "cd '$NORI_RS_DIR' && '$NORI_BINARY' cloud --broker-url '$BROKER_URL' --skip-welcome --skip-trust-directory" Enter

  if ! tmux_assert "›" 90; then
    log "Broker log tail:"
    tail -10 /tmp/nori-cloud-e2e-broker.log | jq -r '.message' 2>/dev/null || tail -10 /tmp/nori-cloud-e2e-broker.log
    fail "TUI did not show prompt within 90s on re-acquisition"
  fi

  if tmux_capture | grep -qF "Failed to start agent"; then
    log "TUI output:"
    tmux_capture | tail -30
    fail "Agent failed to start on re-acquisition: $(tmux_capture | grep 'Failed to start agent')"
  fi

  if tmux_capture | grep -qF "Cloud session creation failed"; then
    log "TUI output:"
    tmux_capture | tail -30
    fail "Cloud session creation failed on re-acquisition"
  fi

  sleep 5
  pass "TUI loaded on re-acquisition"

  # --- Message in re-acquired session ---
  log "Sending message in re-acquired session: 'what is 3 plus 5'"
  tmux send-keys -t "$TMUX_SESSION" "what is 3 plus 5"
  sleep 2
  tmux send-keys -t "$TMUX_SESSION" Enter

  if ! tmux_assert "8" 90; then
    fail "No response to message in re-acquired session within 90s"
  fi
  pass "Re-acquired session: got response"

  # Clean up the re-acquisition tmux session
  tmux kill-session -t "$TMUX_SESSION" 2>/dev/null || true

  pass "Session lifecycle test complete — close + re-acquire succeeded"
}

# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------

main() {
  trap cleanup EXIT
  log "=== Nori Cloud E2E Test ==="
  log ""

  check_prereqs
  build_nori
  patch_broker
  start_broker
  verify_auth
  prepare_sprite
  wait_for_sprite_ready
  run_e2e
  close_session
  wait_for_sprite_available
  run_reacquire_test

  log ""
  log "=== ALL TESTS PASSED ==="
}

main "$@"
