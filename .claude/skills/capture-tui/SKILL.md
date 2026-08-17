---
name: capture-tui
description: Run, drive, inspect, and refresh real terminal UI and storybook sessions through isolated PTYs, libghostty-vt semantic state, and ghostty-web PNG rendering. Use when Codex needs to test a TUI or interactive CLI, renew a storybook screenshot, attach terminal visual evidence to a PR, compare layouts at fixed terminal sizes, or replace a synthetic terminal mockup with reproducible rendered output.
---

# Capture TUI

Produce reviewable evidence from the application running in a real PTY. Treat
the text snapshot and PNG as two views of the same recorded terminal state.

## Capture contract

- Run the production command rather than reconstructing its output.
- Set explicit columns and rows; record both in the result.
- Wait for observable text or stable screen state instead of sleeping.
- Clear inherited `NO_COLOR` only inside the capture shell and advertise
  true-color support there.
- Use `libghostty-vt` for semantic screen inspection when available and
  `ghostty-web` for reference PNG rendering.
- Describe the PNG as Ghostty-backed reference rendering, not pixel-identical
  native Ghostty output.
- Destroy every PTY session, including failed runs.

## One-shot or refreshed capture

Create an agent-tty batch file. A batch should launch the TUI without waiting,
wait for its initial screen, send the minimum input needed to reach the target,
and wait for a label unique to that target.

Example for the Nori component storybook:

```json
[
  {
    "run": "cargo run --manifest-path tui-components/Cargo.toml --example nori_storybook",
    "noWait": true
  },
  {
    "wait": {
      "text": "Nori component storybook",
      "timeout": 30000
    }
  },
  {
    "sendKeys": ["5"]
  },
  {
    "wait": {
      "text": "Handroll-style bottom panel",
      "timeout": 10000
    }
  }
]
```

Run the bundled script from the repository root or pass an explicit working
directory:

```bash
SKILL_DIR=.claude/skills/capture-tui
"$SKILL_DIR/scripts/capture-tui.sh" \
  --cwd nori-rs \
  --steps /tmp/nori-storybook-steps.json \
  --out /tmp/nori-storybook.png \
  --snapshot-out /tmp/nori-storybook.txt \
  --metadata-out /tmp/nori-storybook.capture.json \
  --cols 120 \
  --rows 36
```

The first run may install the pinned Playwright Chromium build. The script
prints a JSON result containing the semantic backend, visual backend, terminal
dimensions, screen hash, PNG SHA-256, and output path. Re-run it with the same
`--out` path to atomically refresh the view after code changes.

## Review the result

1. Read the text snapshot and confirm the expected target state is present.
2. Inspect the PNG with the available image-viewing tool.
3. Check truncation, wrapping, colors, focus, selection, and terminal edges.
4. Repeat at a narrow size when responsive behavior is in scope.
5. Include the image inline when reporting visual evidence.

Do not commit generated screenshots unless the user requests repository-owned
fixtures or documentation. Uploading an image or posting a PR comment remains
an external side effect and requires the user's authorization.

## Iterative sessions and native-terminal bugs

Use the one-shot script for normal refreshes and PR evidence. Read
[references/live-sessions.md](references/live-sessions.md) when a single TUI
must remain alive across several input/capture cycles or when diagnosing a
renderer-specific difference.

If a bug depends on native Ghostty font shaping, Metal rendering, DPI, window
chrome, or another native surface detail, keep this reference artifact and add
a separate native-window capture. Do not present either artifact as the other.
