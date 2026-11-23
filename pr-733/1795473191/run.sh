#!/bin/bash
# usage: run.sh <target/debug dir> [extra toml]
BIN_DIR=$1
H=$(mktemp -d)
WORK=$(mktemp -d)
cat > "$H/config.toml" <<TOML
agent = "mock-model"

[projects."$WORK"]
trust_level = "trusted"
$2
TOML
export NORI_HOME=$H CODEX_HOME=$H MOCK_ACP_AGENT_BIN=$BIN_DIR/mock_acp_agent
export MOCK_AGENT_THOUGHT=$'**Reading the config loader**\n\nThe loader merges three sources: the user file, the project file, and command-line overrides. The `tui` table is read last, so a default set there wins.'
export MOCK_AGENT_RESPONSE="I flipped the default in the loader and updated its test."
cd "$WORK"
exec env -u NO_COLOR COLORTERM=truecolor TERM=xterm-256color "$BIN_DIR/nori" --agent mock-model --skip-trust-directory
