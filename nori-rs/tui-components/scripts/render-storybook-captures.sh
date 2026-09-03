#!/usr/bin/env bash
set -euo pipefail

# Review aid only. Re-render the exact ANSI captured by tests; no image assertion.
: "${TUI_CAPTURE_DIR:?Set this to CSRessel/skills/tui-capture-with-ghostty-web}"
component_dir=$(cd "$(dirname "$0")/.." && pwd)
cd "$component_dir/.."
target_dir=$(cargo metadata --format-version 1 --no-deps | jq -r .target_directory)
latest_run="$target_dir/storybook-captures/latest-run"
if [[ ! -f "$latest_run" ]]; then
  echo "Run scripts/storybook-e2e.sh first to produce ANSI captures" >&2
  exit 1
fi
capture_dir=$(< "$latest_run")
if [[ ! -f "$capture_dir/passed" ]]; then
  echo "Latest storybook run did not pass; review snapshots and rerun before rendering" >&2
  exit 1
fi

while IFS= read -r ansi; do
  artifact_dir=$(dirname "$ansi")
  name=$(basename "$artifact_dir")
  example=$(basename "$(dirname "$artifact_dir")")
  read -r cols rows < "$artifact_dir/geometry.txt"
  # Storybooks live beside the code they exercise, so the example directory
  # is resolved per crate rather than assumed to be a component example.
  if [[ -d "$component_dir/examples/$example" ]]; then
    example_dir="$component_dir/examples/$example"
  else
    example_dir="$component_dir/../tui/examples/$example"
  fi
  output="$example_dir/screenshots/$name.png"
  bun "$TUI_CAPTURE_DIR/scripts/render-ansi.ts" \
    --input "$ansi" --output "$output" --cols "$cols" --rows "$rows" >/dev/null
  printf '%s\n' "$output"
done < <(find "$capture_dir" -name replay.ansi -type f | sort)
