### open-64  (open, 64 sections, 100x30, extra rows 0, native load False; n=base 5, fix 5)

| metric | base | fix |
|---|---:|---:|
| launch_to_list_ms | 8.8 (8.4–9.7) | 9.7 (8.3–11.4) |
| enter_to_first_content_ms | 8.7 (7.2–13.0) | 14.2 (8.8–16.0) |
| draft_ms | 13.2 (10.2–18.9) | 16.3 (11.7–19.2) |
| enter_to_latest_reply_ms | 3,030 (3,022–3,042) | 29.8 (27.3–54.2) |
| enter_to_final_tool_ms | 3,033 (3,025–3,046) | 29.8 (27.3–54.2) |
| enter_to_live_reply_ms | 3,064 (3,052–3,073) | 55.8 (54.2–59.4) |
| key_during_stream_ms | 27.2 (26.9–27.3) | 13.4 (13.2–14.3) |
| settled_key_ms | 21.3 (20.3–25.2) | 13.8 (13.5–14.4) |
| page_up_first_ms | 7.5 (6.5–10.2) | 3.0 (2.9–3.4) |
| page_up_ms_median | 6.6 (6.5–6.7) | 6.5 (6.2–6.7) |
| page_up_ms_max | 10.7 (10.2–11.8) | 9.0 (8.4–10.4) |
| page_up_ms_last5_median | 6.6 (6.5–8.2) | 6.5 (6.1–7.7) |
| frames_to_catchup | 1,419 (1,419–1,419) | 30.0 (29.0–31.0) |
| bytes_to_catchup | 1,661,589 (1,661,585–1,661,609) | 40,047 (38,306–40,671) |

- base: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True; scrollback sha ['be5a43f29834']; final screen sha ['55cb79519d6a']
- fix: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True; scrollback sha ['be5a43f29834']; final screen sha ['55cb79519d6a']

### open-64-rows24-160x50  (open, 64 sections, 160x50, extra rows 24, native load False; n=base 3, fix 3)

| metric | base | fix |
|---|---:|---:|
| launch_to_list_ms | 9.7 (8.7–10.1) | 9.9 (9.8–10.8) |
| filter_to_single_row_ms | 8.1 (5.8–8.8) | 4.8 (4.6–9.1) |
| enter_to_first_content_ms | 8.9 (7.9–9.8) | 13.6 (8.7–16.0) |
| draft_ms | 13.2 (10.1–15.1) | 13.9 (12.3–15.5) |
| enter_to_latest_reply_ms | 3,173 (3,170–3,189) | 53.8 (34.0–56.6) |
| enter_to_final_tool_ms | 3,177 (3,173–3,193) | 53.8 (34.0–56.6) |
| enter_to_live_reply_ms | 3,203 (3,202–3,224) | 53.8 (34.0–56.6) |
| key_during_stream_ms | 28.4 (28.2–28.6) | 13.7 (13.3–13.9) |
| settled_key_ms | 24.6 (23.8–25.2) | 14.7 (13.3–15.5) |
| page_up_first_ms | 7.2 (7.1–9.8) | 3.2 (3.0–3.2) |
| page_up_ms_median | 6.8 (6.8–7.0) | 6.5 (6.5–6.5) |
| page_up_ms_max | 12.0 (11.4–14.5) | 9.0 (8.6–9.8) |
| page_up_ms_last5_median | 6.8 (6.7–7.8) | 6.8 (6.4–6.8) |
| frames_to_catchup | 1,419 (1,419–1,420) | 32.0 (29.0–33.0) |
| bytes_to_catchup | 2,970,756 (2,970,755–2,971,765) | 74,442 (69,744–75,643) |

- base: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True; scrollback sha ['df87158b09f4']; final screen sha ['f93f5e54e947']
- fix: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True; scrollback sha ['df87158b09f4']; final screen sha ['f93f5e54e947']

### open-4  (open, 4 sections, 100x30, extra rows 0, native load False; n=base 3, fix 3)

