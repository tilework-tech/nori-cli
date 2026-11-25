"""Each recovery sample ends in one labelled outcome; harness-side misses stay inconclusive."""

import unittest

from audit import AuditError, NoReply, classify

PROMPT = "RecoveryProbe"


def screen(history, composer):
    return "\n".join(["Session", "", *history, "", composer, "", "hints"])


class ClassifyTest(unittest.TestCase):
    def test_a_clean_sample_passes(self):
        self.assertEqual(classify(None, PROMPT), ("pass", True))

    def test_a_notice_inside_restored_history_fails(self):
        error = AuditError("connection notice in restored history: 'Connection lost.'")
        self.assertEqual(classify(error, PROMPT), ("notice_in_history", True))

    def test_other_history_damage_fails(self):
        self.assertEqual(classify(AuditError("SectionBeacon0003 appears 2 times"), PROMPT),
                         ("history_mismatch", True))

    def test_a_prompt_dropped_by_a_reconnect_fails(self):
        shown = screen(["• Connection restored. A previous request may have reached the host "
                        "and was not retried"], "› ")
        self.assertEqual(classify(NoReply(shown), PROMPT), ("prompt_lost", True))

    def test_a_draft_still_in_the_composer_is_inconclusive(self):
        shown = screen(["Synthetic reply 160."], f"› {PROMPT}")
        self.assertEqual(classify(NoReply(shown), PROMPT), ("unsent_draft", False))

    def test_a_sent_prompt_without_an_answer_fails(self):
        shown = screen([f"› {PROMPT}"], "› ")
        self.assertEqual(classify(NoReply(shown), PROMPT), ("no_reply", True))

    def test_a_history_past_the_journal_retention_is_labelled(self):
        error = RuntimeError("Timed out observing terminal:\n Replay unavailable: cursor for session"
                             " s has expired; first available sequence is 672")
        self.assertEqual(classify(error, PROMPT), ("replay_unavailable", True))

    def test_any_other_error_is_inconclusive(self):
        self.assertEqual(classify(RuntimeError("Timed out observing terminal"), PROMPT),
                         ("error", False))


if __name__ == "__main__":
    unittest.main()
