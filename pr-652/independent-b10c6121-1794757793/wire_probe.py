#!/usr/bin/env python3
"""Independent D2 black-box probes. Only scratch output and isolated host homes."""
import argparse
import base64
import hashlib
import json
import os
from pathlib import Path
import signal
import socket
import struct
import subprocess
import time
import traceback
from urllib.parse import urlsplit

P = argparse.ArgumentParser()
P.add_argument('--binary', required=True)
P.add_argument('--mock', required=True)
P.add_argument('--out', required=True)
ARGS = P.parse_args()
OUT = Path(ARGS.out).resolve()
OUT.mkdir(parents=True, exist_ok=True)
LOG = (OUT / 'wire.jsonl').open('w')
RESULTS = []


def log(kind, **data):
    LOG.write(json.dumps(dict(time=time.time(), kind=kind, **data)) + '\n')
    LOG.flush()


def check(name, condition, details=''):
    RESULTS.append(dict(name=name, passed=bool(condition), details=details))
    print(('PASS ' if condition else 'FAIL ') + name + (' ' + str(details) if details else ''), flush=True)
    log('assertion', **RESULTS[-1])
    (OUT / 'results.json').write_text(json.dumps(RESULTS, indent=2))


class WS:
    """Minimal standard-library RFC6455 client, with bounded exact reads."""
    def __init__(self, endpoint, token=None, name='client'):
        self.name, self.messages, self.ident = name, [], 0
        url = urlsplit(endpoint)
        self.sock = socket.create_connection((url.hostname, url.port), timeout=8)
        self.sock.settimeout(12)
        nonce = base64.b64encode(os.urandom(16)).decode()
        headers = [f'GET {url.path} HTTP/1.1', f'Host: {url.netloc}', 'Upgrade: websocket',
                   'Connection: Upgrade', f'Sec-WebSocket-Key: {nonce}', 'Sec-WebSocket-Version: 13']
        if token is not None:
            headers.append('Authorization: Bearer ' + token)
        self.sock.sendall(('\r\n'.join(headers) + '\r\n\r\n').encode())
        data = b''
        while b'\r\n\r\n' not in data:
            data += self.sock.recv(1)
            if len(data) > 65536:
                raise RuntimeError('HTTP header exceeded limit')
        self.status = int(data.split(b' ')[1])
        log('handshake', client=name, path=url.path, status=self.status,
            auth='absent' if token is None else 'provided')
        if self.status == 101:
            accept = base64.b64encode(hashlib.sha1((nonce + '258EAFA5-E914-47DA-95CA-C5AB0DC85B11').encode()).digest())
            assert accept in data, 'invalid WebSocket accept'

    def exact(self, size):
        data = b''
        while len(data) < size:
            chunk = self.sock.recv(size - len(data))
            if not chunk:
                raise EOFError(self.name + ' socket closed')
            data += chunk
        return data

    def frame(self, payload, opcode=1):
        mask = os.urandom(4)
        size = len(payload)
        prefix = bytes([128 | opcode])
        if size < 126:
            prefix += bytes([128 | size])
        elif size < 65536:
            prefix += bytes([128 | 126]) + struct.pack('!H', size)
        else:
            prefix += bytes([128 | 127]) + struct.pack('!Q', size)
        self.sock.sendall(prefix + mask + bytes(v ^ mask[i % 4] for i, v in enumerate(payload)))

    def recv(self):
        fragments = b''
        while True:
            first, second = self.exact(2)
            opcode, length = first & 15, second & 127
            if length == 126:
                length = struct.unpack('!H', self.exact(2))[0]
            elif length == 127:
                length = struct.unpack('!Q', self.exact(8))[0]
            assert not second & 128, 'server frame must not be masked'
            payload = self.exact(length)
            if opcode == 8:
                raise EOFError(self.name + ' received Close')
            if opcode == 9:
                self.frame(payload, 10)
                continue
            if opcode == 10:
                continue
            fragments += payload
            if first & 128:
                message = json.loads(fragments)
                self.messages.append(message)
                log('receive', client=self.name, message=message)
                return message

    def request(self, method, params=None):
        self.ident += 1
        req = dict(jsonrpc='2.0', id=self.ident, method=method, params=params or {})
        log('send', client=self.name, message=req)
        self.frame(json.dumps(req).encode())
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            response = self.recv()
            if response.get('id') == self.ident:
                return response
        raise TimeoutError(method)

    def ok(self, method, params=None):
        reply = self.request(method, params)
        assert 'result' in reply, reply
        return reply['result']

    def close(self):
        try:
            self.frame(b'', 8)
        except OSError:
            pass
        self.sock.close()


