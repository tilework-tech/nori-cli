# We only care about the ACP backend

We only care about the ACP backend and the code that compiles into the nori bin. We do not care about the default codex backend or the code that compiles into the codex bin. Make sure all changes and responses are aligned to this critical fact. For example, if I ask 'add notifications to the cli', you should assume I mean 'add notification support to the ACP backend for the nori cli'.

# Rust/nori-rs

In the nori-rs folder where the rust code lives:

- Crate names are prefixed with `codex-`. For example, the `core` folder's crate is named `codex-core`
- When using format! and you can inline variables into {}, always do that.
- Install any commands the repo relies on (for example `just`, `rg`, or `cargo-insta`) if they aren't already available before running instructions here.
- Never add or modify any code related to `CODEX_SANDBOX_NETWORK_DISABLED_ENV_VAR` or `CODEX_SANDBOX_ENV_VAR`.
  - You operate in a sandbox where `CODEX_SANDBOX_NETWORK_DISABLED=1` will be set whenever you use the `shell` tool. Any existing code that uses `CODEX_SANDBOX_NETWORK_DISABLED_ENV_VAR` was authored with this fact in mind. It is often used to early exit out of tests that the author knew you would not be able to run given your sandbox limitations.
  - Similarly, when you spawn a process using Seatbelt (`/usr/bin/sandbox-exec`), `CODEX_SANDBOX=seatbelt` will be set on the child process. Integration tests that want to run Seatbelt themselves cannot be run under Seatbelt, so checks for `CODEX_SANDBOX=seatbelt` are also often used to early exit out of tests, as appropriate.
- Always collapse if statements per https://rust-lang.github.io/rust-clippy/master/index.html#collapsible_if
- Always inline format! args when possible per https://rust-lang.github.io/rust-clippy/master/index.html#uninlined_format_args
- Use method references over closures when possible per https://rust-lang.github.io/rust-clippy/master/index.html#redundant_closure_for_method_calls
- Do not use unsigned integer even if the number cannot be negative.
- When possible, make `match` statements exhaustive and avoid wildcard arms.
- Do not create small helper methods that are referenced only once.
- When making a change that adds or changes an API, ensure that the documentation in the `docs/` folder is up to date if applicable.
- Avoid large modules:
  - Prefer adding new modules instead of growing existing ones.
  - Target Rust modules under 500 LoC, excluding tests.
  - If a file exceeds roughly 800 LoC, add new functionality in a new module instead of extending the existing file unless there is a strong documented reason not to.
  - This rule applies especially to high-touch files such as `nori-rs/tui/src/app.rs`, `nori-rs/tui/src/bottom_pane/chat_composer.rs`, `nori-rs/tui/src/bottom_pane/footer.rs`, `nori-rs/tui/src/chatwidget.rs`, `nori-rs/tui/src/bottom_pane/mod.rs`, and similarly central orchestration modules.
  - When extracting code from a large module, move the related tests and module/type docs toward the new implementation so the invariants stay close to the code that owns them.
- When running Rust commands (e.g. `just fix` or `cargo test`) be patient and never try to kill them using the PID. Rust lock contention can make execution slow; this is expected.

Run `just fmt` (in `nori-rs` directory) automatically after you have finished making Rust code changes; do not ask for approval to run it. Additionally, run the tests:

1. Run the test for the specific project that was changed. For example, if changes were made in `nori-rs/tui`, run `cargo test -p nori-tui`.
2. Once those pass, if any changes were made in common, core, or protocol, run the complete test suite with `cargo test`. Avoid `--all-features` for routine local runs because it expands the build matrix and can significantly increase `target/` disk usage; use it only when you specifically need full feature coverage.
3. If any changes were made in tui, cli, or acp, run `cargo build --bin nori && cargo test -p tui-pty-e2e`. The E2E tests require the `nori` binary (from the `cli` crate), not `nori-tui`.

Before finalizing a change to `nori-rs`, run `just fix -p <project>` (in `nori-rs` directory) to fix any linter issues in the code. Prefer scoping with `-p` to avoid slow workspace‑wide Clippy builds; only run `just fix` without `-p` if you changed shared crates. Do not re-run tests after running `fix` or `fmt`.

## TUI style conventions

See `nori-rs/tui/styles.md`.

## TUI code conventions

### Styling (ratatui)

- Prefer Stylize helpers (`"text".dim()`, `.bold()`, `.cyan()`, `.underlined()`) over manual `Style`. Chain for readability (e.g., `url.cyan().underlined()`).
- Basic spans: `"text".into()`. Styled: `"text".red()`. Use `Line::from(…)` / `Span::from(…)` only when target type is ambiguous.
- `vec![…].into()` for lines when type is obvious; `Line::from(vec![…])` otherwise.
- Computed styles: `Span::styled` or `.set_style()` are fine.
- Avoid `.white()`; prefer default foreground.
- Avoid churn: don't refactor between equivalent forms without a clear gain; follow file-local conventions.
- Prefer the form that stays on one line after rustfmt.

### Text wrapping

