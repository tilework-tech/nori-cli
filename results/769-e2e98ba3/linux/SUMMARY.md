### recover-base-native-64  (recover, 64 sections, 100x30, extra rows 0, native load True; n=fix 10, base 10)

| metric | fix | base |
|---|---:|---:|
| launch_to_list_ms | 7.8 (6.0–9.3) | 8.7 (5.9–13.2) |
| ctrl_r_to_ready_row_ms | 49.0 (45.7–58.3) | 52.3 (39.9–57.8) |
| enter_to_first_content_ms | 14.1 (10.1–16.7) | 10.1 (8.6–13.3) |
| draft_ms | 17.6 (10.9–23.6) | 14.4 (10.1–17.3) |
| enter_to_latest_reply_ms | 47.4 (30.1–65.0) | 3,235 (3,219–3,269) |
| enter_to_final_tool_ms | 47.4 (30.1–65.0) | 3,242 (3,222–3,275) |
| enter_to_new_answer_ms | 271 (230–294) | 6,589 (6,554–6,628) |
| settled_key_ms | 13.9 (12.4–16.6) | 24.3 (16.7–27.8) |
| page_up_first_ms | 2.8 (1.9–3.6) | 8.8 (8.0–9.5) |
| page_up_ms_median | 5.8 (5.4–6.0) | 8.8 (8.6–9.0) |
| page_up_ms_max | 10.6 (9.2–17.1) | 11.4 (11.2–14.6) |
| page_up_ms_last5_median | 8.8 (6.5–11.0) | 8.8 (8.6–9.1) |
| frames_to_catchup | 51.0 (35.0–54.0) | 1,412 (1,412–1,412) |
| bytes_to_catchup | 72,197 (49,021–76,520) | 1,656,954 (1,656,934–1,656,961) |

- fix: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True, new prompt answered by replacement 10/10, scrollback unreadable (pinned notice) 0, samples with a disconnect 0/10, samples with a disconnect row left in scrollback 0; scrollback sha ['92217e090a65']; final screen sha ['60ca8ba5cefb']
- base: scrollback ordered exactly once False (duplicates 0), reached start False, draft intact True, new prompt answered by replacement 6/10, scrollback unreadable (pinned notice) 4, samples with a disconnect 4/10, samples with a disconnect row left in scrollback 0; scrollback sha ['92217e090a65']; final screen sha ['23c96850e06a', '60ca8ba5cefb']

### recover-base-transcript-64  (recover, 64 sections, 100x30, extra rows 0, native load False; n=fix 10, base 10)

| metric | fix | base |
|---|---:|---:|
| launch_to_list_ms | 8.3 (7.5–11.1) | 7.9 (4.5–8.5) |
| ctrl_r_to_ready_row_ms | 33.0 (31.5–55.5) | 33.3 (30.8–38.1) |
| enter_to_first_content_ms | 13.8 (8.7–15.8) | 10.9 (8.3–15.7) |
| draft_ms | 14.6 (11.1–24.4) | 14.4 (10.9–18.1) |
| enter_to_latest_reply_ms | 57.6 (47.0–82.1) | 3,234 (3,215–3,284) |
| enter_to_final_tool_ms | 57.6 (47.0–82.1) | 3,238 (3,219–3,287) |
| enter_to_new_answer_ms | 281 (270–322) | 6,575 (6,546–6,605) |
| settled_key_ms | 13.7 (11.9–15.3) | 23.4 (20.1–24.4) |
| page_up_first_ms | 2.7 (1.8–3.9) | 9.0 (8.0–9.1) |
| page_up_ms_median | 5.8 (5.2–6.0) | 8.8 (8.3–9.0) |
| page_up_ms_max | 10.5 (9.4–10.9) | 13.2 (10.7–17.8) |
| page_up_ms_last5_median | 8.6 (8.3–9.2) | 8.8 (8.1–9.3) |
| frames_to_catchup | 51.0 (50.0–54.0) | 1,412 (1,412–1,412) |
| bytes_to_catchup | 72,407 (70,196–76,418) | 1,656,959 (1,656,955–1,656,969) |

- fix: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True, new prompt answered by replacement 10/10, scrollback unreadable (pinned notice) 0, samples with a disconnect 0/10, samples with a disconnect row left in scrollback 0; scrollback sha ['14575b3c800f']; final screen sha ['0c216f12a9bd']
- base: scrollback ordered exactly once False (duplicates 0), reached start False, draft intact True, new prompt answered by replacement 6/10, scrollback unreadable (pinned notice) 4, samples with a disconnect 4/10, samples with a disconnect row left in scrollback 0; scrollback sha ['14575b3c800f']; final screen sha ['0c216f12a9bd', '23c96850e06a']

