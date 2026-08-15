---
name: customizing-bottom-pane
description: Use when a user wants to customize Nori CLI's status footer or textarea corner indicators in $NORI_HOME/config.toml, including built-in segments, context metrics, placement, visibility, or custom format chunks.
---

<required>
*CRITICAL* Add these steps to your Todo list:

1. Resolve and inspect the active Nori config file
2. Confirm the desired information and placement
3. Check the current segment source when repository access is available
4. Edit only the relevant config.toml sections
5. Validate the TOML and explain the resulting layout
</required>

# Customize the Bottom Pane

Use Nori's layout entries to place built-in status segments or compose several
built-ins into one styled chunk. The bottom pane currently exposes the status
footer and four corners around the prompt textarea.

## Locate the active config

Use `$NORI_HOME/config.toml` when `NORI_HOME` is set. Otherwise Nori uses
`~/.nori/cli/config.toml`.

Read the existing file before editing it. Preserve unrelated settings and merge
with existing `[tui.footer_segments]` and `[tui.footer_layout]` tables instead of
creating duplicate TOML tables.

## Placement

`[tui.footer_layout]` supports six arrays:

- `footer_left`
- `footer_right`
- `textarea_top_left`
- `textarea_top_right`
- `textarea_bottom_left`
- `textarea_bottom_right`

Each entry can be a built-in segment name:

```toml
[tui.footer_layout]
footer_left = ["git_branch", "context", "approval_mode"]
footer_right = ["mode_indicator"]
```

Or an inline custom chunk:

```toml
[tui.footer_layout]
footer_left = [
  "git_branch",
  { format = "{context_used_percent} / {context_window_tokens}" },
  "approval_mode",
]
textarea_top_right = [
  { format = "{context_remaining_percent} remaining" },
]
```

A placement field replaces that placement's default array. A directly listed
built-in is also moved out of its other default placement. A custom chunk is a
distinct item; if it references a built-in that is still directly placed
elsewhere, both can render.

## Available built-ins

The current built-in names are:

- `prompt_summary`
- `session_title`
- `vim_mode`
- `git_branch`
- `worktree_name`
- `git_stats`
- `context`
- `context_used_percent`
- `context_remaining_percent`
- `context_used_tokens`
- `context_remaining_tokens`
- `context_window_tokens`
- `approval_mode`
- `skillset`
- `nori_version`
- `token_usage`
- `mode_indicator`
- `cloud_session`

`context` is the default context indicator and renders like `44% / 272k` when
both usage and window size are known. The five `context_*` primitives exist for
standalone placement and custom composition. `session_title` renders the title
the agent reports over ACP session-info updates (`Title: Fix login flakes`) and
self-hides for agents that never send one.

Treat the source as authoritative when working in a Nori CLI checkout:

- `nori-rs/nori-config/src/types/mod.rs`: names, visibility defaults, layout
  defaults, and custom-format parsing
- `nori-rs/tui/src/bottom_pane/footer.rs`: rendered text, styles, and missing-data
  behavior
- `nori-rs/tui/src/bottom_pane/chat_composer/rendering.rs`: session state supplied
  to segments
- `nori-rs/tui/docs.md`: user-facing footer configuration reference

If the source list differs from this skill, follow the checked-out source and
flag the skill as stale.

## Format rules

Custom chunks use `{segment_name}` placeholders. Apply these rules:

- Placeholders may reference built-in segments only.
- Custom chunks cannot reference other custom chunks.
- Plain text and any number of placeholders may be combined.
- `{{` and `}}` render literal braces.
- Expressions, conditions, conversions, and format specifiers are unsupported.
- An unknown placeholder or unmatched brace is a configuration error.
- If any referenced segment lacks runtime data, Nori hides the entire custom
  chunk.
- Referenced built-ins keep their normal terminal styles.
- A reference renders even when that built-in's standalone visibility toggle is
  off.

Formats are validated and compiled when Nori loads the config; they are not
interpreted as arbitrary code.

## Visibility

`[tui.footer_segments]` controls directly placed built-ins:

```toml
[tui.footer_segments]
git_stats = true
token_usage = false
context = false
```

The atomic context segments default off because `context` is the concise default.
Enable an atomic segment here when listing it directly. A custom format reference
does not require enabling its referenced segment.

## Editing workflow

1. Ask what information should appear and in which of the six placements if the
   request is ambiguous.
2. Inspect the current config and source-backed segment list.
3. Make the smallest TOML edit that produces the requested layout.
4. Check that every placeholder is a built-in name and every brace is balanced.
5. Parse the resulting file with an available TOML-aware validator. In a Nori CLI
   checkout, run the relevant `nori-config` tests after source changes.
6. Tell the user that an already-running Nori session must be restarted to load
   the edited config.

Do not add a template engine, dependency, or code change merely to express a
layout that the built-in names and custom chunks already support.
