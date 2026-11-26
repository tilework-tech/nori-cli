#!/usr/bin/env bash
# Startup-face acceptance scenarios for nori-cli #801. Writes captures under $OUT/<scenario>/.
set -uo pipefail
here=$(cd "$(dirname "$0")" && pwd)
source "$here/lib.sh"
export WORK=${WORK:-/tmp/v801}
log() { echo "[$SCN] $*" | tee -a "$OUT/$SCN/notes.txt"; }
# The composer treats machine-speed bursts as a paste; pace edits like a person.
pk() { sleep 0.25; keys "$@"; sleep 0.25; }
pt() { sleep 0.25; type_text "$@"; sleep 0.25; }
first_frame() { # session: wait until anything is drawn
  for _ in $(seq 1 1000); do cap "$1" | grep -q '[^[:space:]]' && return 0; sleep 0.01; done; return 1; }

s1_delayed() { # gated session/new; type + edit from first frame; release; wink+retype once
  SCN=s1-delayed; read S W < <(bash "$here/run.sh" $SCN 80 24 MOCK_AGENT_NEW_SESSION_RELEASE_FILE=$WORK/$SCN/release)
  first_frame $S; type_text $S "draft tide tabels"; save $S 01-first-frame-typed
  pk $S BSpace; pk $S BSpace; pk $S BSpace; pt $S "les"; for i in 1 2 3 4 5 6; do pk $S Left; done; pt $S "low "
  wait_for $S "〉draft tide low tables" 5 && log "PASS typed+edited draft before readiness"
  sleep 5; save $S 02-holding
  grep -qF "〉_ ▐" $OUT/$SCN/02-holding.txt && ! grep -q "nori CLI\|〉_ -\|options" $OUT/$SCN/02-holding.txt && log "PASS holding open-eyed, no wink/wordmark/config before ready"
  touch $W/release; wait_for $S "nori CLI v" 5; sleep 1; save $S 03-ready
  sleep 0.5; caph $S > $OUT/$SCN/03-ready-history.txt
  log "wordmark count in history: $(grep -c '〉nori CLI' $OUT/$SCN/03-ready-history.txt); Agent rows: $(grep -c 'Agent ' $OUT/$SCN/03-ready-history.txt); face rows: $(grep -c '〉_ ' $OUT/$SCN/03-ready-history.txt)"
  grep '^〉' $OUT/$SCN/03-ready.txt | tail -1 | tee -a $OUT/$SCN/notes.txt
  finish $S
}

s2_submit_early() { # submit before readiness; delivered exactly once after release
  SCN=s2-submit-early; read S W < <(bash "$here/run.sh" $SCN 80 30 MOCK_AGENT_NEW_SESSION_RELEASE_FILE=$WORK/$SCN/release MOCK_AGENT_RESPONSE=Seven_pools_counted.)
  first_frame $S; type_text $S "count the tide pools"; pk $S Enter; sleep 2; save $S 01-submitted-before-ready
  log "prompt log before release: $(cat $W/prompts.log 2>/dev/null | wc -l) lines"
  touch $W/release; wait_for $S "Seven_pools_counted." 10; sleep 2; save $S 02-after-ready; caph $S > $OUT/$SCN/02-history.txt
  log "prompt log after release: $(wc -l < $W/prompts.log) line(s): $(cat $W/prompts.log | tr '\n' '|')"
  log "prompt shown in history: $(grep -c 'count the tide pools' $OUT/$SCN/02-history.txt); responses: $(grep -c 'Seven_pools_counted' $OUT/$SCN/02-history.txt); wordmarks: $(grep -c '〉nori CLI' $OUT/$SCN/02-history.txt)"
  sleep 3; log "prompt log after 3 s more: $(wc -l < $W/prompts.log)"
  finish $S
}

