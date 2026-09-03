#!/usr/bin/env bash
set -euo pipefail

# Keep the external skill installation explicit: no developer-specific paths,
# network bootstrap, browser dependency, or nested Cargo call inside a test.
: "${TUI_PUPPETEERING_DIR:?Set this to CSRessel/skills/tui-puppeteering-with-tmux}"
component_dir=$(cd "$(dirname "$0")/.." && pwd)
cd "$component_dir/.."
target_dir=$(cargo metadata --format-version 1 --no-deps | jq -r .target_directory)
capture_root="$target_dir/storybook-captures"
mkdir -p "$capture_root"
export NORI_STORYBOOK_ARTIFACT_DIR
NORI_STORYBOOK_ARTIFACT_DIR=$(mktemp -d "$capture_root/run.XXXXXX")
# Record even a failed attempt: rendering must never fall back to an older run.
printf '%s\n' "$NORI_STORYBOOK_ARTIFACT_DIR" > "$capture_root/latest-run"
export TUI_PUPPETEERING_DIR

# The status-card storybook renders the CLI's own status view, so it lives in
# `nori-tui` behind the `storybook` feature; the rest are component specimens.
cargo build -p nori-tui-components --examples --message-format=json > "$NORI_STORYBOOK_ARTIFACT_DIR/build.jsonl"
cargo build -p nori-tui --features storybook --examples --message-format=json >> "$NORI_STORYBOOK_ARTIFACT_DIR/build.jsonl"
example_binary=$(jq -r 'select(.reason == "compiler-artifact" and .target.name == "nori_storybook" and .executable != null) | .executable' "$NORI_STORYBOOK_ARTIFACT_DIR/build.jsonl")
if [[ ! -f "$example_binary" ]]; then
  echo "Cargo did not report the built nori_storybook executable" >&2
  exit 1
fi
export NORI_STORYBOOK_BIN_DIR
NORI_STORYBOOK_BIN_DIR=$(dirname "$example_binary")
cargo test -p nori-tui-components --test 'storybook_*' --no-fail-fast -- --include-ignored "$@"
cargo test -p nori-tui --features storybook --test 'storybook_*' --no-fail-fast -- --include-ignored "$@"
touch "$NORI_STORYBOOK_ARTIFACT_DIR/passed"
printf 'Captures: %s\n' "$NORI_STORYBOOK_ARTIFACT_DIR"
