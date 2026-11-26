"""Replay an asciinema v2 cast through pyte and print every change to the
header face row, with timestamps and the colors of the face cells.

Usage: python3 timeline.py session.cast
"""
import json
import sys

import pyte

FACE_GLYPHS = set("〉›-_▐▗▁ ")


def header_row(screen):
    """First non-blank row that looks like the face or the wordmark."""
    for y in range(screen.lines):
        text = "".join(screen.buffer[y][x].data for x in range(screen.columns)).rstrip()
        stripped = text.strip()
        if not stripped:
            continue
        if "nori CLI" in stripped or (len(stripped) <= 12 and set(stripped) <= FACE_GLYPHS | set("noriCLI v0123456789.")):
            return y, text
    return None, None


def colors(screen, y):
    row = screen.buffer[y]
    out = []
    for x in range(screen.columns):
        ch = row[x]
        if ch.data.strip():
            dim = "dim" if getattr(ch, "dim", False) else ""
            out.append(f"{ch.data}:{ch.fg}{'+bold' if ch.bold else ''}{dim}")
    return " ".join(out)


def main(path):
    with open(path) as f:
        header = json.loads(f.readline())
        screen = pyte.Screen(header["width"], header["height"])
        stream = pyte.Stream(screen)
        last = None
        for line in f:
            t, kind, data = json.loads(line)
            if kind != "o":
                continue
            stream.feed(data)
            y, text = header_row(screen)
            if text is None:
                continue
            face = text.strip()
            if face != last:
                print(f"{t:8.3f}  row{y:<2} {face:<20} {colors(screen, y)}")
                last = face


if __name__ == "__main__":
    main(sys.argv[1])
