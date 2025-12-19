# Nori Footer Customization Implementation Plan

**Goal:** Replace the generic "100% context left · ? for shortcuts" footer with Nori-specific status information including git branch, profile name, Nori version, and git modification counts.

**Architecture:** Extend the existing `FooterProps` structure to include new Nori-specific fields (git branch, profile, version, git stats). Create a new data collection module that periodically gathers this information from system commands and config files. Modify the footer rendering logic to conditionally display segments based on data availability (git info only in repos, Nori info only when nori-ai is installed).

**Tech Stack:** Rust, ratatui for TUI rendering, tokio for async/periodic refresh, serde_json for config parsing

---

## Testing Plan

I will add unit tests for:
1. **Config parsing behavior** - Test that profile name is correctly extracted from .nori-config.json files, handling cases where the file doesn't exist, is malformed, or uses different profile structures
2. **Git status parsing behavior** - Test that git branch names and modification counts (+/- lines) are correctly parsed from command output, handling detached HEAD, no repo, and clean/dirty states
3. **Footer segment conditional rendering** - Test that git segments only appear when in a git repo, Nori segments only appear when nori-ai is available, and the footer gracefully degrades when data is unavailable
4. **Footer formatting behavior** - Test that the footer string is correctly formatted with proper separators, spacing, and handles edge cases like very long branch names or missing data

I will add integration tests for:
1. **Full footer rendering** - Test the complete footer render pipeline with mocked system state (in repo, has nori-ai) to verify all segments appear correctly
2. **Degraded states** - Test footer rendering when not in a git repo, when nori-ai is not installed, to verify graceful fallback behavior
3. **Refresh mechanism** - Test that the footer data refresh happens periodically and updates the display without blocking the UI

NOTE: I will write *all* tests before I add any implementation behavior.

---

## Implementation Tasks

### Phase 1: Data Structure Setup

**Task 1.1:** Add new fields to `FooterProps` struct
- File: `/home/clifford/Documents/source/nori/cli/.worktrees/nori-footer-customization/codex-rs/tui/src/bottom_pane/footer.rs`
- Add to `FooterProps`:
  - `git_branch: Option<String>`
  - `nori_profile: Option<String>`
  - `nori_version: Option<String>`
  - `git_lines_added: Option<i32>`
  - `git_lines_removed: Option<i32>`
- Update all `FooterProps` construction sites to include `None` for new fields initially

**Task 1.2:** Write failing tests for FooterProps construction
- File: `/home/clifford/Documents/source/nori/cli/.worktrees/nori-footer-customization/codex-rs/tui/src/bottom_pane/footer.rs` (at bottom in `#[cfg(test)]` module)
- Test that `FooterProps` can be constructed with all new optional fields
- Run tests to ensure they compile and pass

### Phase 2: Data Collection Module

**Task 2.1:** Create new module for system info collection
- File: `/home/clifford/Documents/source/nori/cli/.worktrees/nori-footer-customization/codex-rs/tui/src/system_info.rs` (new file)
- Create module with struct `SystemInfo`:
  ```rust
  pub struct SystemInfo {
      pub git_branch: Option<String>,
      pub nori_profile: Option<String>,
      pub nori_version: Option<String>,
      pub git_lines_added: Option<i32>,
      pub git_lines_removed: Option<i32>,
  }
  ```
- Add to `/home/clifford/Documents/source/nori/cli/.worktrees/nori-footer-customization/codex-rs/tui/src/lib.rs`: `mod system_info;`

**Task 2.2:** Write failing test for git branch detection
- File: `/home/clifford/Documents/source/nori/cli/.worktrees/nori-footer-customization/codex-rs/tui/src/system_info.rs`
- Test function `test_get_git_branch_in_repo()` - mock `git branch --show-current` output
- Test function `test_get_git_branch_not_in_repo()` - mock command failure
- Run tests to verify they fail

**Task 2.3:** Implement `get_git_branch() -> Option<String>`
- File: `/home/clifford/Documents/source/nori/cli/.worktrees/nori-footer-customization/codex-rs/tui/src/system_info.rs`
- Execute `git branch --show-current` using `std::process::Command`
- Return `Some(branch)` on success, `None` on failure
- Run tests to verify they pass

**Task 2.4:** Write failing test for git modification stats
- File: `/home/clifford/Documents/source/nori/cli/.worktrees/nori-footer-customization/codex-rs/tui/src/system_info.rs`
- Test function `test_get_git_stats_with_changes()` - mock `git diff --shortstat` output like "2 files changed, 10 insertions(+), 3 deletions(-)"
- Test function `test_get_git_stats_clean()` - mock empty output
- Test function `test_parse_git_shortstat()` - test parsing logic for various formats
- Run tests to verify they fail

