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

## The Root Cause

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

## Fix Options

### Option A: Don't flush when retrieving from pending
If we're about to set active_cell from a pending cell, we shouldn't have anything
in active_cell (since streaming should have flushed it). If there is something,
it might be another pending cell that was restored - we should handle it differently.

### Option B: Track "already completed" call_ids
Keep a set of call_ids that have already been completed. When flushing an incomplete
cell to pending, check if any of its call_ids have already been completed. If so,
don't save to pending.

### Option C: Complete all matching calls when retrieving
When we retrieve a cell from pending and move to active_cell, check if there are
any other pending completion events for this cell's call_ids.

## Tracing Targets

- `cell_flushing` - All cell state transitions
- `pending_exec_cells` - PendingExecCellTracker operations

Enable with: `RUST_LOG=cell_flushing=debug,pending_exec_cells=debug`
