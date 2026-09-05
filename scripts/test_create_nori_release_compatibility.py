#!/usr/bin/env python3
"""Compatibility tests for the create_nori_release executable."""

import os
import subprocess
import sys
import unittest


_SCRIPT = os.path.join(os.path.dirname(__file__), "create_nori_release")


class CreateNoriReleaseCompatibilityTest(unittest.TestCase):
    def test_help_loads_under_the_current_python(self):
        result = subprocess.run(
            [sys.executable, _SCRIPT, "--help"],
            text=True,
            capture_output=True,
        )

        self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
