### open-64  (open, 64 sections, 100x30, extra rows 0, native load False; n=base 3, fix 3)

| metric | base | fix |
|---|---:|---:|
| launch_to_list_ms | 20.2 (16.6–20.8) | 20.6 (14.5–20.9) |
| enter_to_first_content_ms | 9.8 (7.8–15.3) | 17.0 (8.6–18.6) |
| draft_ms | 12.6 (12.6–16.9) | 19.2 (13.6–22.4) |
| enter_to_latest_reply_ms | 2,977 (2,822–2,999) | 28.2 (28.2–34.8) |
| enter_to_final_tool_ms | 2,977 (2,831–3,006) | 28.2 (28.2–34.8) |
| enter_to_live_reply_ms | 3,012 (2,855–3,038) | 28.2 (28.2–34.8) |
| key_during_stream_ms | 32.7 (28.1–35.8) | 18.8 (18.0–23.1) |
| settled_key_ms | 29.2 (25.8–31.7) | 23.8 (17.1–26.9) |
| page_up_first_ms | 14.5 (9.1–16.4) | 8.0 (7.3–8.5) |
| page_up_ms_median | 9.3 (9.2–14.3) | 8.8 (8.7–8.9) |
| page_up_ms_max | 18.7 (18.6–19.5) | 17.4 (15.5–17.7) |
| page_up_ms_last5_median | 13.9 (8.8–16.6) | 10.6 (9.3–14.2) |
| frames_to_catchup | 1,420 (1,419–1,420) | 28.0 (28.0–29.0) |
| bytes_to_catchup | 1,662,244 (1,661,653–1,662,251) | 36,878 (36,555–38,159) |

- base: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True; scrollback sha ['be5a43f29834']; final screen sha ['55cb79519d6a']
- fix: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True; scrollback sha ['be5a43f29834']; final screen sha ['55cb79519d6a']

### open-4  (open, 4 sections, 100x30, extra rows 0, native load False; n=base 2, fix 2)

| metric | base | fix |
|---|---:|---:|
| launch_to_list_ms | 20.6 (19.7–21.4) | 18.6 (17.8–19.3) |
| enter_to_first_content_ms | 8.9 (8.4–9.3) | 12.7 (8.4–16.9) |
| draft_ms | 12.1 (12.0–12.2) | 17.6 (15.1–20.2) |
| enter_to_latest_reply_ms | 70.3 (68.6–72.0) | 22.9 (17.4–28.4) |
| enter_to_final_tool_ms | 70.3 (68.6–72.0) | 22.9 (17.4–28.4) |
| enter_to_live_reply_ms | 74.6 (72.0–77.2) | 22.9 (17.4–28.4) |
| key_during_stream_ms | 27.1 (26.5–27.8) | 21.6 (17.7–25.6) |
| settled_key_ms | 19.9 (15.5–24.2) | 21.5 (16.6–26.5) |
| page_up_first_ms | 8.9 (8.3–9.5) | 7.7 (6.9–8.5) |
| page_up_ms_median | 8.7 (8.5–8.9) | 8.5 (8.2–8.7) |
| page_up_ms_max | 11.0 (9.6–12.4) | 9.3 (9.2–9.4) |
| page_up_ms_last5_median | 9.0 (9.0–9.0) | 8.8 (8.7–8.8) |
| frames_to_catchup | 186 (186–186) | 10.5 (10.0–11.0) |
| bytes_to_catchup | 174,000 (174,000–174,001) | 10,026 (9,818–10,235) |

- base: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True; scrollback sha ['e4dcc5281e29']; final screen sha ['55cb79519d6a']
- fix: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True; scrollback sha ['e4dcc5281e29']; final screen sha ['55cb79519d6a']

### recover-transcript-64  (recover, 64 sections, 100x30, extra rows 0, native load False; n=base 2, fix 2)

| metric | base | fix |
|---|---:|---:|
| launch_to_list_ms | 21.6 (20.2–22.9) | 21.1 (20.9–21.4) |
| ctrl_r_to_ready_row_ms | 54.8 (50.5–59.1) | 56.7 (51.9–61.5) |
| enter_to_first_content_ms | 13.3 (9.7–16.9) | 22.4 (18.2–26.7) |
| draft_ms | 13.1 (12.9–13.4) | 12.9 (12.6–13.2) |
| enter_to_latest_reply_ms | 2,984 (2,974–2,994) | 43.9 (43.8–44.0) |
| enter_to_final_tool_ms | 2,988 (2,974–3,002) | 43.9 (43.8–44.0) |
| enter_to_new_answer_ms | 6,088 (6,083–6,093) | 263 (257–269) |
| settled_key_ms | 29.6 (25.7–33.6) | 17.9 (17.9–18.0) |
| page_up_first_ms | 9.3 (8.6–10.0) | 8.6 (8.3–8.8) |
| page_up_ms_median | 12.4 (9.7–15.0) | 9.0 (8.8–9.2) |
| page_up_ms_max | 20.0 (19.7–20.4) | 18.4 (17.5–19.3) |
| page_up_ms_last5_median | 10.0 (9.0–10.9) | 13.5 (9.4–17.6) |
| frames_to_catchup | 1,412 (1,412–1,413) | 49.5 (49.0–50.0) |
| bytes_to_catchup | 1,657,276 (1,656,959–1,657,593) | 71,784 (71,370–72,197) |

- base: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True, answered by replacement True; scrollback sha ['3affe3f731a5']; final screen sha ['119a33acf40f']
- fix: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True, answered by replacement True; scrollback sha ['3affe3f731a5']; final screen sha ['119a33acf40f']

### recover-native-64  (recover, 64 sections, 100x30, extra rows 0, native load True; n=base 2, fix 2)

| metric | base | fix |
|---|---:|---:|
| launch_to_list_ms | 20.3 (19.3–21.3) | 21.2 (20.3–22.2) |
| ctrl_r_to_ready_row_ms | 65.3 (64.5–66.1) | 61.6 (56.2–66.9) |
| enter_to_first_content_ms | 9.6 (8.0–11.1) | 15.1 (13.6–16.5) |
| draft_ms | 17.0 (15.4–18.5) | 14.1 (11.6–16.6) |
| enter_to_latest_reply_ms | 2,995 (2,980–3,009) | 44.4 (41.5–47.4) |
| enter_to_final_tool_ms | 2,998 (2,987–3,009) | 44.4 (41.5–47.4) |
| enter_to_new_answer_ms | 5,983 (5,922–6,044) | 256 (254–258) |
| settled_key_ms | 30.8 (30.2–31.4) | 19.0 (16.1–21.9) |
| page_up_first_ms | 16.3 (16.2–16.4) | 12.5 (7.6–17.4) |
| page_up_ms_median | 15.4 (15.2–15.6) | 8.9 (8.5–9.2) |
| page_up_ms_max | 26.9 (21.3–32.4) | 25.2 (22.7–27.8) |
| page_up_ms_last5_median | 12.4 (8.7–16.1) | 18.0 (10.6–25.3) |
| frames_to_catchup | 1,413 (1,413–1,413) | 50.0 (50.0–50.0) |
| bytes_to_catchup | 1,657,578 (1,657,575–1,657,581) | 71,559 (70,370–72,748) |

- base: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True, answered by replacement True; scrollback sha ['7aead9ab9266']; final screen sha ['98ab07402d53']
- fix: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True, answered by replacement True; scrollback sha ['7aead9ab9266']; final screen sha ['98ab07402d53']

