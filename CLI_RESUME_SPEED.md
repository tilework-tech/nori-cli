# CLI Resume Picker Speed Investigation

## Worktree

- Workspace root used by the operator: `/home/clifford/Documents/source/nori`
- CLI worktree: `/home/clifford/Documents/source/nori/cli/.worktrees/debug-resume-picker-tracing`
- Branch: `debug/resume-picker-tracing`
- Debug binary used for capture: `/home/clifford/Documents/source/nori/cli/.worktrees/debug-resume-picker-tracing/codex-rs/target/debug/nori`
- Command cwd passed to Nori: `/home/clifford/Documents/source/nori/cli`
- Nori state directory: `/home/clifford/.nori/cli`
- Active agent filter during `/resume`: `codex`
- Instrumentation target: `nori_resume`

Note: this capture was taken before `origin/main` renamed the Rust workspace
directory from `codex-rs` to `nori-rs`. The captured binary path is intentionally
kept exact. Source-code references below use the final post-merge `nori-rs`
paths where applicable.

## Captures

- stderr capture: `/tmp/nori-resume-debug/stderr-20260427-154408.log`
- resume tracing capture: `/tmp/nori-resume-debug/nori-resume-20260427-154408.log`
- stderr lines: `0`
- resume tracing lines: `4296`

The stderr capture was empty. The useful signal is entirely in the `nori_resume`
tracing log.

## High-Level Finding

The `/resume` picker delay happens before the picker appears because the CLI is
doing expensive transcript I/O and parsing before emitting the picker event. The
captured delay is not ACP subprocess startup.

The pre-picker path took `142427 ms` from `/resume` start to
`ShowResumeSessionPicker` event send.

## Timeline Evidence

The relevant aggregate phase markers from
`/tmp/nori-resume-debug/nori-resume-20260427-154408.log`:

```text
2026-04-27T19:44:12.401664Z  phase="open_resume_session_picker.start" cwd=/home/clifford/Documents/source/nori/cli agent=codex
2026-04-27T19:44:12.401723Z  phase="open_resume_session_picker.nori_home_resolved" elapsed_ms=0 nori_home=/home/clifford/.nori/cli
2026-04-27T19:44:12.401773Z  phase="open_resume_session_picker.load_task.start" cwd=/home/clifford/Documents/source/nori/cli agent=codex nori_home=/home/clifford/.nori/cli
2026-04-27T19:44:12.401808Z  phase="load_resumable_sessions.start" nori_home=/home/clifford/.nori/cli cwd=/home/clifford/Documents/source/nori/cli agent_filter=codex
2026-04-27T19:44:12.401819Z  phase="load_sessions_with_preview.start" nori_home=/home/clifford/.nori/cli cwd=/home/clifford/Documents/source/nori/cli
2026-04-27T19:44:12.401831Z  phase="transcript_loader.find_sessions_for_cwd.start" cwd=/home/clifford/Documents/source/nori/cli
2026-04-27T19:44:12.404872Z  phase="transcript_loader.find_sessions_for_cwd.project_id" project_id=4e82a48c698c1d38 project_name=nori-cli git_remote="https://github.com/tilework-tech/nori-cli.git" elapsed_ms=3 total_elapsed_ms=3
2026-04-27T19:44:32.190632Z  phase="transcript_loader.list_sessions.done" project_id="4e82a48c698c1d38" file_count=349 loaded_session_count=349 elapsed_ms=19785
2026-04-27T19:44:32.190651Z  phase="transcript_loader.find_sessions_for_cwd.done" session_count=349 list_elapsed_ms=19785 total_elapsed_ms=19788
2026-04-27T19:44:32.190659Z  phase="load_sessions_with_preview.sessions_found" elapsed_ms=19788 total_elapsed_ms=19788 session_count=349
2026-04-27T19:46:28.143232Z  phase="load_sessions_with_preview.done" total_elapsed_ms=135741 returned_session_count=269
2026-04-27T19:46:28.143238Z  phase="load_resumable_sessions.preview_loaded" elapsed_ms=135741 total_elapsed_ms=135741 all_session_count=269
2026-04-27T19:46:34.828793Z  phase="transcript_loader.list_sessions.done" project_id="4e82a48c698c1d38" file_count=349 loaded_session_count=349 elapsed_ms=6682
2026-04-27T19:46:34.828811Z  phase="transcript_loader.find_sessions_for_cwd.done" session_count=349 list_elapsed_ms=6682 total_elapsed_ms=6685
2026-04-27T19:46:34.828963Z  phase="load_resumable_sessions.agent_filter_metadata_loaded" elapsed_ms=6685 total_elapsed_ms=142427 session_info_count=349 matching_session_count=92 agent_filter=codex
2026-04-27T19:46:34.829071Z  phase="load_resumable_sessions.done" total_elapsed_ms=142427 returned_session_count=72 agent_filter=codex
2026-04-27T19:46:34.829081Z  phase="open_resume_session_picker.load_task.loaded" elapsed_ms=142427 session_count=72
2026-04-27T19:46:34.829097Z  phase="open_resume_session_picker.load_task.event_sent" elapsed_ms=142427
```

