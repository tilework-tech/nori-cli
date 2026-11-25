"""Synthetic homes, project roots and agent servers for acceptance runs.

Layout of one run directory:
  homes/host/        NORI_HOME of the host (also the plain `nori` home)
  homes/laptop/      NORI_HOME of an attaching client with its own decoys
  roots/alpha/app/   project root A
  roots/beta/app/    project root B, same logical path `app/` as A
  roots/laptop/app/  client-side decoy root (never offered by a remote host)
  ledger.jsonl       every frame each synthetic adapter received
"""
import json
import os
import sys  # noqa: F401  (sys.executable runs the adapter)
from pathlib import Path

HERE = Path(__file__).resolve().parent
ADAPTER = HERE / "lantern_adapter.py"

# Distinct trees under the same logical path. Only alpha has `lantern`
# files, only beta has `harbor` files, only the laptop has `decoy` files.
TREES = {
    "alpha": ["app/src/lantern_wick.rs", "app/src/lantern_glass.rs", "app/docs/alpha_notes.md",
              "app/shared/README.md", "app/assets/alpha-only/"],
    "beta": ["app/src/harbor_tide.rs", "app/src/harbor_bell.rs", "app/docs/beta_notes.md",
             "app/shared/README.md", "app/assets/beta-only/"],
    "laptop": ["app/src/decoy_laptop.rs", "app/docs/laptop_notes.md", "app/assets/laptop-only/"],
}

# Agent servers: (config name, slug, modes). `claude-code` and `codex` map to
# host Ask (`default` / `read-only`); `opencode` has no Ask mode. Each starts
# in a mode no host mode maps to, so the ledger shows which mode the host set.
AGENTS = [
    ("Lantern Claude", "claude-code", "auto,default,acceptEdits,plan"),
    ("Lantern Codex", "codex", "agent,read-only,workspace-write,agent-full-access"),
    ("Lantern Opencode", "opencode", "build,plan"),
]


def build(run_dir: Path, default_agent="claude-code", host_extra="", top_extra=""):
    run_dir = run_dir.resolve()
    adapter = run_dir.parent / "bin" / ADAPTER.name
    adapter.parent.mkdir(parents=True, exist_ok=True)
    adapter.write_bytes(ADAPTER.read_bytes())
    roots = {}
    for root, entries in TREES.items():
        base = run_dir / "roots" / root
        for entry in entries:
            path = base / entry
            if entry.endswith("/"):
                path.mkdir(parents=True, exist_ok=True)
            else:
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(f"synthetic {root} file {entry}\n")
        roots[root] = (base / "app").resolve()
    ledger = run_dir / "ledger.jsonl"
    ledger.touch()
    control = run_dir / "control"
    control.mkdir(exist_ok=True)
    saved = run_dir / "saved-sessions.json"
    saved.write_text(json.dumps([
        {"sessionId": "saved-orchard", "cwd": str(roots["beta"]), "title": "Saved orchard survey"},
    ]))
    homes = {}
    for name in ("host", "laptop"):
        home = run_dir / "homes" / name
        home.mkdir(parents=True, exist_ok=True)
        lines = [f'agent = "{default_agent}"', top_extra]
        for root in roots.values():
            lines += [f'[projects."{root}"]', 'trust_level = "trusted"']
        if host_extra:
            lines += ["[host]", host_extra]
        for title, slug, modes in AGENTS:
            lines += ["", "[[agents]]", f'name = "{title}"', f'slug = "{slug}"',
                      "[agents.distribution.local]", f'command = "{sys.executable}"',
                      f'args = ["-I", "{adapter}"]',
                      "[agents.distribution.local.env]",
                      f'LANTERN_NAME = "{slug}"', f'LANTERN_LEDGER = "{ledger}"',
                      f'LANTERN_MODES = "{modes}"', f'LANTERN_SAVED = "{saved}"',
                      f'LANTERN_CONTROL = "{control}"']
        (home / "config.toml").write_text("\n".join(lines) + "\n")
        homes[name] = home
    return {"run": run_dir, "roots": roots, "homes": homes, "ledger": ledger, "control": control}


def env_for(home: Path, extra=None):
    env = {
        "PATH": os.environ.get("PATH", "/usr/bin:/bin"),
        "HOME": str(home),
        "NORI_HOME": str(home),
        "CODEX_HOME": str(home),
        "NORI_NO_ANALYTICS": "1",
        "TERM": "xterm-256color",
        "COLORTERM": "truecolor",
        "LANG": "C.UTF-8",
    }
    if os.environ.get("NORI_VERIFY_JOINED_APP"):
        env["NORI_VERIFY_JOINED_APP"] = "1"  # #777's verifier-only joined App entry
    env.update(extra or {})
    return env


def read_ledger(ledger: Path):
    out = []
    for line in ledger.read_text().splitlines():
        if line.strip():
            out.append(json.loads(line))
    return out