| metric | base | fix |
|---|---:|---:|
| launch_to_list_ms | 8.8 (8.5–8.9) | 10.3 (8.6–11.7) |
| enter_to_first_content_ms | 8.4 (7.6–9.8) | 11.0 (4.0–11.2) |
| draft_ms | 13.3 (12.5–13.7) | 12.0 (11.9–15.0) |
| enter_to_latest_reply_ms | 68.7 (68.5–69.9) | 46.5 (45.1–49.7) |
| enter_to_final_tool_ms | 68.7 (68.5–69.9) | 46.5 (45.1–49.7) |
| enter_to_live_reply_ms | 73.2 (72.0–73.4) | 46.5 (45.1–49.7) |
| key_during_stream_ms | 14.7 (14.4–17.1) | 13.2 (13.2–14.3) |
| settled_key_ms | 13.8 (13.7–14.7) | 13.4 (13.4–14.1) |
| page_up_first_ms | 3.2 (3.0–3.2) | 3.3 (2.8–3.4) |
| page_up_ms_median | 3.0 (3.0–3.1) | 3.0 (2.9–3.1) |
| page_up_ms_max | 4.2 (3.3–4.2) | 4.1 (3.9–7.2) |
| page_up_ms_last5_median | 3.1 (2.8–3.3) | 3.0 (3.0–3.2) |
| frames_to_catchup | 186 (186–186) | 11.0 (9.0–13.0) |
| bytes_to_catchup | 173,995 (173,992–174,002) | 11,640 (8,749–12,044) |

- base: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True; scrollback sha ['e4dcc5281e29']; final screen sha ['55cb79519d6a']
- fix: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True; scrollback sha ['e4dcc5281e29']; final screen sha ['55cb79519d6a']

### open-150  (open, 150 sections, 100x30, extra rows 0, native load False; n=base 3, fix 3)

| metric | base | fix |
|---|---:|---:|
| launch_to_list_ms | 9.4 (8.3–10.6) | 8.6 (8.3–8.6) |
| enter_to_first_content_ms | 11.9 (9.6–15.1) | 11.3 (8.1–19.3) |
| draft_ms | 21.0 (9.7–21.1) | 27.5 (27.1–27.6) |
| enter_to_latest_reply_ms | 15,620 (15,563–15,628) | 59.5 (58.3–61.8) |
| enter_to_final_tool_ms | 15,627 (15,570–15,638) | 59.5 (58.3–61.8) |
| enter_to_live_reply_ms | 15,694 (15,635–15,704) | 59.5 (58.3–61.8) |
| key_during_stream_ms | 64.0 (34.9–83.2) | 13.6 (13.4–14.3) |
| settled_key_ms | 31.9 (30.8–32.7) | 13.7 (13.2–14.4) |
| page_up_first_ms | 13.2 (13.1–13.2) | 3.2 (3.2–3.4) |
| page_up_ms_median | 13.5 (13.5–13.6) | 9.5 (8.2–9.5) |
| page_up_ms_max | 31.7 (28.2–33.1) | 23.8 (19.8–24.9) |
| page_up_ms_last5_median | 13.5 (13.3–13.6) | 13.6 (13.1–13.6) |
| frames_to_catchup | 3,260 (3,260–3,260) | 58.0 (58.0–59.0) |
| bytes_to_catchup | 3,865,827 (3,865,826–3,865,837) | 81,037 (80,469–82,753) |

- base: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True, samples with a disconnect 3/3, samples with a disconnect row left in scrollback 3; scrollback sha ['e403b748ccdd']; final screen sha ['d4c7d71bb763']
- fix: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True, samples with a disconnect 0/3, samples with a disconnect row left in scrollback 0; scrollback sha ['ccc7a8ae778a']; final screen sha ['55cb79519d6a']

### recover-transcript-64  (recover, 64 sections, 100x30, extra rows 0, native load False; n=base 1, fix 2)

