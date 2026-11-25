#!/usr/bin/env python3
"""Long-history open: today's `nori attach` vs the #777 joined App (verifier entry).

For each sample: fresh kiln host seeded with N sections, `nori attach <id>` in a
draining tmux pane with a deep history limit. Measures launch -> first mark /
latest reply / final tool, a key typed 50 ms after launch until it shows in the
composer, raw bytes written, and for the inline joined App audits the whole tmux
scrollback: every KilnMark once and in order, opening and closing notes once.
"""
import argparse, hashlib, json, os, platform, re, shlex, statistics, subprocess, sys, tempfile, time, uuid
from pathlib import Path
sys.path.insert(0, str(Path(__file__).resolve().parent))
from kiln import NS, KilnHost, ms_since  # noqa: E402

MARK = re.compile(r"KilnMark(\d{4})")


class DeepPane:
    def __init__(self, workdir, env, cwd, argv, cols=100, rows=30):
        self.sock = f"jo-{uuid.uuid4().hex[:10]}"
        self.raw = Path(workdir) / "pty.raw"; self.raw.touch()
        self.gate = Path(workdir) / "gate"; os.mkfifo(self.gate)
        cmd = shlex.join(["env", "-i", *[f"{k}={v}" for k, v in env.items()], *argv])
        self.tmux("set-option", "-g", "history-limit", "500000", ";",
                  "new-session", "-d", "-s", "k", "-x", str(cols), "-y", str(rows), "-c", str(cwd),
                  f"read -r _ < {shlex.quote(str(self.gate))}; exec {cmd}")
        assert self.tmux("display", "-p", "-t", "k", "#{history_limit}").strip() == "500000"
        self.tmux("set-option", "-t", "k", "status", "off")
        self.tmux("pipe-pane", "-O", "-t", "k", "cat >> " + shlex.quote(str(self.raw)))

    def tmux(self, *a):
        return subprocess.check_output(["tmux", "-f", "/dev/null", "-L", self.sock, *a], text=True, timeout=20)

    def start(self):
        with self.gate.open("w") as g:
            t0 = NS(); g.write("go\n")
        return t0

    def screen(self):
        return self.tmux("capture-pane", "-p", "-t", "k")

    def history(self):
        return self.tmux("capture-pane", "-p", "-J", "-S", "-", "-t", "k")

    def text(self, s):
        self.tmux("send-keys", "-t", "k", "-l", s)

    def wait(self, pred, timeout=90):
        end = time.monotonic() + timeout
        while True:
            s = self.screen()
            if pred(s):
                return s
            if time.monotonic() > end:
                raise TimeoutError(s)
            time.sleep(0.002)

    def settle(self, quiet=0.4, timeout=90):
        end = time.monotonic() + timeout; prev = size = None; since = time.monotonic()
        while time.monotonic() < end:
            s, n = self.screen(), self.raw.stat().st_size
            if (s, n) != (prev, size):
                prev, size, since = s, n, time.monotonic()
            elif time.monotonic() - since >= quiet:
                return s
            time.sleep(0.01)
        raise TimeoutError("never settled")

    def close(self):
        subprocess.run(["tmux", "-L", self.sock, "kill-server"], capture_output=True)


def audit(hist, sections):
    marks = [int(m.group(1)) for m in MARK.finditer(hist)]
    return {"marks_in_order_once": marks == list(range(1, sections + 1)),
            "mark_count": len(marks), "dup_marks": len(marks) - len(set(marks)),
            "opening": hist.count("KilnOpening"), "latest": hist.count("KilnLatestEnd"),
            "closing_090": hist.count("Closing note 090"),
            "disconnect_rows": hist.count("Lost the connection") + hist.count("Connection lost")}


def sample(label, binary, joined, sections, workdir, cols, rows):
    out = {"build": label, "sections": sections}
    with KilnHost(binary, sections) as host:
        env = dict(host.env)
        if joined:
            env["NORI_VERIFY_JOINED_APP"] = "1"
        pane = DeepPane(workdir, env, host.cwd, [host.nori, "attach", host.session_id[:8]], cols, rows)
        try:
            t0 = pane.start()
            time.sleep(0.05)
            pane.text("ж")
            seen = {}
            def check(s):
                if "first_mark_ms" not in seen and MARK.search(s): seen["first_mark_ms"] = ms_since(t0)
                if "latest_ms" not in seen and "KilnLatestEnd" in s: seen["latest_ms"] = ms_since(t0)
                if "final_tool_ms" not in seen and ("KilnFinalTool" in s or "final.txt" in s): seen["final_tool_ms"] = ms_since(t0)
                last = [l for l in s.splitlines() if l.strip()]
                if "key_ms" not in seen and any(l.lstrip().startswith("›") and "ж" in l for l in last[-4:]):
                    seen["key_ms"] = ms_since(t0)
                return len(seen) == 4
            try:
                pane.wait(check, 120)
            except TimeoutError as e:
                out["timeout_screen"] = str(e)[-1500:]
            out.update(seen)
            pane.settle(0.5)
            out["raw_bytes"] = pane.raw.stat().st_size
            if joined:
                out["audit"] = audit(pane.history(), sections)
            out["final_screen"] = pane.screen()
        finally:
            pane.close()
    return out


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--build", nargs=3, action="append", metavar=("LABEL", "BINARY", "JOINED"), required=True)
    p.add_argument("--sections", type=int, nargs="+", default=[64, 150])
    p.add_argument("--samples", type=int, default=3)
    p.add_argument("--cols", type=int, default=100); p.add_argument("--rows", type=int, default=30)
    p.add_argument("--out", type=Path, required=True)
    a = p.parse_args()
    builds = [(l, str(Path(b).resolve()), j == "1") for l, b, j in a.build]
    res = {"platform": platform.platform(), "builds": {l: hashlib.sha256(Path(b).read_bytes()).hexdigest() for l, b, _ in builds}, "samples": []}
    for n in a.sections:
        for i in range(a.samples):
            for label, b, j in (builds if i % 2 == 0 else builds[::-1]):
                with tempfile.TemporaryDirectory(prefix=".jo-") as d:
                    row = sample(label, b, j, n, d, a.cols, a.rows)
                row["index"] = i
                res["samples"].append(row)
                print(json.dumps({k: v for k, v in row.items() if k not in ("final_screen", "timeout_screen")}), flush=True)
                a.out.write_text(json.dumps(res, indent=1) + "\n")
    print("== medians")
    for n in a.sections:
        for label, _, _ in builds:
            rows = [r for r in res["samples"] if r["build"] == label and r["sections"] == n]
            med = lambda k: round(statistics.median([r[k] for r in rows if k in r]), 1) if any(k in r for r in rows) else None
            print(n, label, {k: med(k) for k in ("first_mark_ms", "latest_ms", "final_tool_ms", "key_ms", "raw_bytes")})


if __name__ == "__main__":
    main()
