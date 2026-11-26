"""Synthetic driver for verifying the nori-cli selector stack (#795, #797-#799).

Each terminal is a private tmux pane. A shaded terminal sets a pane background
so tmux answers the OSC 11 query; an unshaded one leaves it unset, so nori
gets no background report and falls back to bold marks.
"""
import json, re, subprocess, sys, time
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent.parent))
import term as T

T.SOCKET = "vsel"
WORK = Path("/tmp/vsel/runs")
SGR = re.compile(r"\x1b\[[0-9;:]*m")


def plain(s):
    return SGR.sub("", s)


def setup(nori_dir: Path, name: str, extra_cfg=""):
    run = WORK / name
    subprocess.run(["rm", "-rf", str(run)])
    home, proj, bindir = run / "home", run / "orchard", run / "bin"
    for d in (home, proj / "src", bindir):
        d.mkdir(parents=True, exist_ok=True)
    (proj / "src" / "pear_ledger.rs").write_text("// synthetic pear ledger\n")
    (proj / "src" / "plum_rows.rs").write_text("// synthetic plum rows\n")
    (bindir / "nori").symlink_to(nori_dir / "nori")
    (home / "config.toml").write_text(
        f'agent = "mock-model"\ncheck_for_update_on_startup = false\n'
        f'[projects."{proj}"]\ntrust_level = "trusted"\n{extra_cfg}\n')
    return {"run": run, "home": home, "proj": proj, "bin": bindir, "mock": nori_dir / "mock_acp_agent",
            "nori": nori_dir / "nori"}


def env(fx, **extra):
    h = str(fx["home"])
    e = {"PATH": f"{fx['bin']}:/usr/bin:/bin", "HOME": h, "NORI_HOME": h, "CODEX_HOME": h,
         "NORI_NO_ANALYTICS": "1", "TERM": "xterm-256color", "COLORTERM": "truecolor", "LANG": "C.UTF-8",
         "MOCK_ACP_AGENT_BIN": str(fx["mock"]), "MOCK_AGENT_PROMPT_LOG": str(fx["run"] / "prompts.jsonl")}
    e.update({k: str(v) for k, v in extra.items()})
    return e


def term(fx, name, out, shaded, cols=120, rows=32, **extra):
    t = T.Term(name, env(fx, **extra), fx["proj"], out, cols, rows)
    if shaded:
        T.tmux("set-option", "-p", "-t", name, "window-style", "bg=#141414")
    return t


def resize(t, cols, rows=None):
    T.tmux("resize-window", "-t", t.name, "-x", str(cols), "-y", str(rows or t.rows))
    t.cols = cols
    time.sleep(0.8)


def slow_type(t, text, delay=0.04):
    for ch in text:
        T.tmux("send-keys", "-t", t.name, "-l", ch)
        time.sleep(delay)
    time.sleep(0.35)


def focus_rows(t):
    """Screen rows (plain text) that carry the focused-row left rail."""
    return [l for l in t.screen().splitlines() if "▏" in l]


def focus_ansi(t):
    return [l for l in t.screen(ansi=True).splitlines() if "▏" in plain(l)]


class Results:
    def __init__(self, out: Path, build: str):
        self.out, self.build, self.rows = out, build, []
        out.mkdir(parents=True, exist_ok=True)

    def check(self, sid, cid, ok, detail="", pr=None):
        self.rows.append({"build": self.build, "scenario": sid, "check": cid, "pr": pr,
                          "passed": bool(ok), "detail": str(detail)[:600]})
        print(("PASS " if ok else "FAIL ") + f"[{self.build}] {sid}/{cid} {str(detail)[:220]!r}", flush=True)
        (self.out / "results.json").write_text(json.dumps(self.rows, indent=2, ensure_ascii=False))
