# Helpers for driving a tmux session started by run.sh.
cap() { tmux capture-pane -p -t "$1"; }                # plain text
cape() { tmux capture-pane -p -e -t "$1"; }            # with SGR colors
caph() { tmux capture-pane -p -S - -t "$1"; }          # including scrollback
wait_for() { # session text timeout_s
  local end=$((SECONDS + ${3:-15}))
  while [ $SECONDS -lt $end ]; do cap "$1" | grep -qF -- "$2" && return 0; sleep 0.05; done
  echo "TIMEOUT waiting for: $2" >&2; cap "$1" >&2; return 1
}
keys() { tmux send-keys -t "$@"; }
type_text() { tmux send-keys -t "$1" -l -- "$2"; }
save() { # session name  -> OUT/scenario/name.txt and name.ansi
  local dir=$OUT/$SCN
  cap "$1" > "$dir/$2.txt"; cape "$1" > "$dir/$2.ansi"
}
finish() { # session
  keys "$1" C-c; sleep 0.3; keys "$1" C-c; sleep 1
  caph "$1" > "$OUT/$SCN/scrollback.txt" 2>/dev/null || true
  tmux kill-session -t "$1" 2>/dev/null || true
}
