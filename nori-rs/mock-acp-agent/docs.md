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

Prompt-fidelity fixtures may require the final text block, its matching-block
count, image count, image MIME type, and image data through the
`MOCK_AGENT_EXPECT_*` prompt variables. A mismatch returns a structured prompt
error, allowing PTY lifecycle tests to verify that deferred positional or typed
input reaches ACP exactly once without transforming text or attachments.

Cloud readiness fixtures can hold a successful `session/new` response until a
`MOCK_AGENT_NEW_SESSION_RELEASE_FILE` appears or return a structured
broker-style failure. The handlers in [`main.rs`](src/main.rs) keep these
controls on the real ACP request path, so PTY tests can observe the
pre-`SessionStarted` UI state and user-facing ACP error projection without
replacing Handroll or bypassing the stdio transport.

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

Created and maintained by Nori.
