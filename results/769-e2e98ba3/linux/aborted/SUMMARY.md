### recover-base-transcript-64  (recover, 64 sections, 100x30, extra rows 0, native load False; n=fix 1, base 1)

| metric | fix | base |
|---|---:|---:|
| launch_to_list_ms | 8.9 (8.9–8.9) | 11.4 (11.4–11.4) |
| ctrl_r_to_ready_row_ms | 32.7 (32.7–32.7) | 39.2 (39.2–39.2) |
| enter_to_first_content_ms | 16.4 (16.4–16.4) | 11.1 (11.1–11.1) |
| draft_ms | 15.7 (15.7–15.7) | 17.1 (17.1–17.1) |
| enter_to_latest_reply_ms | 46.5 (46.5–46.5) | 3,226 (3,226–3,226) |
| enter_to_final_tool_ms | 46.5 (46.5–46.5) | 3,233 (3,233–3,233) |
| enter_to_new_answer_ms | 275 (275–275) | 6,543 (6,543–6,543) |
| settled_key_ms | 12.5 (12.5–12.5) | 20.4 (20.4–20.4) |
| page_up_first_ms | 3.2 (3.2–3.2) | 8.2 (8.2–8.2) |
| page_up_ms_median | 6.0 (6.0–6.0) | 9.1 (9.1–9.1) |
| page_up_ms_max | 12.1 (12.1–12.1) | 14.9 (14.9–14.9) |
| page_up_ms_last5_median | 9.2 (9.2–9.2) | 8.5 (8.5–8.5) |
| frames_to_catchup | 51.0 (51.0–51.0) | 1,412 (1,412–1,412) |
| bytes_to_catchup | 73,025 (73,025–73,025) | 1,656,958 (1,656,958–1,656,958) |

- fix: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True, new prompt answered by replacement 1/1, scrollback unreadable (pinned notice) 0, samples with a disconnect 0/1, samples with a disconnect row left in scrollback 0; scrollback sha ['c21c8ac71285']; final screen sha ['347c719fff63']
- base: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True, new prompt answered by replacement 1/1, scrollback unreadable (pinned notice) 0, samples with a disconnect 0/1, samples with a disconnect row left in scrollback 0; scrollback sha ['c21c8ac71285']; final screen sha ['347c719fff63']

### recover-native-64  (recover, 64 sections, 100x30, extra rows 0, native load True; n=fix 1, oldfix 1, parent 0)

| metric | fix | oldfix | parent |
|---|---:|---:|---:|
| launch_to_list_ms | 8.1 (8.1–8.1) | 8.6 (8.6–8.6) | — |
| ctrl_r_to_ready_row_ms | 52.7 (52.7–52.7) | 48.6 (48.6–48.6) | — |
| enter_to_first_content_ms | 16.5 (16.5–16.5) | 13.6 (13.6–13.6) | — |
| draft_ms | 13.7 (13.7–13.7) | 14.8 (14.8–14.8) | — |
| enter_to_latest_reply_ms | 64.1 (64.1–64.1) | 816 (816–816) | — |
| enter_to_final_tool_ms | 64.1 (64.1–64.1) | 816 (816–816) | — |
| enter_to_new_answer_ms | 290 (290–290) | 1,043 (1,043–1,043) | — |
| settled_key_ms | 13.5 (13.5–13.5) | 13.0 (13.0–13.0) | — |
| page_up_first_ms | 2.1 (2.1–2.1) | 2.1 (2.1–2.1) | — |
| page_up_ms_median | 5.7 (5.7–5.7) | 6.0 (6.0–6.0) | — |
| page_up_ms_max | 9.8 (9.8–9.8) | 10.9 (10.9–10.9) | — |
| page_up_ms_last5_median | 8.5 (8.5–8.5) | 9.2 (9.2–9.2) | — |
| frames_to_catchup | 50.0 (50.0–50.0) | 56.0 (56.0–56.0) | — |
| bytes_to_catchup | 70,776 (70,776–70,776) | 77,586 (77,586–77,586) | — |

- fix: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True, new prompt answered by replacement 1/1, scrollback unreadable (pinned notice) 0, samples with a disconnect 0/1, samples with a disconnect row left in scrollback 0; scrollback sha ['7bf638a5a45c']; final screen sha ['2d8d525211e4']
- oldfix: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True, new prompt answered by replacement 1/1, scrollback unreadable (pinned notice) 0, samples with a disconnect 1/1, samples with a disconnect row left in scrollback 1; scrollback sha ['ab08bcfa151b']; final screen sha ['2d8d525211e4']
- parent: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True; scrollback sha []; final screen sha []

### recover-transcript-64  (recover, 64 sections, 100x30, extra rows 0, native load False; n=fix 1, oldfix 1, parent 1)

| metric | fix | oldfix | parent |
|---|---:|---:|---:|
| launch_to_list_ms | 8.4 (8.4–8.4) | 9.1 (9.1–9.1) | 9.6 (9.6–9.6) |
| ctrl_r_to_ready_row_ms | 31.9 (31.9–31.9) | 36.5 (36.5–36.5) | 35.5 (35.5–35.5) |
| enter_to_first_content_ms | 11.5 (11.5–11.5) | 11.0 (11.0–11.0) | 12.2 (12.2–12.2) |
| draft_ms | 18.9 (18.9–18.9) | 24.6 (24.6–24.6) | 15.1 (15.1–15.1) |
| enter_to_latest_reply_ms | 45.0 (45.0–45.0) | 828 (828–828) | 3,255 (3,255–3,255) |
| enter_to_final_tool_ms | 45.0 (45.0–45.0) | 828 (828–828) | 3,261 (3,261–3,261) |
| enter_to_new_answer_ms | 269 (269–269) | 1,054 (1,054–1,054) | 6,583 (6,583–6,583) |
| settled_key_ms | 12.4 (12.4–12.4) | 14.7 (14.7–14.7) | 22.2 (22.2–22.2) |
| page_up_first_ms | 3.5 (3.5–3.5) | 2.0 (2.0–2.0) | 7.8 (7.8–7.8) |
| page_up_ms_median | 6.1 (6.1–6.1) | 5.9 (5.9–5.9) | 8.8 (8.8–8.8) |
| page_up_ms_max | 61.1 (61.1–61.1) | 12.4 (12.4–12.4) | 11.0 (11.0–11.0) |
| page_up_ms_last5_median | 8.0 (8.0–8.0) | 9.0 (9.0–9.0) | 7.8 (7.8–7.8) |
| frames_to_catchup | 51.0 (51.0–51.0) | 59.0 (59.0–59.0) | 1,412 (1,412–1,412) |
| bytes_to_catchup | 71,639 (71,639–71,639) | 78,752 (78,752–78,752) | 1,656,958 (1,656,958–1,656,958) |

- fix: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True, new prompt answered by replacement 1/1, scrollback unreadable (pinned notice) 0, samples with a disconnect 0/1, samples with a disconnect row left in scrollback 0; scrollback sha ['f5f828fb3c34']; final screen sha ['74129c55790c']
- oldfix: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True, new prompt answered by replacement 1/1, scrollback unreadable (pinned notice) 0, samples with a disconnect 1/1, samples with a disconnect row left in scrollback 1; scrollback sha ['de786fae9ddd']; final screen sha ['74129c55790c']
- parent: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True, new prompt answered by replacement 1/1, scrollback unreadable (pinned notice) 0, samples with a disconnect 0/1, samples with a disconnect row left in scrollback 0; scrollback sha ['f5f828fb3c34']; final screen sha ['74129c55790c']