| metric | base | fix |
|---|---:|---:|
| launch_to_list_ms | 9.3 (9.3–9.3) | 9.8 (9.8–9.8) |
| ctrl_r_to_ready_row_ms | 39.1 (39.1–39.1) | 34.6 (33.9–35.2) |
| enter_to_first_content_ms | 11.4 (11.4–11.4) | 12.5 (11.7–13.2) |
| draft_ms | 19.9 (19.9–19.9) | 15.6 (9.9–21.2) |
| enter_to_latest_reply_ms | 3,019 (3,019–3,019) | 800 (780–821) |
| enter_to_final_tool_ms | 3,022 (3,022–3,022) | 800 (780–821) |
| enter_to_new_answer_ms | 6,113 (6,113–6,113) | 1,007 (966–1,047) |
| settled_key_ms | 23.7 (23.7–23.7) | 13.1 (12.9–13.3) |
| page_up_first_ms | 6.7 (6.7–6.7) | 3.0 (2.8–3.2) |
| page_up_ms_median | 6.7 (6.7–6.7) | 6.6 (6.5–6.7) |
| page_up_ms_max | 10.9 (10.9–10.9) | 8.8 (8.6–9.0) |
| page_up_ms_last5_median | 7.5 (7.5–7.5) | 6.6 (6.3–6.9) |
| frames_to_catchup | 1,412 (1,412–1,412) | 57.5 (57.0–58.0) |
| bytes_to_catchup | 1,656,967 (1,656,967–1,656,967) | 78,799 (78,562–79,036) |

- base: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True, answered by replacement True; scrollback sha ['764b52f40987']; final screen sha ['1203ac784f9a']
- fix: scrollback ordered exactly once False (duplicates 0), reached start True, draft intact True, answered by replacement True; scrollback sha ['1909b6fb80e5', '9711e38925e5']; final screen sha ['1203ac784f9a']

### recover-transcript-64-repro  (recover, 64 sections, 100x30, extra rows 0, native load False; n=base 3, fix 4)

| metric | base | fix |
|---|---:|---:|
| launch_to_list_ms | 8.5 (8.5–9.7) | 9.2 (8.6–9.9) |
| ctrl_r_to_ready_row_ms | 35.9 (32.3–39.9) | 36.3 (36.0–36.8) |
| enter_to_first_content_ms | 15.2 (11.0–16.5) | 11.5 (9.9–12.6) |
| draft_ms | 15.4 (13.1–17.3) | 19.9 (17.9–24.7) |
| enter_to_latest_reply_ms | 3,027 (3,016–3,033) | 52.3 (51.1–58.0) |
| enter_to_final_tool_ms | 3,031 (3,019–3,036) | 52.3 (51.1–58.0) |
| enter_to_new_answer_ms | 6,150 (6,133–6,164) | 278 (274–286) |
| settled_key_ms | 23.0 (21.6–24.7) | 14.6 (13.3–15.1) |
| page_up_first_ms | 6.4 (6.4–6.4) | 3.0 (2.9–3.1) |
| page_up_ms_median | 6.6 (6.5–6.6) | 6.4 (6.4–6.4) |
| page_up_ms_max | 10.9 (10.6–11.5) | 8.5 (8.3–8.8) |
| page_up_ms_last5_median | 6.5 (6.4–6.6) | 6.5 (6.2–7.4) |
| frames_to_catchup | 1,412 (1,412–1,412) | 51.0 (51.0–52.0) |
| bytes_to_catchup | 1,656,970 (1,656,960–1,656,973) | 71,108 (70,478–73,164) |

- base: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True, answered by replacement True; scrollback sha ['adfa3664086d']; final screen sha ['68d03242ed32']
- fix: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True, answered by replacement True; scrollback sha ['adfa3664086d']; final screen sha ['68d03242ed32']

### recover-transcript-64-instrumented  (recover, 64 sections, 100x30, extra rows 0, native load False; n=base 6, fix 6)

