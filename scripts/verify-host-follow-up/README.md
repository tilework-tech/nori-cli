# Host follow-up verification

Run from the repository root after building the candidate binary. The independent Bun client starts isolated hosts and exercises the authenticated WebSocket transports using the installed adapter packages. Use Bun 1.4 or newer with built-in zstd decompression.

```sh
bun scripts/verify-host-follow-up/verify.mjs nori-rs/target/debug/nori
```

The default module directory is `~/.nori/npm/cli-runtime/node_modules`. Set `NORI_TEST_ADAPTER_MODULES` to use another installed tree containing the Claude 0.102.0 and Codex 2.7.0 adapters and their provider binaries. For a standalone Claude adapter, point the module directory at the isolated install and set `NORI_TEST_CLAUDE_ADAPTER_VERSION` to the release under test. The verifier checks the package metadata and the live adapter identity on `/managed`. Each host runs in a temporary home with synthetic credentials, loopback provider URLs and an explicit child environment.

- On both `/sessions` and the scoped `/managed` transport, the provider receives an initial prompt. While the first model request is held, the client queues a follow-up, steers, retries the steer with the same message ID and queues identical text under a second ID. When the model receives the correction, the original prompt is still open and both queued messages are absent from provider input. The client checks that the corrected output precedes the original prompt’s `end_turn` with usage and that each queued submission runs once on its own subsequent turn.
- Each transport checks negative cases against two live sessions. A forged sender, stale turn, mismatched session and changed payload under a reused ID are refused. The endpoint checks that the sentinel in these requests never appears in model input.
- On `/sessions`, a watcher tries to steer and gets the read-only error. The controller reconnects while the original request is held and retries with the same identifiers.
- On `/managed`, the delivery scenario negotiates durable recovery v1 and sends the original prompt with an outer command ID. After receiving the follow-up receipts, the machine client closes the WebSocket while inference is held. Its replacement attaches to generation 1, waits for the host's `attached` marker, retries the same follow-up IDs, and receives the original reply and both queued terminal notifications.
- On `/managed`, each real adapter asks for permission to run a synthetic tool. The client steers while the request is pending, then explicitly rejects the original permission request. The original Claude prompt completes without creating the proposed file. The original Codex prompt reports cancellation. Both return usage.

Select a single path with positional arguments, for example `bun scripts/verify-host-follow-up/verify.mjs nori-rs/target/debug/nori codex managed permission`. The final three arguments accept `claude|codex|both`, `sessions|managed|both` and `delivery|permission|both`. Permission scenarios use the scoped transport.

## Checking a Claude adapter upgrade

Record what the default `@latest` launch resolves to. Install that exact release into an isolated tree with its provider binary, and record the SDK version so the run can be repeated.

```sh
package=@agentclientprotocol/claude-agent-acp
version=$(npm view "$package@latest" version)  # use 0.109.0 to repeat that release
fixture=$(mktemp -d)
npm install --prefix "$fixture" --ignore-scripts --no-audit --no-fund "$package@$version"
npm ls --prefix "$fixture" "$package" @anthropic-ai/claude-agent-sdk
```

1. Replay the [raw ACP probe from #672](https://tilework-tech.slack.com/archives/C0CJSCNNE7N/p1795037839415079) with the candidate adapter, a synthetic credential and a loopback model. In the archive, `probe.py claude-agent-acp` resolves modules relative to the probe user's home. Run it against the isolated install.

   ```sh
   mkdir -p "$fixture/home/.nori/npm/cli-runtime"
   ln -s "$fixture/node_modules" "$fixture/home/.nori/npm/cli-runtime/node_modules"
   HOME="$fixture/home" python3 /path/to/probe.py claude-agent-acp
   ```

   Check the ACP v1 initialize response for the exact package identity, version and steering advertisement. Before the first prompt and after it completes, send `_session/steering` with `_meta.steering.idleBehavior = "promptRequired"`. Both idle replies must report `promptRequired` with reason `noRunningTurn`. During a held prompt the same request must report `injected`, and the model must receive the correction while the original request still owns completion.
2. Add the candidate's exact version to the [shared guard](../../nori-rs/harness/src/follow_up.rs) on a verification branch and rebuild the CLI. Run the independent client on both transports:

   ```sh
   (cd nori-rs && cargo build -p nori-cli)
   NORI_TEST_ADAPTER_MODULES="$fixture/node_modules" NORI_TEST_CLAUDE_ADAPTER_VERSION="$version" \
     bun scripts/verify-host-follow-up/verify.mjs nori-rs/target/debug/nori claude
   ```

3. Repeat the queue-first sequence with actual keys in direct `nori` and controller `nori attach`, using synthetic text and loopback inference. Hold a prompt open, queue a follow-up with Tab, then submit a distinct correction with Enter. Check the corrected reply on the original turn and one provider delivery of the queued text on the next turn. With an unverified adapter, check that Enter visibly queues its input for the next turn. Keep the version entry only after the candidate passes.
