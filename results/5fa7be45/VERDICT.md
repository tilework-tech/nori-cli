# PR #749 at `5fa7be45` (base `b3a4e8df`)

Verdict: behavior passes my matrix; not passed overall because CI is red at the exact SHA on Linux and native macOS.

- Matrix: 279/316, no regression against #748. Turned green: S33b/new.refusal_keeps_current_session.
- S33a: `/new` starts on a fresh server process, the next prompt reaches it, and the previous server keeps running.
  Folder inheritance is not observable yet: `/new DIR` in plain `nori` ignores DIR at #748 and #749
  (S33a/b new.dir_argument_sets_session_folder), so the foreground folder is always the launch folder.
- Linux CI run 45345682032: FAIL `tui-pty-e2e --test session_lifecycle`
  `ctrl_d_in_the_list_closes_a_session_and_frees_its_slot` ("the composer never rendered "/new"", 885123 PTY bytes).
  That test passed on #748 CI.
- Native macOS run 45345705515 (`verify/pr-749-5fa7be45-native-host`, pinned): FAIL `host_new_session`
  `new_starts_beside_the_current_session_on_a_prepared_server` (waiting for "amber is answering (1).").
- Locally on Linux both suites pass 3/3 sequentially and 4/4 concurrently; not reproduced.