### recover-fixonly-native-64  (recover, 64 sections, 100x30, extra rows 0, native load True; n=fix 16)

| metric | fix |
|---|---:|
| launch_to_list_ms | 9.0 (5.8–9.8) |
| ctrl_r_to_ready_row_ms | 51.0 (42.2–57.6) |
| enter_to_first_content_ms | 15.0 (10.5–21.1) |
| draft_ms | 15.6 (10.4–21.4) |
| enter_to_latest_reply_ms | 47.8 (28.5–66.8) |
| enter_to_final_tool_ms | 47.8 (28.5–66.8) |
| enter_to_new_answer_ms | 270 (229–285) |
| settled_key_ms | 14.4 (12.5–16.7) |
| page_up_first_ms | 2.8 (2.0–3.8) |
| page_up_ms_median | 5.8 (5.5–6.3) |
| page_up_ms_max | 10.1 (9.2–11.1) |
| page_up_ms_last5_median | 8.4 (8.1–10.2) |
| frames_to_catchup | 50.5 (27.0–52.0) |
| bytes_to_catchup | 71,128 (36,672–74,461) |

- fix: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True, new prompt answered by replacement 16/16, scrollback unreadable (pinned notice) 0, samples with a disconnect 0/16, samples with a disconnect row left in scrollback 0; scrollback sha ['129cb6005774']; final screen sha ['c32d232368d2']

### recover-fixonly-transcript-64  (recover, 64 sections, 100x30, extra rows 0, native load False; n=fix 24)

| metric | fix |
|---|---:|
| launch_to_list_ms | 8.5 (5.5–10.0) |
| ctrl_r_to_ready_row_ms | 34.0 (31.6–39.2) |
| enter_to_first_content_ms | 14.1 (6.8–18.7) |
| draft_ms | 14.5 (10.4–25.2) |
| enter_to_latest_reply_ms | 55.5 (34.5–64.5) |
| enter_to_final_tool_ms | 55.5 (34.5–64.5) |
| enter_to_new_answer_ms | 281 (229–293) |
| settled_key_ms | 14.3 (12.4–16.7) |
| page_up_first_ms | 2.4 (1.9–4.2) |
| page_up_ms_median | 5.8 (5.4–6.1) |
| page_up_ms_max | 10.7 (9.1–12.3) |
| page_up_ms_last5_median | 8.5 (7.7–9.7) |
| frames_to_catchup | 51.0 (30.0–54.0) |
| bytes_to_catchup | 72,245 (39,855–75,693) |

- fix: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True, new prompt answered by replacement 24/24, scrollback unreadable (pinned notice) 0, samples with a disconnect 0/24, samples with a disconnect row left in scrollback 0; scrollback sha ['b08f1eb82979']; final screen sha ['1beb19b44eef']

### recover-native-64  (recover, 64 sections, 100x30, extra rows 0, native load True; n=fix 12, oldfix 12, parent 12)

| metric | fix | oldfix | parent |
|---|---:|---:|---:|
| launch_to_list_ms | 8.0 (5.4–10.3) | 7.6 (4.5–11.8) | 8.3 (4.9–9.6) |
| ctrl_r_to_ready_row_ms | 47.0 (38.9–53.6) | 49.1 (42.8–58.4) | 47.1 (41.3–58.9) |
| enter_to_first_content_ms | 12.7 (6.3–18.7) | 12.2 (6.4–15.6) | 10.0 (7.1–11.7) |
| draft_ms | 14.9 (10.6–17.8) | 14.6 (10.2–24.2) | 13.6 (11.1–15.8) |
| enter_to_latest_reply_ms | 57.2 (26.9–62.9) | 59.6 (49.4–811.5) | 3,254 (3,213–3,293) |
| enter_to_final_tool_ms | 57.2 (26.9–62.9) | 59.6 (49.4–811.5) | 3,259 (3,219–3,298) |
| enter_to_new_answer_ms | 275 (243–289) | 282 (245–1,034) | 6,592 (6,532–6,680) |
| settled_key_ms | 14.6 (12.8–15.3) | 14.0 (11.7–16.1) | 22.5 (17.9–27.0) |
| page_up_first_ms | 2.3 (1.9–3.9) | 2.4 (2.0–4.1) | 8.4 (7.2–10.2) |
| page_up_ms_median | 5.6 (5.2–5.9) | 5.6 (5.4–5.7) | 8.6 (8.2–8.9) |
| page_up_ms_max | 10.0 (8.6–10.5) | 10.5 (9.9–15.5) | 11.6 (10.7–15.5) |
| page_up_ms_last5_median | 8.3 (7.8–9.1) | 8.3 (7.8–9.9) | 8.4 (7.8–9.7) |
| frames_to_catchup | 51.5 (33.0–53.0) | 52.0 (50.0–57.0) | 1,412 (1,412–1,412) |
| bytes_to_catchup | 72,642 (45,700–74,808) | 74,154 (70,632–78,650) | 1,656,952 (1,656,932–1,656,959) |

