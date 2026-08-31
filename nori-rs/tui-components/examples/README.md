# Storybook E2E snapshots

The storybooks have neighboring `<example>/e2e.rs` integration tests, registered
in [`../Cargo.toml`](../Cargo.toml). They drive the built applications through
CSRessel's isolated tmux scripts and compare both styled ANSI display rows and
plain text with Insta. The shared [`support/e2e.rs`](support/e2e.rs) adapter is
test-only; it is not exported by `nori-tui-components`.

## Prerequisites

Install tmux and jq, and use a compatible checkout of
[`CSRessel/skills`](https://github.com/CSRessel/skills/tree/414aa5123b81c682f9e5372b377d262853fdfb53).
The referenced commit is the shared baseline for the tmux and Ghostty Web
skills. Keep the tmux version consistent between snapshot authors and CI:
display-row ANSI serialization can change between versions.

Set `TUI_PUPPETEERING_DIR` to the skill directory directly containing its
executables, such as `tui-start` and `tui-capture`:

```bash
export TUI_PUPPETEERING_DIR=/absolute/path/to/skills/tui-puppeteering-with-tmux
```

The tests do not download scripts, install dependencies, or need a browser.
They use unique isolated sessions, fixed terminal dimensions and environment,
and the storybooks' fixture data. The session guard cleans up on success or
failure, including snapshot assertion failures.

## Run and review snapshots

Run these commands from `nori-rs/`:

```bash
./tui-components/scripts/storybook-e2e.sh
```

The runner builds the examples first and locates their executables from Cargo's
compiler-artifact output, including target-specific paths when
`CARGO_BUILD_TARGET` is set. It uses Cargo metadata to locate capture storage
and runs the storybook tests including ignored E2E cases. Rust's test
runner can execute the independent sessions in parallel. Additional arguments
are forwarded to the test harness, for example `--nocapture`.

Ordinary `cargo test -p nori-tui-components` runs the pure capture-helper tests
but leaves the external tmux E2E cases ignored. To update expected screens,
inspect and accept the generated `.snap.new` files, then rerun:

```bash
cargo insta review
./tui-components/scripts/storybook-e2e.sh
```

The text assertion runs before ANSI, so accepting changed text may reveal an
ANSI change on the next run. Repeat review and rerun until the suite passes.

For an intentional snapshot refresh, the alternative is
`INSTA_UPDATE=always ./tui-components/scripts/storybook-e2e.sh`, followed by
reviewing the snapshot diff. Use `INSTA_UPDATE=no` when running in CI.

Each assertion captures a settled ANSI screen and derives text from that same
frame. ANSI snapshots escape ESC as `\x1b` and escape literal backslashes, so
style sequences remain reviewable and unambiguous. Each snapshot row is wrapped
in `│...│` to preserve blank rows and trailing cells without Git whitespace
errors; those bars are not part of the captured UI.
These are tmux display-row captures, not raw PTY byte streams:
only SGR styling is stripped to derive text, and unsupported control sequences
fail the capture.

Text snapshots and `screen.txt` restore trailing cells omitted by tmux: each
row is padded to the viewport width using Unicode display-cell widths, including
wide and combining characters. Blank rows are retained and overwide rows fail
the capture. This rectangular text formatting leaves raw ANSI, escaped ANSI
snapshots, replay ANSI, and PNG output unchanged.

The helper also writes unescaped `screen.ansi`, derived `screen.txt`, and
`geometry.txt` under the Cargo target directory's
`storybook-captures/run.XXXXXX/<example>/<case>/`. Each attempt gets a fresh run
directory, recorded in `storybook-captures/latest-run`, and a `passed` marker
only after its tests succeed. These are generated replay artifacts; the
committed expectations are the adjacent Insta `.snap` files.

For PNG rendering it additionally writes `replay.ansi`: the captured ANSI with
exactly its final LF row separator removed and a hide-cursor sequence prepended.
This preserves blank display rows while preventing Ghostty replay from scrolling
away the top row or adding a synthetic cursor. The raw ANSI and text used by
Insta are unaffected.

## Regenerate review PNGs

After approving the snapshots, rerun the tests so saved ANSI matches the
accepted output. Then use the compatible Ghostty Web skill installation, with
its Bun dependencies already installed and an available Chrome browser:

```bash
export TUI_CAPTURE_DIR=/absolute/path/to/skills/tui-capture-with-ghostty-web
./tui-components/scripts/render-storybook-captures.sh
```

Unlike the tmux skill, this directory contains its renderer under
`scripts/render-ansi.ts`. Follow that skill's setup instructions before running
the rendering command; the Nori runner performs no installation.

The renderer requires the latest attempt to have passed, reads only that run's
saved `replay.ansi` and geometry, and writes
`<example>/screenshots/<case>.png` without launching the storybooks. Failed
attempts cannot fall back to stale captures from older runs. Inspect and commit those PNGs
alongside the accepted snapshots. They are review aids only: there is no image
snapshot assertion, pixel comparison, or hash check. Browser and font differences
can affect PNG appearance without changing the text/ANSI test result.
