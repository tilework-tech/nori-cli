# PR #740 at `95a66b49` (base `992da486`)

Verdict: pass

- Matrix: 212/260 checks pass (`results.json`). Regressions against the base: 0.
- Turned green by this layer: none (no behavior change expected or observed)
- Linux CI on the head: run 45319989248 (success). Native macOS on `verify/pr-740-95a66b49-native-host`, workflow pinned to the head: run 45320092796 (success).

Remaining failures (all present at the base too unless listed above):

- S01/hint_row.exactly_one
- S01/hint_row.at_bottom
- S05/slash.menu_opens_with_normal_commands
- S05/slash.diff_listed_with_reason
- S05/slash.browse_listed_with_reason
- S05/slash.undo_listed_with_reason
- S05/slash.unavailable_not_sent_as_prompt
- S06/cwd.plain_new_inherits_foreground
- S06/cwd.confirmation_names_folder
- S06/cwd.dir_completion_from_session_fs
- S07/at.alpha_files_offered
- S07/at.beta_files_offered_same_logical_path
- S08/watcher.enter_notice_shown_once
- S10/approvals.offers_host_modes
- S10/approvals.not_sent_as_prompt
- S11b/home.terminal_title_names_agent
- S12/default.first_new_uses_host_default
- S12/approvals.controller_can_pick_approve_all
- S12/default.second_new_does_not_copy_foreground
- S13/approvals.ask_hidden_for_adapter_without_ask
- S13/approvals.change_reaches_only_foreground
- S13/approvals.footer_shows_foreground_mode
- S13/approvals.watcher_sees_greyed_with_take_control
- S14/resume.lists_agent_server_saved_sessions
- S14/resume.adapter_loaded_saved_session_on_host
- S15a/migration.full-access_maps_to_approve-all
- S15b/migration.read-only_maps_to_ask
- S15c/migration.read-only-no-ask_maps_to_agent
- S18b/indicator.thought_1_on_working_row
- S18b/indicator.thought_2_on_working_row
- S18b/indicator.thought_3_on_working_row
- S19b/cap.total_rows_at_most_cap
- S19b/cap.hidden_marker_counts_the_rest
- S23/local.echo_works
- S27/standalone.binary_built
- S28b/cancel.exactly_one_cancel
- S28b/cancel.next_turn_completes
- S28b/cancel.no_cancel_for_next_turn
- S29a/close_refusal.adapter_refused
- S29a/close_refusal.shown_once
- S29a/close_refusal.retry_reaches_adapter
- S29b/close_refusal.adapter_refused
- S29b/close_refusal.shown_once
- S29b/close_refusal.session_still_live_on_host
- S29b/close_refusal.session_keeps_working
- S29b/close_refusal.retry_reaches_adapter
- S30/prepared.capability_advertised
- S30/prepared.prepare_returns_selection
