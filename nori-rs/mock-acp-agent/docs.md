# Noridoc: mock-acp-agent

Path: @/nori-rs/mock-acp-agent

### Overview

`mock-acp-agent` is the deterministic ACP agent used by host, harness, and TUI
integration tests. It is a test binary, not a production client component.

### How it fits into the larger codebase

The binary uses the official `agent-client-protocol` SDK on the agent side and
imports its schema values through `nori_protocol::acp`. The host spawns it over
stdio exactly as it spawns a real agent, so tests cover the real SDK, JSON-RPC,
request correlation, subprocess lifecycle, harness stream, and terminal path.

### Core Implementation

Typed SDK handlers implement initialize, authentication, session new/load,
resume/list/close, prompt, cancel, mode, and config-option behavior. Scenario
environment variables select deterministic streams for:

- messages, thoughts, plans, tools, usage, modes, config, and commands;
- permission round trips with schema-native success and error responses;
- host-handled filesystem reads and writes;
- session load/list/resume/close and structured ACP failures;
- initialize/new-session failures (including candidate-only activation
  failure), setup notifications, partial failed-load history, and prompt-time
  child exit;
- cancellation, disconnects, EOF teardown, stalled children, and
  interleaved tool/message ordering; and
- cloud, MCP, browser, transcript, and presentation regressions.

The prompt handler in [`main.rs`](src/main.rs) clears the cancellation flag
before spawning prompt work. SDK incoming handlers run in wire order, while
spawned work runs separately so dispatch can deliver cancellations and client
responses during streaming. A cancel received after prompt admission remains
set even if the prompt task has not started; the task must not reset it. The
next prompt admission clears the previous turn's cancellation. The shared
agent builder also lets [`cancellation_tests.rs`](src/cancellation_tests.rs)
queue prompt and cancel messages before polling the SDK connection, checking
both this ordering and a subsequent turn's ability to stream.

Prompt-fidelity fixtures may require the final text block, its matching-block
count, image count, image MIME type, and image data through the
`MOCK_AGENT_EXPECT_*` prompt variables. A mismatch returns a structured prompt
error, allowing PTY lifecycle tests to verify that deferred positional or typed
input reaches ACP exactly once without transforming text or attachments.
`MOCK_AGENT_MCP_LOG=<path>` writes the name of every MCP server a
`session/new` request carried to that file, one per line.

`MOCK_AGENT_PROMPT_LOG=<path>` appends each delivered prompt's last text block
to that file as one JSON string per line, so tests can assert on exactly which
prompts reached the agent and how many times (for example, that a reclaimed
queued prompt is never delivered).

Cloud readiness fixtures can hold a successful `session/new` response until a
`MOCK_AGENT_NEW_SESSION_RELEASE_FILE` appears or return a structured
broker-style failure. The handlers in [`main.rs`](src/main.rs) keep these
controls on the real ACP request path, so PTY tests can observe the
pre-`SessionStarted` UI state and user-facing ACP error projection without
replacing Handroll or bypassing the stdio transport.

[`context_usage.rs`](src/context_usage.rs) emits schema-native context usage
fixtures on the actual prompt path. `MOCK_AGENT_CONTEXT_SEQUENCE` supplies a
JSON array of per-prompt usage arrays, allowing fresh, absent, and reset
readings; invalid JSON returns a structured ACP error. The optional release
file holds completion after usage arrives, so tests can inspect an active turn
and then release or cancel it without a timing guess. The same fixtures drive
[Harness timeline tests](../harness/tests/context_timeline.rs) and
[terminal context tests](../tui-pty-e2e/tests/context.rs).

Some scenarios can also be chosen per prompt, so several sessions on one mock
binary can behave differently without sharing an environment. A prompt whose
text block is exactly `mock:exit-during-prompt`, or that contains
`mock:stream-until-cancel`, `mock:prompt-fail-after-delay`, or
`mock:request-permission` anywhere, selects the
scenario for that turn. A named permission command runs from the colon to the
end of the text block:

