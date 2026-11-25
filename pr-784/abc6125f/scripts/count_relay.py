"""TCP relay that logs every JSON-RPC method a WebSocket client sends."""
import json, socket, struct, sys, threading

listen_port, target_port, log_path = int(sys.argv[1]), int(sys.argv[2]), sys.argv[3]
log = open(log_path, "a", buffering=1)

def pipe(src, dst, decode):
    buf = b""
    handshake = decode
    while True:
        data = src.recv(65536)
        if not data:
            break
        dst.sendall(data)
        if not decode:
            continue
        buf += data
        if handshake:
            end = buf.find(b"\r\n\r\n")
            if end < 0:
                continue
            buf, handshake = buf[end + 4:], False
        while len(buf) >= 2:
            b1, b2 = buf[0], buf[1]
            n, off = b2 & 0x7F, 2
            if n == 126:
                if len(buf) < 4: break
                n, off = struct.unpack(">H", buf[2:4])[0], 4
            elif n == 127:
                if len(buf) < 10: break
                n, off = struct.unpack(">Q", buf[2:10])[0], 10
            masked = b2 & 0x80
            mask = buf[off:off + 4] if masked else b""
            off += 4 if masked else 0
            if len(buf) < off + n: break
            payload = bytes(c ^ mask[i % 4] for i, c in enumerate(buf[off:off + n])) if masked else buf[off:off + n]
            buf = buf[off + n:]
            if b1 & 0x0F == 1:
                try:
                    method = json.loads(payload).get("method")
                except ValueError:
                    method = None
                if method:
                    log.write(method + "\n")
    dst.close()

server = socket.socket()
server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
server.bind(("127.0.0.1", listen_port))
server.listen()
while True:
    client, _ = server.accept()
    upstream = socket.create_connection(("127.0.0.1", target_port))
    threading.Thread(target=pipe, args=(client, upstream, True), daemon=True).start()
    threading.Thread(target=pipe, args=(upstream, client, False), daemon=True).start()
