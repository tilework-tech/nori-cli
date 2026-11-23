#!/usr/bin/env python3
"""Writes synthetic scenarios. All paths and data are invented."""
import json, sys, os

out = sys.argv[1]
n = [0]

def tool(kind, title, raw_input, locations=None, done_output=None, content_text=None, late_meta=None, meta_on_done=None):
    n[0] += 1
    tid = f"call-{n[0]:02d}"
    call = {"sessionUpdate": "tool_call", "toolCallId": tid, "title": title, "kind": kind,
            "status": "in_progress", "rawInput": raw_input}
    if locations:
        call["locations"] = locations
    done = {"sessionUpdate": "tool_call_update", "toolCallId": tid, "status": "completed"}
    if done_output is not None:
        done["rawOutput"] = done_output
    if content_text is not None:
        done["content"] = [{"type": "content", "content": {"type": "text", "text": content_text}}]
    if meta_on_done is not None:
        done["_meta"] = meta_on_done
    steps = [{"update": call}, {"update": done}]
    if late_meta is not None:
        steps.append({"update": {"sessionUpdate": "tool_call_update", "toolCallId": tid, "_meta": late_meta}})
    return steps

def claude(tool_name, resp):
    return {"claudeCode": {"toolName": tool_name, "toolResponse": resp}}

# Scenario A: Claude adapter shapes (rawInput + late PostToolUse report)
a = []
a += tool("read", "Read src/engine/layout.rs", {"file_path": "{cwd}/src/engine/layout.rs", "offset": 180, "limit": 80},
          [{"path": "{cwd}/src/engine/layout.rs", "line": 180}], content_text="(80 lines of source)",
          late_meta=claude("Read", {"type": "text", "file": {"filePath": "{cwd}/src/engine/layout.rs", "content": "...", "numLines": 80, "startLine": 180, "totalLines": 2230}}))
a += tool("read", "Read Cargo.toml", {"file_path": "{cwd}/Cargo.toml"}, [{"path": "{cwd}/Cargo.toml"}],
          content_text="(whole file)",
          late_meta=claude("Read", {"type": "text", "file": {"filePath": "{cwd}/Cargo.toml", "content": "...", "numLines": 64, "startLine": 1, "totalLines": 64}}))
a += tool("search", "grep -c \"render_rows\" src", {"pattern": "render_rows", "path": "{cwd}/src", "output_mode": "count"},
          late_meta=claude("Grep", {"mode": "count", "numFiles": 2, "filenames": [], "content": "src/a.rs:2\nsrc/b.rs:1", "numMatches": 3}))
a += tool("search", "grep \"fn paint\" src", {"pattern": "fn paint", "path": "{cwd}/src", "output_mode": "content", "-n": True},
          content_text="src/paint.rs:12:fn paint_row()\nsrc/paint.rs:40:fn paint_cell()\nsrc/view.rs:7:fn paint_all()",
          late_meta=claude("Grep", {"mode": "content", "numFiles": 0, "filenames": [], "content": "src/paint.rs:12:fn paint_row()\nsrc/paint.rs:40:fn paint_cell()\nsrc/view.rs:7:fn paint_all()", "numLines": 3, "totalLines": 3}))
a += tool("search", "grep \"Widget\"", {"pattern": "Widget", "path": "{cwd}"},
          late_meta=claude("Grep", {"mode": "files_with_matches", "filenames": ["x"] , "numFiles": 250, "totalFiles": 1203, "appliedLimit": 250}))
a += tool("search", "grep -c \"use \" src", {"pattern": "use ", "path": "{cwd}/src", "output_mode": "count", "head_limit": 50},
          late_meta=claude("Grep", {"mode": "count", "numFiles": 60, "filenames": [], "content": "...", "numMatches": 412, "appliedLimit": 50}))
a += tool("search", "Find **/*.rs", {"pattern": "**/*.rs", "path": "{cwd}/crates/tui"},
          late_meta=claude("Glob", {"durationMs": 9, "numFiles": 100, "filenames": [], "truncated": True, "totalMatches": 100, "countIsComplete": False}))
a += tool("search", "grep \"ZZ_NOT_THERE\"", {"pattern": "ZZ_NOT_THERE", "path": "{cwd}/src"},
          late_meta=claude("Grep", {"mode": "files_with_matches", "filenames": [], "numFiles": 0, "totalFiles": 0}))
a += [{"text": "Synthetic Claude-style exploration finished."}]

# Scenario B: other servers, paths, malformed and truncated metadata
b = []
b += tool("search", "Search TODO", {"pattern": "TODO", "path": "{cwd}"})
b += tool("search", "Search config in sibling", {"pattern": "config", "path": "{cwd}-old/src"})
b += tool("read", "Read notes.md", {"file_path": "/srv/shared/notes.md"}, done_output={"lines": 12})
b += tool("search", "rg TODO src", {"query": "TODO", "path": "{cwd}/src"},
          content_text="src/a.rs:1: TODO one\nsrc/a.rs:9: TODO two\n42 matches in 7 files")
b += tool("search", "grep (opencode)", {"pattern": "panic!", "path": "{cwd}/src"},
          done_output={"output": "...", "metadata": {"matches": 0, "truncated": False}})
b += tool("search", "glob (opencode)", {"pattern": "*.toml", "path": "{cwd}"},
          done_output={"output": "...", "metadata": {"count": 17, "truncated": True}})
