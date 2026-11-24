"""TCP relay between an attach client and a host that can drop every link."""
import socket
import threading
import time
from urllib.parse import urlsplit


class Proxy:
    def __init__(self, endpoint):
        target = urlsplit(endpoint)
        self.target = (target.hostname, target.port)
        self.server = socket.socket()
        self.server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        self.server.bind(("127.0.0.1", 0))
        self.server.listen()
        self.url = f"ws://127.0.0.1:{self.server.getsockname()[1]}{target.path}"
        self.links, self.blocked_until, self.closed = [], 0.0, False
        threading.Thread(target=self._accept, daemon=True).start()

    def _accept(self):
        while not self.closed:
            try:
                client, _ = self.server.accept()
            except OSError:
                return
            if time.time() < self.blocked_until:
                client.close()
                continue
            upstream = socket.create_connection(self.target)
            self.links.append((client, upstream))
            for a, b in ((client, upstream), (upstream, client)):
                threading.Thread(target=self._pump, args=(a, b), daemon=True).start()

    @staticmethod
    def _pump(src, dst):
        try:
            while True:
                data = src.recv(65536)
                if not data:
                    break
                dst.sendall(data)
        except OSError:
            pass
        for s in (src, dst):
            try:
                s.close()
            except OSError:
                pass

    def drop_all(self, hold=0.0):
        """Close every live link and refuse new ones for `hold` seconds."""
        self.blocked_until = time.time() + hold
        for pair in self.links:
            for s in pair:
                try:
                    s.shutdown(socket.SHUT_RDWR)
                    s.close()
                except OSError:
                    pass
        self.links = []

    def close(self):
        self.closed = True
        self.drop_all()
        self.server.close()