Summary:

- `/resume` starts at `19:44:12.401`.
- First `find_sessions_for_cwd` scan takes `19788 ms`.
- Preview loading finishes at `135741 ms`.
- Agent-filter metadata scan adds another `6685 ms`.
- Picker event is sent at `142427 ms`.

## Transcript State Evidence

The project id for `/home/clifford/Documents/source/nori/cli` resolved to:

```text
4e82a48c698c1d38
```

The scan target was:

```text
/home/clifford/.nori/cli/transcripts/by-project/4e82a48c698c1d38/sessions
```

The picker path scanned:

- `349` transcript JSONL files.
- `349` sessions loaded in the first metadata/list pass.
- `269` non-empty sessions returned from preview loading.
- `92` sessions matched the active `codex` agent filter.
- `72` non-empty `codex` sessions were returned to the picker.

Aggregates from the trace:

```text
load_session_info_done=698
unique_sessions=349
total_bytes_read=29972053864
unique_bytes=14986026932
summed_read_elapsed_ms=23436
summed_total_elapsed_ms=24802
preview_transcript_loads=269
summed_preview_elapsed_ms=115223
```

This means `/resume` read the metadata/list information twice for all `349`
sessions, and loaded `269` full transcripts to compute preview text.

## Largest Transcript Evidence

The largest transcript was:

```text
/home/clifford/.nori/cli/transcripts/by-project/4e82a48c698c1d38/sessions/b0b6e8ce-90a5-4be7-96b3-1bc454844a08.jsonl
```

File size from `ls -lh`:

```text
7.4G Apr  5 23:06 b0b6e8ce-90a5-4be7-96b3-1bc454844a08.jsonl
```

Trace evidence for that file:

```text
2026-04-27T19:44:14.396956Z  phase="transcript_loader.load_session_info.start" transcript_bytes=7887772089
2026-04-27T19:44:24.546062Z  phase="transcript_loader.load_session_info.done" session_id=b0b6e8ce-90a5-4be7-96b3-1bc454844a08 agent="codex-debug-acp" entry_count=7365 transcript_bytes=7887772089 read_elapsed_ms=9812 count_elapsed_ms=336 total_elapsed_ms=10149
2026-04-27T19:45:08.199109Z  phase="load_sessions_with_preview.session.start" session_index=64 total_sessions=349 session_id=b0b6e8ce-90a5-4be7-96b3-1bc454844a08 agent="codex-debug-acp" entry_count=7365 transcript_bytes=7887772089
2026-04-27T19:45:08.199154Z  phase="transcript_loader.load_transcript.start" transcript_bytes=7887772089
2026-04-27T19:46:15.080861Z  phase="transcript_loader.load_transcript.done" session_id=b0b6e8ce-90a5-4be7-96b3-1bc454844a08 agent="codex-debug-acp" line_count=7365 parsed_entry_count=7359 skipped_count=6 bytes_seen=7887772089 transcript_bytes=7887772089 elapsed_ms=66881
2026-04-27T19:46:15.080915Z  phase="load_first_message_preview.transcript_loaded" elapsed_ms=66881 entry_count=7359 session_id="b0b6e8ce-90a5-4be7-96b3-1bc454844a08"
2026-04-27T19:46:28.813155Z  phase="transcript_loader.load_session_info.start" transcript_bytes=7887772089
2026-04-27T19:46:32.119467Z  phase="transcript_loader.load_session_info.done" session_id=b0b6e8ce-90a5-4be7-96b3-1bc454844a08 agent="codex-debug-acp" entry_count=7365 transcript_bytes=7887772089 read_elapsed_ms=2972 count_elapsed_ms=333 total_elapsed_ms=3306
```

