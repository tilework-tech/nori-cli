# Tool Call Cell Lifecycle

How ACP tool calls become history cells in the chat widget, and how those cells
render. Every tool call, whatever its kind, renders through
[`ClientToolCell`](../client_tool_cell.rs); command-line rendering helpers live
in [`exec_cell`](../exec_cell/render.rs).

## Input: tool snapshots

ACP tool activity crosses the Harness boundary as raw `SessionEvent::Acp`
notifications. The TUI's private [presentation](../presentation) reducer merges
`ToolCall` and `ToolCallUpdate` messages by `call_id` into progressively
enriched `ToolSnapshot` values. One tool call usually yields several snapshots:

```
phase=Pending     title="Read File"            invocation=None
phase=InProgress  title="Read /repo/src/a.rs"  invocation=Read { path }
phase=Completed   title="Read /repo/src/a.rs"  artifacts=[...] raw_output=...
```

Early snapshots are often incomplete, so a cell must accept later snapshots for
the same `call_id` and re-render from the newest one. Display heuristics stay in
the TUI presentation layer, never in `nori-protocol`.

## State held by `ChatWidget`

| State | Role |
| --- | --- |
| `active_cell` | The one live cell rendered in the viewport (a tool cell, streaming text, or another cell kind). |
| `pending_client_tool_cells` | Incomplete **Execute** cells displaced from `active_cell` by a newer tool call (parallel calls). Invisible until their completion arrives. |
| `completed_client_tool_calls` | `call_id`s already sent to history. Later snapshots for these ids are dropped so they cannot spawn duplicate cells. |
| History | Cells sent with `AppEvent::InsertHistoryCell`; they land in scrollback and the transcript overlay. |

`pending_client_tool_cells` is cleared (its cells are discarded) at turn
completion, turn finalization, and context compaction.
`completed_client_tool_calls` is cleared when a turn starts and ends.

## Routing a snapshot

`handle_client_tool_snapshot` in [event_handlers.rs](event_handlers.rs) applies
the first matching rule:

```
snapshot
  |
  +-- same call_id as active ClientToolCell --> apply_snapshot; flush if it is
  |                                             now terminal and not exploring
  +-- call_id in completed_client_tool_calls -> drop
  +-- call_id in pending_client_tool_cells ---> apply_snapshot; if terminal,
  |                                             insert straight into history
  |                                             (active_cell untouched), else
  |                                             put back in the buffer
  +-- exploring snapshot and active cell -----> merge_exploring into the group
  |   is an exploring group
  +-- otherwise: new cell
        finalize the answer stream
        displace active_cell: an incomplete Execute cell goes to
          pending_client_tool_cells; anything else is flushed to history
        create ClientToolCell (with_tool_output, with_output_max_lines from config); mark_exploring
          if exploring
        flush immediately if the snapshot is already terminal and not exploring
```

Buffered cells are inserted directly into history rather than through
`add_boxed_history`, because that path flushes `active_cell` first and would
prematurely finalize the currently running Execute cell.

`flush_active_cell` in [user_input.rs](user_input.rs) sends the active cell to
history. For a tool cell it records the cell's `call_id` in
`completed_client_tool_calls` if it is still running, and records every
`call_id` in its exploring group, so completions arriving after the flush are
dropped instead of re-merged into a later group. At turn end,
`finalize_active_cell_as_failed` marks a still-running active tool cell failed
before flushing it.

Invariant: the answer stream is never open while a tool cell is active. Every
path that sets `active_cell` finalizes the stream first, and every answer delta
flushes `active_cell`. In-place snapshot updates rely on this and skip the
stream flush. No-op updates must not finalize the stream, or one assistant
message fragments into several cells.

## Exploring groups

