#!/usr/bin/env bash
# usage: run-all.sh NAME BINARY OUTROOT  -- every scenario against one build
H=$(cd "$(dirname "$0")" && pwd); N=$1; B=$2; O=$3/$N; mkdir -p "$O"
{ "$B" --version; { sha256sum "$B" 2>/dev/null || shasum -a 256 "$B"; }; } > "$O/version.txt"
for s in agent model mode mention; do mkdir -p "$O/$s-run"; "$H/$s.sh" "$B" "$O/$s-run" > "$O/$s-run/stdout.txt" 2>&1; done
cat "$O"/*-run/checks.txt > "$O/checks.txt"
