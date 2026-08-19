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
- initialize/new-session failures, setup notifications, partial failed-load
  history, and prompt-time child exit;
- cancellation, disconnects, EOF teardown, stalled children, and
  interleaved tool/message ordering; and
- cloud, MCP, browser, transcript, and presentation regressions.

The mock deliberately issues outbound client requests through the SDK rather
than calling host internals. This makes the public `AcpEvent::Request` and
`HarnessHandle::respond_to_agent` path observable end to end.

### Things to Know

- Direct SDK use is allowed here because this is an ACP agent/conformance
  fixture. It does not weaken the rule that only `nori-acp-host` uses the SDK
  among client-side product crates.
- The mock has no catch-all dispatch handler; the SDK default must be allowed to
  forward responses to the mock's own pending client requests.
- `ExitOnEof` preserves the host's stdin-EOF shutdown contract. A dedicated
  scenario can ignore EOF to test the hard-exit watchdog.
- A Unix lifecycle fixture can leave a descendant running after the mock agent
  exits and record its PID for the host integration test. This proves the host
  sweeps the inherited process group without relying on a shell wrapper.
- Local tests expect `target/debug/mock_acp_agent` unless
  `MOCK_ACP_AGENT_BIN` is set.

Created and maintained by Nori.