**Task 2.5:** Implement `get_git_stats() -> (Option<i32>, Option<i32>)`
- File: `/home/clifford/Documents/source/nori/cli/.worktrees/nori-footer-customization/codex-rs/tui/src/system_info.rs`
- Execute `git diff HEAD --shortstat` using `std::process::Command` (shows cumulative changes from HEAD, staged or unstaged)
- Parse output to extract insertions/deletions counts
- Return `(Some(added), Some(removed))` or `(None, None)` if not in repo
- Run tests to verify they pass

**Task 2.6:** Write failing test for nori-ai version detection
- File: `/home/clifford/Documents/source/nori/cli/.worktrees/nori-footer-customization/codex-rs/tui/src/system_info.rs`
- Test function `test_get_nori_version_installed()` - mock `nori-ai --version` output like "nori-ai 19.1.1"
- Test function `test_get_nori_version_not_installed()` - mock command failure
- Run tests to verify they fail

**Task 2.7:** Implement `get_nori_version() -> Option<String>`
- File: `/home/clifford/Documents/source/nori/cli/.worktrees/nori-footer-customization/codex-rs/tui/src/system_info.rs`
- Execute `nori-ai --version` using `std::process::Command`
- Parse version from output (extract just the version number)
- Return `Some(version)` on success, `None` if command fails
- Run tests to verify they pass

**Task 2.8:** Write failing test for profile detection from config
- File: `/home/clifford/Documents/source/nori/cli/.worktrees/nori-footer-customization/codex-rs/tui/src/system_info.rs`
- Test function `test_get_nori_profile_from_config()` - mock .nori-config.json with profile field
- Test function `test_get_nori_profile_from_install_location()` - mock `nori-ai install-location` output
- Test function `test_find_nori_config_current_dir()` - test finding config in current dir
- Test function `test_find_nori_config_parent_dir()` - test finding config in parent dirs
- Test function `test_get_nori_profile_no_config()` - test when no config found
- Run tests to verify they fail

**Task 2.9:** Implement `get_nori_profile() -> Option<String>`
- File: `/home/clifford/Documents/source/nori/cli/.worktrees/nori-footer-customization/codex-rs/tui/src/system_info.rs`
- Search current dir and parent dirs for `.nori-config.json` (ONLY read from config file, not from nori-ai commands)
- Parse JSON to extract `agents.claude-code.profile.baseProfile` field
- Return `Some(profile_name)` on success, `None` otherwise
- Dependencies: Add `serde` and `serde_json` to Cargo.toml (APPROVED)
- Run tests to verify they pass

**Task 2.10:** Write failing test for SystemInfo::collect()
- File: `/home/clifford/Documents/source/nori/cli/.worktrees/nori-footer-customization/codex-rs/tui/src/system_info.rs`
- Test function `test_system_info_collect()` - integration test that calls all collection functions
- Run test to verify it fails

**Task 2.11:** Implement `SystemInfo::collect() -> SystemInfo`
- File: `/home/clifford/Documents/source/nori/cli/.worktrees/nori-footer-customization/codex-rs/tui/src/system_info.rs`
- Call all the individual collection functions
- Return populated `SystemInfo` struct
- Run test to verify it passes

### Phase 3: Footer Rendering Updates

**Task 3.1:** Write failing test for new footer format
- File: `/home/clifford/Documents/source/nori/cli/.worktrees/nori-footer-customization/codex-rs/tui/src/bottom_pane/footer.rs`
- Test function `test_format_nori_footer_full()` - all fields present
- Expected: "⎇ main · Profile: clifford · Nori v19.1.1 · +10 -3 · ? for shortcuts"
- Test function `test_format_nori_footer_no_git()` - git fields None
- Expected: "Profile: clifford · Nori v19.1.1 · ? for shortcuts"
- Test function `test_format_nori_footer_no_nori()` - nori fields None
- Expected: "⎇ main · +10 -3 · ? for shortcuts"
- Test function `test_format_nori_footer_only_context()` - all new fields None (fallback)
- Expected: "100% context left · ? for shortcuts"
- Run tests to verify they fail

**Task 3.2:** Update `context_window_line()` to `format_footer_line()`
- File: `/home/clifford/Documents/source/nori/cli/.worktrees/nori-footer-customization/codex-rs/tui/src/bottom_pane/footer.rs`
- Rename function and add new params: `format_footer_line(props: FooterProps) -> Line<'static>`
- Build footer string with segments:
  1. If `git_branch.is_some()`: "⎇ {branch}"
  2. If `nori_profile.is_some()`: "Profile: {profile}"
  3. If `nori_version.is_some()`: "Nori v{version}"
  4. If git stats available and either > 0: "+{added} -{removed}"
  5. Always end with: "{context}% context left · ? for shortcuts"
