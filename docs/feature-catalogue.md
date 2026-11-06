# Feature catalogue: candidate CLI features

Candidate features for Nori CLI, for humans to pick from and prioritize.
Each entry has a one-sentence description of what a Nori user would get, one reference link to an open source implementation, and media when available.
Size is a rough implementation hint for Nori's Ratatui TUI: S (days), M (a week or two), L (multi-week or needs new architecture).
Features Nori already ships are left out; "Partially in Nori" names what exists today.
Entries are grouped by theme; keys and command names are illustrative, not a final keymap.

## Input and composer

| Feature | Size | What you get | Link | Media |
| --- | --- | --- | --- | --- |
| Steering vs follow-up queue | M | While the agent works, `Enter` queues a steering message for its next step and `Alt+Enter` queues a follow-up for after the task. Partially in Nori: messages queue while a turn runs; there is no steer/follow-up split. | [reference](https://github.com/earendil-works/pi/blob/96667f342a8da98ec7969caee7f920ace104436f/packages/coding-agent/docs/usage.md#change-direction) | — |
| Presets | M | `/presets` gives ten numbered slots that each replay a saved prompt, slash command, or model/effort combination through the normal submit queue. | [reference](https://github.com/openai/codex/pull/54380) | — |
| Leader key and which-key overlay | M | `Ctrl+X` starts two-key chords (`n` new, `l` sessions, `m` models, `t` themes), and an optional popup lists the keys available next. Partially in Nori: single hotkeys are rebindable in `/settings`; there are no chords. | [reference](https://github.com/anomalyco/opencode/blob/05620f8ff9938ed3af3308cc40bb6cbac92b0f09/packages/web/src/content/docs/keybinds.mdx#L188-L198) | — |
| Prompt stash | S | Stash a half-written prompt, send something else, then pop it back or pick it from a stash list. | [reference](https://github.com/anomalyco/opencode/pull/6021) | [video](https://github.com/user-attachments/assets/8e7b637a-bbf2-4392-b9d2-51d1f743f7f1) |
| Markdown highlighting in the composer | M | The prompt editor highlights Markdown as you type, including unclosed code fences, with mouse selection and cut/copy/paste. | [reference](https://github.com/batrachianai/toad/blob/dd4f90e8b3700c3de80ad4b0eaa488ad0105e2c1/src/toad/widgets/highlighted_textarea.py) | [GIF](https://github.com/user-attachments/assets/3d619b94-ec53-4e7a-b905-5aef6f4fa8a6) |
| Fuzzy `@` picker with tree toggle | M | `@` opens a gitignore-aware fuzzy file search, and `Tab` flips it into a browsable directory tree overlay. Partially in Nori: `@` fuzzy file search, with `/browse` as a separate file manager. | [reference](https://github.com/batrachianai/toad/pull/148) | [GIF](https://github.com/user-attachments/assets/ab25c389-1d2f-4006-a1d8-159edbd3ed00) |
| More `@`-mention types | M | `@` can also attach folders, symbols, past threads, diagnostics, branch diffs, and URLs, not just files. Partially in Nori: `@` files and `$` skills/agent commands. | [reference](https://github.com/zed-industries/zed/blob/1478fb5e60f8b7852a2b0da32aa3da5cd9bb6954/docs/src/ai/agent-panel.md) | — |
| Persistent PTY shell | L | `!` commands run in one real shell that keeps `cd` and env vars, shows full color, and can run interactive apps like `htop`. Partially in Nori: `!` runs one-off user shell commands. | [reference](https://github.com/batrachianai/toad/blob/dd4f90e8b3700c3de80ad4b0eaa488ad0105e2c1/src/toad/widgets/shell_terminal.py) | [GIF](https://github.com/user-attachments/assets/ac9247bb-3daa-4bb7-b3fd-e0bbd22475fa) |

## Output rendering

| Feature | Size | What you get | Link | Media |
| --- | --- | --- | --- | --- |
| Mermaid diagrams in the terminal | L | A finished `mermaid` block renders as a colored text diagram, falling back to source when it is invalid or too large. | [reference](https://github.com/openai/codex/pull/46054) | — |
| Raw scrollback mode | M | A toggle switches output to unwrapped text, so copying from the terminal gives whole paragraphs and unbroken commands. | [reference](https://github.com/openai/codex/pull/20819) | [video](https://github.com/user-attachments/assets/0ef85c84-9363-4aad-b43a-15fce062a443) |
| Inline images | M | Images render inline on terminals that support inline graphics and crop correctly while scrolling, with a text placeholder elsewhere. | [reference](https://github.com/earendil-works/pi/blob/96667f342a8da98ec7969caee7f920ace104436f/packages/tui/README.md#image) | — |
| Side-by-side diffs | M | Edit diffs show unified, split, or auto (split when wide) with syntax and character-level highlights. | [reference](https://github.com/batrachianai/toad/pull/285) | [screenshot](https://github.com/user-attachments/assets/b3d6c29c-d6ec-4253-a9dc-2df0ff21e293) |
| Review agent edits per hunk | L | After a turn, one review view lists every edit, and you keep or reject each hunk or all of them at once. Partially in Nori: `/diff` shows the git diff and `/undo` asks the agent to undo a turn. | [reference](https://github.com/zed-industries/zed/blob/1478fb5e60f8b7852a2b0da32aa3da5cd9bb6954/crates/agent_ui/src/agent_diff.rs) | [screenshot](https://images.zed.dev/blog/fastest-ai-code-editor/blog-review.webp) |
| Named themes with a live picker | M | Pick from bundled or user-defined themes with a live preview of each one. | [reference](https://github.com/anomalyco/opencode/blob/05620f8ff9938ed3af3308cc40bb6cbac92b0f09/packages/tui/src/component/dialog-theme-list.tsx) | — |
| Theme from the terminal palette | M | The default theme is built from the terminal's own 16-color palette, keeps body text at 4.5:1 contrast or better, and rebuilds when the terminal flips light/dark. Partially in Nori: it reads the terminal's foreground and background to pick light or dark diff and shimmer colors. | [reference](https://github.com/earendil-works/pi/blob/96667f342a8da98ec7969caee7f920ace104436f/packages/coding-agent/docs/themes.md#use-your-terminals-colors) | — |
| Block cursor through the conversation | M | `Alt+Up/Down` steps through past output block by block, so you can collapse a block or copy it to the clipboard or prompt. | [reference](https://github.com/batrachianai/toad/blob/dd4f90e8b3700c3de80ad4b0eaa488ad0105e2c1/src/toad/widgets/conversation.py#L1745-L1760) | — |

## Session and history

| Feature | Size | What you get | Link | Media |
| --- | --- | --- | --- | --- |
| Session tree | L | `/tree` draws every branch of the session as a navigable tree that you can filter, fold, and label, and lets you branch from any node. Partially in Nori: `/fork` and Esc-Esc rewind, with no tree view. | [reference](https://github.com/earendil-works/pi/blob/96667f342a8da98ec7969caee7f920ace104436f/packages/coding-agent/docs/sessions.md#choose-how-to-branch) | [screenshot](https://raw.githubusercontent.com/earendil-works/pi/96667f342a8da98ec7969caee7f920ace104436f/packages/coding-agent/docs/images/tree-view.png) |
| Export to HTML and share | M | `/export` saves the session as themed HTML or JSONL, and `/share` uploads it and returns a viewer link. | [reference](https://github.com/earendil-works/pi/blob/96667f342a8da98ec7969caee7f920ace104436f/packages/coding-agent/docs/sessions.md#export-or-share-a-session) | — |
| Copy last response | S | `/copy` copies the last assistant response to the clipboard. | [reference](https://github.com/earendil-works/pi/blob/96667f342a8da98ec7969caee7f920ace104436f/packages/coding-agent/docs/usage.md#copy-export-or-share-results) | — |
| Session tabs | L | Independent sessions stay open as full-screen tabs that you switch with `Ctrl+Tab`, and browsing old sessions reuses one preview tab. | [reference](https://github.com/anomalyco/opencode/pull/46497) | [video](https://github.com/user-attachments/assets/7e3dd5f9-f127-43f7-b94c-3f4f812984d0) |
| Side conversation panes | L | `/side` splits the screen to open a side chat next to the main one, with its own draft and transcript, in panes you can resize and zoom. | [reference](https://github.com/openai/codex/pull/58992) | — |
| Timeline jump | M | A hotkey opens a flat list of the current branch's messages so you can scroll the transcript straight to one, or revert or fork from it. Partially in Nori: Esc-Esc rewind and `/fork`. | [reference](https://github.com/anomalyco/opencode/blob/05620f8ff9938ed3af3308cc40bb6cbac92b0f09/packages/tui/src/routes/session/dialog-timeline.tsx) | — |
| In-thread search | S | Search the current conversation and step through matches with next/previous. | [reference](https://github.com/zed-industries/zed/pull/57231) | [screenshot](https://github.com/user-attachments/assets/8971e432-61d3-46db-a6a4-2bbd355089e7) |
| Import sessions from other agents | M | Sessions started in other coding agents can be pulled into one cross-agent history and resumed, and re-importing skips duplicates. | [reference](https://github.com/zed-industries/zed/blob/1478fb5e60f8b7852a2b0da32aa3da5cd9bb6954/crates/agent_ui/src/thread_import.rs) | — |
| Side questions with `/btw` | M | `/btw <question>` asks a quick question using the session's context and shows the answer in a dialog without adding it to the conversation. | [reference](https://github.com/anomalyco/opencode/pull/49646) | — |

## Agent and model control

| Feature | Size | What you get | Link | Media |
| --- | --- | --- | --- | --- |
| Agents dashboard | L | A searchable dashboard lists your sessions and their subagent trees, with status, unread results, and pinning. | [reference](https://github.com/openai/codex/pull/39094) | — |
| Concurrent agents across providers | L | Agents from different providers run side by side in one app, and `Ctrl+S` shows each one's state. | [reference](https://github.com/batrachianai/toad/pull/226) | [screenshot](https://github.com/user-attachments/assets/cd3128eb-b348-4f40-b5c0-4816d07dcbe9) |
| Recent and favorite model cycling | S | `F2` cycles recently used models and `Ctrl+F` favorites one, so you can switch models without opening a picker. Partially in Nori: `/model` picker. | [reference](https://github.com/anomalyco/opencode/blob/05620f8ff9938ed3af3308cc40bb6cbac92b0f09/packages/web/src/content/docs/keybinds.mdx#L55-L67) | — |
| Thinking-level cycling with border color | S | A single key cycles the thinking level, and the editor border color always shows the current level. Partially in Nori: reasoning effort is set from `/model`. | [reference](https://github.com/earendil-works/pi/blob/96667f342a8da98ec7969caee7f920ace104436f/packages/coding-agent/docs/keybindings.md#models-and-thinking) | — |
| Extensions that add UI | L | Extensions can add dialogs, widgets, status items, overlays, and custom tool renderers, and `/reload` picks up changes live. | [reference](https://github.com/earendil-works/pi/blob/96667f342a8da98ec7969caee7f920ace104436f/packages/coding-agent/docs/extensions.md#custom-ui) | [screenshot](https://raw.githubusercontent.com/earendil-works/pi/96667f342a8da98ec7969caee7f920ace104436f/packages/coding-agent/docs/images/doom-extension.png) |
| Pattern-based tool permission rules | M | You approve a tool once, always, or always for a pattern like `^cargo test`, and regex allow/deny/confirm rules then decide future calls. Partially in Nori: ACP "allow always" options, with no user-editable pattern rules. | [reference](https://github.com/zed-industries/zed/blob/1478fb5e60f8b7852a2b0da32aa3da5cd9bb6954/docs/src/ai/tool-permissions.md) | [screenshot](https://images.zed.dev/blog/fastest-ai-code-editor/blog-3-confirmation.webp) |
| Dangerous-command highlighting | M | Permission prompts highlight commands that may change files outside the project. Partially in Nori: `/approvals` sets what runs without asking, but prompts do not flag risky commands. | [reference](https://github.com/batrachianai/toad/pull/68) | — |

## Status and context visibility

| Feature | Size | What you get | Link | Media |
| --- | --- | --- | --- | --- |
| Warnings footer and viewer | S | The footer counts deduplicated warnings, and `/warnings` opens a scrollable list of startup, MCP, and runtime messages kept out of the transcript. | [reference](https://github.com/openai/codex/pull/46751) | — |
| PR status in the footer | S | The status line can show the current branch's pull request number and its diff against the base branch. Partially in Nori: git branch and `+/-` stats footer segments. | [reference](https://github.com/openai/codex/pull/20892) | [screenshot](https://github.com/user-attachments/assets/10b4380b-c3e9-4729-9ee1-3f742068fa47) |

## ACP client capabilities

| Feature | Size | What you get | Link | Media |
| --- | --- | --- | --- | --- |
| Subagent session navigation | L | A hotkey opens a subagent's own transcript, Left/Right moves between siblings, and Up returns to the parent. Partially in Nori: `/context` lists observed subagents. | [reference](https://github.com/anomalyco/opencode/blob/05620f8ff9938ed3af3308cc40bb6cbac92b0f09/packages/web/src/content/docs/keybinds.mdx#L48-L51) | — |
| Elicitation forms | M | When the agent needs a decision, it shows a keyboard-driven form with choices and fields instead of waiting for a free-text reply. | [reference](https://github.com/zed-industries/zed/pull/61497) | [video](https://github.com/user-attachments/assets/9d438d87-af65-4fa2-b500-b1232007d0a0) |

## Onboarding and setup

| Feature | Size | What you get | Link | Media |
| --- | --- | --- | --- | --- |
| ACP Registry install | M | Browse the ACP Registry and install an agent in one step, with checksum check, license display, and an option to make it the default. Partially in Nori: `/agent` switches between locally configured agents. | [reference](https://github.com/zed-industries/zed/pull/47218) | [screenshot](https://images.zed.dev/blog/acp-registry/in-zed.webp) |
| Searchable settings panel | M | A tabbed, searchable settings panel saves to config, applies changes live, and flags settings that need a restart or are overridden. Partially in Nori: a searchable `/settings` picker without tabs, live apply, or restart flags. | [reference](https://github.com/openai/codex/pull/52990) | — |
