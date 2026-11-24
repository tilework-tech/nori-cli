#!/usr/bin/env python3
"""Compare two results.json files check by check: python3 -I harness/compare.py OLD NEW"""
import json, sys
old = {(r["scenario"], r["check"]): r for r in json.load(open(sys.argv[1]))}
new = {(r["scenario"], r["check"]): r for r in json.load(open(sys.argv[2]))}
for key in sorted(set(old) | set(new)):
    o, n = old.get(key), new.get(key)
    if o and n and o["passed"] == n["passed"]:
        continue
    state = lambda r: "absent" if r is None else ("pass" if r["passed"] else "FAIL")
    print(f"{key[0]}/{key[1]}: {state(o)} -> {state(n)}  {(n or o)['detail'][:200]}")
