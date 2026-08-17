#!/usr/bin/env bash
set -euo pipefail

readonly AGENT_TTY_VERSION="0.5.0"
readonly PLAYWRIGHT_VERSION="1.60.0"

usage() {
  cat <<'EOF'
Usage: capture-tui.sh --cwd DIR --steps FILE --out FILE [options]

Required:
  --cwd DIR             Working directory for the captured terminal
  --steps FILE          agent-tty batch steps as a JSON array
  --out FILE            Destination PNG, replaced atomically

Options:
  --cols N              Terminal columns (default: 120)
  --rows N              Terminal rows (default: 36)
  --profile NAME        agent-tty render profile (default: reference-dark)
  --shell PATH          Session shell (default: /bin/bash)
  --snapshot-out FILE   Write the semantic text snapshot
  --metadata-out FILE   Write the capture result JSON
  -h, --help            Show this help
EOF
}

capture_cwd=""
steps_file=""
output_path=""
snapshot_output=""
metadata_output=""
capture_cols=120
capture_rows=36
capture_profile="reference-dark"
capture_shell="/bin/bash"

while (($# > 0)); do
  case "$1" in
    --cwd)
      capture_cwd=${2:?"--cwd requires a directory"}
      shift 2
      ;;
    --steps)
      steps_file=${2:?"--steps requires a file"}
      shift 2
      ;;
    --out)
      output_path=${2:?"--out requires a file"}
      shift 2
      ;;
    --cols)
      capture_cols=${2:?"--cols requires a number"}
      shift 2
      ;;
    --rows)
      capture_rows=${2:?"--rows requires a number"}
      shift 2
      ;;
    --profile)
      capture_profile=${2:?"--profile requires a name"}
      shift 2
      ;;
    --shell)
      capture_shell=${2:?"--shell requires a path"}
      shift 2
      ;;
    --snapshot-out)
      snapshot_output=${2:?"--snapshot-out requires a file"}
      shift 2
      ;;
    --metadata-out)
      metadata_output=${2:?"--metadata-out requires a file"}
      shift 2
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ -z "$capture_cwd" || -z "$steps_file" || -z "$output_path" ]]; then
  usage >&2
  exit 2
fi

for required_command in npx jq; do
  if ! command -v "$required_command" >/dev/null 2>&1; then
    echo "Required command not found: $required_command" >&2
    exit 2
  fi
done

if [[ ! -d "$capture_cwd" ]]; then
  echo "Capture working directory does not exist: $capture_cwd" >&2
  exit 2
fi
if [[ ! -f "$steps_file" ]]; then
  echo "Steps file does not exist: $steps_file" >&2
  exit 2
fi
if [[ ! -x "$capture_shell" ]]; then
  echo "Capture shell is not executable: $capture_shell" >&2
  exit 2
fi
if [[ ! "$capture_cols" =~ ^[1-9][0-9]*$ || ! "$capture_rows" =~ ^[1-9][0-9]*$ ]]; then
  echo "--cols and --rows must be positive integers" >&2
  exit 2
fi
if ! jq -e 'type == "array" and length > 0' "$steps_file" >/dev/null; then
  echo "Steps file must contain a non-empty JSON array" >&2
  exit 2
fi

capture_cwd=$(cd "$capture_cwd" && pwd)
steps_file=$(cd "$(dirname "$steps_file")" && pwd)/$(basename "$steps_file")

mkdir -p "$(dirname "$output_path")"
output_path=$(cd "$(dirname "$output_path")" && pwd)/$(basename "$output_path")
if [[ -n "$snapshot_output" ]]; then
  mkdir -p "$(dirname "$snapshot_output")"
  snapshot_output=$(cd "$(dirname "$snapshot_output")" && pwd)/$(basename "$snapshot_output")
fi
if [[ -n "$metadata_output" ]]; then
  mkdir -p "$(dirname "$metadata_output")"
  metadata_output=$(cd "$(dirname "$metadata_output")" && pwd)/$(basename "$metadata_output")
fi

capture_home=$(mktemp -d /tmp/capture-tui.XXXXXX)
session_id=""
agent_tty=(npx -y -p node@24 -p "agent-tty@${AGENT_TTY_VERSION}" agent-tty)

cleanup() {
  if [[ -n "$session_id" ]]; then
    "${agent_tty[@]}" --home "$capture_home" destroy "$session_id" --json \
      >/dev/null 2>&1 || true
  fi
  case "$capture_home" in
    /tmp/capture-tui.*)
      rm -rf -- "$capture_home"
      ;;
  esac
}
trap cleanup EXIT

doctor_json="$capture_home/doctor.json"
doctor_status=0
"${agent_tty[@]}" --home "$capture_home" doctor --json >"$doctor_json" \
  || doctor_status=$?
