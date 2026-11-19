import asyncio, json, tempfile, pathlib, os, signal, shutil, threading, time, sys
from http.server import ThreadingHTTPServer, BaseHTTPRequestHandler
ROOT=pathlib.Path.home()/'.nori/npm/cli-runtime/node_modules'
class Mock(BaseHTTPRequestHandler):
    def log_message(self,*args): pass
    def do_GET(self):
        self.send_response(200); self.send_header('content-type','application/json');self.end_headers();self.wfile.write(b'{"models":[],"data":[]}')
    def do_POST(self):
        data=self.rfile.read(int(self.headers.get('content-length',0)))
        if self.headers.get('content-encoding')=='zstd':
            import subprocess
            try: data=subprocess.check_output(['zstd','-d','-c'],input=data)
            except Exception: pass
        req=json.loads(data) if data.startswith(b'{') else {}
        print('http',self.path,'base' if b'BASE_MARKER' in data else '', 'follow' if b'FOLLOW_MARKER' in data else '',flush=True)
        if 'messages' not in self.path and 'responses' not in self.path:
            self.send_response(200); self.end_headers(); self.wfile.write(b'{}'); return
        if "responses" in self.path or req.get("tools"):
            self.server.got.set()
            self.server.release.wait(15)
        self.send_response(200); self.send_header('content-type','text/event-stream');self.end_headers()
        def evt(t,d):
            self.wfile.write(('event: '+t+'\ndata: '+json.dumps(dict(d,type=t))+'\n\n').encode())
        try:
            if 'messages' in self.path:
                evt('message_start',{'message':dict(id='msg_demo',type='message',role='assistant',model=req.get('model','demo'),content=[],stop_reason=None,usage={'input_tokens':1,'output_tokens':0})})
                evt('content_block_start',dict(index=0,content_block=dict(type='text',text='')))
                evt('content_block_delta',dict(index=0,delta=dict(type='text_delta',text=('SYNTHETIC_CORRECTED_REPLY' if b'FOLLOW_MARKER' in data else 'SYNTHETIC_REPLY'))))
                evt('content_block_stop',dict(index=0))
                evt('message_delta',dict(delta=dict(stop_reason='end_turn',stop_sequence=None),usage={'output_tokens':1}))
                evt('message_stop',{})
            else:
                msg={'id':'msg_demo','type':'message','role':'assistant','status':'completed','content':[{'type':'output_text','text':'SYNTHETIC_REPLY','annotations':[]}]}
                evt('response.created',{'response':{'id':'resp_demo','status':'in_progress','output':[]}})
                evt('response.output_item.added',{'output_index':0,'item':dict(msg,content=[])})
                evt('response.output_text.delta',{'output_index':0,'content_index':0,'item_id':'msg_demo','delta':'SYNTHETIC_REPLY'})
                evt('response.output_item.done',{'output_index':0,'item':msg})
                evt('response.completed',{'response':{'id':'resp_demo','status':'completed','output':[msg],'usage':{'input_tokens':1,'output_tokens':1,'total_tokens':2}}})
        except (BrokenPipeError, ConnectionResetError): pass
