// Run with Bun from the repository root:
//   bun scripts/verify-pi-endpoint/verify.mjs nori-rs/target/debug/nori
// Drives the real pi-acp adapter and Pi coding agent (installed by npx) through
// `nori exec`, against a loopback OpenAI-compatible endpoint with a synthetic key.
import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdir, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { startEndpoint } from "./endpoint.mjs";

const binary = resolve(process.argv[2] ?? "nori-rs/target/debug/nori");
const home = await mkdtemp(join(tmpdir(), "nori-pi-check-"));
const noriHome = join(home, ".nori/cli");
const project = join(home, "synthetic-project");
const log = join(home, "endpoint.jsonl");
const key = "synthetic-key-7f3a";
const endpoint = startEndpoint(log);
const baseUrl = `http://127.0.0.1:${endpoint.port}/v1`;

const requests = async () =>
  (await readFile(log, "utf8").catch(() => "")).split("\n").filter(Boolean).map(line => JSON.parse(line));

async function exec(env) {
  const child = Bun.spawn([binary, "exec", "--agent", "pi", "--dangerously-bypass-approvals-and-sandbox",
    "-C", project, "Read notes.txt, count its lines in a terminal, and report both."], {
    env, stdout: "pipe", stderr: "pipe",
  });
  const timer = setTimeout(() => child.kill(), 240_000);
  const [stdout, stderr, code] = await Promise.all([
    new Response(child.stdout).text(), new Response(child.stderr).text(), child.exited,
  ]);
  clearTimeout(timer);
  return { stdout, stderr, code };
}

try {
  await mkdir(noriHome, { recursive: true });
  await mkdir(project);
  await writeFile(join(project, "notes.txt"), "token=orchid-42\nsecond synthetic line\nthird synthetic line\n");
  await writeFile(join(noriHome, "config.toml"), [
    `agent = "pi"`,
    ``,
    `[[token_providers]]`,
    `name = "lab"`,
    `preset = "custom"`,
    `protocol = "openai-completions"`,
    `model = "lab-small"`,
    `env_key = "SYNTHETIC_MODEL_KEY"`,
    `base_url = "${baseUrl}"`,
    ``,
    `[agent_token_providers]`,
    `pi = "lab"`,
    ``,
  ].join("\n"));
  const env = {
    HOME: home, PATH: process.env.PATH, NORI_HOME: noriHome, NORI_NO_ANALYTICS: "1",
    PI_SKIP_VERSION_CHECK: "1", PI_TELEMETRY: "0", TERM: "dumb",
    npm_config_cache: process.env.npm_config_cache ?? join(process.env.HOME, ".npm"),
    ...(process.env.npm_config_offline ? { npm_config_offline: process.env.npm_config_offline } : {}),
  };

  const missing = await exec(env);
  assert.notEqual(missing.code, 0, "Pi must not start without its key");
  assert.match(missing.stdout + missing.stderr, /SYNTHETIC_MODEL_KEY/);
  assert.equal((await requests()).length, 0, "no model traffic without a key");
  console.log("missing key: refused before launch, naming SYNTHETIC_MODEL_KEY");

  const absentBinary = await exec({ ...env, PATH: join(home, "no-programs"), SYNTHETIC_MODEL_KEY: key });
  assert.notEqual(absentBinary.code, 0);
  assert.match(absentBinary.stderr, /npx/);
  assert.deepEqual(await readdir(join(noriHome, ".pi-launches")), [".lock"]);
  assert.equal((await requests()).length, 0);

  const run = await exec({ ...env, SYNTHETIC_MODEL_KEY: key });
  console.log(run.stdout);
  if (run.code !== 0) console.error(run.stderr);
  assert.equal(run.code, 0, "nori exec with Pi should succeed");
  assert.match(run.stdout, /Read token=orchid-42; ran synthetic terminal: 3\./);
  const chats = (await requests()).filter(request => request.path.endsWith("/chat/completions"));
  assert.ok(chats.length >= 3, `expected three model turns, saw ${chats.length}`);
  for (const chat of chats) {
    assert.equal(chat.authorizationFingerprint, createHash("sha256").update(`Bearer ${key}`).digest("hex"));
    assert.equal(chat.client, null);
    assert.equal(chat.model, "lab-small");
  }
  const config = await readFile(join(noriHome, "config.toml"), "utf8");
  assert.ok(!config.includes(key), "the named connection must store only a key reference");
  assert.deepEqual(await readdir(join(noriHome, ".pi-launches")), [".lock"]);
  const history = await readdir(join(noriHome, "token-providers/lab-pi-acp/sessions"), { recursive: true });
  assert.ok(history.some(file => file.endsWith(".jsonl")), "Pi session history survives profile cleanup");
  console.log(`Pi + custom loopback: ${chats.length} model turns, file read and terminal command reported`);
} finally {
  endpoint.stop(true);
  await rm(home, { recursive: true, force: true });
}
