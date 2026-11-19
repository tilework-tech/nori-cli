# Host follow-up verification

Run from the repository root after building the candidate binary. The independent Bun client starts isolated hosts and exercises the authenticated WebSocket transports using the installed adapter packages. Use Bun 1.4 or newer with built-in zstd decompression.

```sh
bun scripts/verify-host-follow-up/verify.mjs nori-rs/target/debug/nori
```

The default module directory is `~/.nori/npm/cli-runtime/node_modules`. Set `NORI_TEST_ADAPTER_MODULES` to use another installed tree containing the Claude 0.102.0 and Codex 2.7.0 adapters and their provider binaries. For a standalone Claude adapter, point the module directory at the isolated install and set `NORI_TEST_CLAUDE_ADAPTER_VERSION` to the release under test. The verifier checks the package metadata and the live adapter identity on `/managed`. Each host runs in a temporary home with synthetic credentials, loopback provider URLs and an explicit child environment. The tests cover six scenarios.

- On both `/sessions` and the scoped `/managed` transport, the provider receives an initial prompt. While its HTTP request is held, the client steers, retries the same message ID and queues two identical follow-ups under distinct IDs. The endpoint checks the order and number of times each marker appears in provider input. The client checks the correlated terminal responses for the original prompt and both queued prompts.
- Each transport checks negative cases against two live sessions. A forged sender, stale turn, mismatched session and changed payload under a reused ID are refused. The endpoint checks that the sentinel in these requests never appears in model input.
- On `/sessions`, a watcher tries to steer and gets the read-only error. The controller reconnects while the original request is held and retries with the same identifiers.
- On `/managed`, the delivery scenario negotiates durable recovery v1 and sends the original prompt with an outer command ID. After receiving the follow-up receipts, the machine client closes the WebSocket while inference is held. Its replacement attaches to generation 1, waits for the host's `attached` marker, retries the same follow-up IDs, and receives the original reply and both queued terminal notifications.
- On `/managed`, each real adapter asks for permission to run a synthetic tool. The client steers while the request is pending, then explicitly rejects the original permission request. The original Claude prompt completes without creating the proposed file. The original Codex prompt reports cancellation. Both return usage.

Select a single path with positional arguments, for example `bun scripts/verify-host-follow-up/verify.mjs nori-rs/target/debug/nori codex managed permission`. The final three arguments accept `claude|codex|both`, `sessions|managed|both` and `delivery|permission|both`. Permission scenarios use the scoped transport.

To repeat the standalone release check in a fresh temporary directory, install its own pinned SDK and provider binary alongside the adapter, then select Claude for all three scenarios.

```sh
fixture=$(mktemp -d)
npm install --prefix "$fixture" --ignore-scripts --no-audit --no-fund @agentclientprotocol/claude-agent-acp@0.108.1
NORI_TEST_ADAPTER_MODULES="$fixture/node_modules" NORI_TEST_CLAUDE_ADAPTER_VERSION=0.108.1 \
  bun scripts/verify-host-follow-up/verify.mjs nori-rs/target/debug/nori claude
```
