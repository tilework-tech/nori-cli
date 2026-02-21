# Current Progress: Per-Session Skillset

## Status: Complete

All items from APPLICATION-SPEC.md have been implemented.

## Implemented Features

### Config field + loading
- Added `skillset_per_session: Option<bool>` to `TuiConfigToml` and `skillset_per_session: bool` to `NoriConfig` (defaults to `false`)
- When `skillset_per_session=true`, forces `auto_worktree=true` at config resolution time in `loader.rs`
- 3 unit tests covering enabled, default, and force-auto-worktree behavior

### Config picker integration
- "Per Session Skillsets" toggle added to `/config` picker with nori-skillsets availability check
- Auto Worktree item shows as locked ("required by Per Session Skillsets") when per-session is enabled
- Persistence writes both `skillset_per_session` and `auto_worktree` to TOML when enabling

### Updated nori-skillsets CLI commands
- Changed from `list-skillsets` to `list` and from `install` to `switch --install-dir`
- Added `switch_skillset()` function in `skillset_picker.rs`
- `skillset_picker_params` accepts `install_dir: Option<PathBuf>` to select between `SwitchSkillset` and `InstallSkillset` events

### Statusline reads activeSkillset
- Both `system_info.rs::get_nori_profile()` and `session_header/mod.rs::read_nori_profile()` now try `activeSkillset` first from `.nori-config.json`, falling back to `agents.claude-code.profile.baseProfile`, then `profile.baseProfile`

### Session-local skillset tracking
- `session_skillset_name: Option<String>` field on `ChatWidget`, propagated through `BottomPane` -> `ChatComposer` -> `FooterProps`
- Footer prefers `session_skillset_name` over `nori_profile` for the "Skillset:" display segment
- 2 snapshot tests for footer session skillset display

### Startup skillset picker
- When `skillset_per_session` enabled and session is in a worktree, automatically opens skillset picker at startup via `App::run()`

### /switch-skillset worktree awareness
- Detects worktree context (parent dir named `.worktrees`) and passes `install_dir` to the switch flow
- On successful switch, updates `session_skillset_name` for the session

## New AppEvent variants
- `SetConfigSkillsetPerSession(bool)`
- `SwitchSkillset { name: String, install_dir: PathBuf }`
- `SkillsetSwitchResult { name: String, success: bool, message: String }`
- `install_dir: Option<PathBuf>` added to `SkillsetListResult`

## Remaining Work
- Full E2E flow testing with actual `nori-skillsets` CLI tool (requires the tool to be installed)
