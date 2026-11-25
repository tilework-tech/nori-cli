"""Regression checks for the opt-in attach wire diagnostic."""

import json
import struct
import unittest

from transport import FrameReader, Trace


def frame(payload, opcode=1, final=True, mask=b""):
    size = len(payload)
    header = bytes([opcode | (128 if final else 0), (size if size < 126 else 126) | (128 if mask else 0)])
    if size >= 126:
        header += struct.pack("!H", size)
    if mask:
        payload = bytes(byte ^ mask[i % 4] for i, byte in enumerate(payload))
    return header + mask + payload


class TransportTests(unittest.TestCase):
    def test_reads_split_http_and_fragmented_masked_message_with_ping(self):
        reader = FrameReader()
        message = json.dumps({"text": "synthetic" * 40}).encode()
        stream = (b"GET /sessions HTTP/1.1\r\nAuthorization: Bearer sample\r\n\r\n" +
                  frame(message[:220], final=False, mask=b"abcd") + frame(b"x", opcode=9, mask=b"efgh") +
                  frame(message[220:], opcode=0, mask=b"ijkl"))
        records = []
        for i in range(0, len(stream), 13):
            records.extend(reader.messages(stream[i:i+13]))
        self.assertEqual(records, [message])

    def test_reselect_requires_client_cursor_through_live_update(self):
        trace = Trace("s")
        def subscribe(cursor, timestamp):
            trace.observe(True, json.dumps({"method": "_nori/session/subscribe", "params": {
                "session_id": "s", "cursors": {"s": cursor}}}).encode(), timestamp)
        def event(seq, content, timestamp):
            trace.observe(False, json.dumps({"method": "_nori/session/event", "params": {
                "seq": seq, "conversation_id": "s", "event": content}}).encode(), timestamp)
        subscribe(0, 1)
        event(1, {"source": "nori", "event": {"event_type": "replay_finished"}}, 2)
        event(2, {"source": "acp", "event": {"text": "LiveReplayBeacon0001"}}, 3)
        subscribe(1, 4)
        with self.assertRaises(AssertionError):
            trace.verify_applied()
        event(3, {"source": "nori", "event": {"event_type": "session_phase_changed"}}, 5)
        subscribe(2, 6)
        with self.assertRaises(AssertionError):
            trace.verify_applied()
        subscribe(3, 7)
        trace.verify_applied()
        self.assertEqual(trace.snapshot()["replay_finished_ns"], 2)
        self.assertEqual(trace.snapshot()["marker_seq"]["LiveReplayBeacon0001"], 2)


if __name__ == "__main__":
    unittest.main()
