# #790 return a queued prompt only to the terminal that typed it

Head: 39fd5283 (tilework-tech/nori-cli PR #790). Debug build of that tree, mock agent (MOCK_AGENT_DELAY_MS=300).

One background `nori --agent mock-model host --port 0` with its own NORI_HOME and HOME; session folder `<host NORI_HOME>/project`.
Terminals A and B each run `nori --agent mock-model attach --host <endpoint> --token-file <host token>` with their own NORI_HOME, HOME and folder.

1. `1-a-holds-two-prompts`: terminal A started `mock:stream-until-cancel`, queued `plant the cedar row` and `water the orchard` with Tab, then pressed Esc. The status line shows "2 queued · paused (interrupted)", with both prompts listed and an `↑ select` hint.
2. `2-b-controls-but-cannot-select`: terminal B opened the same session, took control with Alt+T ("This terminal now controls the host.", `controlling`), and pressed Up. Queue selection does not open: no "j/k select", no `↑ select` hint, and both prompts are still queued. Up recalled `water the orchard` into B's composer from the host-wide prompt history (the host's history.jsonl records queued prompts). This is history recall; the queue still lists both prompts.
3. `3-a-takes-its-own-back`: terminal A took control back with Alt+T, pressed Up ("j/k select" appears), Up again to select the older prompt, then Enter. The status line shows "1 queued · paused (interrupted)" and `› plant the cedar row` is in A's composer; only `water the orchard` is still queued.
4. `4-b-sees-one-left`: terminal B, now `watching · read-only`, also shows "1 queued · paused (interrupted)" with only `water the orchard` left.

Each frame folder holds screen.png (Ghostty Web 0.4.0 render, 120x40), screen.txt, screen.ansi and metadata.json. The reference renderer's font does not draw the `↳` marker in front of queued prompts; screen.txt has it.

All data is synthetic.