if [[ ! -s "$doctor_json" ]]; then
  echo "agent-tty doctor failed without returning diagnostics (exit $doctor_status)" >&2
  exit 1
fi
if ! jq -e '.result.ok == true' "$doctor_json" >/dev/null; then
  browser_missing=$(jq -r '
    any(.result.checks.renderer[]?;
      (.name == "browser_cache_accessible" or .name == "browser_launch")
      and .status == "fail")
  ' "$doctor_json")
  if [[ "$browser_missing" == "true" ]]; then
    echo "Installing the pinned Playwright Chromium build..." >&2
    npx -y -p "playwright@${PLAYWRIGHT_VERSION}" playwright install chromium >&2
    doctor_status=0
    "${agent_tty[@]}" --home "$capture_home" doctor --json >"$doctor_json" \
      || doctor_status=$?
    if [[ ! -s "$doctor_json" ]]; then
      echo "agent-tty doctor failed without returning diagnostics (exit $doctor_status)" >&2
      exit 1
    fi
  fi
fi
if ! jq -e '.result.ok == true' "$doctor_json" >/dev/null; then
  echo "agent-tty environment checks failed:" >&2
  jq '.result.checks' "$doctor_json" >&2
  exit 1
fi

normalized_steps="$capture_home/steps.json"
jq '
  map(
    if (.run? | type) == "string" then
      .run = ("unset NO_COLOR; export COLORTERM=truecolor; " + .run)
    else
      .
    end
  )
' "$steps_file" >"$normalized_steps"

create_json="$capture_home/create.json"
"${agent_tty[@]}" --home "$capture_home" create \
  --cols "$capture_cols" \
  --rows "$capture_rows" \
  --cwd "$capture_cwd" \
  --name capture-tui \
  --json \
  -- "$capture_shell" >"$create_json"
session_id=$(jq -er '.result.sessionId' "$create_json")

batch_json="$capture_home/batch.json"
batch_status=0
"${agent_tty[@]}" --home "$capture_home" batch "$session_id" \
  --file "$normalized_steps" \
  --json >"$batch_json" || batch_status=$?
if ! jq -e '.ok == true and (.result.failedIndices | length == 0)' "$batch_json" >/dev/null; then
  echo "TUI interaction batch failed:" >&2
  jq '.' "$batch_json" >&2
  if ((batch_status > 0)); then
    exit "$batch_status"
  fi
  exit 1
fi

snapshot_json="$capture_home/snapshot.json"
"${agent_tty[@]}" --home "$capture_home" snapshot "$session_id" \
  --format text \
  --json >"$snapshot_json"

screenshot_json="$capture_home/screenshot.json"
"${agent_tty[@]}" --home "$capture_home" screenshot "$session_id" \
  --profile "$capture_profile" \
  --hide-cursor \
  --json >"$screenshot_json"

artifact_path=$(jq -er '.result.artifactPath' "$screenshot_json")
staged_png=$(mktemp "${output_path}.tmp.XXXXXX")
cp "$artifact_path" "$staged_png"
mv "$staged_png" "$output_path"

if [[ -n "$snapshot_output" ]]; then
  staged_snapshot=$(mktemp "${snapshot_output}.tmp.XXXXXX")
  jq -r '.result.text' "$snapshot_json" >"$staged_snapshot"
  mv "$staged_snapshot" "$snapshot_output"
fi

semantic_backend=$(jq -r '
  if any(.result.checks.renderer[]?;
    .name == "libghostty_vt_available" and .status == "pass")
  then "libghostty-vt"
  else "ghostty-web"
  end
' "$doctor_json")

capture_result=$(jq -n \
  --arg outputPath "$output_path" \
  --arg semanticBackend "$semantic_backend" \
  --slurpfile snapshot "$snapshot_json" \
  --slurpfile screenshot "$screenshot_json" \
  '{
    ok: true,
    outputPath: $outputPath,
    semanticBackend: $semanticBackend,
    visualBackend: $screenshot[0].result.rendererBackend,
    profileName: $screenshot[0].result.profileName,
    cols: $screenshot[0].result.cols,
    rows: $screenshot[0].result.rows,
    pixelWidth: $screenshot[0].result.pixelWidth,
    pixelHeight: $screenshot[0].result.pixelHeight,
    screenHash: $snapshot[0].result.screenHash,
    pngSha256: $screenshot[0].result.sha256,
    capturedAtSeq: $screenshot[0].result.capturedAtSeq
  }')

if [[ -n "$metadata_output" ]]; then
  staged_metadata=$(mktemp "${metadata_output}.tmp.XXXXXX")
  printf '%s\n' "$capture_result" >"$staged_metadata"
  mv "$staged_metadata" "$metadata_output"
fi

printf '%s\n' "$capture_result"