`is_exploring_snapshot` in [client_event_format.rs](../client_event_format.rs)
treats as exploring: `Read` and `Search` kinds, `ListFiles` invocations, and
`Execute` calls whose command `exec_cell::is_read_only` accepts. That check
parses the command with `nori_harness::bash::summarize` and requires every
stage to be a known read or search tool (leading setup segments such as `cd`
and `export` allowed), with argument guards for tools that can mutate (`sed`
needs `-n` and no `-i`; `find` rejects `-delete`/`-exec`/`-ok`; `awk` rejects
`system(` and `>`), no file-writing redirect other than `/dev/null`, and no
heredoc.

Consecutive exploring snapshots accumulate in one cell's `exploring_snapshots`
list. A completed exploring group stays in `active_cell` so later exploring
calls can join it; it is flushed when the next non-exploring cell, answer text,
or other history arrives. Each step stays a separate entry; reads are not merged
into one line.

## Rendering: compact and full

`ClientToolCell::display_lines` is the full view unless
`NoriConfig::tool_output` resolves to compact (command-line override, the
project's `tool_output`, or `[tui] tool_output`; see
[config.md](../../../config.md)). `transcript_lines` (the Ctrl+T/Ctrl+O
overlay) is always full. The `nori attach` frontend builds cells without
`with_tool_output`, so they keep `ToolOutput::default()` and render full.

`with_output_max_lines` adds an optional line cap from
`NoriConfig::tool_output_max_lines` (off when unset). It changes only
`display_lines`, and only where the table below says "capped". A capped block
keeps its first and last lines and puts one faint italic `┄ N lines hidden`
row in place of the middle (built by `exec_cell::hidden_lines_marker`). Lines
are counted before wrapping. `transcript_lines` and `nori attach` cells are
never capped.

| Cell | Compact | Full |
| --- | --- | --- |
| Execute | `• Ran`/`Running` + `compact_command` summary, then one `└` line with the last non-blank output line (ANSI stripped, dim, red on failure); no `└` line when there is no output | `• Ran` + the whole command, syntax highlighted with command names bold (`full_command_lines`), then all output dim under `└` (capped) |
| Exploring group | `• Explored N reads · M searches · K failed` + `└` the last step's label | Header, then every step with its own status dot; Execute steps render in full with all output (never capped) |
| Create/Edit/Delete/Move | Header only: `Edited path (+A -R)` or `Edited N files (+A -R)` (`Moved` for moves); failures show the error under `└` | Header plus the diff |
| Read/Search/Fetch/Think/Other (outside an Exploring group) | Header plus invocation and artifact detail lines (capped; same in both views) | Same |

Status is carried by the bullet colour alone: green for success, red for
failure, a spinner while running. Execute success comes from `raw_output.exit_code`
when present, otherwise from the snapshot phase. There is no status word or
duration suffix.

`compact_command` summarizes a shell command line to fit the available width:
command names (basenames, plus the script name for interpreters such as
`python3 tools/gen.py`) in bold, arguments as a faint `…`, pipes inline, and
`&&`/`||`/`;`/newline separators between segments. Leading setup segments
(`cd`, `export`, `pushd`, `source`) are dropped when another segment follows,
`VAR=` assignments are never shown, heredocs show as `<<heredoc (N lines)`, and
segments that do not fit collapse into `+N more`. If the command cannot be
parsed into any segment, its first line is truncated instead.

The transcript overlay ([pager_overlay.rs](../pager_overlay.rs)) has its own
fixed row cap per cell, separate from `tool_output_max_lines`. It ends a capped
cell with a faint italic `┄ N more lines` row, built inline in the overlay, so
the marker never reads as command output.

## Debugging

Use the `acp_event_flow` tracing target for raw ACP transport and Harness event
flow:

```bash
RUST_LOG=acp_event_flow=debug nori 2>&1 | tee event_flow.log
```

Check that each `call_id` moves from pending or in-progress to a terminal phase
without producing a second cell. The PTY suite in `@/nori-rs/tui-pty-e2e/`
covers real-terminal tool call rendering and transcript keys.
