# Attach built-ins acceptance harness (#802 `/agent`, #803 `/model` `/config`, #804 `/mention`)

Drives real `nori host` + `nori attach --host` binaries in tmux PTYs against
Quill, a synthetic ACP agent server (`quill_agent.py`). No real providers or
credentials. Each script prints `PASS`/`FAIL` per check and saves `.txt` and
`.ansi` captures of every step.

| Script | Covers |
|---|---|
| `agent.sh BIN OUT` | `/agent` and `/agents` from an empty host, unavailable agent, in-place change before the first prompt, new session beside a prompted one, watcher and later attach |
| `model.sh BIN OUT` | `/model` and `/config` against agent-confirmed values, rejection, delayed answers across Alt+S and a late attach, watchers, plain `nori` parity |
| `mode.sh BIN OUT` | Native mode change via `/approvals` then a later attach; same-ID replacement by a zero-option agent then a later attach (by ID and through the list) |
| `mention.sh BIN OUT` | `/mention` from distinct host and client folders, a session in another host folder, watchers, image matches, plain `nori` parity |
| `inplace-mode.sh BIN OUT` | In-place `/agent` change to an agent with a native mode: badge and hint row over 12 s, where the next prompt goes, the host receipt, fresh attaches |
| `watcher-two-sessions.sh BIN OUT` | A watcher on a session that changes agent in place while the host has another live session |
| `gates.sh CLIENT HOST OUT LABEL` | Mixed client and host versions (`GATED=1` adds the older-host checks) |
| `mixed.sh OUTROOT` | 1.10 clients on 1.9 hosts and the reverse |
| `macos/make-workflow.sh` | The native macOS verify workflow (PR PTY suites with the Linux gate removed, plus this harness) |
| `run-all.sh NAME BIN OUTROOT` | agent, model, mode and mention against one build |

Host agents: Kestrel and Lark (model and effort options), Moss (no options),
Osprey under the `codex` slug (adds a native `mode` option so `/approvals`
maps onto it), Cedar (command not installed). The client home has only Wren,
so anything the client shows about host agents must come from the host.

Quill reads `$QUILL_CTL/delay-config` (seconds) and `$QUILL_CTL/reject-model`
(a model value to refuse), and answers the prompt `status` with its agent,
model, effort, mode and working directory.

Needs bash, tmux, python3. Pace: typing is 60 ms per key to stay below the
composer's paste-burst detection.
