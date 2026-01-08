# Fix Skills Tracking Implementation Plan

**Goal:** Track skills when they are read via the Read tool, not just when the Skill tool is invoked.

**Architecture:** Add skill detection for Read tool calls to SKILL.md files alongside the existing Skill tool detection. When a Read tool call is made to a path matching `/.claude/skills/{skill-name}/SKILL.md`, extract and record the skill name.

**Tech Stack:** Rust, regex crate (already in dependencies)

---

## Testing Plan

I will add unit tests to `session_stats.rs` that verify:
1. `extract_skill_from_read_path` correctly extracts skill names from valid SKILL.md paths
2. `extract_skill_from_read_path` returns None for non-skill paths
3. `extract_skill_from_read_path` handles various path formats (home dir, tilde expansion, etc.)

NOTE: I will write *all* tests before I add any implementation behavior.

---

## Implementation Steps

### Step 1: Write failing tests for `extract_skill_from_read_path`

**File:** `/home/amol/code/nori/nori-cli/.worktrees/fix-skills-tracking/codex-rs/tui/src/session_stats.rs`

Add tests after line 393 (after the existing `extract_subagent_from_missing_field_returns_none` test):

```rust
#[test]
fn extract_skill_from_read_path_with_absolute_path() {
    let result = extract_skill_from_read_path(Some("/home/user/.claude/skills/brainstorming/SKILL.md"));
    assert_eq!(result, Some("brainstorming".to_string()));
}

#[test]
fn extract_skill_from_read_path_with_tilde_path() {
    let result = extract_skill_from_read_path(Some("~/.claude/skills/test-driven-development/SKILL.md"));
    assert_eq!(result, Some("test-driven-development".to_string()));
}

#[test]
fn extract_skill_from_read_path_with_non_skill_path() {
    let result = extract_skill_from_read_path(Some("/home/user/code/project/src/main.rs"));
    assert_eq!(result, None);
}

#[test]
fn extract_skill_from_read_path_with_none() {
    let result = extract_skill_from_read_path(None);
    assert_eq!(result, None);
}

#[test]
fn extract_skill_from_read_path_with_partial_skill_path() {
    // Not a SKILL.md file
    let result = extract_skill_from_read_path(Some("/home/user/.claude/skills/brainstorming/README.md"));
    assert_eq!(result, None);
}
```

### Step 2: Run tests to verify they fail

```bash
cd /home/amol/code/nori/nori-cli/.worktrees/fix-skills-tracking/codex-rs && cargo test -p codex-tui extract_skill_from_read_path
```

### Step 3: Implement `extract_skill_from_read_path`

**File:** `/home/amol/code/nori/nori-cli/.worktrees/fix-skills-tracking/codex-rs/tui/src/session_stats.rs`

Add after `extract_subagent_from_raw_input` (around line 229):

```rust
/// Extract skill name from a Read tool call's file_path.
///
/// Matches paths like:
/// - `/home/user/.claude/skills/skill-name/SKILL.md`
/// - `~/.claude/skills/skill-name/SKILL.md`
///
/// Returns the skill name if the path matches, None otherwise.
pub fn extract_skill_from_read_path(file_path: Option<&str>) -> Option<String> {
    use regex::Regex;

    let path = file_path?;

    // Match paths like ~/.claude/skills/{skill-name}/SKILL.md
    // or /home/user/.claude/skills/{skill-name}/SKILL.md
    let re = Regex::new(r"[/~]\.claude/skills/([^/]+)/SKILL\.md$").ok()?;

    re.captures(path)
        .and_then(|caps| caps.get(1))
        .map(|m| m.as_str().to_string())
}
```

### Step 4: Run tests to verify they pass

```bash
cd /home/amol/code/nori/nori-cli/.worktrees/fix-skills-tracking/codex-rs && cargo test -p codex-tui extract_skill_from_read_path
```

### Step 5: Write failing test for Read tool integration

**File:** `/home/amol/code/nori/nori-cli/.worktrees/fix-skills-tracking/codex-rs/tui/src/session_stats.rs`

Add a test that verifies the integration function exists and works:

```rust
#[test]
fn extract_skill_from_raw_input_for_read_tool() {
    let raw_input = json!({"file_path": "/home/user/.claude/skills/using-skills/SKILL.md"});
    let result = extract_skill_from_read_file_path(Some(&raw_input));
    assert_eq!(result, Some("using-skills".to_string()));
}

#[test]
fn extract_skill_from_raw_input_for_read_tool_non_skill() {
    let raw_input = json!({"file_path": "/home/user/code/main.rs"});
    let result = extract_skill_from_read_file_path(Some(&raw_input));
    assert_eq!(result, None);
}
```

### Step 6: Implement `extract_skill_from_read_file_path`

**File:** `/home/amol/code/nori/nori-cli/.worktrees/fix-skills-tracking/codex-rs/tui/src/session_stats.rs`

Add after `extract_skill_from_read_path`:

```rust
/// Extract skill name from a Read tool call's raw_input JSON.
///
/// The Read tool is invoked with `{"file_path": "/path/to/file"}`.
/// If the file_path matches a SKILL.md pattern, returns the skill name.
pub fn extract_skill_from_read_file_path(raw_input: Option<&serde_json::Value>) -> Option<String> {
    let file_path = raw_input
        .and_then(|v| v.get("file_path"))
        .and_then(|v| v.as_str())?;

    extract_skill_from_read_path(Some(file_path))
}
```

### Step 7: Update `handle_mcp_begin_now` in chatwidget.rs

**File:** `/home/amol/code/nori/nori-cli/.worktrees/fix-skills-tracking/codex-rs/tui/src/chatwidget.rs`

Around line 1336, after the existing Skill tool check, add:

```rust
// Check if this is a Read tool call to a SKILL.md file
if ev.invocation.tool == "Read"
    && let Some(skill_name) = extract_skill_from_read_file_path(ev.invocation.arguments.as_ref())
{
    self.session_stats.record_skill(&skill_name);
}
```

Also update the import at the top of the file to include the new function:
```rust
use crate::session_stats::{
    extract_skill_from_raw_input, extract_skill_from_read_file_path,
    extract_subagent_from_raw_input, SessionStats,
};
```

### Step 8: Add regex to Cargo.toml (if not present)

Check if regex is already a dependency:
```bash
grep -q "^regex" /home/amol/code/nori/nori-cli/.worktrees/fix-skills-tracking/codex-rs/tui/Cargo.toml
```

If not present, add to `[dependencies]`:
```toml
regex = "1"
```

### Step 9: Run all tests

```bash
cd /home/amol/code/nori/nori-cli/.worktrees/fix-skills-tracking/codex-rs && cargo test -p codex-tui
```

### Step 10: Manual verification

1. Build the TUI
2. Run a session
3. Read a SKILL.md file
4. Verify the skill appears in session statistics

---

**Testing Details:** The tests verify the behavior of skill extraction from Read tool file paths by testing various path formats (absolute, tilde-prefixed, non-skill paths). Tests do not just test mocks - they test the actual regex matching and string extraction behavior.

**Implementation Details:**
- Uses regex pattern `[/~]\.claude/skills/([^/]+)/SKILL\.md$` matching nori-profiles' approach
- Skill detection happens at MCP tool call begin time
- Skills are deduplicated (existing behavior in `record_skill`)
- No changes to display logic needed - skills already display correctly when recorded

**Questions:**
- Should we also track skills read by subagents? (Currently subagent Read calls wouldn't be captured)
- Should skill tracking include the full path or just the skill name? (Currently just the name, matching nori-profiles)

---