class Host:
    def __init__(self, name):
        self.home = OUT / name
        self.home.mkdir(exist_ok=True)
        self.cwd = self.home / 'project'
        self.cwd.mkdir(exist_ok=True)
        (self.home / 'config.toml').write_text(
            'agent = "mock-model"\n[features]\nmulti_session = true\n' +
            '[projects.' + json.dumps(str(self.cwd)) + ']\ntrust_level = "trusted"\n')
        self.env = dict(os.environ, NORI_HOME=str(self.home), NORI_NO_ANALYTICS='1',
                        MOCK_ACP_AGENT_BIN=str(Path(ARGS.mock).resolve()), MOCK_AGENT_RESPONSE='WIRE_PROBE_RESPONSE')
        self.env['PATH'] = str(Path(ARGS.mock).resolve().parent) + ':' + self.env.get('PATH', '')
        self.clients = []

    def command(self, *args):
        result = subprocess.run([str(Path(ARGS.binary).resolve()), *args], env=self.env,
                                cwd=self.cwd, capture_output=True, text=True, timeout=40)
        log('command', argv=list(args), code=result.returncode, stdout=result.stdout, stderr=result.stderr)
        return result

    def state(self):
        return json.loads((self.home / 'host/state.json').read_text())

    def token(self):
        return (self.home / 'host/token').read_text().strip()

    def client(self, name, observe=False):
        c = WS(self.state()['endpoint'], self.token(), name)
        assert c.status == 101
        self.clients.append(c)
        role = c.ok('initialize', dict(observe=observe))['control']['role']
        return c, role

    def clean(self):
        for c in self.clients:
            c.close()
        self.command('host', 'stop')


def wait_status(client, sid, status, timeout=20):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        rows = client.ok('_nori/session/list')['sessions']
        row = next((r for r in rows if r['session_id'] == sid), None)
        if row and row['status'] == status:
            return row
        time.sleep(.1)
    raise TimeoutError(f'{sid} did not reach {status}: {rows}')


def refused(client, method, params):
    result = client.request(method, params)
    return result.get('error', {}).get('code') == -32016, result


