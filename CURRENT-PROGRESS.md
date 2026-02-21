# Current Progress: Per-Session Skillset

## Completed

### Session-local skillset tracking (Part A)
- Added `session_skillset_name: Option<String>` field to `ChatWidget` in `chatwidget/mod.rs`
- Added setter method `set_session_skillset_name` that propagates through `BottomPane` -> `ChatComposer` -> `FooterProps`
- Wired the field through all three ChatWidget constructors (`new`, `new_from_existing`, `new_resumed_acp`)

### Footer displays session skillset (Part B)
- Updated `footer_segments()` in `footer.rs` to prefer `session_skillset_name` over `nori_profile` for the "Skillset:" segment
- Added `session_skillset_name` field to `FooterProps`, `ChatComposer`, and `BottomPane` with passthrough setters
- Added two snapshot tests verifying override behavior

### /switch-skillset uses worktree path (Part C)
- Updated `handle_switch_skillset_command` to detect worktree context (parent dir named `.worktrees`) and pass `install_dir`
- Added `install_dir: Option<PathBuf>` to `SkillsetListResult` variant in `AppEvent`
- Updated `on_skillset_list_result` to accept and forward `install_dir` to `skillset_picker_params`
- Updated `on_skillset_switch_result` to call `set_session_skillset_name` on success
- Changed `handle_switch_skillset_command` visibility from `pub(super)` to `pub(crate)` for startup access

### Startup skillset picker (Part D)
- Added startup trigger in `App::run()` that fires `handle_switch_skillset_command` when `skillset_per_session` is enabled and session is in a worktree

### Config picker integration (prior commits on this branch)
- Added `skillset_per_session` toggle to config picker
- Auto-worktree is locked on when `skillset_per_session` is enabled
- Added `skillset_per_session` field to `NoriConfig` with persistence

### Skillset picker enhancements (prior commits on this branch)
- `skillset_picker_params` accepts `install_dir: Option<PathBuf>` and sends `InstallSkillset` or `SwitchSkillset` events accordingly
- Snapshot tests for install-dir behavior

## Remaining Work
- Active skillset detection from `nori-config.json` `activeSkillset` field (currently uses `nori_profile` which reads from agents field)
- Full E2E flow testing with `nori-skillsets` CLI tool
- The config picker should show a message when user tries to disable auto-worktree while `skillset_per_session` is enabled
