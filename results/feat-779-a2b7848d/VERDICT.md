# Independent acceptance: #779 at a2b7848d (prompt history from any terminal)

## Verdict: pass, two low notes

Wire probes pass 11 of 12:
- A controller adds; a watcher's add gets `-32016`.
- Metadata counts the additions, and the watcher reads each entry in order.
- Search returns the added entries and not the refused one.
- Entries are stored in the host's `NORI_HOME/history.jsonl`, and the client home is untouched.
- An offset out of range returns `{entry: null}`.

Plain `nori` K01: Up and Ctrl+R recall a prompt from an earlier run, recorded once.

Low notes:
- **L1:** `_nori/host/history/add` accepts an unknown `sessionId` and stores the entry. Other session methods return `-32002`.
- **L2:** `nori resume --last`, which stays in-process and is checked by control S34m, now sends `_nori/host/history/metadata` to the owned host. Main sends no host requests there (`../feat-782-bd7d9f54/s34m-bisect/`). It's functionally harmless because the history file is the same; confirm it's intended, or update the control.
