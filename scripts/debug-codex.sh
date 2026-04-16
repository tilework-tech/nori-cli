#!/bin/bash

# Set "chatgpt.cliExecutable": "/Users/<USERNAME>/code/codex/scripts/debug-codex.sh" in VSCode settings to always get the
# latest nori binary when debugging Codex Extension.


set -euo pipefail

NORI_RS_DIR=$(realpath "$(dirname "$0")/../nori-rs")
(cd "$NORI_RS_DIR" && cargo run --quiet --bin nori -- "$@")