This one file cost:

- `10149 ms` during the first metadata/list pass.
- `66881 ms` during preview loading.
- `3306 ms` during the second metadata/filter pass.

It is also not resumable for the current `codex` filter because its agent is
`codex-debug-acp`, but the preview path loaded it before filtering.

## Other Large Transcript Evidence

Largest files observed in the session directory:

```text
7.4G  b0b6e8ce-90a5-4be7-96b3-1bc454844a08.jsonl  codex-debug-acp
1.6G  63daab06-462b-4127-b67c-4224925e839a.jsonl  codex
1.6G  0e220389-b311-4446-aad3-4d0606e90250.jsonl  codex
1.4G  6d0fec70-ce54-420e-93ac-17c48b383541.jsonl  codex
1.3G  082c2f9b-f2f3-47df-a5ee-92168f6d3384.jsonl  codex
657M  dacbd57a-43a2-41cd-8796-822c24da0548.jsonl  codex
```

Top preview-load costs:

```text
66881 ms  7359 entries   b0b6e8ce-90a5-4be7-96b3-1bc454844a08
11221 ms  38406 entries  0e220389-b311-4446-aad3-4d0606e90250
10907 ms  24024 entries  63daab06-462b-4127-b67c-4224925e839a
9241 ms   1787 entries   6d0fec70-ce54-420e-93ac-17c48b383541
8932 ms   42716 entries  082c2f9b-f2f3-47df-a5ee-92168f6d3384
4633 ms   18891 entries  dacbd57a-43a2-41cd-8796-822c24da0548
350 ms    447 entries    b5366eaf-cbe5-471d-984e-a66c7eea75dd
277 ms    6017 entries   07670a18-a589-4f1b-b8b3-c812ec581c84
192 ms    10351 entries  952e6ab5-2086-4b19-baa3-688954feed34
182 ms    7063 entries   b0bb8365-391e-462c-b7a7-ac3b5421e5d9
```

Bytes by agent in the preview pass:

```text
codex-debug-acp count=20  bytes=7.35 GiB  entries=18801
codex           count=92  bytes=6.51 GiB  entries=243978
claude          count=59  bytes=0.06 GiB  entries=46566
claude-code     count=122 bytes=0.03 GiB  entries=23179
<unknown>       count=3   bytes=0.00 GiB  entries=629
claude-debug-acp count=22 bytes=0.00 GiB  entries=791
gemini-debug-acp count=6  bytes=0.00 GiB  entries=132
opencode        count=3   bytes=0.00 GiB  entries=67
gemini          count=10  bytes=0.00 GiB  entries=106
elizacp         count=12  bytes=0.00 GiB  entries=48
```

## Code Path Evidence

The pre-fix `/resume` pre-picker path captured in the debug run loaded every
session preview before filtering by agent:

- `load_resumable_sessions` calls `load_sessions_with_preview(nori_home, cwd)`
  before knowing which preview rows will survive the active-agent filter.
- The agent filter happens later by rescanning session metadata with
  `loader.find_sessions_for_cwd(cwd)`.

Relevant pre-fix source locations from the captured behavior:

```text
nori-rs/tui/src/nori/resume_session_picker.rs
  load_resumable_sessions
  line 110: let all_sessions = load_sessions_with_preview(nori_home, cwd).await?;
  line 124: let session_infos = loader.find_sessions_for_cwd(cwd).await?;
  line 144: let filtered: Vec<SessionPickerInfo> = all_sessions.into_iter()
```

The preview loader loads a full transcript to find the first user message:

