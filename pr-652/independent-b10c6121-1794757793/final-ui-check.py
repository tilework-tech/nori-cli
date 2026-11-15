import sys,subprocess,time
from pathlib import Path
sys.path.insert(0,str(Path(__file__).parent)); from tui_driver import Driver
p=Path(__file__).resolve().parent; run=str(p/'interactive/run'); a=Driver('d2-newhost')
a.send('/new'); a.expect('ready'); a.send('D2 final lifecycle'); a.expect('D2 independent answer'); time.sleep(.4)
b=Driver('d2-plain'); b.start([run,'attach'],p); b.expect('Sessions 1'); b.expect('D2 final lifecycle'); b.capture(p/'captures/plain-attach-nonempty','Sessions 1',True); b.key('C-d')
a.send('/close'); a.expect('closed'); a.capture(p/'captures/last-session-closed','closed',True)
s=Driver('d2-shell'); s.send('clear'); s.send(run+' host ls'); s.expect('closed'); s.send(run+' host stop'); s.expect('Host stopped.'); s.send(run+' host ls'); s.expect('no host running'); s.capture(p/'captures/host-stop','Host stopped.')
for name in ['d2-newhost','d2-reattach','d2-network']:
 try: Driver(name).key('C-d')
 except Exception: pass
(p/'proxy.control').write_text('stop')
print('PASS plain nonempty attach; close last session leaves host alive; host stop exits',flush=True)
