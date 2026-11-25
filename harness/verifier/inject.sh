#!/bin/bash
# Usage: bash inject.sh <nori-cli checkout> ; copies verifier test modules into nori-rs/tui/src/app
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
app="$1/nori-rs/tui/src/app"
for m in host_link_verifier joined_feed_verifier; do
  [ -f "$app/${m#*_verifier}" ] || true
  case $m in joined_feed_verifier) [ -f "$app/joined_feed.rs" ] || continue;; esac
  cp "$here/$m.rs" "$app/$m.rs"
  grep -q "mod $m;" "$app/mod.rs" || printf '\n#[cfg(test)]\nmod %s;\n' "$m" >> "$app/mod.rs"
done