def run():
    h = Host('wire-home')
    try:
        started = h.command('host', '--port', '0')
        check('host starts detached', started.returncode == 0, started.stdout.strip())
        assert started.returncode == 0, started.stderr
        initial = h.state()
        duplicate = h.command('host', '--port', '0')
        check('duplicate start retains same PID and endpoint', duplicate.returncode == 0 and h.state() == initial)
        check('token and host directory permissions', (h.home / 'host/token').stat().st_mode & 0o777 == 0o600 and (h.home / 'host').stat().st_mode & 0o777 == 0o700)
        check('startup does not print token', h.token() not in started.stdout + started.stderr)
        for name, token, path, expected in [('missing', None, '/sessions', 401), ('wrong', 'wrong', '/sessions', 401), ('legacy-valid', h.token(), '/acp', 404), ('legacy-missing', None, '/acp', 404)]:
            c = WS(initial['endpoint'].replace('/sessions', path), token, name)
            check('auth ' + name, c.status == expected, f'HTTP {c.status}')
            c.sock.close()

        admin, role = h.client('admin', observe=True)
        check('admin observe does not claim control', role == 'watcher')
        a, role = h.client('A')
        check('first interactive client controls', role == 'controller')
        b, role = h.client('B')
        check('second client watches', role == 'watcher')
        c, role = h.client('C')
        check('third client watches', role == 'watcher')
        sid = a.ok('_nori/session/new')['session_id']
        wait_status(admin, sid, 'ready')
        for client in [a, b, c]:
            client.ok('_nori/session/subscribe', {'session_id': sid, 'cursors': {sid: 0}})
        denied, reply = refused(b, '_nori/session/prompt', {'session_id': sid, 'prompt': [{'type': 'text', 'text': 'WATCHER_MUST_NOT_EXECUTE'}]})
        check('watcher prompt refused on wire', denied, reply)
        for method in ['new', 'close', 'cancel', 'resume']:
            denied, reply = refused(c, '_nori/session/' + method, {'session_id': sid})
            check('third client cannot ' + method, denied, reply)

        a.ok('_nori/session/prompt', {'session_id': sid, 'prompt': [{'type': 'text', 'text': 'mock:stream-until-cancel MIDTURN_A'}]})
        wait_status(admin, sid, 'working')
        check('B takes control during active turn', b.ok('_nori/control/take')['role'] == 'controller')
        check('former controller sees watcher role mid-turn', a.ok('_nori/control/status')['role'] == 'watcher')
        check('former controller receives role notification', any(m.get('method') == '_nori/control/changed' and m['params']['role'] == 'watcher' for m in a.messages))
        denied, reply = refused(a, '_nori/session/cancel', {'session_id': sid})
        check('former controller cannot cancel admitted turn', denied, reply)
        b.ok('_nori/session/cancel', {'session_id': sid})
        wait_status(admin, sid, 'ready')
        check('new controller can cancel former controller turn', True)

        b.ok('_nori/session/prompt', {'session_id': sid, 'prompt': [{'type': 'text', 'text': 'mock:request-permission:echo HANDOFF_PERMISSION'}]})
        pending = wait_status(admin, sid, 'waiting_on_you')
        request_id = pending['pending_requests'][0]
        answer = {'session_id': sid, 'request_id': request_id, 'response': {'outcome': {'outcome': 'selected', 'optionId': 'allow'}}}
        denied, reply = refused(a, '_nori/session/respond', answer)
        check('watcher permission answer refused on wire', denied, reply)
        check('permission remains pending after refused watcher answer', wait_status(admin, sid, 'waiting_on_you')['pending_requests'] == [request_id])
        check('C takes control with permission pending', c.ok('_nori/control/take')['role'] == 'controller')
        denied, reply = refused(b, '_nori/session/respond', answer)
        check('former controller cannot answer after permission takeover', denied, reply)
        c.ok('_nori/session/respond', answer)
        wait_status(admin, sid, 'ready')
        check('new controller resolves same pending permission', True)

        for client in [a, b, c]:
            client.ok('_nori/session/list')
        seqs = [[m['params']['seq'] for m in cl.messages if m.get('method') == '_nori/session/event' and m['params']['conversation_id'] == sid] for cl in [a, b, c]]
        check('three clients receive contiguous nonduplicated envelope prefixes', all(s and s == list(range(1, s[-1] + 1)) for s in seqs), [len(s) for s in seqs])
        cursor = seqs[0][-1]
        a.close()
        h.clients.remove(a)
        c.ok('_nori/session/prompt', {'session_id': sid, 'prompt': [{'type': 'text', 'text': 'AFTER_WIRE_RECONNECT'}]})
        wait_status(admin, sid, 'ready')
        recovered, role = h.client('A-reconnected')
        recovered.ok('_nori/session/subscribe', {'session_id': sid, 'cursors': {sid: cursor}})
        recovered.ok('_nori/session/list')
        replayed = [m['params']['seq'] for m in recovered.messages if m.get('method') == '_nori/session/event']
        check('reconnect replays only strictly newer envelopes without gaps', bool(replayed) and replayed == list(range(cursor + 1, replayed[-1] + 1)), {'cursor': cursor, 'replayed': replayed})
        table = h.command('host', 'ls')
        check('piped list flat headings and no ANSI', table.returncode == 0 and table.stdout.splitlines()[0].split() == ['ID', 'AGENT', 'STATUS', 'AGE', 'TITLE'] and '\x1b' not in table.stdout, table.stdout)
        c.ok('_nori/session/close', {'session_id': sid})
        check('last closed session leaves host running', wait_status(admin, sid, 'closed') is not None and h.command('host', 'ls').returncode == 0)
        token_before = h.token()
        for client in h.clients:
            client.close()
        h.clients = []
        os.kill(initial['pid'], signal.SIGKILL)
        time.sleep(.2)
        restarted = h.command('host', '--port', '0')
        check('kill9 stale state and lock recover', restarted.returncode == 0 and h.state()['pid'] != initial['pid'], restarted.stdout)
        check('host restart rotates token', h.token() != token_before)
        after, _ = h.client('after-restart', observe=True)
        check('host restart has empty in-memory registry', after.ok('_nori/session/list')['sessions'] == [])
        old = WS(h.state()['endpoint'], token_before, 'old-token')
        check('old token refused after host restart', old.status == 401, f'HTTP {old.status}')
        old.sock.close()
        stopped = h.command('host', 'stop')
        check('explicit stop releases lock and removes discovery', stopped.returncode == 0 and not (h.home / 'host/state.json').exists())
    finally:
        h.clean()

    occupied = Host('occupied-home')
    listener = socket.socket()
    listener.bind(('127.0.0.1', 0))
    listener.listen(1)
    port = listener.getsockname()[1]
    try:
        failed = occupied.command('host', '--port', str(port))
        check('occupied port startup fails without publishing state', failed.returncode != 0 and not (occupied.home / 'host/state.json').exists(), failed.stderr)
        listener.close()
        retried = occupied.command('host', '--port', str(port))
        check('failed occupied-port startup leaves lock reusable', retried.returncode == 0)
    finally:
        listener.close()
        occupied.clean()


try:
    run()
except Exception as error:
    check('probe completed all scenarios', False, f'{type(error).__name__}: {error}')
    traceback.print_exc()
finally:
    totals = dict(passed=sum(r['passed'] for r in RESULTS), failed=sum(not r['passed'] for r in RESULTS))
    (OUT / 'totals.json').write_text(json.dumps(totals, indent=2))
    print(json.dumps(totals), flush=True)
    LOG.close()
raise SystemExit(1 if totals['failed'] else 0)