```text
nori-rs/tui/src/nori/viewonly_session_picker.rs
  load_first_message_preview
  line 149: let transcript = loader.load_transcript(project_id, session_id).await.ok()?;
```

The transcript list path reads the entire transcript file just to count lines:

```text
nori-rs/acp/src/transcript/loader.rs
  load_session_info
  line 343: let content = tokio::fs::read_to_string(path).await?;
  line 346: let entry_count = content.lines().count();
```

The full transcript loader parses every line and materializes all entries:

```text
nori-rs/acp/src/transcript/loader.rs
  load_transcript_from_path
  lines 423-484: reads each line, deserializes JSON, pushes parsed entries
```

## Causes

1. Agent filtering happens after all previews are loaded.

   Evidence: `/resume` loaded `269` previews for all non-empty sessions, then
   later filtered to `72` returned sessions for `agent_filter=codex`. The
   largest single transcript, `b0b6e8ce-90a5-4be7-96b3-1bc454844a08`, was
   `codex-debug-acp`, cost `66881 ms` to preview, and was discarded by the
   `codex` filter.

2. Preview generation loads the full transcript before finding the first user
   message.

   Evidence: `load_first_message_preview` calls `loader.load_transcript`, and
   the largest transcript took `66881 ms` in `transcript_loader.load_transcript`
   just to compute picker preview text.

3. Session listing reads entire transcripts just to count lines.

   Evidence: `load_session_info` calls `tokio::fs::read_to_string(path)` and
   then `content.lines().count()`. The first metadata/list pass took
   `19785 ms`; the second pass took `6682 ms`.

4. `/resume` repeats the session metadata scan.

   Evidence: `transcript_loader.load_session_info.done` appears `698` times
   for `349` unique sessions. `total_bytes_read=29972053864`, while
   `unique_bytes=14986026932`.

5. The local `~/.nori/cli` transcript state contains several giant JSONL files.

   Evidence: one `7.4G` transcript and several `1.3G-1.6G` transcripts are in
   the project session directory. The largest six files account for most of the
   observed wall time.

6. The picker pre-load is sequential.

   Evidence: per-session preview tracing shows one session finishing before the
   next begins. A single giant file blocks the whole picker from appearing.

7. The transcript format/state lacks cheap cached picker metadata.

   Evidence: every picker open recomputes metadata, line counts, and preview
   text from raw JSONL transcripts rather than using a small sidecar/index with
   `session_id`, `agent`, `started_at`, `entry_count`, and first user preview.

## Initial Fix Direction

The first narrow fix should be to filter by agent before preview loading and
avoid the second session metadata scan.

Expected effect from this capture:

- Avoid previewing the `7.4G` `codex-debug-acp` transcript when active agent is
  `codex`.
- Remove the second `find_sessions_for_cwd` pass.
- Likely remove more than half of the observed delay in this capture.

The next fix should be to compute preview text by streaming only until the first
user message, rather than loading and parsing the full transcript.

After that, address line-count metadata by streaming counts or persisting cheap
session summary metadata so picker open does not read multi-GiB files.

## Fix Slice 1: Agent Filter Before Preview Loading

Implemented in worktree branch `debug/resume-picker-tracing`.

This slice addresses causes 1 and 4:

- `/resume` now calls `TranscriptLoader::find_sessions_for_cwd` once.
- It filters the resulting `SessionInfo` list to the active `agent_filter`
  before preview loading.
- It passes the already filtered `SessionInfo` values into preview loading.
- It no longer does the second metadata scan to build a separate matching
  session id set.

Expected effect against the captured state:

- The `7.4G` `codex-debug-acp` transcript
  `b0b6e8ce-90a5-4be7-96b3-1bc454844a08.jsonl` is not preview-loaded when the
  active agent filter is `codex`.
- The second `find_sessions_for_cwd` pass is removed.
- The remaining known bottlenecks are causes 2, 3, and 5: full-transcript
  preview loading, full-file line counting, and very large transcript files.

Validation added:

- `load_resumable_sessions_filters_agent_before_loading_previews` creates one
  matching `codex` transcript and one nonmatching `claude-code` transcript,
  then asserts the nonmatching session never enters
  `load_first_message_preview.start`.
