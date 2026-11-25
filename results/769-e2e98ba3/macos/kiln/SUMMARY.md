### open-150  (open, 150 sections, 100x30, extra rows 0, native load False; n=fix 2, parent 2)

| metric | fix | parent |
|---|---:|---:|
| launch_to_list_ms | 19.0 (16.6–21.4) | 19.5 (18.9–20.1) |
| enter_to_first_content_ms | 15.6 (13.6–17.6) | 13.3 (9.8–16.9) |
| draft_ms | 15.7 (13.1–18.3) | 13.4 (13.4–13.5) |
| enter_to_latest_reply_ms | 48.4 (43.5–53.3) | 15,673 (15,224–16,121) |
| enter_to_final_tool_ms | 48.4 (43.5–53.3) | 15,682 (15,234–16,130) |
| enter_to_live_reply_ms | 48.4 (43.5–53.3) | 15,743 (15,290–16,196) |
| key_during_stream_ms | 18.1 (16.7–19.6) | 37.9 (31.4–44.4) |
| settled_key_ms | 19.4 (17.8–20.9) | 39.5 (37.1–41.9) |
| page_up_first_ms | 7.8 (6.8–8.9) | 16.8 (16.3–17.2) |
| page_up_ms_median | 15.2 (15.0–15.3) | 17.4 (17.1–17.7) |
| page_up_ms_max | 23.0 (22.4–23.5) | 25.7 (19.7–31.8) |
| page_up_ms_last5_median | 17.1 (16.0–18.2) | 17.4 (17.1–17.7) |
| frames_to_catchup | 56.0 (56.0–56.0) | 3,260 (3,260–3,261) |
| bytes_to_catchup | 81,635 (81,290–81,980) | 3,866,153 (3,865,869–3,866,437) |

- fix: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True, samples with a disconnect 0/2, samples with a disconnect row left in scrollback 0; scrollback sha ['ccc7a8ae778a']; final screen sha ['55cb79519d6a']
- parent: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True, samples with a disconnect 2/2, samples with a disconnect row left in scrollback 2; scrollback sha ['e403b748ccdd']; final screen sha ['d4c7d71bb763']

### recover-native-64  (recover, 64 sections, 100x30, extra rows 0, native load True; n=fix 8, oldfix 8, parent 8)

| metric | fix | oldfix | parent |
|---|---:|---:|---:|
| launch_to_list_ms | 20.2 (11.6–24.0) | 19.7 (12.7–22.6) | 20.5 (18.0–22.4) |
| ctrl_r_to_ready_row_ms | 59.4 (55.4–70.7) | 58.4 (50.9–60.4) | 57.5 (51.6–62.9) |
| enter_to_first_content_ms | 18.4 (7.1–21.6) | 15.9 (13.6–20.1) | 12.1 (7.2–14.0) |
| draft_ms | 13.2 (10.0–20.2) | 16.4 (12.1–18.7) | 11.2 (9.6–22.6) |
| enter_to_latest_reply_ms | 42.5 (38.4–45.4) | 40.3 (37.1–48.9) | 2,703 (2,565–2,984) |
| enter_to_final_tool_ms | 42.5 (38.4–45.4) | 40.3 (37.1–48.9) | 2,703 (2,565–2,994) |
| enter_to_new_answer_ms | 256 (246–264) | 263 (242–285) | 5,549 (5,205–6,098) |
| settled_key_ms | 19.5 (16.4–26.9) | 21.6 (17.7–27.3) | 33.9 (25.1–47.3) |
| page_up_first_ms | 8.5 (6.3–15.7) | 8.5 (5.8–19.2) | 20.7 (10.7–27.6) |
| page_up_ms_median | 8.0 (6.9–11.1) | 9.1 (7.1–12.9) | 14.7 (10.8–23.6) |
| page_up_ms_max | 17.2 (15.9–26.3) | 18.8 (16.4–33.8) | 27.7 (17.9–33.6) |
| page_up_ms_last5_median | 15.7 (10.3–24.1) | 15.5 (11.1–18.7) | 23.7 (12.1–25.8) |
| frames_to_catchup | 50.0 (49.0–50.0) | 50.0 (49.0–50.0) | 1,412 (1,411–1,414) |
| bytes_to_catchup | 71,655 (69,150–73,326) | 70,713 (69,261–72,958) | 1,656,677 (1,656,364–1,658,182) |

