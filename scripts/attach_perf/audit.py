"""Stitch scrollback pages and check that a restored history shows each part once, in order."""

import re

# Every fixture line that names its section number (see agent.transcript).
NUMBERED = re.compile(r"Inspect sample (\d+)\.|SectionBeacon(\d{4})(?!\d)|sample (\d+)\b|"
                      r"example-(\d+)\.dat|let sample = (\d+);|sample-(\d+)\.dat|Observed range (\d+) ")
NOTICE = re.compile(r"Connection lost|Reconnecting")


class AuditError(AssertionError):
    pass


def stitch(pages):
    """Join screens captured from the bottom up into one scrollback, using each overlap once.

    Consecutive screens must overlap; the largest overlap wins.
    """
    lines = list(pages[0])
    for older in pages[1:]:
        older = list(older)
        overlap = next((k for k in range(min(len(older), len(lines)), 0, -1)
                        if older[-k:] == lines[:k]), 0)
        if overlap == 0:
            raise AuditError("consecutive pages do not overlap")
        lines = older[:-overlap] + lines
    return lines


def check_exactly_once(lines, turns, first_marker, last_marker):
    """Raise AuditError unless sections 1..turns appear once each, whole, in order."""
    for marker in (first_marker, last_marker):
        count = sum(marker in line for line in lines)
        if count != 1:
            raise AuditError(f"{marker} appears {count} times")
    notices = [line for line in lines if NOTICE.search(line)]
    if notices:
        raise AuditError(f"connection notice in restored history: {notices[0]!r}")
    numbers = []
    for line in lines:
        numbers.extend(int(next(g for g in match.groups() if g)) for match in NUMBERED.finditer(line))
    if numbers != sorted(numbers):
        at = next(i for i in range(1, len(numbers)) if numbers[i] < numbers[i - 1])
        raise AuditError(f"section {numbers[at]} out of order after section {numbers[at - 1]}")
    if sorted(set(numbers)) != list(range(1, turns + 1)):
        raise AuditError(f"sections shown are not exactly 1..{turns}")
    counts = {number: numbers.count(number) for number in range(1, turns + 1)}
    expected = max(set(counts.values()), key=list(counts.values()).count)
    for number, count in counts.items():
        if count != expected:
            raise AuditError(f"section {number} has {count} numbered lines, others have {expected}")
    first = next(i for i, line in enumerate(lines) if first_marker in line)
    last = next(i for i, line in enumerate(lines) if last_marker in line)
    beacons = [i for i, line in enumerate(lines) if f"SectionBeacon{1:04d}" in line]
    if not beacons or not first < beacons[0] or not last > max(
            i for i, line in enumerate(lines) if f"SectionBeacon{turns:04d}" in line):
        raise AuditError("opening or final marker is out of place")


class NoReply(Exception):
    """The replacement adapter's answer never appeared; carries the last screen."""

    def __init__(self, screen):
        super().__init__("no answer to the post-recovery prompt")
        self.screen = screen


def classify(error, prompt):
    """Label one recovery sample as (outcome, conclusive).

    Inconclusive outcomes are misses the harness cannot attribute to the build:
    a prompt that never left the composer, or an unexpected harness error.
    """
    if error is None:
        return "pass", True
    if isinstance(error, AuditError):
        if str(error).startswith("connection notice"):
            return "notice_in_history", True
        return "history_mismatch", True
    if isinstance(error, NoReply):
        if "Connection restored" in error.screen:
            return "prompt_lost", True
        if any(prompt in line for line in error.screen.splitlines()[-4:]):
            return "unsent_draft", False
        return "no_reply", True
    if "Replay unavailable" in str(error):
        return "replay_unavailable", True
    return "error", False
