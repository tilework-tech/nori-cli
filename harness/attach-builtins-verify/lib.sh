# Shared helpers: one synthetic host per scenario, real tmux PTYs per terminal.
# Source with NORI (client binary), OUT (capture dir) and optional HOSTNORI.
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
QUILL=$HERE/quill_agent.py
SOCK=abv-$$
HOSTNORI=${HOSTNORI:-$NORI}
: > "$OUT/checks.txt" 2>/dev/null || true
t() { tmux -L "$SOCK" "$@"; }

setup_host() { # setup_host NAME DEFAULT_AGENT
  ROOT=$(mktemp -d /tmp/abv-XXXX); HOSTHOME=$ROOT/host; PROJECT=$ROOT/host-project
  CLIENT=$ROOT/client; FOLDER=$ROOT/client-folder; OTHER=$ROOT/host-other
  mkdir -p "$HOSTHOME" "$PROJECT" "$CLIENT" "$FOLDER" "$OTHER"
  export QUILL_CTL=$ROOT
  { printf 'agent = "%s"\ncheck_for_update_on_startup = false\n' "$2"
    for d in "$PROJECT" "$OTHER" "$FOLDER"; do printf '[projects."%s"]\ntrust_level = "trusted"\n' "$d"; done
    for a in Kestrel Lark; do
      printf '[[agents]]\nname = "%s"\nslug = "%s"\n[agents.distribution.local]\ncommand = "python3"\nargs = ["%s", "%s"]\n' "$a" "$(printf %s "$a" | tr A-Z a-z)" "$QUILL" "$a"
    done
    printf '[[agents]]\nname = "Moss"\nslug = "moss"\n[agents.distribution.local]\ncommand = "python3"\nargs = ["%s", "Moss", "--bare"]\n' "$QUILL"
    # Osprey takes the codex slug so the host maps /approvals onto its native mode option.
    printf '[[agents]]\nname = "Osprey"\nslug = "codex"\n[agents.distribution.local]\ncommand = "python3"\nargs = ["%s", "Osprey", "--modes", "workspace-write,read-only"]\n' "$QUILL"
    printf '[[agents]]\nname = "Cedar"\nslug = "cedar"\n[agents.distribution.local]\ncommand = "cedar-adapter-not-installed"\n'
  } > "$HOSTHOME/config.toml"
  # The client shares nothing with the host: its own default and one local-only agent.
  { printf 'agent = "wren"\ncheck_for_update_on_startup = false\n[projects."%s"]\ntrust_level = "trusted"\n' "$FOLDER"
    printf '[[agents]]\nname = "Wren"\nslug = "wren"\n[agents.distribution.local]\ncommand = "python3"\nargs = ["%s", "Wren"]\n' "$QUILL"
  } > "$CLIENT/config.toml"
  echo "synthetic tide table" > "$PROJECT/harbor-chart-on-host.md"
  echo "synthetic tide table" > "$OTHER/harbor-chart-in-other-host-folder.md"
  echo "synthetic tide table" > "$FOLDER/harbor-chart-on-client.md"
  export NORI_NO_ANALYTICS=1
  (cd "$PROJECT" && HOME=$HOSTHOME NORI_HOME=$HOSTHOME QUILL_CTL=$ROOT "$HOSTNORI" host --port 0 >"$ROOT/host.out" 2>&1)
  for _ in $(seq 100); do [ -s "$HOSTHOME/host/state.json" ] && break; sleep 0.2; done
  ENDPOINT=$(python3 -c 'import json,sys;print(json.load(open(sys.argv[1]))["endpoint"])' "$HOSTHOME/host/state.json")
  SC=$OUT/$1; mkdir -p "$SC"; echo "$ENDPOINT" > "$SC/endpoint.txt"
}
hostcmd() { (cd "$PROJECT" && HOME=$HOSTHOME NORI_HOME=$HOSTHOME "$HOSTNORI" host "$@"); }
teardown() { t kill-server 2>/dev/null; hostcmd stop >/dev/null 2>&1; cp "$ROOT"/quill-*.log "$SC/" 2>/dev/null; rm -rf "$ROOT"; }

term() { # term NAME [attach args...]  -- a client terminal in the client folder
  local n=$1; shift
  t kill-session -t "$n" 2>/dev/null
  t new-session -d -s "$n" -x 110 -y 40 \
    "cd $FOLDER && HOME=$CLIENT NORI_HOME=$CLIENT NORI_NO_ANALYTICS=1 $NORI attach --host $ENDPOINT --token-file $HOSTHOME/host/token $* ; echo EXITED; sleep 600"
  t set -t "$n" window-style bg=#141414
}
plain() { # plain NAME  -- plain nori on the host machine, same agents
  t kill-session -t "$1" 2>/dev/null
  t new-session -d -s "$1" -x 110 -y 40 \
    "cd $PROJECT && HOME=$HOSTHOME NORI_HOME=$HOSTHOME QUILL_CTL=$ROOT NORI_NO_ANALYTICS=1 $NORI; echo EXITED; sleep 600"
  t set -t "$1" window-style bg=#141414
}
k() { t send-keys -t "$1" "${@:2}"; sleep 0.3; }
typed() { local i; for ((i=0; i<${#2}; i++)); do t send-keys -t "$1" -l "${2:i:1}"; sleep 0.06; done; sleep 0.4; }
cmd() { typed "$1" "$2"; k "$1" Enter; }
screen() { t capture-pane -t "$1" -p; }
wait_for() { # wait_for TERM TEXT [tries]
  for _ in $(seq "${3:-60}"); do screen "$1" | grep -qF -- "$2" && return 0; sleep 0.25; done; return 1; }
down_to() { # move the › pointer onto a row containing TEXT
  local key; for key in Down Up; do for _ in $(seq 15); do screen "$1" | grep '▏›' | grep -qF -- "$2" && return 0; k "$1" $key; done; done; return 1; }
snap() { screen "$1" > "$SC/$2.txt"; t capture-pane -t "$1" -p -e > "$SC/$2.ansi"; }
check() { # check ID PASS|FAIL DESCRIPTION
  printf '%-4s %-10s %s\n' "$2" "$1" "$3" | tee -a "$OUT/checks.txt"; }
expect() { # expect ID FILE REGEX DESCRIPTION
  if grep -qE -- "$3" "$SC/$2.txt"; then check "$1" PASS "$4"; else check "$1" FAIL "$4 (missing /$3/ in $2)"; fi; }
expect_not() {
  if grep -qE -- "$3" "$SC/$2.txt"; then check "$1" FAIL "$4 (found /$3/ in $2)"; else check "$1" PASS "$4"; fi; }
sessions_json() { hostcmd ls --json 2>/dev/null || hostcmd ls 2>/dev/null; }
closed() { # press Esc until no picker is open (a confirmed choice reopens /config)
  for _ in 1 2 3 4; do screen "$1" | grep -qE 'esc close|esc stop search' || return 0; k "$1" Escape; sleep 0.3; done; }
ids() { hostcmd ls 2>/dev/null | awk 'NR>1{print $1}' | sort; }
mark() { ids > "$ROOT/ids.before"; }
new_sid() { for _ in $(seq 20); do local n; n=$(ids | comm -13 "$ROOT/ids.before" - | head -1); [ -n "$n" ] && { echo "$n"; return; }; sleep 0.3; done; }