| Prompt marker | Behavior | Environment equivalent |
| --- | --- | --- |
| `mock:stream-until-cancel` | Streams text chunks until the turn is cancelled | `MOCK_AGENT_STREAM_UNTIL_CANCEL` |
| `mock:exit-during-prompt` | Exits with status 17 mid-prompt | `MOCK_AGENT_EXIT_DURING_PROMPT` |
| `mock:prompt-fail-after-delay` | Waits `MOCK_AGENT_DELAY_MS`, then fails the prompt with a JSON-RPC error, so prompts can queue behind a failing turn | `MOCK_AGENT_PROMPT_FAIL` (fails at once) |
| `mock:request-permission` | Sends a permission request for the default `echo` command | `MOCK_AGENT_REQUEST_PERMISSION` |
| `mock:request-permission:<command>` | Sends a permission request naming `<command>` | none |

The permission command comes from the prompt marker first, then
`MOCK_AGENT_PERMISSION_COMMAND`, then the default `echo` command. A named
command is reported as the tool call's content and as `{"command": ...}` raw
input, which lets multi-session tests tell requests from different sessions
apart. `MOCK_AGENT_PERMISSION_DELAY_MS` waits before the request is sent, so a
test can move to another session before the request arrives in the
background.

Two line markers make the agent report session metadata before its normal
reply, one update per matching prompt line, so session-list tests can drive
titles and context use from a prompt:

| Prompt line | Sends |
| --- | --- |
| `mock:title <title>` | `session_info_update` with that title |
| `mock:usage <used> <size>` | `usage_update` with those token counts (ignored unless both parse) |

[`delegation.rs`](src/delegation.rs) exercises the real authenticated HTTP
MCP boundary. A prompt beginning `mock:delegate <agent> <task>` calls `delegate`
on the `nori-client` server advertised to that ACP session, using its advertised
bearer header. `MOCK_AGENT_MCP_HTTP` enables the fixture's HTTP MCP capability.
Nested task markers let a child issue its own delegation. Prompts carrying the
manager's delegation-result prefix follow the result path instead of repeating
a marker embedded in the returned text, so nested results do not recursively
launch work. The [harness tests](../harness/tests/delegation.rs) and
[PTY tests](../tui-pty-e2e/tests/delegation.rs) use this path to cover process
lifetime, approvals, result continuation, and terminal navigation.

The mock deliberately issues outbound client requests through the SDK rather
than calling host internals. This makes the public `AcpEvent::Request` and
`HarnessHandle::respond_to_agent` path observable end to end.

### Things to Know

- Direct SDK use is allowed here because this is an ACP agent/conformance
  fixture. It does not weaken the rule that only `nori-acp-host` uses the SDK
  among client-side product crates.
- The mock has no catch-all dispatch handler; the SDK default must be allowed to
  forward responses to the mock's own pending client requests.
- `MOCK_AGENT_INITIALIZE_META` accepts a JSON object for initialize-response
  metadata. Tests use it to exercise extension recognition without making the
  fixture itself identify as a Nori remote-control surface; invalid or
  non-object JSON is ignored.
- `ExitOnEof` preserves the host's stdin-EOF shutdown contract. A dedicated
  scenario can ignore EOF to test the hard-exit watchdog.
- A Unix lifecycle fixture can leave a descendant running after the mock agent
  exits and record its PID for the host integration test. This proves the host
  sweeps the inherited process group without relying on a shell wrapper.
- Local tests expect `target/debug/mock_acp_agent` unless
  `MOCK_ACP_AGENT_BIN` is set.
- The mock honors `MOCK_AGENT_INJECTED_MODEL` as its model config option's
  current value (its own model-injection channel), so tests can verify that an
  out-of-catalog model forced through spawn-time injection becomes the session's
  current model.
- `MOCK_AGENT_RESPONSE_<MODEL>` overrides the generic `MOCK_AGENT_RESPONSE`.
  The suffix uppercases the model name and replaces hyphens with underscores
  (for example, `MOCK_AGENT_RESPONSE_MOCK_MODEL_ALT`). Multi-agent PTY tests
  use this to prove which subprocess produced a visible response.
- `MOCK_AGENT_SEND_PLAN` sends a two-step plan update before the reply, so
  PTY tests can drive the plan drawer and plan-dependent keys.
- `MOCK_AGENT_SEND_COMMAND_OUTPUT` runs one synthetic `cargo test` command
  with five output lines before the reply, so PTY tests can tell full tool
  output from compact output.

Created and maintained by Nori.
