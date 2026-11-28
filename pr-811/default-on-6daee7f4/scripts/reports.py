import base64, re, sys
raw = open(sys.argv[1], 'rb').read()
for m in re.finditer(rb'\x1b\]7501;([^\x1b\x07]*)(\x1b\\|\x07)', raw):
    body = m.group(1).decode()
    msg = re.search(r'msg=([A-Za-z0-9+/=]+)', body)
    note = f'   # msg = "{base64.b64decode(msg.group(1)).decode()}"' if msg else ''
    print(f'ESC]7501;{body}ESC\\{note}')
n = raw.count(b'\x1b]7501;')
print(f'-- {n} OSC 7501 records, {len(raw)} bytes total')
