import subprocess, sys
repo, mapf, outf = sys.argv[1:4]
T = b"\nCo-authored-by: Clifford Ressel <6520751+CSRessel@users.noreply.github.com>"
def git(*a): return subprocess.run(["git","-C",repo,*a],capture_output=True,check=True).stdout
rows=[l.split("\t") for l in open(mapf).read().splitlines()]
m={o:n for o,n,_ in rows}; fails=0; out=[]
def parse(raw):
    h,msg=raw.split(b"\n\n",1); d={}; cur=None
    for ln in h.split(b"\n"):
        if ln.startswith(b" "): d[cur][-1]+=b"\n"+ln; continue
        k,v=ln.split(b" ",1); cur=k.decode(); d.setdefault(cur,[]).append(v)
    return d,msg
for o,n,_ in rows:
    do,mo=parse(git("cat-file","commit",o)); dn,mn=parse(git("cat-file","commit",n))
    checks={
     "tree": do["tree"]==dn["tree"],
     "author": do["author"]==dn["author"],
     "committer": do["committer"]==dn["committer"],
     "parents": [m.get(p.decode(),p.decode()).encode() for p in do["parent"]]==dn["parent"],
     "headers": set(do)-{"gpgsig"}==set(dn) and "gpgsig" in do and "gpgsig" not in dn,
     "message": mn==mo+T,
     "one-coauthor-added": mn.count(b"Co-authored-by:")==mo.count(b"Co-authored-by:")+1,
    }
    bad=[k for k,v in checks.items() if not v]; fails+=len(bad)
    subj=mo.split(b"\n",1)[0].decode(); pr=subj.rsplit("(#",1)[1].rstrip(")")
    out.append("\t".join([pr,o,n,do["tree"][0].decode(),do["author"][0].decode().rsplit(" ",2)[1]+" "+do["author"][0].decode().rsplit(" ",1)[1],do["committer"][0].decode().rsplit(" ",2)[1]+" "+do["committer"][0].decode().rsplit(" ",1)[1],"ok" if not bad else "FAIL:"+",".join(bad)]))
open(outf,"w").write("pr\told_sha\tnew_sha\ttree\tauthor_ts\tcommitter_ts\tverify\n"+"\n".join(out)+"\n")
print("failures:",fails)
