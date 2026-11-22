// Test instrumentation loaded by Node into the installed adapter and its Pi child.
const { createHash } = require("node:crypto");
const fs = require("node:fs");
const path = require("node:path");

const real = value => value && fs.realpathSync(value);
const digest = value => value == null ? null : createHash("sha256").update(value).digest("hex");
const executable = process.argv[1] && fs.existsSync(process.argv[1]) && real(process.argv[1]);
const role = executable === process.env.FIXTURE_ADAPTER ? "adapter"
  : executable === process.env.FIXTURE_PI ? "pi" : null;
if (role) {
  const runtime = process.env.NORI_AGENT_RUNTIME_DIR;
  const variables = Object.fromEntries(
    ["NORI_ACCEPT_VALUE", "ACCEPT_AMBIENT_KEY", "ACCEPT_LOOPBACK_KEY"].map(key => [key, digest(process.env[key])])
  );
  fs.appendFileSync(process.env.FIXTURE_CAPTURE, JSON.stringify({
    role, pid: process.pid, argv: process.argv.slice(2), variables,
    cwd: real(process.cwd()), home: real(process.env.HOME),
    inner: process.env.PI_ACP_PI_COMMAND || null,
    runtime: real(runtime) || null,
    mode: runtime ? fs.statSync(runtime).mode & 0o777 : null,
    marker: runtime ? digest(fs.readFileSync(path.join(runtime, "marker"))) : null,
  }) + "\n");
}
