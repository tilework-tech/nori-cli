# Noridoc: tui-pty-e2e

Path: @/nori-rs/tui-pty-e2e

### Overview

`tui-pty-e2e` drives the real `nori` binary inside a pseudo-terminal and asserts
on the parsed terminal screen. It is the final regression boundary for behavior
that unit tests cannot prove across process, harness, event, and rendering
layers.

### How it fits into the larger codebase

Tests launch `nori` with `mock-acp-agent`, send keyboard input, and observe
screen output and process lifecycle. The path under test is:

```text
PTY -> nori-tui -> nori-harness -> nori-acp-host -> mock ACP agent
```

### Core Implementation

`portable_pty` supplies the terminal, `vt100` maintains a virtual screen, and
test helpers send keys, wait for stable text, capture output, and enforce exit
deadlines.

Scenarios cover representative raw ACP messages, tools, plans, permissions and
responses; Nori lifecycle and failure behavior; query-driven pickers; cloud
list/resume/close/detach; transcript persistence and view-only selection; MCP
and browser workflows; and ordering races between streaming text and tool
updates.

The protocol hard cut did not introduce a test-only compatibility path. PTY
tests exercise source-first `SessionEvent::{Acp, Nori}` dispatch and the same
typed `HarnessHandle` methods available to headless embedders.

### Things to Know

- Build the mock agent before local runs when CI has not supplied its path.
- Tests require the TUI `vt100-tests` feature.
- Snapshot normalization removes dynamic session IDs, timestamps, and status
  text while retaining meaningful terminal ordering and content.
- Transcript pickers treat a v3 transcript as non-empty only when it has at
  least one user turn; lifecycle records alone do not make it resumable.
- Timing-sensitive scenarios use bounded waits, but assertions target visible
  behavior rather than private reducer calls.

Created and maintained by Nori.
