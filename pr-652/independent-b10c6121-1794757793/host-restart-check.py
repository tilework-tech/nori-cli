import sys,subprocess,time,json,os,signal,http.client
from pathlib import Path
sys.path.insert(0,str(Path(__file__).parent)); from tui_driver import Driver
p=Path(__file__).resolve().parent; run=str(p/'interactive/run'); state=json.loads((p/'interactive/home/host/state.json').read_text()); oldtoken=(p/'interactive/home/host/token').read_text(); old=Driver('d2-reattach')
os.kill(state['pid'],signal.SIGKILL); old.expect('RECONNECTING',10)
started=subprocess.check_output([run,'host','--port','17420'],text=True); print(started,flush=True)
newstate=json.loads((p/'interactive/home/host/state.json').read_text()); conn=http.client.HTTPConnection('127.0.0.1',17420,timeout=3); conn.request('GET','/sessions',headers={'Authorization':'Bearer '+oldtoken}); status=conn.getresponse().status
print('old token status:',status,'new pid:',newstate['pid'],'old pid:',state['pid'],flush=True)
assert status==401
fresh=Driver('d2-newhost'); fresh.start([run,'attach'],p); fresh.expect('Sessions 0'); fresh.capture(p/'captures/restarted-host-empty','Sessions 0',True)
target=time.monotonic()+35
while time.monotonic()<target:
 time.sleep(.5)
old.expect('RECONNECTING'); screen=old.screen(); old.capture(p/'captures/host-restart-stuck','RECONNECTING',True)
assert '401' not in screen and 'restart' not in screen.lower(),screen
msg='SHOULD-FIX: original attachment remains RECONNECTING >35 seconds after same-port restart; old bearer gets HTTP401; new attachment shows Sessions 0. Old UI incorrectly says sessions continue on the host.\n'
(p/'host-restart-result.txt').write_text(msg+started+'old-token HTTP '+str(status)+'\n'+screen)
print(msg,flush=True)