b += tool("read", "Read bad meta", {"file_path": "{cwd}/src/lib.rs", "offset": 30, "limit": 10},
          late_meta=claude("Read", {"type": "text", "file": {"startLine": "abc", "numLines": -3, "totalLines": 1.5}}))
b += tool("read", "Read truncated meta", {"file_path": "{cwd}/src/main.rs"},
          late_meta={"claudeCode": {"toolName": "Read"}})
b += tool("search", "grep null resp", {"pattern": "x", "path": "{cwd}/src"},
          late_meta={"claudeCode": {"toolName": "Grep", "toolResponse": None}})
b += tool("search", "grep neg", {"pattern": "y", "path": "{cwd}/src"},
          late_meta=claude("Grep", {"mode": "count", "numMatches": -5, "numFiles": "two"}))
b += tool("read", "Read zero line", {"file_path": "{cwd}/src/zero.rs", "offset": 0, "limit": 0},
          [{"path": "{cwd}/src/zero.rs", "line": 0}])
b += tool("read", "Read start-only", {"file_path": "{cwd}/src/tail.rs", "offset": 500})
b += [{"text": "Synthetic mixed-server exploration finished."}]


# Scenario D (recheck at 27b37e17): exact Claude Code 0.3.321 Grep/Glob report shapes
d = []
G = lambda pat, sub, extra, resp: tool("search", f"grep {pat}", dict({"pattern": pat, "path": "{cwd}/" + sub}, **extra), late_meta=claude("Grep", resp))
d += G("fn_paint", "src", {"output_mode": "content"}, {"mode": "content", "numFiles": 0, "filenames": [], "content": "...", "numLines": 3, "totalLines": 3})
d += G("draw_cell", "src", {"output_mode": "content", "-C": 2, "head_limit": 12}, {"mode": "content", "numFiles": 0, "filenames": [], "content": "...", "numLines": 12, "totalLines": 40, "appliedLimit": 12})
d += G("past_end", "src", {"output_mode": "content", "offset": 30}, {"mode": "content", "numFiles": 0, "filenames": [], "content": "", "numLines": 0, "totalLines": 25, "appliedOffset": 30})
d += G("ZZ_NONE_C", "src", {"output_mode": "content"}, {"mode": "content", "numFiles": 0, "filenames": [], "content": "", "numLines": 0, "totalLines": 0})
d += G("ZZ_NONE_N", "src", {"output_mode": "count"}, {"mode": "count", "numFiles": 0, "filenames": [], "content": "", "numMatches": 0})
d += G("use_decl", "src", {"output_mode": "count", "head_limit": 50}, {"mode": "count", "numFiles": 60, "filenames": [], "content": "...", "numMatches": 412, "appliedLimit": 50})
d += G("Widget", "lib", {"head_limit": 250}, {"mode": "files_with_matches", "filenames": ["x"], "numFiles": 250, "totalFiles": 1203, "appliedLimit": 250})
d += G("Widget2", "lib", {"offset": 50}, {"mode": "files_with_matches", "filenames": [], "numFiles": 0, "totalFiles": 40, "appliedOffset": 50})
d += G("ZZ_NONE_F", "lib", {}, {"mode": "files_with_matches", "filenames": [], "numFiles": 0, "totalFiles": 0})
d += G("slow_ctx", "vendor", {"output_mode": "content"}, {"mode": "content", "numFiles": 0, "filenames": [], "content": "...", "numLines": 7, "totalLines": 7, "timedOut": True})
d += G("slow_count", "vendor", {"output_mode": "count"}, {"mode": "count", "numFiles": 9, "filenames": [], "content": "...", "numMatches": 88, "timedOut": True})
d += G("slow_files", "vendor", {}, {"mode": "files_with_matches", "filenames": ["x"], "numFiles": 14, "totalFiles": 14, "timedOut": True})
d += G("slow_none", "vendor", {}, {"mode": "files_with_matches", "filenames": [], "numFiles": 0, "totalFiles": 0, "timedOut": True})
d += G("legacy", "lib", {}, {"filenames": ["x"], "numFiles": 5, "appliedLimit": 5})
L = lambda pat, sub, resp: tool("search", f"Find {pat}", {"pattern": pat, "path": "{cwd}/" + sub}, late_meta=claude("Glob", resp))
d += L("**/*.rs", "crates", {"durationMs": 9, "numFiles": 100, "filenames": [], "truncated": True, "totalMatches": 100, "countIsComplete": False})
d += L("**/*.snap", "crates", {"durationMs": 30001, "numFiles": 30, "filenames": [], "truncated": False, "totalMatches": 30, "timedOut": True})
d += L("**/*.orig", "crates", {"durationMs": 30001, "numFiles": 0, "filenames": [], "truncated": False, "timedOut": True})
d += L("**/*.bak", "crates", {"durationMs": 4, "numFiles": 0, "filenames": [], "truncated": False, "totalMatches": 0})
d += [{"text": "Synthetic Grep/Glob recheck finished."}]

os.makedirs(out, exist_ok=True)
for name, steps in (("a", a), ("b", b), ("d", d)):
    with open(os.path.join(out, f"scenario-{name}.json"), "w") as f:
        json.dump(steps, f, indent=1)
print("ok")
