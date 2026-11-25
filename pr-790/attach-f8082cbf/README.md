# PR #790: a fresh terminal takes another terminal's queued prompt back

Head `f8082cbf` (tests only) on main base `85da3524`; the binary is built from main's product code.
One background `nori host` (mock agent, `MOCK_AGENT_DELAY_MS=300`); each terminal runs
`nori attach --host ... --token-file ...` with its own `NORI_HOME`, `HOME` and folder. 120x40.

| Capture | What it shows |
|---|---|
| `1-a-queues-a-prompt` | Terminal A started `mock:stream-until-cancel`, queued `plant the cedar row` with Tab, then pressed Esc: `1 queued · paused (interrupted)`. A then detached with Ctrl+D and exited. |
| `2-fresh-terminal-takes-it-back` | Terminal B attached fresh, opened the session (already `controlling`, so no Alt+T), pressed Up then Enter: `› plant the cedar row` is in B's composer and nothing is queued. |
| `3-watcher-sees-only-the-corrected-prompt` | Terminal C watched read-only while B appended ` by the fence` and pressed Enter: C's transcript shows only `plant the cedar row by the fence` and its answer; no queue. |

`prompts.jsonl` is the mock agent's prompt log: exactly `mock:stream-until-cancel`, then `plant the cedar row by the fence`.
The original `plant the cedar row` never reached the agent.

The Ghostty Web reference font does not draw `↳`, so the queued row in capture 1 shows without its arrow; `screen.txt` has it.
The `⎇ main` in the footer is the branch of the folder the scratch host ran in.

All data is synthetic.
