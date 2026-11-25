#!/usr/bin/env python3
"""One line per check whose pass/fail state differs: diffstates.py OLD.json NEW.json"""
import json, sys
load = lambda p: {(r["scenario"], r["check"]): r["passed"] for r in json.load(open(p))}
old, new = load(sys.argv[1]), load(sys.argv[2])
st = lambda v: "absent" if v is None else ("pass" if v else "FAIL")
for k in sorted(set(old) | set(new)):
    if old.get(k) != new.get(k):
        print(f"{k[0]}/{k[1]}: {st(old.get(k))} -> {st(new.get(k))}")
