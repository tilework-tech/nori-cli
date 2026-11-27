# Source me. Thin tmux helpers for audit terminals (one tmux session per terminal).
# Requires: RUN (run dir), NORI (binary), env file $RUN.env from setup.py.
TT_SOCK=${TT_SOCK:-hostaudit}
tt() { tmux -f /dev/null -L "$TT_SOCK" "$@"; }
# tnew NAME [cols rows] : clean bash with the audit home's env, raw output logged
tnew() {
  local n=$1 c=${2:-120} r=${3:-36}
  tt kill-session -t "$n" 2>/dev/null
  tt new-session -d -s "$n" -x "$c" -y "$r" -c "$ALPHA" \
    "env -i PATH=$(dirname "$NORI"):/usr/bin:/bin TERM=xterm-256color COLORTERM=truecolor LANG=C.UTF-8 HOME=$HOME NORI_HOME=$NORI_HOME CODEX_HOME=$CODEX_HOME NORI_NO_ANALYTICS=1 PS1='shell\$ ' bash --norc --noprofile"
  tt set-option -t "$n" status off >/dev/null
  tt pipe-pane -t "$n" -o "cat >> $RUN/$n.raw"
  sleep 0.5
}
# ttype NAME TEXT : literal text, paced to stay under paste-burst detection
ttype() { local n=$1 s=$2 i; for ((i=0;i<${#s};i++)); do tt send-keys -t "$n" -l "${s:i:1}"; sleep 0.03; done; sleep 0.25; }
# tkey NAME KEY... : named keys (Enter, C-d, Escape, Up, Down, M-t ...), 250 ms apart
tkey() { local n=$1; shift; local k; for k in "$@"; do tt send-keys -t "$n" "$k"; sleep 0.25; done; }
# tcap NAME LABEL : save plain text + ansi of the visible screen
tcap() { mkdir -p "$RUN/cap"; tt capture-pane -p -t "$1" > "$RUN/cap/$2.txt"; tt capture-pane -p -e -t "$1" > "$RUN/cap/$2.ansi"; cat "$RUN/cap/$2.txt"; }
# twait NAME REGEX [secs] : wait until the screen matches
twait() { local n=$1 re=$2 s=${3:-15} i; for ((i=0;i<s*4;i++)); do tt capture-pane -p -t "$n" | grep -qE "$re" && return 0; sleep 0.25; done; echo "TIMEOUT waiting for /$re/ on $n" >&2; return 1; }
