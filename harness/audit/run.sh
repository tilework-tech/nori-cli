#!/usr/bin/env bash
# Repeatable reproduction of the host-audit findings.
#
# usage: harness/audit/run.sh NORI_BINARY OUT_DIR
#
# Builds isolated synthetic homes (Lantern agents, no real credentials), starts
# a background host per scenario on its own NORI_HOME, drives real tmux PTYs,
# and saves a text + ANSI capture of every step under OUT_DIR/cap/.
# Each check prints "OBSERVED" when the screen shows the behavior recorded in
# FINDINGS.md, or "DIFFERS" when it does not (e.g. after a fix).
set -u
NORI=$(readlink -f "$1"); OUT=$(readlink -f "$2"); HERE=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$OUT"; : > "$OUT/checks.txt"
export TT_SOCK=hostaudit-run
source "$HERE/tt.sh"

check() { # check ID DESCRIPTION CAPTURE_LABEL REGEX
  if grep -qE "$4" "$RUN/cap/$3.txt"; then r=OBSERVED; else r=DIFFERS; fi
  printf '%-8s %-4s %s\n' "$r" "$1" "$2" | tee -a "$OUT/checks.txt"
}
host() { env -i HOME="$HOME" NORI_HOME="$NORI_HOME" PATH=/usr/bin:/bin "$NORI" host "$@"; }
scenario() { # scenario NAME DEFAULT_AGENT
  host stop >/dev/null 2>&1; tt kill-server 2>/dev/null
  export RUN="$OUT/$1"; rm -rf "$RUN"
  eval "$(python3 "$HERE/setup.py" "$RUN" "$2")"
  mkdir -p "$RUN/cap"; host >/dev/null; sleep 3
}
attach() { tnew "$1"; ttype "$1" "nori attach ${2:-}"; tkey "$1" Enter; sleep 4; }

{ "$NORI" --version; sha256sum "$NORI"; } > "$OUT/version.txt" 2>&1

# --- S1: zero sessions, then the agent commands --------------------------------
scenario s1-zero claude-code
host ls > "$RUN/cap/host-ls-empty.txt" 2>&1 || true
attach A
tcap A empty-picker >/dev/null
check F6a "empty picker shows developer copy, no /new hint" empty-picker "caller has not supplied any rows"
tkey A Escape; sleep 1
ttype A "/agents"; tkey A Enter; sleep 1.5
tcap A agents-typed >/dev/null
check F3a "/agents is not a command; sent as a prompt with no session" agents-typed "No session is open on this host"
tkey A C-u; ttype A "/"; sleep 1; tcap A slash-menu >/dev/null; tkey A C-u
check F3b "/agent is greyed in attach" slash-menu "/agent +not available in nori attach yet"
check F3c "/config is greyed in attach" slash-menu "/config +not available in nori attach yet"
tkey A C-d; sleep 2; tcap A composer-ctrl-d >/dev/null
check OK1 "Ctrl+D in empty composer detaches" composer-ctrl-d 'shell\$ *$'
attach A; tkey A C-d; sleep 2; tcap A empty-picker-ctrl-d >/dev/null
check OK2 "Ctrl+D in empty picker detaches" empty-picker-ctrl-d 'shell\$ *$'

# --- S2: choosing an agent --------------------------------------------------------
attach A; tkey A Escape; sleep 1
ttype A "/new"; tkey A Enter; sleep 3; ttype A "who answers"; tkey A Enter; sleep 2
tcap A new-default >/dev/null
check OK3 "/new starts the host default agent and replies" new-default "claude-code heard: who answers"
ttype A "/new codex"; tkey A Enter; sleep 3; ttype A "who now"; tkey A Enter; sleep 2
tcap A new-codex >/dev/null
check OK4 "/new <agent> picks the agent (only documented in tui.md)" new-codex "codex heard: who now"
ttype A "/new codx"; tkey A Enter; sleep 3; tcap A new-typo >/dev/null
check F3d "an agent typo leaves a failed session" new-typo "Unknown ACP agent: codx"
tkey A C-d; sleep 1
tnew B; ttype B "nori --agent opencode attach"; tkey B Enter; sleep 4; tkey B Escape; sleep 1
ttype B "/new"; tkey B Enter; sleep 3; ttype B "root flag"; tkey B Enter; sleep 2
tcap B root-agent-flag >/dev/null
check F3e "root --agent is ignored by attach /new (docs say it applies)" root-agent-flag "claude-code heard: root flag"
tkey B C-d; sleep 1

