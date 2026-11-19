// Usage: bun verify.mjs /absolute/path/to/nori [claude|codex|both] [sessions|managed|both] [delivery|permission|both]
import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { mkdtemp, mkdir, rm, writeFile } from "node:fs/promises";
import { homedir, tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { startModel, original, correction, queued, refused, correctedReply, checkDeliveries } from "./model.mjs";
import { bounded, connect } from "./rpc.mjs";

const binary = resolve(process.argv[2] ?? "nori-rs/target/debug/nori");
const packages = resolve(process.env.NORI_TEST_ADAPTER_MODULES ?? join(homedir(), ".nori/npm/cli-runtime/node_modules"));
const pins = {
  claude: { name: "claude-agent-acp", version: process.env.NORI_TEST_CLAUDE_ADAPTER_VERSION ?? "0.102.0" },
  codex: { name: "codex-acp", version: "2.7.0" },
};
const text = value => [{ type: "text", text: value }];
const follow = "_nori/session/follow_up";

async function eventually(description, check) {
  const deadline = Date.now() + 30000;
  while (Date.now() < deadline) {
    const value = await check();
    if (value) return value;
    await Bun.sleep(50);
  }
  throw new Error("Timed out: " + description);
}

async function run(provider, transport, scenario) {
  const pin = pins[provider];
  const module = join(packages, "@agentclientprotocol", pin.name);
  assert.equal((await Bun.file(join(module, "package.json")).json()).version, pin.version);
  const home = await mkdtemp(join(tmpdir(), "nori-follow-up-"));
  const noriHome = join(home, ".nori/cli");
  const binding = "synthetic";
  const bindingRoot = join(noriHome, "hosted-sessions", binding);
  const cwd = join(bindingRoot, "work");
  const agentHome = join(bindingRoot, "home");
  const permission = scenario === "permission";
  const testFile = join(home, "denied-write.txt");
  const model = startModel(permission && testFile);
  const env = {
    PATH: process.env.PATH, ANTHROPIC_API_KEY: "synthetic", OPENAI_API_KEY: "synthetic",
    CODEX_API_KEY: "synthetic", CODEX_PATH: join(packages, ".bin/codex"),
    ANTHROPIC_BASE_URL: model.origin, OPENAI_BASE_URL: model.origin + "/v1",
    CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: "1",
    HTTP_PROXY: "http://127.0.0.1:1", HTTPS_PROXY: "http://127.0.0.1:1",
    ALL_PROXY: "http://127.0.0.1:1", NO_PROXY: "localhost,127.0.0.1",
    CODEX_CONFIG: JSON.stringify({
      approval_policy: "on-request", sandbox_mode: "workspace-write",
      model: "gpt-5.4", model_provider: "fixture",
      model_providers: { fixture: { name: "fixture", wire_api: "responses",
        base_url: model.origin + "/v1", env_key: "OPENAI_API_KEY" } },
    }),
  };
  const bun = process.execPath;
  const script = join(module, "dist/index.js");
  const slug = "synthetic-" + provider;
  let host;
  let control;
  let machine;
  let observer;
  try {
    await Promise.all([mkdir(cwd, { recursive: true }), mkdir(join(agentHome, ".codex"), { recursive: true })]);
    // Keep even credential lookup away from the runner's real provider homes.
    await mkdir(join(home, ".codex"), { recursive: true });
    for (const root of [home, agentHome]) {
      await writeFile(join(root, ".codex/auth.json"), JSON.stringify({ OPENAI_API_KEY: "synthetic" }));
    }
    const config = [
      'agent = ' + JSON.stringify(slug), '[features]', 'multi_session = true',
      '[host]', 'managed = ' + (transport === "managed"),
      ...[slug, "synthetic-peer"].flatMap((name, index) => [
        '[[agents]]', 'name = ' + JSON.stringify(name), 'slug = ' + JSON.stringify(name),
        '[agents.distribution.local]', 'command = ' + JSON.stringify(bun), 'args = ' + JSON.stringify([script]),
        '[agents.distribution.local.env]', 'CODEX_HOME = ' + JSON.stringify(join(index ? agentHome : home, ".codex")),
        'CLAUDE_CONFIG_DIR = ' + JSON.stringify(join(index ? agentHome : home, ".claude")),
      ]),
    ];
    await writeFile(join(noriHome, "config.toml"), config.join("\n"));
    host = Bun.spawn([binary, "host", "--foreground", "--port", "0"], {
      cwd, env: { ...env, HOME: home, NORI_HOME: noriHome, CODEX_HOME: join(home, ".codex") },
      stdout: "ignore", stderr: "inherit",
    });
    const stateFile = join(noriHome, "host/state.json");
    await eventually("host startup", async () => {
      assert.equal(host.exitCode, null, "host exited during startup");
      return existsSync(stateFile);
    });
    const { endpoint } = await Bun.file(stateFile).json();
    const token = (await Bun.file(join(noriHome, "host/token")).text()).trim();
    control = await connect(endpoint, token);
    const discovery = await control.request("initialize");
    const durable = transport === "managed" && !permission;
    if (durable) assert.equal(discovery.agentCapabilities._meta["nori.dev/durableRecovery"].version, 1);
    let sessionId;
    let otherSession;
    let expectedTurn;
    let completion;
    let client;
    let originalSettled = false;
    if (transport === "sessions") {
      ({ session_id: sessionId } = await control.request("_nori/session/new", { agent: slug, cwd }));
      if (!permission) {
        const ready = id => eventually("ordinary session ready", async () => {
          const { sessions } = await control.request("_nori/session/list");
          const row = sessions.find(row => row.session_id === id);
          assert.notEqual(row?.status, "failed", "provider session failed to initialize");
          return row?.status === "ready";
        });
        await ready(sessionId);
        ({ session_id: otherSession } = await control.request("_nori/session/new", { agent: "synthetic-peer", cwd }));
        await ready(otherSession);
      }
      await control.request("_nori/session/subscribe", { session_id: sessionId });
      ({ request_id: expectedTurn } = await control.request("_nori/session/prompt", {
        session_id: sessionId, prompt: text(original),
      }));
      client = control;
    } else {
      await control.request("_nori/session/ensure", {
        bindingId: binding, generation: 1, recoveryVersion: durable ? 1 : undefined, commandId: "launch", conversationId: crypto.randomUUID(),
        spec: { projectId: "fixture", runId: "fixture", agent: provider, cwd, home: agentHome, env,
          adapter: { path: bun, version: pin.version, args: [script] },
          additions: { promptEcho: false, statusEvents: false, recording: false, clientMcp: false } },
      });
      machine = await connect(endpoint.replace(/\/sessions$/, "/managed"), token, binding);
      const init = await machine.request("initialize", { protocolVersion: 1, clientCapabilities: {} });
      assert.equal(init.agentInfo.name, "@agentclientprotocol/" + pin.name);
      assert.equal(init.agentInfo.version, pin.version);
      assert.equal(init._meta.steering.supported, true);
      if (provider === "codex") await machine.request("authenticate", { methodId: "api-key" });
      ({ sessionId } = await machine.request("session/new", { cwd, mcpServers: [] }));
      if (!permission) ({ sessionId: otherSession } = await machine.request("session/new", { cwd, mcpServers: [] }));
      if (permission) await machine.request("session/set_mode", { sessionId, modeId: provider === "codex" ? "read-only" : "default" });
      expectedTurn = "original-prompt";
      completion = machine.request("session/prompt", { sessionId, prompt: text(original) }, expectedTurn, durable ? "durable-original" : undefined);
      completion.then(() => { originalSettled = true; }, () => { originalSettled = true; });
      client = machine;
    }
    await bounded(model.reached, "first provider request");

    const command = (intent, marker) => ({
      session_id: sessionId, message_id: crypto.randomUUID(), expected_turn: expectedTurn, intent, prompt: text(marker),
    });
    let approval;
    if (permission) approval = await eventually("pending permission", () => machine.notifications.find(
      msg => msg.method === "session/request_permission" && msg.params.sessionId === sessionId));
    const steer = command("steer", correction);
    const next = command("queue", queued);
    if (transport === "sessions") {
      observer = await connect(endpoint, token);
      await observer.request("initialize", { observe: true });
      await assert.rejects(observer.request(follow, steer), e => e.code === -32016);
    }
    let nextReceipt;
    if (!permission) {
      nextReceipt = await client.request(follow, next);
      assert.equal(nextReceipt.status, "queued");
      assert.equal(nextReceipt.message_id, next.message_id);
      const invalid = () => command("steer", refused);
      await assert.rejects(client.request(follow, { ...invalid(), sender: "forged" }), e => e.code === -32602);
      const wrongTurn = await client.request(follow, { ...invalid(), expected_turn: "expired-turn" });
      assert.equal(wrongTurn.status, "rejected");
      const wrongSession = await client.request(follow, { ...invalid(), session_id: otherSession });
      assert.equal(wrongSession.status, "rejected");
    }
    const receipt = await client.request(follow, steer);
    assert.equal(receipt.message_id, steer.message_id);
    assert.equal(receipt.status, "injected");
    assert.equal(receipt.sender, transport === "sessions" ? "host-controller" : "machine-client");

    assert.deepEqual(await client.request(follow, steer), receipt);
    if (permission) {
      assert.equal(originalSettled, false, "the original prompt is still awaiting its approval");
      const option = approval.params.options.find(o => o.kind === "reject_once");
      assert.ok(option, "adapter offered a one-time rejection");
      machine.answer(approval.id, { outcome: { outcome: "selected", optionId: option.optionId } });
      const result = await completion;
      assert.equal(result.stopReason, provider === "codex" ? "cancelled" : "end_turn");
      assert.ok(result.usage?.totalTokens > 0);
      assert.equal(existsSync(testFile), false);
      console.log(provider, pin.version, "/" + transport, "PASS  permission rejected after injection. Original stopReason=" + result.stopReason);
      return;
    }
    await assert.rejects(client.request(follow, { ...steer, prompt: text(refused) }),
      e => [-32602, -32000].includes(e.code));
    const twin = { ...next, message_id: crypto.randomUUID() };
    assert.equal((await client.request(follow, twin)).status, "queued");
    if (transport === "sessions") {
      await control.close();
      control = await connect(endpoint, token);
      await control.request("initialize");
      await control.request("_nori/control/take");
      await control.request("_nori/session/subscribe", { session_id: sessionId });
      client = control;
    } else {
      assert.equal(originalSettled, false);
      await machine.close();
      machine = await connect(endpoint.replace(/\/sessions$/, "/managed"), token, binding);
      completion = machine.waitForReply(expectedTurn, "reattached original prompt");
      await machine.attach();
      client = machine;
      originalSettled = false;
      completion.then(() => { originalSettled = true; }, () => { originalSettled = true; });
    }
    assert.deepEqual(await client.request(follow, steer), receipt);
    assert.deepEqual(await client.request(follow, next), nextReceipt);
    assert.equal(originalSettled, false);
    assert.ok(model.requests.every(item => item[queued] === 0));
    model.release();
    await bounded(model.corrected, "provider received the active correction");
    assert.ok(model.requests.every(item => item[queued] === 0), "queue reached the active turn");
    const ownerResponse = msg => {
      const event = msg.params?.event?.event;
      return event?.message_type === "response" && event.request_id === expectedTurn;
    };
    assert.equal(originalSettled, false);
    assert.ok(!control.notifications.some(ownerResponse));
    model.finishCorrection();
    let updates;
    let result;
    if (completion) {
      const reply = await completion;
      result = reply.result;
      updates = machine.notifications.slice(0, reply.notificationCount)
        .filter(msg => msg.method === "session/update").map(msg => msg.params.update);
    } else {
      const owner = await eventually("original prompt completion", () => control.notifications.find(ownerResponse));
      result = owner.params.event.event.response.Ok;
      updates = control.notifications.slice(0, control.notifications.indexOf(owner)).map(msg => msg.params?.event?.event?.update);
    }
    const output = updates.filter(update => update?.sessionUpdate === "agent_message_chunk")
      .map(update => update.content.text).join("");
    assert.ok(output.includes(correctedReply), "original prompt completed before the corrected reply");
    assert.equal(result.stopReason, "end_turn");
    assert.ok(result.usage?.totalTokens > 0);
    if (transport === "managed") {
      const finished = () => machine.notifications.filter(msg => msg.method === follow + "/finished");
      await eventually("queued completion notifications", () => finished().length >= 2);
      assert.deepEqual(finished().map(msg => msg.params.message_id), [next.message_id, twin.message_id]);
      assert.ok(finished().every(msg => msg.params.result.stopReason === "end_turn"));
    } else {
      const done = () => control.notifications.map(msg => msg.params?.event?.event).filter(
        event => event?.message_type === "response" && event.response?.Ok?.stopReason === "end_turn");
      await eventually("original and two queued completions", () => done().length >= 3);
      assert.equal(new Set(done().map(event => event.request_id)).size, 3);
    }
    checkDeliveries(model.requests);
    console.log(provider, pin.version, "/" + transport, "PASS  one steer, two identical queued inputs, three completed prompts");
  } finally {
    model.release();
    model.finishCorrection();
    observer?.close();
    machine?.close();
    if (control) {
      try { await control.request("_nori/host/stop"); } catch { /* host may already have stopped */ }
      control.close();
    }
    if (host && host.exitCode === null) {
      host.kill();
      await host.exited;
    }
    model.close();
    await rm(home, { recursive: true, force: true });
  }
}

console.log("Host follow-up verification  |  Independent WebSocket client, loopback inference");
const provider = process.argv[3] ?? "both";
const transport = process.argv[4] ?? "both";
assert.ok(["claude", "codex", "both"].includes(provider));
assert.ok(["sessions", "managed", "both"].includes(transport));
const scenario = process.argv[5] ?? "both";
assert.ok(["delivery", "permission", "both"].includes(scenario));
if (transport === "sessions" && scenario === "permission") throw new Error("Permission probe uses /managed");
for (const surface of transport === "both" ? ["sessions", "managed"] : [transport]) {
  for (const agent of provider === "both" ? Object.keys(pins) : [provider]) {
    if (scenario !== "permission") await run(agent, surface, "delivery");
    if (surface === "managed" && scenario !== "delivery") await run(agent, surface, "permission");
  }
}
