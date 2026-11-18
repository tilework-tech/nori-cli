import subprocess, sys
repo, base, tip, out = sys.argv[1:5]
TRAILER = b"Co-authored-by: Clifford Ressel <6520751+CSRessel@users.noreply.github.com>"
def git(*a, inp=None):
    return subprocess.run(["git", "-C", repo, *a], input=inp, capture_output=True, check=True).stdout
commits = git("rev-list", "--reverse", "--topo-order", f"{base}..{tip}").decode().split()
mapping = {}
rows = []
for old in commits:
    raw = git("cat-file", "commit", old)
    hdr, msg = raw.split(b"\n\n", 1)
    lines, skip, parents = [], False, []
    for ln in hdr.split(b"\n"):
        if skip and ln.startswith(b" "):
            continue
        skip = False
        if ln.startswith(b"gpgsig ") or ln.startswith(b"gpgsig-sha256 "):
            skip = True; continue
        if ln.startswith(b"parent "):
            p = ln[7:].decode(); np = mapping.get(p, p); parents.append((p, np))
            ln = b"parent " + np.encode()
        lines.append(ln)
    low = msg.lower()
    if b"6520751+csressel@users.noreply.github.com" in low or b"<csressel@" in low or b"clifford@tilework.tech" in low or b"clifford@noriagentic.com" in low:
        action = "skip-already-credited"; newmsg = msg
    else:
        assert not msg.endswith(b"\n") and msg.rsplit(b"\n", 1)[-1].startswith(b"Co-authored-by: "), old
        newmsg = msg + b"\n" + TRAILER; action = "trailer-added"
    newraw = b"\n".join(lines) + b"\n\n" + newmsg
    new = git("hash-object", "-t", "commit", "-w", "--stdin", inp=newraw).decode().strip()
    mapping[old] = new
    rows.append((old, new, action))
with open(out, "w") as f:
    for r in rows: f.write("\t".join(r) + "\n")
print(mapping[commits[-1]])