- Always use textwrap::wrap to wrap plain strings.
- If you have a ratatui Line and you want to wrap it, use the helpers in tui/src/wrapping.rs, e.g. word_wrap_lines / word_wrap_line.
- If you need to indent wrapped lines, use the initial_indent / subsequent_indent options from RtOptions if you can, rather than writing custom logic.
- If you have a list of lines and you need to prefix them all with some prefix (optionally different on the first vs subsequent lines), use the `prefix_lines` helper from line_utils.

## Tests

### Snapshot tests

This repo uses snapshot tests (via `insta`), especially in `nori-rs/tui`, to validate rendered output.

**Requirement:** Any change that affects user-visible UI (including adding new UI) must include corresponding `insta` snapshot coverage (add a new snapshot test if one doesn't exist yet, or update the existing snapshot). Review and accept snapshot updates as part of the PR so UI impact is easy to review and future diffs stay visual.

Snapshot workflow (`cargo install cargo-insta` if missing):

1. `cargo test -p nori-tui` — generates updated `.snap.new` files
2. `cargo insta pending-snapshots -p nori-tui` — list pending changes
3. Review the `.snap.new` files or `cargo insta show -p nori-tui path/to/file.snap.new`
4. `cargo insta accept -p nori-tui` — accept all new snapshots

### Test assertions

- Tests should use pretty_assertions::assert_eq for clearer diffs. Import this at the top of the test module if it isn't already.
- Prefer deep equals comparisons whenever possible. Perform `assert_eq!()` on entire objects, rather than individual fields.
- Avoid mutating process environment in tests; prefer passing environment-derived flags or dependencies from above.

### Integration tests (core)

- Prefer the utilities in `core_test_support::responses` when writing end-to-end tests.

- All `mount_sse*` helpers return a `ResponseMock`; hold onto it so you can assert against outbound `/responses` POST bodies.
- Use `ResponseMock::single_request()` when a test should only issue one POST, or `ResponseMock::requests()` to inspect every captured `ResponsesRequest`.
- `ResponsesRequest` exposes helpers (`body_json`, `input`, `function_call_output`, `custom_tool_call_output`, `call_output`, `header`, `path`, `query_param`) so assertions can target structured payloads instead of manual JSON digging.
- Build SSE payloads with the provided `ev_*` constructors and the `sse(...)`.
- Prefer `wait_for_event` over `wait_for_event_with_timeout`.
- Prefer `mount_sse_once` over `mount_sse_once_match` or `mount_sse_sequence`.

- Typical pattern:

  ```rust
  let mock = responses::mount_sse_once(&server, responses::sse(vec![
      responses::ev_response_created("resp-1"),
      responses::ev_function_call(call_id, "shell", &serde_json::to_string(&args)?),
      responses::ev_completed("resp-1"),
  ])).await;

  codex.submit(Op::UserTurn { ... }).await?;

  // Assert request body if needed.
  let request = mock.single_request();
  // assert using request.function_call_output(call_id) or request.json_body() or other helpers.
  ```

## Critical: How to Close the Loop

After implementation is complete and all tests, linting, and formatting pass,
you MUST verify your changes work end-to-end using one or more of the options
below. Choose the option that matches the area of the codebase you changed.

### Option 1: Build and Drive the TUI

**When to use:** Any change to `nori-rs/tui/`, `nori-rs/cli/`, or `nori-rs/acp/` crates. Also consider running this for changes to shared crates (`core/`, `common/`, `protocol/`) that affect TUI behavior.

**Skill:** `tui-puppeteering-with-tmux`

**Steps:**

Run all steps from the `nori-rs/` directory.

1. Build the `nori` binary: `cargo build --bin nori`
2. Ensure `elizacp` is installed: `which elizacp || cargo install --locked elizacp`
3. Create a temporary config directory with the ElizACP agent so the TUI bypasses onboarding with a lightweight local agent:
   ```bash
   VERIFY_HOME=$(mktemp -d)
   cat > "$VERIFY_HOME/config.toml" <<'TOML'
   agent = "elizacp"

   [[agents]]
   name = "ElizACP"
   slug = "elizacp"

   [agents.distribution.local]
   command = "elizacp"
   args = ["acp"]
   TOML
   ```
4. Read the `tui-puppeteering-with-tmux` skill and set the `SCRIPTS` variable to its scripts directory.
5. Start an isolated tmux session running the binary with the ElizACP config:
   `CODEX_HOME="$VERIFY_HOME" NORI_HOME="$VERIFY_HOME" $SCRIPTS/tui-start nori-verify "./target/debug/nori --agent elizacp --skip-trust-directory"`
6. Assert the TUI renders and the `›` prompt appears: `$SCRIPTS/tui-assert nori-verify "›" 10`
7. Type a test message and press Enter:
   `$SCRIPTS/tui-send nori-verify "hello"` then `$SCRIPTS/tui-send nori-verify --keys Enter`
8. Assert the TUI accepted the input (the prompt reappears): `$SCRIPTS/tui-assert nori-verify "›" 10`
9. Clean up: `$SCRIPTS/tui-stop nori-verify && rm -rf "$VERIFY_HOME"`

**You know it works when:** The TUI launches without panicking, the `›` input prompt appears, the application accepts keyboard input, and the prompt returns after submission.