| metric | base | fix |
|---|---:|---:|
| launch_to_list_ms | 9.0 (8.6–10.8) | 9.0 (8.8–10.8) |
| ctrl_r_to_ready_row_ms | 37.0 (35.9–37.6) | 35.0 (31.9–36.2) |
| enter_to_first_content_ms | 14.1 (11.6–18.2) | 16.1 (11.2–19.0) |
| draft_ms | 13.1 (9.8–24.7) | 18.0 (13.2–26.1) |
| enter_to_latest_reply_ms | 3,031 (3,021–3,110) | 58.1 (49.1–824.2) |
| enter_to_final_tool_ms | 3,036 (3,025–3,113) | 58.1 (49.1–824.2) |
| enter_to_new_answer_ms | 6,152 (6,123–6,262) | 285 (241–1,047) |
| settled_key_ms | 22.1 (20.4–24.2) | 14.0 (12.9–17.1) |
| page_up_first_ms | 7.0 (6.5–8.1) | 3.0 (2.9–6.3) |
| page_up_ms_median | 6.8 (6.5–7.3) | 6.6 (6.5–7.1) |
| page_up_ms_max | 11.0 (10.6–11.7) | 8.7 (7.7–10.1) |
| page_up_ms_last5_median | 7.1 (6.5–7.8) | 6.6 (6.5–7.7) |
| frames_to_catchup | 1,412 (1,412–1,412) | 51.0 (50.0–60.0) |
| bytes_to_catchup | 1,656,952 (1,656,943–1,656,966) | 73,029 (70,920–79,712) |

- base: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True, answered by replacement True, samples with a disconnect 0/6, samples with a disconnect row left in scrollback 0; scrollback sha ['ffacf89eff20']; final screen sha ['a28825492a0d']
- fix: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True, answered by replacement True, samples with a disconnect 1/6, samples with a disconnect row left in scrollback 1; scrollback sha ['daddff4c6246', 'ffacf89eff20']; final screen sha ['a28825492a0d']

### recover-native-64  (recover, 64 sections, 100x30, extra rows 0, native load True; n=base 3, fix 3)

| metric | base | fix |
|---|---:|---:|
| launch_to_list_ms | 8.9 (8.8–10.0) | 9.9 (9.8–10.0) |
| ctrl_r_to_ready_row_ms | 55.0 (52.8–55.5) | 52.4 (51.6–53.6) |
| enter_to_first_content_ms | 11.9 (11.3–14.7) | 20.5 (8.7–20.6) |
| draft_ms | 10.5 (8.7–14.5) | 22.3 (17.9–23.9) |
| enter_to_latest_reply_ms | 3,032 (3,029–3,038) | 54.5 (51.4–782.5) |
| enter_to_final_tool_ms | 3,035 (3,032–3,044) | 54.5 (51.4–782.5) |
| enter_to_new_answer_ms | 6,149 (6,133–6,173) | 281 (276–1,007) |
| settled_key_ms | 21.6 (21.3–23.6) | 13.2 (13.2–14.2) |
| page_up_first_ms | 6.9 (6.5–6.9) | 3.1 (2.9–3.2) |
| page_up_ms_median | 6.7 (6.7–6.9) | 6.5 (6.5–6.5) |
| page_up_ms_max | 10.5 (9.8–10.8) | 8.6 (8.4–8.6) |
| page_up_ms_last5_median | 6.8 (6.4–7.5) | 6.7 (6.6–7.1) |
| frames_to_catchup | 1,412 (1,412–1,413) | 50.0 (50.0–57.0) |
| bytes_to_catchup | 1,656,969 (1,656,960–1,657,558) | 70,229 (70,199–79,069) |

- base: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True, answered by replacement True; scrollback sha ['7c83e4265edc']; final screen sha ['721269a943da']
- fix: scrollback ordered exactly once False (duplicates 0), reached start True, draft intact True, answered by replacement True; scrollback sha ['7c83e4265edc', 'ca06797c22d4']; final screen sha ['721269a943da']

### recover-native-64-instrumented  (recover, 64 sections, 100x30, extra rows 0, native load True; n=base 6, fix 6)

