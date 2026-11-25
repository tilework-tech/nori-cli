"""Regression checks for the stitched exactly-once scrollback audit."""

import unittest

from audit import AuditError, check_exactly_once, stitch

FIRST, LAST = "FirstReplayBeacon", "FinalReplayBeacon"


def section(number):
    lines = [f"› Inspect sample {number}.", "", f"### SectionBeacon{number:04d}",
             f"Read sample {number} and compare its measured range.",
             f"- Input: example-{number}.dat", "- Output: accepted", f"let sample = {number};",
             f"• Read sample-{number}.dat", f"  Observed range {number} through {number + 3}.", ""]
    if number == 1:
        lines.insert(2, FIRST)
    return lines


def transcript(turns):
    lines = [line for number in range(1, turns + 1) for line in section(number)]
    return lines + [f"Summary for {turns} inspected samples. {LAST}"]


def pages(lines, height=12, step=5):
    """Screens from the bottom up, as PageUp would show them, ending pinned at the top."""
    screens, end = [], len(lines)
    while True:
        start = max(0, end - height)
        screens.append(lines[start:start + height])
        if start == 0:
            screens.append(lines[:height])
            return screens
        end -= step


class StitchTests(unittest.TestCase):
    def test_rebuilds_the_scrollback_from_overlapping_pages(self):
        lines = transcript(6)
        self.assertEqual(stitch(pages(lines)), lines)

    def test_rejects_pages_that_do_not_overlap(self):
        lines = transcript(6)
        with self.assertRaises(AuditError):
            stitch([lines[-10:], lines[:10]])


class ExactlyOnceTests(unittest.TestCase):
    def test_accepts_a_complete_history(self):
        check_exactly_once(transcript(6), 6, FIRST, LAST)

    def test_rejects_a_connection_notice_inside_a_restored_reply(self):
        lines = transcript(6)
        lines.insert(lines.index("let sample = 3;"), "Connection lost. Reconnecting… host connection closed")
        with self.assertRaisesRegex(AuditError, "notice"):
            check_exactly_once(lines, 6, FIRST, LAST)

    def test_rejects_a_duplicated_history(self):
        lines = transcript(6)
        with self.assertRaisesRegex(AuditError, "order|times"):
            check_exactly_once(lines[:-1] + lines, 6, FIRST, LAST)

    def test_rejects_a_section_repeated_after_a_later_one(self):
        lines = transcript(6)
        lines[lines.index("let sample = 5;"):lines.index("let sample = 5;")] = section(2)[3:6]
        with self.assertRaisesRegex(AuditError, "order"):
            check_exactly_once(lines, 6, FIRST, LAST)

    def test_rejects_a_partly_repeated_section(self):
        lines = transcript(6)
        at = lines.index("let sample = 4;")
        lines.insert(at, "- Input: example-4.dat")
        with self.assertRaisesRegex(AuditError, "section 4"):
            check_exactly_once(lines, 6, FIRST, LAST)

    def test_rejects_a_missing_section(self):
        lines = [line for line in transcript(6) if "5" not in line]
        with self.assertRaises(AuditError):
            check_exactly_once(lines, 6, FIRST, LAST)

    def test_rejects_a_repeated_opening_or_final_marker(self):
        lines = transcript(6)
        with self.assertRaisesRegex(AuditError, FIRST):
            check_exactly_once([FIRST] + lines, 6, FIRST, LAST)
        with self.assertRaisesRegex(AuditError, LAST):
            check_exactly_once(lines + [LAST], 6, FIRST, LAST)


if __name__ == "__main__":
    unittest.main()