s3_fast() {
  SCN=s3-fast; read S W < <(bash "$here/run.sh" $SCN 80 24)
  first_frame $S; type_text $S "quick note"; wait_for $S "nori CLI v" 8; sleep 1; save $S 01-ready; caph $S > $OUT/$SCN/01-history.txt
  log "wordmarks: $(grep -c '〉nori CLI' $OUT/$SCN/01-history.txt); faces left: $(grep -c '〉_ ' $OUT/$SCN/01-history.txt); composer: $(grep '^〉' $OUT/$SCN/01-ready.txt | tail -1)"
  finish $S
}

s4_slow_init() { # initialize itself delayed 3 s
  SCN=s4-slow-init; read S W < <(bash "$here/run.sh" $SCN 80 24 MOCK_AGENT_STARTUP_DELAY_MS=3000)
  first_frame $S; type_text $S "waiting on init"; sleep 2; save $S 01-waiting
  wait_for $S "nori CLI v" 10; sleep 1; save $S 02-ready; caph $S > $OUT/$SCN/02-history.txt
  log "wordmarks: $(grep -c '〉nori CLI' $OUT/$SCN/02-history.txt); composer: $(grep '^〉' $OUT/$SCN/02-ready.txt | tail -1)"
  finish $S
}

s5_fail() { # $1=name $2=env
  SCN=$1; read S W < <(bash "$here/run.sh" $SCN 80 24 $2)
  first_frame $S; type_text $S "still typing"; sleep 6; save $S 01-after-failure; caph $S > $OUT/$SCN/01-history.txt
  log "wordmark present: $(grep -c 'nori CLI' $OUT/$SCN/01-history.txt); wink frames on screen: $(grep -c '〉_ -' $OUT/$SCN/01-history.txt); open face rows: $(grep -c '〉_ ▐' $OUT/$SCN/01-history.txt)"
  pt $S " more"; sleep 0.5; log "composer after failure: $(cap $S | grep '^〉' | tail -1)"
  finish $S
}

s6_no_anim() {
  SCN=s6-no-animations; read S W < <(bash "$here/run.sh" $SCN 80 24 MOCK_AGENT_NEW_SESSION_RELEASE_FILE=$WORK/s6-no-animations/release -- '[tui]\nanimations = false')
  first_frame $S; sleep 1.5; save $S 01-before-ready
  log "before ready: wordmark=$(grep -c '〉nori CLI' $OUT/$SCN/01-before-ready.txt) face=$(grep -c '〉_ ' $OUT/$SCN/01-before-ready.txt)"
  touch $W/release; wait_for $S "Mock ACP options" 8; sleep 1; save $S 02-ready; caph $S > $OUT/$SCN/02-history.txt
  log "after ready: wordmarks=$(grep -c '〉nori CLI' $OUT/$SCN/02-history.txt) config rows=$(grep -c 'Mock ACP options' $OUT/$SCN/02-history.txt)"
  finish $S
}

s7_narrow() { # 30 columns, gated, then release
  SCN=s7-narrow; read S W < <(bash "$here/run.sh" $SCN 30 20 MOCK_AGENT_NEW_SESSION_RELEASE_FILE=$WORK/s7-narrow/release)
  first_frame $S; type_text $S "a narrow draft that wraps past thirty"; sleep 4; save $S 01-holding
  touch $W/release; wait_for $S "nori CLI" 8; sleep 1; save $S 02-ready; caph $S > $OUT/$SCN/02-history.txt
  log "wordmarks: $(grep -c '〉nori CLI' $OUT/$SCN/02-history.txt); faces left: $(grep -c '〉_ ' $OUT/$SCN/02-history.txt)"
  finish $S
}

for s in "${@:-s1_delayed s2_submit_early s3_fast s4_slow_init s6_no_anim s7_narrow fail_init fail_new}"; do
  for t in $s; do case $t in
    fail_init) s5_fail s5-fail-initialize MOCK_AGENT_INITIALIZE_FAIL=1 ;;
    fail_new) s5_fail s5-fail-new-session MOCK_AGENT_FAIL_NEW_SESSION_FROM=0 ;;
    *) $t ;; esac; done
done