- Join segments with " · "
- Update all call sites of `context_window_line()` to use new function
- Run tests to verify they pass

**Task 3.3:** Update `footer_lines()` to use new format
- File: `/home/clifford/Documents/source/nori/cli/.worktrees/nori-footer-customization/codex-rs/tui/src/bottom_pane/footer.rs`
- Update `FooterMode::ShortcutSummary` case to use `format_footer_line(props)`
- Update `FooterMode::ContextOnly` case to use simplified version
- Run tests to verify rendering works correctly

### Phase 4: Integration with ChatComposer

**Task 4.1:** Add system info storage to BottomPane
- File: `/home/clifford/Documents/source/nori/cli/.worktrees/nori-footer-customization/codex-rs/tui/src/bottom_pane/mod.rs`
- Add field to `BottomPane`: `system_info: Option<SystemInfo>`
- Add method: `pub fn set_system_info(&mut self, info: SystemInfo)`
- Method should call `self.composer.set_system_info(info)` and `self.request_redraw()`

**Task 4.2:** Add system info storage to ChatComposer
- File: `/home/clifford/Documents/source/nori/cli/.worktrees/nori-footer-customization/codex-rs/tui/src/bottom_pane/chat_composer.rs`
- Add field to `ChatComposer`: `system_info: Option<SystemInfo>`
- Add method: `pub fn set_system_info(&mut self, info: SystemInfo)`
- Update `footer_props()` method to populate new fields from `self.system_info`

**Task 4.3:** Write test for footer_props with system info
- File: `/home/clifford/Documents/source/nori/cli/.worktrees/nori-footer-customization/codex-rs/tui/src/bottom_pane/chat_composer.rs`
- Test that `footer_props()` correctly transfers system_info fields to FooterProps
- Run test to verify it fails

**Task 4.4:** Implement footer_props updates
- File: `/home/clifford/Documents/source/nori/cli/.worktrees/nori-footer-customization/codex-rs/tui/src/bottom_pane/chat_composer.rs`
- Update `footer_props()` to populate git_branch, nori_profile, etc. from `self.system_info`
- Run test to verify it passes

### Phase 5: Periodic Refresh Mechanism

**Task 5.1:** Add refresh interval configuration
- File: `/home/clifford/Documents/source/nori/cli/.worktrees/nori-footer-customization/codex-rs/tui/src/bottom_pane/mod.rs`
- Add constant: `const SYSTEM_INFO_REFRESH_INTERVAL: Duration = Duration::from_secs(10);`
- Add field to `BottomPane`: `last_system_info_refresh: Option<Instant>`

**Task 5.2:** Write test for refresh trigger logic
- File: `/home/clifford/Documents/source/nori/cli/.worktrees/nori-footer-customization/codex-rs/tui/src/bottom_pane/mod.rs`
- Test that `should_refresh_system_info()` returns true after interval expires
- Test that it returns false before interval expires
- Run tests to verify they fail

**Task 5.3:** Implement refresh check in BottomPane
- File: `/home/clifford/Documents/source/nori/cli/.worktrees/nori-footer-customization/codex-rs/tui/src/bottom_pane/mod.rs`
- Add method: `fn should_refresh_system_info(&self) -> bool`
- Check if last_refresh is None or elapsed time > SYSTEM_INFO_REFRESH_INTERVAL
- Run tests to verify they pass

**Task 5.4:** Integrate refresh into ChatWidget
- File: `/home/clifford/Documents/source/nori/cli/.worktrees/nori-footer-customization/codex-rs/tui/src/chatwidget.rs`
- In the main event loop or update cycle, check `bottom_pane.should_refresh_system_info()`
- If true, spawn a task to collect system info and call `bottom_pane.set_system_info()`
- Use tokio::task::spawn_blocking with timeout to ensure commands don't hang (use tokio::time::timeout with 5 second max)
- If timeout occurs, silently fail and try again on next refresh cycle
- Note: Need to identify the right place in the event loop (may need to explore chatwidget.rs structure first)

**Task 5.5:** Write integration test for full refresh cycle
- File: `/home/clifford/Documents/source/nori/cli/.worktrees/nori-footer-customization/codex-rs/tui/src/bottom_pane/mod.rs` or separate integration test file
- Test that system info is collected on startup
- Test that it refreshes after the interval
- Test that UI updates when system info changes
- Run test to verify behavior

