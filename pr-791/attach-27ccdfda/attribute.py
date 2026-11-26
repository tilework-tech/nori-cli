"""Attribute perf samples in the first WINDOW ms of each attach run to the
innermost nori_tui frame and to inclusive buckets. usage: attribute.py WINDOW_MS script.txt..."""
import sys, collections, re
window = float(sys.argv[1]) / 1000
inner = collections.Counter(); incl = collections.Counter(); threads = collections.Counter(); total = 0
BUCKETS = ["append_markdown_with_cwd", "highlight_code_to_lines", "draw_full_screen", "insert_history_lines",
           "handle_streaming_delta", "flush_answer_stream", "on_commit_tick", "AgentMarkdownCell", "display_lines",
           "handle_client_tool_snapshot", "deliver_session_event", "TranscriptRows", "wrap_history_lines",
           "serde_json", "joined_feed", "session_stats", "program_status", "session_previews", "follow_foreground",
           "insert_history_cell", "consolidat", "word_wrap", "host_client::connection", "ratatui", "malloc", "free"]
for path in sys.argv[2:]:
    blocks = open(path).read().split("\n\n"); t0 = None
    for b in blocks:
        lines = b.strip().splitlines()
        if not lines: continue
        head = lines[0].split()
        t = float(next(x for x in head if x.endswith(":") and x[:-1].replace(".", "").isdigit())[:-1])
        t0 = t if t0 is None else t0
        if t - t0 > window: continue
        total += 1; threads[head[0]] += 1
        syms = [l.strip().split(None, 1)[1] if len(l.strip().split(None, 1)) > 1 else "" for l in lines[1:]]
        own = next((s for s in syms if "nori_tui" in s), syms[0] if syms else "?")
        inner[re.sub(r"\+0x[0-9a-f]+.*", "", own)[:110]] += 1
        joined = " ".join(syms)
        for k in BUCKETS:
            if k in joined: incl[k] += 1
print("samples", total, "(0.25 ms each at 4 kHz)")
print("threads", threads.most_common(6))
print("-- inclusive buckets"); [print(f"{v:5d} {k}") for k, v in incl.most_common()]
print("-- innermost nori_tui frame"); [print(f"{v:5d} {k}") for k, v in inner.most_common(25)]