- fix: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True, new prompt answered by replacement 12/12, scrollback unreadable (pinned notice) 0, samples with a disconnect 0/12, samples with a disconnect row left in scrollback 0; scrollback sha ['f388307a4417']; final screen sha ['724530d3f36a']
- oldfix: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True, new prompt answered by replacement 12/12, scrollback unreadable (pinned notice) 0, samples with a disconnect 2/12, samples with a disconnect row left in scrollback 2; scrollback sha ['8dc0e7aada43', 'b87358e9bc59', 'f388307a4417']; final screen sha ['724530d3f36a']
- parent: scrollback ordered exactly once False (duplicates 0), reached start False, draft intact True, new prompt answered by replacement 8/12, scrollback unreadable (pinned notice) 4, samples with a disconnect 4/12, samples with a disconnect row left in scrollback 0; scrollback sha ['f388307a4417']; final screen sha ['23c96850e06a', '724530d3f36a']

### recover-transcript-64  (recover, 64 sections, 100x30, extra rows 0, native load False; n=fix 12, oldfix 12, parent 12)

| metric | fix | oldfix | parent |
|---|---:|---:|---:|
| launch_to_list_ms | 8.8 (5.8–9.9) | 9.2 (8.0–19.8) | 8.5 (7.5–12.8) |
| ctrl_r_to_ready_row_ms | 35.2 (30.9–59.8) | 36.7 (28.8–85.0) | 36.5 (31.2–161.8) |
| enter_to_first_content_ms | 15.9 (8.0–23.8) | 13.0 (9.5–19.9) | 10.9 (8.6–16.4) |
| draft_ms | 13.5 (10.0–19.4) | 14.8 (11.1–22.3) | 13.4 (8.7–15.8) |
| enter_to_latest_reply_ms | 61.1 (35.8–826.1) | 57.5 (39.5–768.2) | 3,258 (3,211–3,310) |
| enter_to_final_tool_ms | 61.1 (35.8–826.1) | 57.5 (39.6–768.2) | 3,262 (3,214–3,313) |
| enter_to_new_answer_ms | 287 (261–1,051) | 283 (238–968) | 6,608 (6,498–6,672) |
| settled_key_ms | 14.3 (13.0–16.6) | 14.0 (12.6–15.0) | 23.8 (16.7–26.0) |
| page_up_first_ms | 2.8 (1.9–4.5) | 3.0 (2.5–3.8) | 8.4 (8.1–10.9) |
| page_up_ms_median | 6.1 (5.6–6.6) | 6.1 (5.7–6.4) | 8.9 (8.3–9.3) |
| page_up_ms_max | 11.5 (9.9–15.7) | 11.8 (9.4–17.7) | 12.7 (11.5–17.8) |
| page_up_ms_last5_median | 9.2 (7.8–9.7) | 8.8 (7.3–11.0) | 8.9 (8.0–10.0) |
| frames_to_catchup | 52.5 (27.0–58.0) | 51.0 (29.0–55.0) | 1,412 (1,412–1,412) |
| bytes_to_catchup | 73,166 (36,669–78,303) | 72,776 (38,134–76,525) | 1,656,960 (1,656,941–1,656,965) |

- fix: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True, new prompt answered by replacement 12/12, scrollback unreadable (pinned notice) 0, samples with a disconnect 1/12, samples with a disconnect row left in scrollback 1; scrollback sha ['31c3380009a2', '387cce0d7434']; final screen sha ['9126d4fcc283']
- oldfix: scrollback ordered exactly once False (duplicates 0), reached start True, draft intact True, new prompt answered by replacement 12/12, scrollback unreadable (pinned notice) 0, samples with a disconnect 1/12, samples with a disconnect row left in scrollback 1; scrollback sha ['31c3380009a2', 'c9dd4458d5f4']; final screen sha ['9126d4fcc283', '9c49441cc648']
- parent: scrollback ordered exactly once False (duplicates 0), reached start False, draft intact True, new prompt answered by replacement 8/12, scrollback unreadable (pinned notice) 4, samples with a disconnect 4/12, samples with a disconnect row left in scrollback 0; scrollback sha ['31c3380009a2']; final screen sha ['23c96850e06a', '9126d4fcc283']

