"""Original synthetic driver for verifying nori-cli #794 composer hints."""
import os, re, shlex, subprocess, sys, time, json
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import term as T

T.SOCKET = "v794"

def setup(work: Path, nori_bin: Path, name: str, extra_cfg=""):
    run = work / name
    subprocess.run(["rm", "-rf", str(run)])
    home = run / "home"; proj = run / "orchard"; laptop = run / "laptop"; lproj = run / "laptop-orchard"
    for d in (home, proj / "src", laptop, lproj): d.mkdir(parents=True, exist_ok=True)
    (proj / "src" / "pear_ledger.rs").write_text("// synthetic pear ledger\n")
    (proj / "src" / "plum_rows.rs").write_text("// synthetic plum rows\n")
    bindir = run / "bin"; bindir.mkdir(exist_ok=True)
    (bindir / "nori").symlink_to(nori_bin)
    for h, p in ((home, proj), (laptop, lproj)):
        (h / "config.toml").write_text(
            f'agent = "mock-model"\ncheck_for_update_on_startup = false\n'
            f'[projects."{p}"]\ntrust_level = "trusted"\n{extra_cfg}\n')
    return {"run": run, "home": home, "proj": proj, "laptop": laptop, "lproj": lproj, "bin": bindir,
            "mock": nori_bin.parent / "mock_acp_agent"}

def env(fx, home=None, **extra):
    h = str(home or fx["home"])
    e = {"PATH": f"{fx['bin']}:/usr/bin:/bin", "HOME": h, "NORI_HOME": h, "CODEX_HOME": h,
         "NORI_NO_ANALYTICS": "1", "TERM": "xterm-256color", "COLORTERM": "truecolor", "LANG": "C.UTF-8",
         "MOCK_ACP_AGENT_BIN": str(fx["mock"]), "MOCK_AGENT_PROMPT_LOG": str(fx["run"] / "prompts.jsonl")}
    e.update(extra)
    return e

def term(fx, name, out, cols=120, rows=30, cwd=None, home=None, **extra):
    return T.Term(name, env(fx, home, **extra), cwd or fx["proj"], out, cols, rows)

def slow_type(t, text, delay=0.04):
    for ch in text:
        T.tmux("send-keys", "-t", t.name, "-l", ch)
        time.sleep(delay)
    time.sleep(0.3)

def hint_row(t):
    """Text of the line directly below the composer box bottom."""
    lines = t.screen().splitlines()
    return lines

def prompts(fx):
    p = fx["run"] / "prompts.jsonl"
    return [json.loads(l) for l in p.read_text().splitlines()] if p.exists() else []
