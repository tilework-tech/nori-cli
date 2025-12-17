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

### Scenario 3: Streaming Interleaves with Tool Calls (THE BUG SCENARIO)

```
1. ExecBegin(call_id=A, cmd=Read file1.txt)
   - active_cell = ExecCell{calls: [A], pending: [A]}

2. StreamingDelta("Some text...")
   - Check: active_cell is ExecCell with is_active()=true
   - FIX: should_flush = false, keep cell in active_cell
   - OLD BUG: would flush incomplete cell to pending_exec_cells (invisible!)

3. ExecEnd(call_id=A)
   - Check pending_exec_cells for A: NOT FOUND (if we kept in active_cell)
   - Find in active_cell
   - Complete call A
   - Flush to history
```

### Scenario 4: Multiple Cells, Complex Interleaving (PROBLEMATIC)

```
1. ExecBegin(call_id=A)
   - active_cell = CellA{pending: [A]}

2. flush_active_cell() [from streaming or other event]
   - CellA is incomplete, save to pending_exec_cells
   - pending = {A: CellA}
   - active_cell = None

3. ExecBegin(call_id=B)
   - active_cell = CellB{pending: [B]}

4. flush_active_cell() [from streaming]
   - CellB is incomplete, save to pending_exec_cells
   - pending = {A: CellA, B: CellB}
   - active_cell = None

5. ExecEnd(call_id=A)
   - Retrieve CellA from pending
   - flush_active_cell() <-- This is called but active_cell should be None!
   - active_cell = CellA
   - Complete A in CellA
   - If CellA still active, it stays in active_cell

6. ExecEnd(call_id=B)
   - Check pending for B: Found CellB
   - flush_active_cell() <-- This flushes CellA to pending again!
   - active_cell = CellB
   - Complete B in CellB
   - CellB may be flushed to history

7. PROBLEM: CellA is now stuck in pending_exec_cells!
   - It was re-saved at step 6
   - No more ExecEnd events for A (already processed at step 5)
   - Cell only appears when drain_failed() runs at TaskComplete
```

## The Root Cause (RESOLVED)

**Primary Issue: Duplicate ExecCommandBegin Events from ACP**

The ACP protocol emits multiple `ToolCall` events for the same `call_id` as details become available:

```
seq=16: ExecCommandBegin call_id=toolu_016g7... title="Read File" (generic)
seq=17: ExecCommandBegin call_id=toolu_016g7... title="Read /home/.../SKILL.md" (detailed)
```

This caused a cascade of problems in the TUI:
1. First Begin creates ExecCell A in active_cell
2. Second Begin (same call_id) triggers "rejecting duplicate call_id" in `with_added_call`
3. This creates a NEW ExecCell, causing the OLD one to be flushed to pending
4. Subsequent Read operations can't merge because they also get duplicate Begins
5. Cells get stuck in pending_exec_cells and only appear at drain_failed

**Fix Applied (acp/src/backend.rs):**

Two-layer deduplication:

1. **At the source** - Skip generic ToolCall events that don't have `raw_input`:
```rust
// In translate_session_update_to_events, for SessionUpdate::ToolCall:
if tool_call.raw_input.is_none() {
    // Skip generic placeholder, wait for detailed event
    return vec![];
}
```

2. **Safety net** - Track emitted call_ids in the dispatch loop:
```rust
let mut emitted_begin_call_ids: HashSet<String> = HashSet::new();
if let EventMsg::ExecCommandBegin(ref begin_ev) = event_msg {
    if emitted_begin_call_ids.contains(&begin_ev.call_id) {
        continue;  // Skip any remaining duplicates
    }
    emitted_begin_call_ids.insert(begin_ev.call_id.clone());
}
```

This ensures:
- Only detailed events (with file paths, commands, etc.) are emitted
- Each call_id gets exactly one ExecCommandBegin event
- The TUI receives complete information for display

---

## Historical Analysis (for reference)

There are TWO issues identified:

### Issue 1: Duplicate Call IDs in a Single Cell

From log analysis, we see: `pending_call_ids=["toolu_016...", "toolu_016..."]`

The same call_id appears TWICE in a single cell's pending list. This can happen if:
- The same ExecBegin event is processed twice
- `with_added_call` doesn't check for duplicate call_ids

When `complete_call` is called:
```rust
if let Some(call) = self.calls.iter_mut().rev().find(|c| c.call_id == call_id) {
    call.output = Some(output);  // Only completes the LAST matching call!
}
```

It uses `.rev().find()` which finds only the LAST call with that ID, leaving the
FIRST duplicate entry still pending. This causes `is_active()` to return true
even after the completion event was processed.

### Issue 2: Flushing Active Cell During Pending Retrieval

In `handle_exec_end_now`:

```rust
if let Some(pending_cell) = self.pending_exec_cells.retrieve(&ev.call_id) {
    self.flush_active_cell();  // <-- THIS CAN SAVE ANOTHER CELL TO PENDING
    self.active_cell = Some(pending_cell);
}
```

When we retrieve a pending cell and there's ANOTHER incomplete cell in `active_cell`,
the `flush_active_cell()` call saves that other cell back to pending. But since its
completion event might have already been processed, it gets stuck.