| metric | base | fix |
|---|---:|---:|
| launch_to_list_ms | 10.0 (9.1–11.7) | 9.5 (9.0–9.8) |
| ctrl_r_to_ready_row_ms | 55.8 (51.8–58.6) | 54.4 (48.3–57.8) |
| enter_to_first_content_ms | 15.2 (10.7–16.6) | 14.9 (11.1–19.3) |
| draft_ms | 12.3 (9.2–24.2) | 19.6 (15.6–23.6) |
| enter_to_latest_reply_ms | 3,045 (3,025–3,053) | 53.4 (52.1–818.5) |
| enter_to_final_tool_ms | 3,048 (3,034–3,057) | 53.4 (52.2–818.5) |
| enter_to_new_answer_ms | 6,178 (6,119–6,218) | 280 (256–1,046) |
| settled_key_ms | 22.5 (20.9–24.8) | 14.5 (13.7–15.6) |
| page_up_first_ms | 7.1 (6.5–10.4) | 3.2 (2.9–4.5) |
| page_up_ms_median | 6.9 (6.7–7.2) | 6.6 (6.3–6.9) |
| page_up_ms_max | 11.6 (11.1–13.3) | 8.7 (8.5–9.2) |
| page_up_ms_last5_median | 7.4 (6.6–8.0) | 6.9 (6.3–7.6) |
| frames_to_catchup | 1,412 (1,412–1,413) | 52.0 (50.0–56.0) |
| bytes_to_catchup | 1,656,966 (1,656,944–1,657,563) | 74,680 (71,176–76,566) |

- base: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True, answered by replacement True, samples with a disconnect 0/6, samples with a disconnect row left in scrollback 0; scrollback sha ['f792e1f25a3f']; final screen sha ['ca3a4dc4d626']
- fix: scrollback ordered exactly once False (duplicates 0), reached start True, draft intact True, answered by replacement True, samples with a disconnect 1/6, samples with a disconnect row left in scrollback 1; scrollback sha ['248331efbe2b', 'f792e1f25a3f']; final screen sha ['ca3a4dc4d626']

### open-64-instrumented  (open, 64 sections, 100x30, extra rows 0, native load False; n=base 3, fix 3)

| metric | base | fix |
|---|---:|---:|
| launch_to_list_ms | 8.6 (8.3–9.7) | 9.5 (9.5–9.6) |
| enter_to_first_content_ms | 9.9 (9.5–11.8) | 9.0 (8.6–14.3) |
| draft_ms | 12.6 (9.5–15.8) | 15.3 (15.3–17.7) |
| enter_to_latest_reply_ms | 3,029 (3,023–3,036) | 27.7 (26.1–30.2) |
| enter_to_final_tool_ms | 3,033 (3,027–3,039) | 27.7 (26.1–30.2) |
| enter_to_live_reply_ms | 3,062 (3,054–3,068) | 56.3 (30.2–59.0) |
| key_during_stream_ms | 25.6 (23.8–27.2) | 13.8 (13.1–14.6) |
| settled_key_ms | 22.1 (21.4–23.7) | 14.5 (14.2–15.1) |
| page_up_first_ms | 6.4 (6.3–7.2) | 3.1 (3.0–3.7) |
| page_up_ms_median | 6.7 (6.5–7.0) | 6.5 (6.4–6.5) |
| page_up_ms_max | 10.3 (9.9–10.7) | 8.6 (8.6–8.8) |
| page_up_ms_last5_median | 6.7 (6.2–7.2) | 6.5 (6.3–6.6) |
| frames_to_catchup | 1,419 (1,419–1,419) | 29.0 (29.0–31.0) |
| bytes_to_catchup | 1,661,585 (1,661,583–1,661,608) | 39,906 (38,848–40,880) |

- base: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True, samples with a disconnect 0/3, samples with a disconnect row left in scrollback 0; scrollback sha ['be5a43f29834']; final screen sha ['55cb79519d6a']
- fix: scrollback ordered exactly once True (duplicates 0), reached start True, draft intact True, samples with a disconnect 0/3, samples with a disconnect row left in scrollback 0; scrollback sha ['be5a43f29834']; final screen sha ['55cb79519d6a']

