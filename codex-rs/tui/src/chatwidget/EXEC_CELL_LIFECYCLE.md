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

### ACP ToolCall Event Behavior

**Critical Discovery**: The ACP protocol emits **multiple ToolCall events** for the
same `call_id` as details become available during streaming:

```
Event 1 (early): ToolCall { call_id="toolu_123", title="Read File", raw_input={} }
Event 2 (later): ToolCall { call_id="toolu_123", title="Read /home/.../file.rs", raw_input={path: "..."} }
```

This happens because:
1. The LLM starts generating a tool call, and ACP emits a placeholder event immediately
2. As more tokens stream in, ACP emits updated events with more details
3. The final event contains the complete information (title with path, raw_input with arguments)

### Why This Caused Problems

Without filtering, the TUI would receive both events and try to create cells for each:

1. First ToolCall → Creates ExecCell A in `active_cell`
2. Second ToolCall (same call_id) → `with_added_call` rejects duplicate → Creates NEW cell
3. Old cell A gets flushed to `pending_exec_cells`
4. When ExecEnd arrives, cell A was already "processed" but is stuck in pending
5. Cell is discarded (with warnings) at `drain_failed()` when the turn completes

### The Solution: Two-Layer Filtering

The fix is implemented in `acp/src/backend.rs` with two layers:

#### Layer 1: Skip Generic Events (Primary Filter)

In `translate_session_update_to_events()`, we skip ToolCall events that don't have
useful display information:

```rust
acp::SessionUpdate::ToolCall(tool_call) => {
    // Check for useful display info in raw_input
    let display_args = tool_call
        .raw_input
        .as_ref()
        .and_then(|input| extract_display_args(&tool_call.title, input));

    // Check for useful info in the title itself (some providers put path there)
    let title_has_path = title_contains_useful_info(&tool_call.title);

    // Skip if NEITHER has useful info
    if display_args.is_none() && !title_has_path {
        // Skip this generic placeholder event
        return vec![];
    }

    // Emit the event with complete information
    // ...
}
```

**Why check both?**
- Some ACP providers put the path/command in `raw_input` (e.g., `{path: "/home/user/file.rs"}`)
- Other providers put it in the title itself (e.g., `"Read /home/user/file.rs"`)
- We need to detect either case to avoid skipping legitimate detailed events

#### Layer 2: Dispatch-Loop Deduplication (Safety Net)

Even with Layer 1, edge cases could still allow duplicates through. The dispatch
loop tracks emitted call_ids and skips any that were already sent:

```rust
let mut emitted_begin_call_ids: HashSet<String> = HashSet::new();

while let Some(update) = update_rx.recv().await {
    let events = translate_session_update_to_events(&update);
    for event_msg in events {
        // Safety net: skip duplicate ExecCommandBegin events
        if let EventMsg::ExecCommandBegin(ref begin_ev) = event_msg {
            if emitted_begin_call_ids.contains(&begin_ev.call_id) {
                continue;  // Skip duplicate
            }
            emitted_begin_call_ids.insert(begin_ev.call_id.clone());
        }
        // ... send event to TUI
    }
}
```

### The `title_contains_useful_info()` Function

This function detects when a title contains actionable information even if `raw_input`
doesn't have extractable arguments:

```rust
fn title_contains_useful_info(title: &str) -> bool {
    // Check for absolute paths (Unix or Windows style)
    if title.contains(" /") || title.contains(" C:\\") || title.contains(" ~") {
        return true;
    }

    // Check for backtick-quoted commands (e.g., "`git status`")
    if title.contains('`') {
        return true;
    }

    // Known generic titles that should be skipped
    let generic_patterns = [
        "Read File", "Read file", "Terminal", "Search",
        "Grep", "Glob", "List", "Write", "Edit",
    ];
    for pattern in &generic_patterns {
        if title == *pattern {
            return false;
        }
    }

    // Long titles with spaces likely contain useful info
    title.len() > 15 && title.contains(' ')
}
```

### Guidelines for Handling ACP Events

When working with ACP tool events, follow these principles:

1. **Never trust a single ToolCall event** - The first event for a call_id is often incomplete
2. **Filter early** - Skip events at the translation layer, not in the TUI
3. **Use multiple signals** - Check both `raw_input` and title for useful information
4. **Have a safety net** - Track emitted call_ids to catch any duplicates that slip through
5. **Log thoroughly** - Use the `acp_event_flow` tracing target to debug event issues

### Tool Display Information Extraction

The `extract_display_args()` function extracts human-readable arguments based on tool type:

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

The `classify_tool_to_parsed_command()` function maps ACP ToolKind to TUI rendering modes:

| ACP ToolKind | ParsedCommand | TUI Mode |
|--------------|---------------|----------|
| `Read` | `ParsedCommand::Read` | Exploring (compact) |
| `Search` | `ParsedCommand::Search` | Exploring (compact) |
| `Other` with "list"/"glob"/"ls" in title | `ParsedCommand::ListFiles` | Exploring (compact) |
| `Execute`, `Edit`, `Delete`, `Move`, `Fetch`, `Think` | `ParsedCommand::Unknown` | Command (full display) |

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

---

## Tracing Targets

### TUI-side tracing
- `cell_flushing` - All cell state transitions (flush_active_cell, handle_exec_*_now)
- `pending_exec_cells` - PendingExecCellTracker operations (save_pending, retrieve, drain_failed)
- `tui_event_flow` - Event reception in the TUI (on_agent_message_delta, on_exec_command_begin, on_exec_command_end)

### ACP-side tracing
- `acp_event_flow` - Event emission from ACP backend (translate_session_update_to_events, dispatch loop)

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
2. **Skipped events**: Look for "skipping generic ToolCall" messages - these should be the placeholder events
3. **Duplicate detection**: Look for "skipping duplicate ExecCommandBegin" - these are the safety net catches
4. **State at reception**: Check `has_active_cell`, `active_cell_is_exec`, `pending_exec_count` at each event
5. **Cell flushing**: Track when cells are saved to pending vs flushed to history
6. **call_id correlation**: Match `ExecCommandBegin` and `ExecCommandEnd` by call_id