## Fixes Applied

### Fix 1: Complete ALL matching call_ids (DONE)
Changed `complete_call()` to complete ALL calls with matching call_id, not just
the last one. This prevents duplicate call_ids from leaving cells stuck as "active".

### Fix 2: Reject duplicate call_ids in with_added_call (DONE)
Added check in `with_added_call()` to reject calls with duplicate call_ids.

### Fix 3: Don't flush incomplete ExecCells during streaming (DONE)
Both `handle_streaming_delta()` and `add_boxed_history()` now check if the
active ExecCell is incomplete before flushing. If `is_active()` returns true,
the cell stays in active_cell instead of being saved to pending.

## Remaining Bug: Cell Re-saved Immediately After Retrieval

### Observed Behavior (from .codex-acp.log)

```
15:40:58.344878Z retrieve: found... call_id=toolu_01YUzurZmApey2q4r1Qf9nzz found=true
15:40:58.344925Z flush_active_cell: incomplete ExecCell, saving to pending pending_call_ids=["toolu_01YUzurZmApey2q4r1Qf9nzz"]
```

The SAME cell that was just retrieved is immediately saved back to pending!
This happens because:

1. Cell is retrieved from pending_exec_cells
2. `flush_active_cell()` is called (to preserve any existing active_cell)
3. Retrieved cell is set as active_cell
4. `complete_call()` is called on the cell
5. BUT: Another event arrives and triggers `flush_active_cell()` BEFORE the
   cell is fully complete, saving it back to pending

### The Cascading Effect

When multiple tool calls complete in rapid succession, this pattern cascades:

```
1. ExecEnd(A) arrives
   - Retrieve CellA from pending
   - flush_active_cell() (nothing there)
   - active_cell = CellA
   - complete_call(A) - but CellA has call B still pending!

2. ExecEnd(B) arrives (before CellA is flushed to history)
   - Retrieve CellB from pending (if it exists) OR check active_cell
   - flush_active_cell() - CellA is STILL incomplete (call B not done!)
   - CellA gets saved to pending AGAIN
   - But ExecEnd(A) already happened, so CellA has no more completion events

3. CellA is now stuck in pending until drain_failed()
```

### User-Visible Symptoms

1. Explored cells "flicker" - briefly appear, then disappear
2. Multiple cells reappear at the END of the assistant turn
3. The `drain_failed: drained_count=3` log shows 3 cells were stuck

### Screen Output Pattern

```
─ Worked for 2s ───────────────────────────────────────────────────────────

• Following Nori workflow...

<MULTIPLE EXPLORED CELLS BRIEFLY FLICKER HERE>
• Explored
  └ Read file

─ Worked for 15s ──────────────────────────────────────────────────────────

• I've read the using-skills ability...

<THE STUCK CELLS REAPPEAR AT THE END VIA drain_failed>
• Explored
  └ Search history.*cell|exec.*cell in /home/...

• Explored
  └ Read SKILL.md

• Explored
  └ Read render.rs, file
```

### Root Cause Analysis

The fundamental problem is that `handle_exec_end_now` uses this pattern:

```rust
if let Some(pending_cell) = self.pending_exec_cells.retrieve(&ev.call_id) {
    self.flush_active_cell();  // <-- Can re-save a different incomplete cell!
    self.active_cell = Some(pending_cell);
    // ... complete the call ...
}
```

When completing call A on a cell that also has call B pending:
- After completing A, the cell is still "active" (B is pending)
- If ExecEnd(B) arrives and there's a DIFFERENT cell in pending for B...
- We retrieve that cell and call flush_active_cell()
- The cell with completed-A-but-pending-B gets saved to pending
- But A's ExecEnd was already processed! No event will retrieve it again.

### Proposed Fix Options

**Option A: Don't flush when the active cell will be replaced**
If we're about to replace active_cell with a retrieved pending cell, AND the
current active_cell is an incomplete ExecCell, don't save it to pending.
Instead, check if any of its pending call_ids match call_ids that have already
been completed (need to track completed call_ids).

**Option B: Track completed call_ids globally**
Keep a `HashSet<String>` of call_ids that have received ExecEnd events.
Before saving a cell to pending, filter out any call_ids that are in this set.
If all call_ids have been completed, send the cell to history instead.

**Option C: Process all pending completions atomically**
When retrieving a cell from pending, check if any OTHER pending ExecEnd events
exist for the same cell's call_ids. If so, complete them all before allowing
the cell to be flushed again.

**Option D: Defer the flush_active_cell call**
Instead of calling flush_active_cell() immediately when retrieving from pending,
defer it until after the completion is applied. Only flush if the cell is STILL
incomplete after completion.

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
2. **Interleaving**: Look for `AgentMessageDelta` events arriving between `ExecCommandBegin` and `ExecCommandEnd`
3. **State at reception**: Check `has_active_cell`, `active_cell_is_exec`, `pending_exec_count` at each event
4. **Cell flushing**: Track when cells are saved to pending vs flushed to history
5. **call_id correlation**: Match `ExecCommandBegin` and `ExecCommandEnd` by call_id
