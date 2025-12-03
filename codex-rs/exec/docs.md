# Noridoc: exec

Path: @/codex-rs/exec

### Overview

The `codex-exec` crate provides headless, non-interactive execution of Codex for automation and CI/CD integration. It runs Codex with a prompt, processes events, and exits when the task completes. Output can be human-readable or JSON-lines format for programmatic consumption.

### How it fits into the larger codebase

Exec is invoked via `codex exec PROMPT`:

- **Uses** `codex-core` for `ConversationManager`, `Config`, `AuthManager`
- **Shares** configuration and auth infrastructure with TUI
- **Uses** `codex-common` for CLI argument parsing
- **Supports** session resume via `codex exec resume`

Unlike TUI, exec requires explicit prompts (positional or stdin) and defaults to non-interactive approvals (`AskForApproval::Never`).

### Core Implementation

**Entry Point:**

`run_main()` in `lib.rs`:
1. Parses prompt from args or stdin
2. Loads configuration with headless-appropriate defaults
3. Initializes tracing and OpenTelemetry
4. Creates conversation (new or resumed)
5. Submits initial prompt
6. Processes events until completion/error

**Event Processing:**

The `event_processor` module defines the `EventProcessor` trait with implementations:
- `EventProcessorWithHumanOutput`: Pretty-printed terminal output
- `EventProcessorWithJsonOutput`: JSONL for automation (`--json` flag)

Events flow from `ConversationManager` through channels to the processor.

**Output Schema:**

The `--output-schema` flag accepts a JSON Schema file that constrains the final model output for structured responses.

### Things to Know

**Prompt Input:**

- Positional argument: `codex exec "your prompt"`
- Stdin: `echo "prompt" | codex exec` or `codex exec -` (explicit stdin)
- Resume: `codex exec resume --last` or `codex exec resume <SESSION_ID>`

**Exit Codes:**

- `0`: Success
- `1`: Error event received or execution failed

**Default Behavior Differences:**

Compared to TUI:
- `approval_policy` defaults to `Never` (no interactive approvals)
- Requires `--skip-git-repo-check` flag if not in a git repository
- No onboarding screens

**Output Modes:**

Human mode prints tool calls, outputs, and final messages with optional ANSI colors.

JSON mode (`--json`) emits structured events for parsing:
- Compatible with `codex-exec/src/exec_events.rs` types
- One JSON object per line
- Suitable for log aggregation and automation pipelines

**Last Message File:**

The `--last-message-file` flag writes the final assistant message to a file, useful for extracting results in scripts.

**CTRL-C Handling:**

Keyboard interrupt sends `Op::Interrupt` to abort in-flight tasks, then exits the event loop gracefully.

Created and maintained by Nori.
