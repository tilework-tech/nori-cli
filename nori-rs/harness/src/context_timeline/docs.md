# Noridoc: Context timeline

Path: @/nori-rs/harness/src/context_timeline

### Overview

- The Harness derives prompt-boundary checkpoints and observed context growth
  from raw ACP usage and Nori lifecycle events. This gives `/context` historical
  detail while retaining the original provider measurements.
- The public [read model](../context_timeline.rs) contains detached snapshots;
  the [reducer](reducer.rs) is private and shared by live and saved sessions.
  No computed checkpoint record is written to the transcript.

### How it fits into the larger codebase

```text
ACP notifications + Nori prompt lifecycle
                  |
             Harness runtime
          stamp identity and time
                  |
          context timeline reducer
             /              \
     live read snapshot    raw evidence -> transcript writer
             |                              |
        /context view             transcript timeline reducer
```

- [`runtime.rs`](../runtime.rs) updates the reducer before recording and
  publishing each public event. `HarnessHandle::context_timeline().await`
  clones the current read model under a shared lock without submitting an
  agent request or waiting for a running prompt to finish. It does not reread
  JSONL or rerun the reducer for each query.
- The [transcript reader](../transcript/context_timeline.rs) exposes
  `Transcript::context_timeline()` and seeds the live reducer when a local
  transcript is supplied for resume. The [TUI](../../../tui/docs.md) owns
  presentation; [shared components](../../../tui-components/docs.md) know only
  caller-provided measured spans.
- ACP remains the owner of `UsageUpdate`, including its original metadata.
  Observation identity and time belong to Nori's storage envelope. Provider
  transcript estimates and cumulative billing totals do not enter this model.

### Core Implementation

- A Nori `Prompting` phase opens a checkpoint for the admitted prompt's actual
  wire request ID. Its correlated prompt response or terminal failure closes
  it. Cancelling is an intermediate phase, and repeated boundaries cannot
  close a turn twice. Queue transitions preserve the completed phase before
  dispatching the next prompt in the
  [session runtime driver](../backend/session_runtime_driver.rs).
- Every boundary snapshots the latest known usage with its original observation
  identity and time. A missing measurement stays unknown. Completed checkpoints
  are immutable: a late update changes current usage without revising an
  already completed turn. Usage from another ACP session is ignored.
- Differences require known observations in the same context epoch and with
  the same capacity. Carrying the same observation across both boundaries is
  not a new measurement and yields no delta; a fresh equal measurement yields
  zero. An active turn compares its start against the latest current reading.
- The first reading becomes an unattributed baseline. Subsequent growth is
  associated with the active Nori prompt, or left unattributed when no prompt
  is active; adjacent spans with the same association are combined. Shrink,
  capacity changes, compaction, and replacement contexts invalidate the prior
  segmentation. Compaction clears current usage until another reading arrives.
- Resume archives the necessary raw timeline events with their original
  identities and times, independently of conversation replay. Archived evidence
  is excluded from `Transcript::records()` and public replay. The reducer
  ignores replayed notifications as new observations and deduplicates evidence
  identities. Fork inherits the parent evidence through transcript seeding.
  See [transcript storage](../../../../docs/transcripts.md).

### Things to Know

- Turn association describes when Nori observed context growth. ACP usage does
  not identify a model request or prove that tokens attributed to a turn remain
  in the model's retained context. Segment sums describe the current measured
  baseline and growth, not cumulative token consumption.
- Missing local history is explicit. Older records use their stored timestamp
  and a transcript-relative identity when original observation metadata is
  absent. Resumed unfinished history is closed with an unknown end reading and
  marked incomplete rather than connected to a new live request.
- Persistence remains asynchronous and best effort. A readable live snapshot
  does not promise that its evidence has reached disk; transcript flush is a
  write barrier, not an `fsync` guarantee. The in-memory timeline also works
  when a recorder is unavailable.
- These APIs are local Harness reads. They do not add an ACP wire query or
  fabricate remote historical prompt boundaries that were never recorded.
- Debug events on the `nori_context_profile` target measure snapshot lock wait
  and clone time separately and include turn/segment sizes without recording
  ACP payloads. The [profiling fixture](../../tests/context_timeline/profile.rs)
  compares repeated JSONL loads, reductions, and live queries with representative
  message sizes and usage metadata; fixture generation is outside the timing.

Run the ignored profile from `nori-rs` with the mock ACP agent built. Set a
shared artifact directory to compare it with the
[TUI instruction inventory profile](../../../tui/docs.md#context-inspection-and-profiling):

```sh
NORI_CONTEXT_PROFILE_DIR="$(pwd)/../.local/context-profile" cargo test -p nori-harness --test context_timeline profile_load_reduce_and_live_snapshot -- --ignored --nocapture --test-threads=1
```

The JSONL artifacts retain individual samples and fixture dimensions. Compare
build mode and cold/warm state when interpreting measurements; the Harness
query, transcript reconstruction, and TUI inventory are different workloads.

Created and maintained by Nori.
