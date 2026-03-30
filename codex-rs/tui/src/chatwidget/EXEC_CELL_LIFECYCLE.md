# ExecCell Lifecycle and State Management

This document describes the lifecycle of ExecCells in the TUI, the state they can be in,
and the various transitions between states.

## Core State Components

### 1. `active_cell: Option<Box<dyn HistoryCell>>`

The currently active cell being displayed/edited. Can be:
- `None` - No active cell
- `Some(ExecCell)` - An exec cell (tool call) in progress
- `Some(AgentMessageCell)` - Streaming agent text
- Other cell types...

**Key property**: Only ONE cell can be in `active_cell` at a time. This is rendered
in the main viewport area.

### 2. `pending_exec_cells: PendingExecCellTracker`

Stores incomplete ExecCells that were flushed from `active_cell` before their tool calls
completed. These cells are **INVISIBLE** - they are not rendered anywhere.

Structure:
```
call_id_to_primary: HashMap<String, String>  // Maps any call_id -> primary_key
cells: HashMap<String, Box<dyn HistoryCell>> // Maps primary_key -> cell
```

### 3. History (Scrollback)

Completed cells are sent to history via `AppEvent::InsertHistoryCell`. These appear
in the scrollback area above the active cell.

## ExecCell Properties

Each ExecCell tracks multiple tool calls:
- `calls: Vec<ExecCall>` - The calls in this cell
- `pending_call_ids()` - Returns call_ids that haven't completed yet (no output)
- `is_active()` - True if any calls are still pending
- `is_exploring_cell()` - True if ALL calls are Read/ListFiles/Search operations

## State Diagram

```

                            +------------------+
                            |   (invisible)    |
                            | pending_exec_    |
                            |     cells        |
                            +--------+---------+
                                     |
                            retrieve |  save_pending
                                     v
+------------+   ExecBegin   +--------------+   flush_active_cell  +----------+
|   None     | ------------> |  active_cell | -------------------> |  History |
| (no cell)  |               |   (visible)  |   (if complete)      | (visible)|
+------------+               +--------------+                      +----------+
      ^                              |
      |                              | flush_active_cell
      |                              | (if incomplete)
      |                              v
      |                      +------------------+
      +----------------------|   (invisible)    |
         drain_failed        | pending_exec_    |
         (on TaskComplete)   |     cells        |
                             +------------------+
```

## Event Flow Scenarios

### Scenario 1: Simple Tool Call (No Streaming Interleave)

```
1. ExecBegin(call_id=A)
   - active_cell = new ExecCell{calls: [A], pending: [A]}

2. ExecEnd(call_id=A)
   - Find cell in active_cell
   - Complete call A
   - Cell is complete, flush to history
   - active_cell = None
```

### Scenario 2: Multiple Exploring Calls (Grouped)

```
1. ExecBegin(call_id=A, cmd=Read file1.txt)
   - active_cell = ExecCell{calls: [A], pending: [A]}

2. ExecBegin(call_id=B, cmd=Read file2.txt)
   - with_added_call succeeds (both are exploring)
   - active_cell = ExecCell{calls: [A,B], pending: [A,B]}

3. ExecEnd(call_id=B)
   - Find cell in active_cell
   - Complete call B
   - Cell still active (A pending), keep in active_cell

4. ExecEnd(call_id=A)
   - Find cell in active_cell
   - Complete call A
   - Cell complete and exploring, keep in active_cell (for grouping)

5. StreamingDelta / Next event
   - flush_active_cell sends to history
```

### Scenario 3: Streaming Interleaves with Tool Calls

```
1. ExecBegin(call_id=A, cmd=Read file1.txt)
   - active_cell = ExecCell{calls: [A], pending: [A]}

2. StreamingDelta("Some text...")
   - Check: active_cell is ExecCell with is_active()=true
   - should_flush = false, keep cell in active_cell

3. ExecEnd(call_id=A)
   - Find in active_cell
   - Complete call A
   - Flush to history
```

---

## ACP Tool Event Handling

This section documents the ACP (Agent Client Protocol) tool event behavior and how
the TUI handles it. This is critical knowledge for anyone working on tool call
display or debugging cell lifecycle issues.

### ACP Snapshot Behavior

ACP session-domain tool activity now reaches the TUI as normalized
`nori_protocol::ClientEvent::ToolSnapshot` values rather than as a translated
`ExecCommandBegin/End` stream from the backend. A single tool call typically
produces multiple snapshots for the same `call_id`:

```
Snapshot 1: phase=Pending, title="Read File", invocation=None
Snapshot 2: phase=InProgress, title="Read /home/.../file.rs", invocation=Read { path }
Snapshot 3: phase=Completed, title="Read /home/.../file.rs", artifacts=[...]
```

The ACP backend merges intermediate `ToolCall` and `ToolCallUpdate` messages by
`call_id` before emitting them, so the TUI sees one progressively enriched
snapshot stream rather than raw protocol churn.

### Why This Still Matters For Cell Lifecycle

Even with normalized snapshots, multiple updates for the same `call_id` can
arrive while the viewport is streaming text:

1. Pending/InProgress snapshot creates or updates an `ExecCell`
2. Another snapshot for the same `call_id` arrives with better title/input
3. The completed snapshot arrives after the cell has already been deferred
4. If pairing breaks, the completion can create an orphan cell or leave the
   pending one stuck until `drain_failed()`

The backend now owns the provider-specific normalization, but the TUI still has
to preserve ordering and deduplicate by `call_id`.