### recover-transcript-85  (recover, 85 sections, 100x30, extra rows 0, native load False; n=fix 4, oldfix 4)

| metric | fix | oldfix |
|---|---:|---:|
| launch_to_list_ms | 8.4 (7.8–10.7) | 9.4 (4.8–11.4) |
| ctrl_r_to_ready_row_ms | 36.8 (33.7–41.2) | 37.3 (36.0–39.9) |
| enter_to_first_content_ms | 11.1 (9.4–20.0) | 15.9 (7.5–18.4) |
| draft_ms | 17.5 (16.8–24.8) | 13.2 (12.3–20.4) |
| enter_to_latest_reply_ms | 62.2 (55.0–70.8) | 818 (32–821) |
| enter_to_final_tool_ms | 62.2 (55.0–70.8) | 818 (33–821) |
| enter_to_new_answer_ms | 285 (276–294) | 1,040 (1,039–1,049) |
| settled_key_ms | 13.0 (12.5–14.0) | 14.3 (13.8–15.9) |
| page_up_first_ms | 2.2 (2.0–2.7) | 2.5 (2.2–3.2) |
| page_up_ms_median | 6.5 (6.3–7.1) | 6.4 (6.2–6.5) |
| page_up_ms_max | 14.1 (10.1–15.1) | 14.1 (12.5–20.3) |
| page_up_ms_last5_median | 9.1 (8.0–10.1) | 9.4 (8.7–9.9) |
| frames_to_catchup | 65.5 (64.0–66.0) | 70.5 (34.0–72.0) |
| bytes_to_catchup | 93,698 (91,189–94,655) | 98,852 (47,116–100,086) |

- fix: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True, new prompt answered by replacement 4/4, scrollback unreadable (pinned notice) 0, samples with a disconnect 0/4, samples with a disconnect row left in scrollback 0; scrollback sha ['41595ed3fdf6']; final screen sha ['c5f8d1bca81a']
- oldfix: scrollback ordered exactly once False (duplicates 0), reached start True, draft intact False, new prompt answered by replacement 3/4, scrollback unreadable (pinned notice) 0, samples with a disconnect 4/4, samples with a disconnect row left in scrollback 4; scrollback sha ['5f632b868cf0', '699f65258424', 'c3f1683613bd', 'ca401d46590a']; final screen sha ['134c4b829da2', 'c5f8d1bca81a']

### open-150  (open, 150 sections, 100x30, extra rows 0, native load False; n=fix 3, parent 3)

| metric | fix | parent |
|---|---:|---:|
| launch_to_list_ms | 7.8 (7.6–8.2) | 8.2 (7.8–8.5) |
| enter_to_first_content_ms | 14.6 (11.5–15.0) | 11.1 (8.4–13.3) |
| draft_ms | 13.6 (13.0–21.4) | 12.1 (10.7–14.9) |
| enter_to_latest_reply_ms | 55.6 (54.7–69.6) | 16,701 (16,694–16,728) |
| enter_to_final_tool_ms | 55.6 (54.7–69.6) | 16,713 (16,700–16,737) |
| enter_to_live_reply_ms | 57.9 (55.6–69.6) | 16,781 (16,771–16,807) |
| key_during_stream_ms | 14.6 (14.4–15.3) | 49.3 (35.6–71.5) |
| settled_key_ms | 14.4 (14.0–15.8) | 48.7 (47.2–50.9) |
| page_up_first_ms | 2.5 (2.1–2.5) | 13.4 (13.4–16.8) |
| page_up_ms_median | 8.8 (8.5–8.8) | 14.2 (14.1–14.6) |
| page_up_ms_max | 16.4 (15.1–17.2) | 32.5 (17.6–35.7) |
| page_up_ms_last5_median | 12.8 (12.7–14.3) | 13.8 (13.3–14.5) |
| frames_to_catchup | 59.0 (57.0–63.0) | 3,261 (3,261–3,261) |
| bytes_to_catchup | 83,126 (80,755–86,833) | 3,866,423 (3,866,404–3,866,429) |

- fix: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True, samples with a disconnect 0/3, samples with a disconnect row left in scrollback 0; scrollback sha ['ccc7a8ae778a']; final screen sha ['55cb79519d6a']
- parent: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True, samples with a disconnect 3/3, samples with a disconnect row left in scrollback 3; scrollback sha ['e403b748ccdd']; final screen sha ['d4c7d71bb763']