- fix: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True, new prompt answered by replacement 8/8, scrollback unreadable (pinned notice) 0, samples with a disconnect 0/8, samples with a disconnect row left in scrollback 0; scrollback sha ['cfb9ffdcfe6d']; final screen sha ['1f37915b0dab']
- oldfix: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True, new prompt answered by replacement 8/8, scrollback unreadable (pinned notice) 0, samples with a disconnect 0/8, samples with a disconnect row left in scrollback 0; scrollback sha ['cfb9ffdcfe6d']; final screen sha ['1f37915b0dab']
- parent: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True, new prompt answered by replacement 8/8, scrollback unreadable (pinned notice) 0, samples with a disconnect 0/8, samples with a disconnect row left in scrollback 0; scrollback sha ['cfb9ffdcfe6d']; final screen sha ['1f37915b0dab']

### recover-transcript-64  (recover, 64 sections, 100x30, extra rows 0, native load False; n=fix 8, oldfix 8, parent 8)

| metric | fix | oldfix | parent |
|---|---:|---:|---:|
| launch_to_list_ms | 20.6 (18.3–21.9) | 19.3 (17.4–22.0) | 19.5 (17.7–20.9) |
| ctrl_r_to_ready_row_ms | 47.2 (45.0–56.2) | 44.0 (41.9–53.9) | 45.6 (44.2–51.7) |
| enter_to_first_content_ms | 13.9 (6.5–16.8) | 14.4 (6.8–18.8) | 7.9 (6.6–15.6) |
| draft_ms | 13.7 (9.6–24.6) | 16.3 (9.8–23.0) | 17.1 (10.5–19.3) |
| enter_to_latest_reply_ms | 40.8 (36.8–51.2) | 41.8 (29.3–44.2) | 2,700 (2,568–3,036) |
| enter_to_final_tool_ms | 40.8 (36.8–51.2) | 41.8 (29.3–44.2) | 2,703 (2,575–3,036) |
| enter_to_new_answer_ms | 252 (235–272) | 263 (238–264) | 5,471 (5,205–6,167) |
| settled_key_ms | 20.2 (16.9–22.8) | 23.3 (20.4–27.1) | 35.4 (31.0–44.0) |
| page_up_first_ms | 7.1 (6.7–16.2) | 9.4 (7.4–17.5) | 17.7 (7.7–26.7) |
| page_up_ms_median | 8.9 (7.0–14.6) | 9.3 (7.0–14.3) | 17.9 (14.1–23.4) |
| page_up_ms_max | 23.6 (16.8–29.1) | 24.0 (16.2–26.8) | 26.0 (19.0–33.0) |
| page_up_ms_last5_median | 18.6 (10.1–24.6) | 16.8 (13.9–25.1) | 17.5 (14.2–23.4) |
| frames_to_catchup | 50.0 (49.0–50.0) | 50.0 (42.0–50.0) | 1,412 (1,411–1,413) |
| bytes_to_catchup | 71,134 (69,333–73,706) | 71,006 (58,554–73,065) | 1,656,970 (1,656,355–1,657,572) |

- fix: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True, new prompt answered by replacement 8/8, scrollback unreadable (pinned notice) 0, samples with a disconnect 0/8, samples with a disconnect row left in scrollback 0; scrollback sha ['f7fbbce4638c']; final screen sha ['c659d0effcad']
- oldfix: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True, new prompt answered by replacement 8/8, scrollback unreadable (pinned notice) 0, samples with a disconnect 0/8, samples with a disconnect row left in scrollback 0; scrollback sha ['f7fbbce4638c']; final screen sha ['c659d0effcad']
- parent: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True, new prompt answered by replacement 8/8, scrollback unreadable (pinned notice) 0, samples with a disconnect 0/8, samples with a disconnect row left in scrollback 0; scrollback sha ['f7fbbce4638c']; final screen sha ['c659d0effcad']