### Snapshot Routing In The TUI

`handle_client_tool_snapshot()` in `chatwidget/event_handlers.rs` routes
normalized ACP snapshots into existing cell types:

| Snapshot kind/phase | TUI handling |
|---------------------|--------------|
| `Edit` / `Delete` / `Move` completed with file operations | `PatchHistoryCell` path |
| `Execute` / `Read` / `Search` / `Fetch` / `Think` / `Other` pending or in-progress | Adapt to exec-begin flow |
| Same kinds completed or failed | Adapt to exec-end flow |

This preserves the existing cell presentation without requiring the backend to
reconstruct Codex-shaped event vocabulary.

### Guidelines for Handling ACP Events

When working with ACP tool events, follow these principles:

1. **Never trust a single snapshot** - early pending snapshots are often incomplete
2. **Preserve `call_id` pairing** - begin and completion state must stay correlated
3. **Route by normalized semantics** - file operations, exploring tools, and generic tools take different cell paths
4. **Keep provider quirks out of the TUI** - the backend should normalize titles and raw input before UI rendering
5. **Log thoroughly** - Use the `acp_event_flow` tracing target to debug event issues

### Tool Display Information Extraction

The TUI still derives concise display text from the normalized invocation/raw
input when adapting snapshots into exec-like cells:

| Tool Type | Checked Fields | Output Format |
|-----------|---------------|---------------|
| Search/Grep | pattern, query, path | `{pattern} in {path}` |
| Terminal/Shell | command, cmd | `{command}` |
| List/LS | path, directory | `{path}` |
| Write/Edit | path, file_path | `{path}` |
| Read/File | path, file_path, file | `{path}` |
| Generic | path, command, query, name | First non-null value |

This enables the TUI to show `"Read File(src/main.rs)"` instead of just `"Read File"`.

### Tool Classification for Exploring vs Command Mode

The snapshot adapter maps normalized `ToolKind` and `Invocation` data to TUI rendering modes:

| ACP ToolKind | ParsedCommand | TUI Mode |
|--------------|---------------|----------|
| `Read` | `ParsedCommand::Read` | Exploring (compact) |
| `Search` | `ParsedCommand::Search` | Exploring (compact) |
| `Other` with "list"/"glob"/"ls" in title | `ParsedCommand::ListFiles` | Exploring (compact) |
| `Execute`, `Fetch`, `Think`, generic `Other` | `ParsedCommand::Unknown` | Command (full display) |
| `Edit`, `Delete`, `Move` | N/A | Patch history path |

This enables the TUI to group and collapse read-only operations while showing
mutating operations prominently.

---

## TUI-Side Safeguards

In addition to ACP-side filtering, the TUI has safeguards to prevent cell lifecycle issues:

### Fix 1: Complete ALL Matching Call IDs

Changed `complete_call()` to complete ALL calls with matching call_id, not just
the last one. This prevents duplicate call_ids from leaving cells stuck as "active".

### Fix 2: Reject Duplicate Call IDs in `with_added_call`

Added check in `with_added_call()` to reject calls with duplicate call_ids.

### Fix 3: Don't Flush Incomplete ExecCells During Streaming

Both `handle_streaming_delta()` and `add_boxed_history()` now check if the
active ExecCell is incomplete before flushing. If `is_active()` returns true,
the cell stays in `active_cell` instead of being saved to pending.

### Fix 4: Flush Stream Before Tool End/Begin Events

Tool End events (`on_exec_command_end`, `on_mcp_tool_call_end`, `on_patch_apply_end`)
and `on_mcp_tool_call_begin` now call `flush_answer_stream_with_separator()` before
`defer_or_handle()`. Without this, End events arriving during active text streaming
were deferred to the interrupt queue and only processed at `TaskComplete`, causing
all tool call results to appear after all text instead of interleaved in their correct
order. The flush finalizes any in-progress text stream, allowing the subsequent
`defer_or_handle()` to take the immediate-handle path. This matches the pattern
already used by `on_exec_command_begin`.

---

## Tracing Targets

### TUI-side tracing
- `cell_flushing` - All cell state transitions (flush_active_cell, handle_exec_*_now)
- `pending_exec_cells` - PendingExecCellTracker operations (save_pending, retrieve, drain_failed)
- `tui_event_flow` - Event reception in the TUI (on_agent_message_delta, on_exec_command_begin, on_exec_command_end)

### ACP-side tracing
- `acp_event_flow` - Normalized approval/tool/lifecycle emission from the ACP backend

### Enable all event flow tracing

```bash
RUST_LOG=acp_event_flow=debug,tui_event_flow=debug,cell_flushing=debug,pending_exec_cells=debug
```

### Capture to file for analysis

```bash
RUST_LOG=acp_event_flow=debug,tui_event_flow=debug,cell_flushing=debug,pending_exec_cells=debug \
  codex 2>&1 | tee event_flow.log
```

### What to look for in the logs

1. **Event sequence**: Events should arrive in order (seq=1, 2, 3...)
2. **Snapshot progression**: Verify the same `call_id` moves from pending/in-progress to completed without spawning extra cells
3. **Duplicate detection**: Look for repeated begin adaptation for the same `call_id`
4. **State at reception**: Check `has_active_cell`, `active_cell_is_exec`, `pending_exec_count` at each event
5. **Cell flushing**: Track when cells are saved to pending vs flushed to history
6. **call_id correlation**: Match begin and completion handling for the same normalized ACP snapshot stream
