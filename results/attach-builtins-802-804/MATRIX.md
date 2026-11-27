| Check | main-214d79b0 | 802-c352fb74 | 803-61789f95-superseded | 803-211a8f87 | 803-8c9ab9ae | 804-e6690ffc-superseded | 804-ef806945 | 804-7c469a25 | What |
|---|---|---|---|---|---|---|---|---|---|
| A2a | pass | pass | pass | pass | pass | pass | pass | pass | slash menu lists /agent once |
| A6a | **FAIL** | pass | pass | pass | pass | pass | pass | pass | Cedar row shows the host's reason |
| A6b | **FAIL** | pass | pass | pass | pass | pass | pass | pass | Enter on Cedar explains the refusal |
| A6c | **FAIL** | pass | pass | pass | pass | pass | pass | pass | picker stays open after the refusal |
| A6d | pass | pass | pass | pass | pass | pass | pass | pass | refusal starts no session |
| A1a | **FAIL** | pass | pass | pass | pass | pass | pass | pass | empty host: /agents started a Lark session on the host |
| A1b | **FAIL** | pass | pass | pass | pass | pass | pass | pass | Lark answers in the new session |
| A3a | **FAIL** | pass | pass | pass | pass | pass | pass | pass | unprompted session changed agent in place under the same ID |
| A3b | **FAIL** | pass | pass | pass | pass | pass | pass | pass | in-place change created no extra session |
| A3c | **FAIL** | pass | pass | pass | pass | pass | pass | pass | the next prompt reaches Lark, not Kestrel |
| A3d | **FAIL** | **FAIL** | **FAIL** | **FAIL** | **FAIL** | **FAIL** | **FAIL** | pass | controller footer names the new agent after the in-place change |
| A3e | **FAIL** | **FAIL** | **FAIL** | **FAIL** | pass | **FAIL** | **FAIL** | pass | controller slash menu names the new agent |
| A5a | pass | pass | pass | pass | pass | pass | pass | pass | watcher keeps the session after the in-place change |
| A5b | **FAIL** | **FAIL** | **FAIL** | **FAIL** | **FAIL** | **FAIL** | **FAIL** | **FAIL** | watcher sees the new agent's reply |
| A5c | **FAIL** | pass | pass | pass | pass | pass | pass | pass | watcher's /agent is greyed with the take-control hint |
| A7a | pass | pass | pass | pass | pass | pass | pass | pass | a later attach opens the in-place-changed session |
| A7b | pass | **FAIL** | **FAIL** | **FAIL** | **FAIL** | **FAIL** | **FAIL** | **FAIL** | a later attach by ID opens the in-place-changed session itself |
| A4a | **FAIL** | pass | pass | pass | pass | pass | pass | pass | prompted session: notice names the new agent and Alt+S |
| A4b | pass | pass | pass | pass | pass | pass | pass | pass | previous conversation keeps running |
| A4c | **FAIL** | pass | pass | pass | pass | pass | pass | pass | new session beside |
| A4d | **FAIL** | pass | pass | pass | pass | pass | pass | pass | Alt+S back: the old conversation still answers |
| A2b | pass | pass | pass | pass | pass | pass | pass | pass | plain nori /agents opens the agent picker |
| C0a | pass | pass | pass | pass | pass | pass | pass | pass | /mention is offered in attach |
| C0b | **FAIL** | **FAIL** | **FAIL** | **FAIL** | **FAIL** | pass | pass | pass | /mention is not greyed on a current host |
| C1a | **FAIL** | **FAIL** | **FAIL** | **FAIL** | **FAIL** | pass | pass | pass | no session: host folder file is offered |
| C1b | pass | pass | pass | pass | pass | pass | pass | pass | no session: the attaching folder is never searched |
| C1c | **FAIL** | **FAIL** | **FAIL** | **FAIL** | **FAIL** | pass | pass | pass | in a session: session folder file is offered |
| C1d | pass | pass | pass | pass | pass | pass | pass | pass | in a session: client file not offered |
| C2a | **FAIL** | **FAIL** | **FAIL** | **FAIL** | **FAIL** | pass | pass | pass | Tab inserts the host path |
| C2b | **FAIL** | **FAIL** | **FAIL** | **FAIL** | **FAIL** | pass | pass | pass | agent receives the mentioned host path |
| C5a | **FAIL** | **FAIL** | **FAIL** | **FAIL** | **FAIL** | pass | pass | pass | image match goes to the agent as a host path, no image bytes |
| C3a | **FAIL** | **FAIL** | **FAIL** | **FAIL** | **FAIL** | pass | pass | pass | search follows the foreground session's folder |
| C3b | pass | pass | pass | pass | pass | pass | pass | pass | other folders are not mixed in |
| C4a | **FAIL** | **FAIL** | **FAIL** | **FAIL** | **FAIL** | pass | pass | pass | a watcher's /mention searches the watched session's host folder |
| C6a | pass | pass | pass | pass | pass | pass | pass | pass | plain nori /mention completes from its own folder |
| C6b | pass | pass | pass | pass | pass | pass | pass | pass | plain nori inserts the same way |
| R1a | pass ×2 | pass ×2 | pass ×2 | pass ×4 | pass ×4 | pass ×2 | pass ×4 | pass ×4 | /config hides the native mode option |
| R1b | 1/2 | 1/2 | pass ×2 | pass ×4 | pass ×4 | pass ×2 | pass ×4 | pass ×4 | agent confirms the native mode change |
| R1d | **FAIL** ×2 | **FAIL** ×2 | **FAIL** ×2 | pass ×4 | pass ×4 | **FAIL** ×2 | pass ×4 | pass ×4 | controller's mode badge does not show the replaced native mode |
| R1e | 1/2 | 1/2 | **FAIL** ×2 | **FAIL** ×4 | **FAIL** ×4 | **FAIL** ×2 | **FAIL** ×4 | **FAIL** ×4 | footer shows the approval mode once |
| R1c | 1/2 | 1/2 | pass ×2 | pass ×4 | pass ×4 | pass ×2 | pass ×4 | pass ×4 | later attach shows Ask as current |
| R1f | **FAIL** ×2 | **FAIL** ×2 | **FAIL** ×2 | pass ×4 | pass ×4 | **FAIL** ×2 | pass ×4 | pass ×4 | later attach's mode badge does not show the replaced native mode |
| R2a | **FAIL** ×2 | **FAIL** ×2 | pass ×2 | pass ×4 | pass ×4 | pass ×2 | pass ×4 | pass ×4 | same-ID replacement by Moss |
| R2b | pass ×2 | pass ×2 | pass ×2 | pass ×4 | pass ×4 | pass ×2 | pass ×4 | pass ×4 | controller no longer offers the previous agent's model |
| R2d | **FAIL** ×2 | **FAIL** ×2 | **FAIL** ×2 | **FAIL** ×4 | **FAIL** ×4 | **FAIL** ×2 | **FAIL** ×4 | **FAIL** ×4 | later attach by ID opens the replaced session itself |
| R2e | **FAIL** | **FAIL** | pass | pass ×3 | pass ×3 | pass | pass ×3 | pass ×3 | list opens the replaced session |
| R2f | pass | pass | **FAIL** | pass ×3 | pass ×3 | **FAIL** | pass ×3 | pass ×3 | replaced session |
| R2c | pass ×2 | pass ×2 | **FAIL** ×2 | **FAIL** ×4 | **FAIL** ×4 | **FAIL** ×2 | **FAIL** ×4 | **FAIL** ×4 | later attach does not offer the replaced agent's models |
| B1a | **FAIL** | **FAIL** | pass | pass | pass | pass | pass | pass | host-confirmed /model change is announced |
| B1b | **FAIL** | **FAIL** | pass | pass | pass | pass | pass | pass | next reply uses the confirmed model |
| B2a | **FAIL** | **FAIL** | pass | pass | pass | pass | pass | pass | agent rejection is shown |
| B2b | pass | pass | pass | pass | pass | pass | pass | pass | rejected value is not announced as set |
| B2c | **FAIL** | **FAIL** | pass | pass | pass | pass | pass | pass | picker still shows the confirmed model after a rejection |
| B2d | **FAIL** | **FAIL** | pass | pass | pass | pass | pass | pass | agent still runs the confirmed model |
| B5a | **FAIL** | **FAIL** | pass | pass | pass | pass | pass | pass | /config lists Model with the confirmed value |
| B5b | pass | pass | pass | pass | pass | pass | pass | pass | /config leaves out agent and permission mode |
| B5c | **FAIL** | **FAIL** | pass | pass | pass | pass | pass | pass | effort change confirmed |
| B5d | **FAIL** | **FAIL** | pass | pass | pass | pass | pass | pass | next reply uses effort high |
| B4a | **FAIL** | **FAIL** | pass | pass | pass | pass | pass | pass | a later attach |
| B3a | pass | pass | pass | pass | pass | pass | pass | pass | delayed answer does not land in the other session |
| B3b | pass | pass | pass | pass | pass | pass | pass | pass | other session's config untouched |
| B3c | **FAIL** | **FAIL** | pass | pass | pass | pass | pass | pass | delayed answer is shown in the session it was made in |
| B3d | **FAIL** | **FAIL** | pass | pass | pass | pass | pass | pass | agent confirms the delayed change |
| B3e | **FAIL** | **FAIL** | pass | pass | pass | pass | pass | pass | terminal attached while the answer was pending sees it |
| B6a | **FAIL** | **FAIL** | pass | pass | pass | pass | pass | pass | watcher's /model is greyed |
| B6b | **FAIL** | **FAIL** | pass | pass | pass | pass | pass | pass | watcher's /config is greyed |
| B7a | pass | pass | pass | pass | pass | pass | pass | pass | plain nori /model lists the advertised models |
| M1a |  | pass |  | pass | pass ×3 |  |  | pass ×3 | in-place change to Osprey announced |
| M1b |  | pass |  | **FAIL** | pass ×3 |  |  | pass ×3 | controller shows Osprey's own native mode |
| M1c |  | **FAIL** |  | **FAIL** | 1/3 |  |  | **FAIL** ×3 | controller hint row names the new agent after 12 s |
| M2a |  | pass |  | pass | pass ×3 |  |  | pass ×3 | next prompt goes to Osprey in its own mode |
| M2b |  | pass |  | pass | pass ×3 |  |  | pass ×3 | next prompt does not reach Kestrel |
| M2c |  | **FAIL** |  | **FAIL** | 1/3 |  |  | **FAIL** ×3 | hint row names the new agent after the first prompt |
| M3a |  | **FAIL** |  | pass | pass ×3 |  |  | pass ×3 | controller badge follows the host receipt |
| M3b |  | pass |  | pass | pass ×3 |  |  | pass ×3 | agent confirms read-only |
| M4a |  | **FAIL** |  | pass | pass ×3 |  |  | pass ×3 | fresh attach |
| M4b |  | pass |  | pass | pass ×3 |  |  | pass ×3 | fresh attach hint row names the new agent |
| M5a |  | **FAIL** |  | pass | pass ×3 |  |  | pass ×3 | fresh attach by ID shows the new agent's current native mode |
| M5b |  | pass |  | pass | pass ×3 |  |  | pass ×3 | fresh attach by ID opens that session |
| M6a |  | pass |  | pass | pass ×3 |  |  | pass ×3 | badge clears when the replacement has no native mode |
| W0 |  | pass |  |  | pass |  |  | pass | watcher opens the Kestrel session |
| W1 |  | **FAIL** |  |  | **FAIL** |  |  | **FAIL** | watcher stays on the changed session and sees its new turns |
| W2 |  | **FAIL** |  |  | **FAIL** |  |  | **FAIL** | watcher is not moved to the other Lark session |
