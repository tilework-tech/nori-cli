# Live and renderer-specific TUI capture

## Renderer boundary

`libghostty-vt` parses terminal sequences, maintains terminal state, and
supports semantic snapshots. It does not define canonical headless RGBA pixels.
`agent-tty` therefore uses `ghostty-web` through Playwright for PNG and WebM
artifacts. The upstream tools describe this as a reference visual renderer,
not a pixel guarantee for a native Ghostty window.

Primary references:

- <https://github.com/coder/agent-tty#how-it-works>
- <https://github.com/coder/agent-tty/blob/main/docs/USAGE.md>
- <https://github.com/ghostty-org/ghostty/discussions/12610>

## Keep one session alive

Use raw `agent-tty` commands when input and screenshots must be refreshed
several times without restarting the TUI. Use an isolated home and the same
home for every command.

```bash
ATTY=(npx -y -p node@24 -p agent-tty@0.5.0 agent-tty)
CAPTURE_HOME=$(mktemp -d /tmp/capture-tui-live.XXXXXX)
"${ATTY[@]}" --home "$CAPTURE_HOME" doctor --json

SESSION_ID=$("${ATTY[@]}" --home "$CAPTURE_HOME" create \
  --cols 120 --rows 36 --cwd "$PWD" --json -- /bin/bash \
  | jq -r '.result.sessionId')
```

If `doctor` reports a missing browser cache, install the renderer-matched build
once with `npx -y -p playwright@1.60.0 playwright install chromium`, then rerun
`doctor` before creating the session.

Drive the session with `batch` so each wait is anchored after its preceding
input. Prefix the first run command with:

```bash
unset NO_COLOR; export COLORTERM=truecolor
```

Inspect before capturing:

```bash
"${ATTY[@]}" --home "$CAPTURE_HOME" snapshot "$SESSION_ID" \
  --format text --json
"${ATTY[@]}" --home "$CAPTURE_HOME" screenshot "$SESSION_ID" \
  --profile reference-dark --hide-cursor --json
```

After a code change, stop or exit the program inside the same session, rerun
the production command, wait for the target state, and capture again. Do not
reuse an old screenshot after a failed rebuild.

Always stop the session and remove its isolated home:

```bash
"${ATTY[@]}" --home "$CAPTURE_HOME" destroy "$SESSION_ID" --json
case "$CAPTURE_HOME" in
  /tmp/capture-tui-live.*) rm -rf -- "$CAPTURE_HOME" ;;
esac
```

## When native capture is required

Add a separate native-terminal capture when investigating font fallback,
ligatures, GPU composition, DPI scaling, window decorations, native clipboard
behavior, or a bug reproduced only by a named emulator. Record the emulator,
version, font, scale, theme, and viewport. Keep the Ghostty-backed reference
artifact because it remains deterministic and replayable.
