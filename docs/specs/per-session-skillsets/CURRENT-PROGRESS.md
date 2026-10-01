# Current Progress: Per-Session Skillset

## Status: Complete

The features in APPLICATION-SPEC.md are implemented. Some parts differ from the original spec; the Notes and Implementation detail sections of APPLICATION-SPEC.md describe what was built.

## Implemented Features

### Config field + loading
- `skillset_per_session` lives under `[tui]` in config.toml and resolves to `NoriConfig.skillset_per_session` (defaults to `false`) in `nori-config`.
- It does not force `auto_worktree` on. The two settings are independent at load time.

### Config picker integration
- `/config` has a "Per Session Skillsets" toggle. Turning it on first checks that `nori-skillsets` is on `PATH` (shows the not-installed message if not), then opens a choice: "With Auto Worktrees" (also sets auto-worktree to automatic) or "Without Auto Worktrees".
- Only `tui.skillset_per_session` is persisted by the toggle itself; the change takes effect on the next session.

### nori-skillsets CLI commands
- Picker lists with `nori-skillsets --non-interactive list`.
- Selection runs `switch <name> --install-dir <dir>` when the session cwd is an auto-worktree (`.worktrees/<name>` layout, see `session_skillset_install_dir`), else `install <name>` into the home install. A non-worktree cwd is never used as an install dir.

### Startup skillset picker
- With `skillset_per_session` on (and not in cloud mode), `App` defers agent spawn and opens the skillset picker at startup. The picker includes a "No Skillset" item.
- Applying a skillset (install or switch) or dismissing the picker resolves the deferred spawn and activates the session, so `nori-skillsets` writes workspace state before the agent reads it.

### Skillset display
- There is no session-local skillset name. The footer `skillset` segment and the `/status` card both read `SystemInfo.active_skillsets`, filled by `nori-skillsets list-active` (every skillset active in the cwd and its parents).
- A successful install or switch requests a system-info refresh, which re-runs `list-active`.
- The `.nori-config.json` `activeSkillset` field is read only as a fallback for old `nori-skillsets` versions without `list-active`.

### /switch-skillset
- Uses the same install-dir rule as the startup picker; `/switch-skillset <name>` switches directly without the picker.

## AppEvent variants
- `SetConfigSkillsetPerSession(bool)`, `OpenSkillsetPerSessionWorktreeChoice`
- `SkillsetListResult { names, error, install_dir }`
- `InstallSkillset { name }`, `SwitchSkillset { name, install_dir }`
- `SkillsetInstallResult` / `SkillsetSwitchResult { name, success, message }`, `SkillsetPickerDismissed`

## Remaining Work
- Full E2E flow testing with actual `nori-skillsets` CLI tool (requires the tool to be installed)