# --- S3: Ctrl+D in a populated picker ----------------------------------------------
attach A; tcap A picker-rows >/dev/null
check F1a "populated picker footer offers ctrl+d close" picker-rows "ctrl\+d close"
before=$(host ls | grep -c ready)
tkey A Down C-d; sleep 2; tcap A picker-ctrl-d >/dev/null
after=$(host ls | grep -c ready)
echo "ready before=$before after=$after" > "$RUN/cap/picker-ctrl-d-count.txt"
[ "$after" -lt "$before" ] && r=OBSERVED || r=DIFFERS
printf '%-8s %-4s %s\n' $r F1b "one Ctrl+D in the picker closes a live session, no confirmation" | tee -a "$OUT/checks.txt"
tkey A Escape; tkey A C-d; sleep 1

# --- S4: failed rows swallow Ctrl+D -------------------------------------------------
scenario s4-failed gemini
attach A; tkey A Escape; ttype A "/new"; tkey A Enter; sleep 4; tkey A C-d; sleep 1
attach A; tcap A failed-picker >/dev/null
for i in 1 2 3 4 5; do tkey A C-d; sleep 1; done
tcap A failed-picker-5x-ctrl-d >/dev/null
check F1c "five Ctrl+D presses on a failed row do nothing" failed-picker-5x-ctrl-d "ctrl\+d close"
tkey A C-r; sleep 2; tcap A failed-ctrl-r >/dev/null
check F1d "ctrl+r resume on a spawn-failed row errors" failed-ctrl-r "no transcript to resume from"
tkey A Escape; sleep 1; tkey A C-d; sleep 2; tcap A failed-esc-ctrl-d >/dev/null
check OK5 "workaround: Esc, then Ctrl+D detaches" failed-esc-ctrl-d 'shell\$ *$'

# --- S5: approvals across two terminals ---------------------------------------------
scenario s5-approve claude-code
attach A; tkey A Escape; ttype A "/new"; tkey A Enter; sleep 3
sid=$(host ls | awk 'NR==2{print substr($1,1,8)}')
attach B "$sid"
ttype A "perm boat"; tkey A Enter; sleep 3
tcap B watcher-overlay >/dev/null
check F4a "watcher sees an actionable-looking approval" watcher-overlay "Allow once \(y\)"
tkey B y; sleep 1
tkey A C-d; sleep 2; tcap A controller-ctrl-d-overlay >/dev/null
check F2  "Ctrl+D does not detach while an approval is open" controller-ctrl-d-overlay "Press enter to confirm"
tkey B M-t; sleep 2; tcap B watcher-took-control >/dev/null
if grep -q "Allow once (y)" "$RUN/cap/watcher-took-control.txt"; then r=DIFFERS; else r=OBSERVED; fi
printf '%-8s %-4s %s\n' $r F4b "after a watcher pressed y, Alt+T never shows the pending approval" | tee -a "$OUT/checks.txt"
attach C "$sid"; tkey C M-t; sleep 2; tcap C fresh-attach-overlay >/dev/null
check OK6 "workaround: a fresh attach plus Alt+T shows the approval" fresh-attach-overlay "Allow once \(y\)"
tkey C y; sleep 2

# --- S6: leave and return ----------------------------------------------------------
ttype C "/new"; tkey C Enter; sleep 3; ttype C "relay 80 reef"; tkey C Enter; sleep 3; tkey C C-d; sleep 2
attach C; tcap C reattach-picker >/dev/null
check F5a "reattach picker cursor starts on the oldest row, not the session just left" reattach-picker "^▏› .*perm boat"
tkey C Escape; tkey C C-d; sleep 1
host ls > "$RUN/cap/host-ls-before-stop.txt"
tnew S; ttype S "nori host stop"; tkey S Enter; sleep 5
tcap B after-host-stop >/dev/null
check F7  "attached clients show RECONNECTING, not 'host stopped'" after-host-stop "RECONNECTING"
ttype S "nori host"; tkey S Enter; sleep 6
host ls > "$RUN/cap/host-ls-after-restart.txt"
tcap B after-host-restart >/dev/null
check F5b "after restart the host lists no sessions" after-host-restart "sessions were lost"
tnew R; ttype R "nori resume"; tkey R Enter; sleep 5; tcap R resume-after-stop >/dev/null
check OK7 "workaround: nori resume (private host) still lists them" resume-after-stop "Resume a previous session"
tkey R C-c; sleep 1

host stop >/dev/null 2>&1; tt kill-server 2>/dev/null
echo "done: $(grep -c OBSERVED "$OUT/checks.txt") observed, $(grep -c DIFFERS "$OUT/checks.txt") differ"
