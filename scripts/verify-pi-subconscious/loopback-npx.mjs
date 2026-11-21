#!/usr/bin/env bun
// Verify the generated production profile before launching real Pi against the local fixture.
import assert from "node:assert/strict";
import { copyFile, mkdtemp, readFile, writeFile } from "node:fs/promises";
import { join } from "node:path";

const { HOME, PI_CODING_AGENT_DIR: source, NORI_PI_TEST_ENDPOINT: endpoint, NORI_PI_TEST_NPX: npx } = process.env;
assert.deepEqual(process.argv.slice(2), ["--yes", "--package=pi-acp@latest", "--package=@earendil-works/pi-coding-agent@latest", "pi-acp"]);
assert.equal(source, join(HOME, ".nori/cli/token-providers/subconscious-pi-acp"));
assert.equal(new URL(endpoint).hostname, "127.0.0.1");
const models = JSON.parse(await readFile(join(source, "models.json"), "utf8"));
assert.deepEqual(Object.keys(models.providers), ["subconscious"]);
assert.equal(models.providers.subconscious.baseUrl, "https://api.subconscious.dev/v1");
models.providers.subconscious.baseUrl = endpoint;
const localProfile = await mkdtemp(join(HOME, "loopback-profile-"));
await writeFile(join(localProfile, "models.json"), JSON.stringify(models), { mode: 0o600 });
await copyFile(join(source, "settings.json"), join(localProfile, "settings.json"));
const child = Bun.spawn([npx, ...process.argv.slice(2)], {
  env: { ...process.env, PI_CODING_AGENT_DIR: localProfile },
  stdin: "inherit", stdout: "inherit", stderr: "inherit",
});
process.exit(await child.exited);
