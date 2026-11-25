"""Run a command in a PTY that behaves like a terminal with the kitty keyboard protocol.

Answers `CSI ? u` with `CSI ? 1 u` (supported) and, while the program has pushed
flags, sends Shift+Enter as `CSI 13;2 u` when the outer terminal sends F12.
Logs every pushed/popped flag sequence to $KITTY_LOG.
"""
import os, pty, select, sys, tty, termios, fcntl, struct
argv = sys.argv[1:]
log = open(os.environ.get("KITTY_LOG", "/dev/null"), "ab")
pid, fd = pty.fork()
if pid == 0:
    os.execvp(argv[0], argv)
rows, cols = struct.unpack("hh", fcntl.ioctl(0, termios.TIOCGWINSZ, b"\0" * 4))[:2]
fcntl.ioctl(fd, termios.TIOCSWINSZ, struct.pack("hhhh", rows, cols, 0, 0))
old = termios.tcgetattr(0); tty.setraw(0)
pushed = 0
try:
    while True:
        r, _, _ = select.select([0, fd], [], [])
        if fd in r:
            try: data = os.read(fd, 65536)
            except OSError: break
            if not data: break
            if b"\x1b[?u" in data:
                os.write(fd, b"\x1b[?1u"); log.write(b"query-answered\n")
                data = data.replace(b"\x1b[?u", b"")
            for seq in (b"\x1b[>",):
                if seq in data:
                    pushed += 1; i = data.index(seq); log.write(b"push " + data[i:i+8] + b"\n")
            if b"\x1b[<" in data:
                log.write(b"pop\n")
            log.flush()
            os.write(1, data)
        if 0 in r:
            data = os.read(0, 65536)
            if not data: break
            data = data.replace(b"\x1b[24~", b"\x1b[13;2u")
            os.write(fd, data)
finally:
    termios.tcsetattr(0, termios.TCSADRAIN, old)
    os.waitpid(pid, 0)
