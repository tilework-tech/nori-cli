import sys,subprocess,time,json
from pathlib import Path
sys.path.insert(0,str(Path(__file__).parent)); from tui_driver import Driver
p=Path(__file__).resolve().parent; run=str(p/'interactive/run'); a=Driver('d2-reattach')
a.send('/new'); a.expect('ready'); a.send('NET baseline'); a.expect('D2 independent answer'); time.sleep(.4)
id=subprocess.check_output([run,'host','ls'],text=True).splitlines()[-1].split()[0]
endpoint=json.loads((p/'proxy-ready.json').read_text())['endpoint']; w=Driver('d2-network'); w.start([run,'attach',id,'--host',endpoint,'--token-file',str(p/'interactive/home/host/token')],p); w.expect('WATCHING'); w.expect('NET baseline')
for num,mode in enumerate(['drop','blackhole'],1):
 before=w.screen().count('D2 independent answer'); (p/'proxy.control').write_text(mode)
 t=time.monotonic(); w.expect('RECONNECTING',25); elapsed=time.monotonic()-t
 a.send('NET missed '+mode); a.expect('NET missed '+mode); time.sleep(.5)
 assert 'NET missed '+mode not in w.screen()
 w.capture(p/('captures/'+mode+'-reconnecting'),'RECONNECTING',True)
 (p/'proxy.control').write_text('forward'); w.expect('WATCHING',20); w.expect('NET missed '+mode,20); time.sleep(.5)
 screen=w.screen(); assert screen.count('D2 independent answer')==before+1,(before,screen); assert screen.count('NET missed '+mode)==1,screen
 w.capture(p/('captures/'+mode+'-recovered'),'NET missed '+mode,True)
 with (p/'network-result.txt').open('a') as f: f.write(f'PASS {mode}: detected in {elapsed:.1f}s; prompt and answer replayed exactly once; answer count {before}->{before+1}.\n')
 print('PASS',mode,'detected',elapsed,flush=True)
w.key('M-t'); w.expect('CONTROLLING'); a.expect('WATCHING'); w.send('NET after recovery'); w.expect('NET after recovery'); a.expect('NET after recovery'); w.capture(p/'captures/network-resumed-prompt','NET after recovery',True)
print('PASS recovered socket can take control and prompt',flush=True)
