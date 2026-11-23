// Run with Bun from the repository root. The native Antigravity CLI must already be on PATH.
import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import net from "node:net";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { connect } from "./rpc.mjs";

const binary = resolve(process.argv[2] ?? "nori-rs/target/debug/nori");
const providers = [
  ["claude-code", "@agentclientprotocol/claude-agent-acp", ["0.102.0", "0.108.1", "0.109.0"]],
  ["codex", "@agentclientprotocol/codex-acp", ["2.7.0", "2.8.0"]],
  ["gemini", undefined, []],
  ["grok-build", "@xai-official/grok", []],
];
const home = await mkdtemp(join(tmpdir(), "nori-provider-check-"));
const noriHome = join(home, ".nori/cli");
const cwd = join(home, "synthetic-project");
const ambient = join(home, "bin");
const connections = new Set();
let holdModelRequest = false;
// Hold remote model connections on loopback so the unsupported-steer receipt
// is observable while the original turn is still running.
const proxy = net.createServer(socket => {
  if (!holdModelRequest) { socket.destroy(); return; }
  connections.add(socket);
  socket.on("error", () => {});
  socket.on("close", () => connections.delete(socket));
  socket.resume();
});
let host, control;
async function until(description, check, timeout = 120_000) {
  const end = Date.now() + timeout;
  while (Date.now() < end) {
    const value = await check();
    if (value) return value;
    await Bun.sleep(50);
  }
  throw new Error("Timed out waiting for " + description);
}

try {
  await Promise.all([mkdir(noriHome, { recursive: true }), mkdir(cwd), mkdir(ambient), mkdir(join(home, ".codex"))]);
  await writeFile(join(noriHome, "config.toml"), "[features]\nmulti_session = true\n");
  await writeFile(join(home, ".codex/auth.json"), JSON.stringify({ OPENAI_API_KEY: "synthetic" }));
  await new Promise(resolve => proxy.listen(0, "127.0.0.1", resolve));
  const loopback = "http://127.0.0.1:" + proxy.address().port;
  for (const command of ["claude", "claude-agent-acp", "codex", "codex-acp", "gemini", "grok"]) {
    await writeFile(join(ambient, command), '#!/bin/sh\necho "Unexpected PATH lookup: $0" >&2\nexit 64\n', { mode: 0o755 });
  }
  const env = {
    HOME: home, PATH: ambient + ":" + process.env.PATH, NORI_HOME: noriHome, CODEX_HOME: join(home, ".codex"),
    NORI_NO_ANALYTICS: "1", CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC: "1",
    ANTHROPIC_API_KEY: "synthetic", OPENAI_API_KEY: "synthetic", GEMINI_API_KEY: "synthetic",
    XAI_API_KEY: "xai-synthetic",
    HTTP_PROXY: loopback, HTTPS_PROXY: loopback, ALL_PROXY: loopback,
    NO_PROXY: "127.0.0.1,localhost,registry.npmjs.org,storage.googleapis.com",
    ANTHROPIC_BASE_URL: loopback, OPENAI_BASE_URL: loopback,
  };
  if (process.env.NORI_MANAGED_BY_NPM) env.NORI_MANAGED_BY_NPM = "1";
  if (process.env.NORI_MANAGED_BY_BUN) env.NORI_MANAGED_BY_BUN = "1";
  host = Bun.spawn([binary, "host", "--foreground", "--port", "0"], { cwd, env, stdout: "ignore", stderr: "inherit" });
  const stateFile = join(noriHome, "host/state.json");
  await until("host startup", () => { assert.equal(host.exitCode, null); return existsSync(stateFile); });
  const { endpoint } = JSON.parse(await readFile(stateFile));
  const token = (await readFile(join(noriHome, "host/token"), "utf8")).trim();
  control = await connect(endpoint, token);
  await control.request("initialize");
  await control.request("_nori/session/subscribe", { session_id: "*" });
  for (const [slug, pkg, verifiedSteering] of providers) {
    let version;
    if (pkg) {
      const dist = await fetch("https://registry.npmjs.org/" + pkg + "/latest");
      assert.ok(dist.ok, "Could not read the upstream release for " + pkg);
      ({ version } = await dist.json());
    } else {
      const check = Bun.spawn(["agy", "--version"], { cwd, env, stdout: "pipe", stderr: "inherit" });
      version = (await new Response(check.stdout).text()).trim();
      assert.equal(await check.exited, 0, "Install Antigravity and place agy on PATH");
    }
    const name = slug === "gemini" ? "Antigravity ACP Server" : slug === "grok-build" ? undefined : pkg;
    const command = Bun.spawn([binary, "host", "new", "--agent", slug, "--json"], { cwd, env, stderr: "inherit", stdout: "pipe" });
    const result = await new Response(command.stdout).text();
    assert.equal(await command.exited, 0);
    const id = JSON.parse(result).session_id;
    assert.ok(id);
    const events = () => control.notifications.filter(msg => msg.params?.conversation_id === id).map(msg => msg.params.event);
    const row = await until(slug + " readiness", async () => {
      const list = await control.request("_nori/session/list");
      const session = list.sessions.find(row => row.session_id === id);
      assert.notEqual(session?.status, "failed", slug + " failed to start: " + JSON.stringify(events().at(-1)));
      return session?.status === "ready" && session;
    });
    const init = await until(slug + " initialize response", () => events().map(e => e.event?.response?.Ok).find(r => r?.protocolVersion));
    assert.equal(init.protocolVersion, 1);
    assert.equal(init.agentInfo?.name, name);
    assert.equal(init.agentInfo?.version ?? init._meta?.agentVersion, version);
    const steering = verifiedSteering.includes(version);
    if (steering) assert.equal(init._meta?.steering?.supported, true);
    assert.ok(row.model, slug + " session did not report a model");
    let receipt = "guarded steering advertised";
    if (!steering) {
      const prompt = text => [{ type: "text", text }];
      holdModelRequest = true;
      const turn = await control.request("_nori/session/prompt", { session_id: id, prompt: prompt("Synthetic initial request") });
      const steer = { session_id: id, expected_turn: turn.request_id, intent: "steer", message_id: crypto.randomUUID(), prompt: prompt("Synthetic correction") };
      const first = await control.request("_nori/session/follow_up", steer);
      assert.equal(first.status, "unsupported_queued");
      assert.deepEqual(await control.request("_nori/session/follow_up", steer), first);
      receipt = first.status + " on initial delivery and retry";
      holdModelRequest = false;
      for (const socket of connections) socket.destroy();
      await control.request("_nori/session/cancel", { session_id: id });
    }
    console.log(slug, "ACP", version, "|", row.model, "|", receipt);
    await control.request("_nori/session/close", { session_id: id });
  }
} finally {
  if (control) {
    await control.request("_nori/host/stop").catch(() => {});
    await control.close();
  }
  if (host) {
    if (host.exitCode === null) host.kill();
    await host.exited;
  }
  for (const socket of connections) socket.destroy();
  proxy.close();
  await rm(home, { recursive: true, force: true });
}