async def test(agent):
    with tempfile.TemporaryDirectory() as temp:
        work=pathlib.Path(temp)/'work';work.mkdir()
        (pathlib.Path(temp)/"codex").mkdir()
        server=ThreadingHTTPServer(('127.0.0.1',0),Mock);server.got=threading.Event();server.release=threading.Event()
        threading.Thread(target=server.serve_forever,daemon=True).start()
        origin='http://127.0.0.1:'+str(server.server_port)
        env=dict(PATH=os.environ['PATH'],HOME=temp,CODEX_HOME=temp+'/codex',CLAUDE_CONFIG_DIR=temp+'/claude',ANTHROPIC_API_KEY='synthetic-key',OPENAI_API_KEY='synthetic-key',CODEX_API_KEY='synthetic-key',OPENAI_BASE_URL=origin+'/v1',ANTHROPIC_BASE_URL=origin,HTTP_PROXY='http://127.0.0.1:1',HTTPS_PROXY='http://127.0.0.1:1',ALL_PROXY='http://127.0.0.1:1',NO_PROXY='127.0.0.1,localhost',CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC='1',CODEX_PATH=str(ROOT/'.bin/codex'))
        env['CODEX_CONFIG']=json.dumps(dict(model='gpt-5.4',model_provider='localfixture',model_providers={'localfixture':dict(name='fixture',base_url=origin+'/v1',wire_api='responses',env_key='OPENAI_API_KEY')}))
        proc=await asyncio.create_subprocess_exec(shutil.which('bun'),str(ROOT/'@agentclientprotocol'/agent/'dist/index.js'),env=env,cwd=work,stdin=-1,stdout=-1,stderr=-1,start_new_session=True)
        pending={}
        def send(m):
            proc.stdin.write((json.dumps(dict(jsonrpc='2.0',**m))+'\n').encode())
        def req(i,name,params):
            f=asyncio.get_running_loop().create_future(); pending[i]=f
            send(dict(id=i,method=name,params=params)); return f
        async def reader():
            while line:=await proc.stdout.readline():
                m=json.loads(line)
                if 'result' in m and m.get('id') in (4,5,7): print('wire reply id', m['id'], flush=True)
                if 'method' in m:
                    if 'id' in m: print('request',m['method'],flush=True);send(dict(id=m['id'],error={'code':-32601,'message':'unimplemented'}))
                    elif m['method']=='session/update':
                        u=m['params']['update']; typ=u.get('sessionUpdate')
                        if typ in ('agent_message_chunk','user_message_chunk','tool_call'): print('update',typ,str(u.get('content'))[:120],flush=True)
                elif m.get('id') in pending:
                    f=pending.pop(m['id'])
                    if not f.done():
                        if 'error' in m: f.set_exception(RuntimeError(str(m['error'])[:400]))
                        else: f.set_result(m['result'])
        rd=asyncio.create_task(reader())
        async def errs():
            while line:=await proc.stderr.readline():
                if 'error' in line.decode(errors='ignore').lower(): print('stderr',line.decode(errors='ignore').strip()[:200],flush=True)
        er=asyncio.create_task(errs())
        try:
            r=await asyncio.wait_for(req(1,'initialize',dict(protocolVersion=1,clientCapabilities={})),12);print('init',agent,r.get('agentInfo'),r.get('_meta'), [(a.get('id'),a.get('type')) for a in r.get('authMethods',[])],flush=True)
            if agent=='codex-acp':
                a=next((a['id'] for a in r.get('authMethods',[]) if 'api' in a['id']),None)
                if a: print('auth',await asyncio.wait_for(req(9,'authenticate',dict(methodId=a)),8),flush=True)
            r=await asyncio.wait_for(req(2,'session/new',dict(cwd=str(work),mcpServers=[])),20); sid=r['sessionId']; print('new ok',flush=True)
            def content(t):return dict(sessionId=sid,prompt=[dict(type='text',text=t)])
            guarded=dict(**content('FOLLOW_MARKER'),_meta={'steering':{'idleBehavior':'promptRequired'}})
            print('idle',await req(3,'_session/steering',guarded),flush=True)
            p=req(4,'session/prompt',content('BASE_MARKER'))
            reached=await asyncio.to_thread(server.got.wait,8)
            print('at HTTP gate',reached,'original open',not p.done(),flush=True)
            print('busy',await asyncio.wait_for(req(5,'_session/steering',guarded),8),'original open',not p.done(),flush=True)
            server.release.set()
            print('original',await asyncio.wait_for(p,18),flush=True)
            print('after',await req(6,'_session/steering',guarded),flush=True)
            print('host-owned prompt', (await asyncio.wait_for(req(7,'session/prompt',content('FOLLOW_MARKER')),12))['stopReason'],flush=True)
            print('unguarded idle',await asyncio.wait_for(req(8,'_session/steering',content('LEGACY_MARKER')),12),flush=True)
        finally:
            server.release.set()
            try:os.killpg(proc.pid,signal.SIGKILL)
            except ProcessLookupError: pass
            await proc.wait(); rd.cancel();er.cancel();server.shutdown();server.server_close()
asyncio.run(test(sys.argv[1]))