**Task 5.6:** Run full integration test manually
- Build the project: `cargo build`
- Run in a git repo with nori-config.json present
- Verify footer shows: "⎇ {branch} · Profile: {profile} · Nori v{version} · +X -Y · ? for shortcuts"
- Make git changes and verify +/- counts update after 10 seconds
- Switch branches and verify branch name updates
- Run in a non-git directory and verify git segments don't appear
- Run without .nori-config.json and verify profile segment doesn't appear

### Phase 6: Edge Cases & Polish

**Task 6.1:** Handle long branch names
- Write test for branch names > 30 chars
- Truncate with "..." to max 30 chars to prevent footer wrapping
- Test and verify

**Task 6.2:** Handle special git states
- Write test for detached HEAD state (should show commit hash or "(detached)")
- Write test for git worktrees (should show correct branch)
- Implement handling
- Test and verify

**Task 6.3:** Handle config parsing errors gracefully
- Write test for malformed .nori-config.json
- Write test for missing profile field
- Ensure `None` is returned instead of panicking
- Test and verify

**Task 6.4:** Add responsive footer for narrow terminals
- Write test for footer segment hiding based on available width
- Implement progressive hiding from right to left: context% → git stats → nori version → nori profile → git branch
- "? for shortcuts" must ALWAYS remain visible
- Segments should gracefully disappear when terminal width is insufficient
- Test with various terminal widths (e.g., 80, 100, 120, 140 columns)
- Test and verify

**Task 6.5:** Manual testing checklist
- [ ] Footer renders correctly in normal git repo with Nori
- [ ] Footer renders correctly without git repo
- [ ] Footer renders correctly without nori-ai
- [ ] Footer updates when making git changes
- [ ] Footer updates when switching branches
- [ ] Footer handles config file changes
- [ ] Context percentage still updates correctly
- [ ] Shortcut overlay still works (? key)
- [ ] Footer doesn't cause performance issues
- [ ] Very long branch names don't break layout

---

## Testing Details

All tests will verify **behavior**, not implementation details:
- Config parsing tests verify that the correct profile name is extracted from various valid config formats, not that specific JSON parsing code is called
- Git command tests verify that branch names and stats are correctly determined from git state, not that specific command strings are executed
- Footer rendering tests verify that the correct human-readable string is displayed given certain system states, not that specific span or style methods are invoked
- Refresh tests verify that footer data updates after the expected interval, not that specific timer mechanisms fire

No tests will mock domain objects or test mock behavior. All tests will either:
1. Test pure functions with real inputs/outputs (parsing, formatting)
2. Test integration points with mocked system boundaries (file I/O, command execution)
3. Test full integration with real system state (manual/integration tests)

---

## Implementation Details

- **New module**: `codex-rs/tui/src/system_info.rs` for all data collection logic
- **Modified files**: `footer.rs` (rendering), `chat_composer.rs` (props), `bottom_pane/mod.rs` (storage), `chatwidget.rs` (refresh)
- **Dependencies**: `serde_json` for JSON parsing (APPROVED)
- **Refresh rate**: 10 seconds (non-blocking with 5 second timeout per collection cycle)
- **Conditional rendering**: Git info only shows in git repos, Nori profile only when .nori-config.json exists
- **Graceful degradation**: Falls back to original footer if no Nori data available
- **Performance**: Commands run in background tokio tasks with timeout to avoid blocking UI or hanging
- **Git commands**: `git branch --show-current`, `git diff HEAD --shortstat` (cumulative changes from HEAD)
- **Nori commands**: `nori-ai --version`
- **Config location**: Search current dir + parent dirs for `.nori-config.json`
- **Branch truncation**: Max 30 chars with "..." suffix
- **Responsive layout**: Segments hide from right to left on narrow terminals, "? for shortcuts" always visible

---

## Decisions (from user feedback)

1. ✅ **serde_json approved** for .nori-config.json parsing
2. ✅ **10 second refresh rate** with non-blocking timeout (5 sec max per collection)
3. ✅ **Branch name truncation** to 30 chars max with "..." suffix
4. ✅ **Git diff scope**: `git diff HEAD --shortstat` (cumulative changes from HEAD, staged or unstaged)
5. ⚠️ **Performance optimizations** are out of scope for now (no config flag, caching, etc.)
6. ✅ **Profile source**: Only from .nori-config.json file (not from nori-ai commands)
7. ✅ **Narrow terminal handling**: Progressive segment hiding from right to left (branch stays longest), "? for shortcuts" always visible

---
