import html, re, sys
F = sys.argv[1]
def ansi(path, start, end):
    lines = open(path, encoding="utf-8", errors="replace").read().split("\n")
    s = next(i for i, l in enumerate(lines) if start in re.sub(r"\x1b\[[0-9;]*m", "", l))
    e = next(i for i in range(s, len(lines)) if end in re.sub(r"\x1b\[[0-9;]*m", "", lines[i]))
    out = []
    for l in lines[s:e + 1]:
        if not re.sub(r"\x1b\[[0-9;]*m", "", l).strip():
            continue
        st = set(); buf = ""
        for tok in re.split(r"(\x1b\[[0-9;]*m)", l):
            m = re.fullmatch(r"\x1b\[([0-9;]*)m", tok)
            if m:
                for c in (m.group(1) or "0").split(";"):
                    if c in ("0", ""): st.clear()
                    elif c in ("1", "2", "3"): st.add(c)
                    elif c == "22": st.discard("1"); st.discard("2")
                    elif c == "23": st.discard("3")
                continue
            if tok:
                css = ("font-weight:bold;" if "1" in st else "") + ("opacity:.6;" if "2" in st else "") + ("font-style:italic;" if "3" in st else "")
                buf += f'<span style="{css}">{html.escape(tok)}</span>'
        out.append(buf.rstrip())
    return "\n".join(out)
def txt(path, start, end):
    lines = [l.rstrip() for l in open(path, encoding="utf-8").read().split("\n")]
    s = next(i for i, l in enumerate(lines) if start in l)
    e = next(i for i in range(s, len(lines)) if end in lines[i])
    return html.escape("\n".join(l for l in lines[s:e + 1] if l.strip() and not set(l.strip()) <= set("─")))
O, N = F + "/out-656b488", F + "/out"
def panel(t, body, cls=""):
    return f'<div class="p {cls}"><h3>{t}</h3><pre>{body}</pre></div>'
rows = [
 ("1. Token-streamed bold (the <code>**</code> marker arrives as its own chunk)",
  [panel("656b488a (superseded): paragraph invented before closing ** (its ┊ gutter does not render in this font)", ansi(O+"/live/final.ansi","› tokemph","Twohead scenario answered"),"bad"),
   panel("cccd32a3: text kept exactly as supplied", ansi(N+"/live/final.ansi","› tokemph","Twohead scenario answered"),"good")]),
 ("2. The same thought block on every surface at cccd32a3",
  [panel("Live TUI", txt(N+"/live/final.txt","› mixed","Startturn scenario answered"),"good"),
   panel("Replay (nori resume)", txt(N+"/replay/final.txt","› mixed","Startturn scenario answered"),"good"),
   panel("nori attach (fresh attach)", txt(N+"/reattach/tall.txt","mixed","Startturn scenario answered"),"good")]),
 ("3. Finding: whole Gemini-style thoughts with no separator run together in history",
  [panel("Working indicator during the turn (cccd32a3)", txt(N+"/live/gemini-indicator.txt","Planning the alias change (","Planning the alias change ("),"good"),
   panel("History block after the turn (cccd32a3)", ansi(N+"/live/final.ansi","› gemini","Gemini scenario answered"),"warn")]),
 ("4. Opt-out: [tui] thoughts_in_history = false at cccd32a3",
  [panel("Live: working indicator still shows the thought", txt(N+"/optout-live/twohead-indicator.txt","Planning the alias change (","Planning the alias change ("),"good"),
   panel("Live history after the turns: no thought blocks", txt(N+"/optout-live/final.txt","› tokemph","Twohead scenario answered"),"good")]),
]
body = "".join(f'<h2>{h}</h2><div class="row">{"".join(ps)}</div>' for h, ps in rows)
open(F + "/proof/sheet.html", "w").write(f"""<!doctype html><meta charset=utf-8><style>
body{{background:#0f1311;color:#d7e0da;font:14px system-ui;margin:24px;width:1500px}}
h1{{font-size:20px;margin:0 0 4px}} .sub{{color:#8fa39a;margin-bottom:12px}} h2{{font-size:15px;margin:20px 0 8px;color:#9fe0b5}}
.row{{display:flex;gap:12px}} .p{{flex:1;background:#151b18;border:1px solid #2a332e;border-radius:6px;padding:8px 10px;min-width:0}}
.p h3{{font-size:12px;margin:0 0 6px;color:#aab8b0}} .bad{{border-color:#a84a4a}} .good{{border-color:#3f8f5a}} .warn{{border-color:#b89a3a}}
pre{{margin:0;font:12px/1.35 'DejaVu Sans Mono',monospace;white-space:pre-wrap}}</style>
<h1>nori-cli #733: thought text in history, verified at cccd32a3</h1>
<div class=sub>Real TUI driven in tmux with a synthetic ACP agent. All text is synthetic. Stack base: #730 c69d9d1c.</div>{body}""")
