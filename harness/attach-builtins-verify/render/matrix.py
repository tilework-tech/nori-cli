"""Print a markdown PASS/FAIL matrix of check IDs across build result dirs.
usage: python3 -I matrix.py RESULTS_DIR dir1 dir2 ...  (first `*-run/checks.txt` per scenario; repeats counted)"""
import glob, os, re, sys
root, dirs = sys.argv[1], sys.argv[2:]
cells, desc, order = {}, {}, []
for d in dirs:
    for f in sorted(glob.glob(os.path.join(root, d, "*-run*", "checks.txt"))):
        for line in open(f):
            m = re.match(r"(PASS|FAIL) +(\S+) +(.*)", line)
            if not m: continue
            st, cid, text = m.groups()
            if cid not in desc: order.append(cid)
            if cid not in desc or st == "PASS": desc[cid] = re.sub(r" \(.*", "", text)
            c = cells.setdefault((d, cid), [0, 0]); c[0 if st == "PASS" else 1] += 1
print("| Check | " + " | ".join(dirs) + " | What |")
print("|---" * (len(dirs) + 2) + "|")
for cid in order:
    row = []
    for d in dirs:
        p, f = cells.get((d, cid), [0, 0])
        row.append("" if p + f == 0 else ("pass" if f == 0 else "**FAIL**" if p == 0 else f"{p}/{p+f}") + ("" if p + f <= 1 else f" ×{p+f}" if f == 0 or p == 0 else ""))
    print(f"| {cid} | " + " | ".join(row) + f" | {desc[cid]} |")
