#!/usr/bin/env python3
"""Build an isolated audit run: synthetic homes, roots and Lantern agent servers.

usage: setup.py RUN_DIR [default_agent]
Prints shell exports for the host home.
"""
import sys
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import fixtures  # noqa: E402

run = Path(sys.argv[1])
agent = sys.argv[2] if len(sys.argv) > 2 else "claude-code"
f = fixtures.build(run, default_agent=agent)
home = f["homes"]["host"]
print(f"export HOME={home} NORI_HOME={home} CODEX_HOME={home} NORI_NO_ANALYTICS=1")
print(f"export ALPHA={f['roots']['alpha']} BETA={f['roots']['beta']} LEDGER={f['ledger']} CONTROL={f['control']}")
