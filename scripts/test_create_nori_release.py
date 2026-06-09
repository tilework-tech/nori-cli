#!/usr/bin/env python3
"""Tests for create_nori_release."""

import importlib.util
import os
import subprocess
import unittest
from importlib.machinery import SourceFileLoader
from unittest import mock

# The release script has no .py extension, so point the loader at it directly.
_PATH = os.path.join(os.path.dirname(__file__), "create_nori_release")
_LOADER = SourceFileLoader("create_nori_release", _PATH)
_SPEC = importlib.util.spec_from_loader("create_nori_release", _LOADER)
release = importlib.util.module_from_spec(_SPEC)
_LOADER.exec_module(release)


def _ls_remote_output(versions: list[str]) -> str:
    """Render `git ls-remote --tags` output for the given matching versions.

    Annotated tags emit both the tag ref and a peeled "^{}" ref; list_tags
    must dedupe those. A non-matching ref is included to exercise filtering.
    """
    lines = []
    for v in versions:
        sha = "0" * 40
        lines.append(f"{sha}\trefs/tags/{release.TAG_PREFIX}{v}")
        lines.append(f"{sha}\trefs/tags/{release.TAG_PREFIX}{v}^{{}}")
    lines.append(f"{'1' * 40}\trefs/tags/some-other-tag")
    return "\n".join(lines) + "\n"


def _completed(stdout: str, returncode: int = 0) -> subprocess.CompletedProcess:
    return subprocess.CompletedProcess(
        args=["git"], returncode=returncode, stdout=stdout, stderr=""
    )


class ListTagsTest(unittest.TestCase):
    def test_parses_remote_tags_and_dedupes_peeled_refs(self):
        """list_tags reads tags from the remote via git (not the REST API) in a
        single call, strips the prefix, drops non-matching refs, and does not
        double-count the peeled "^{}" refs that annotated tags produce.
        """
        with mock.patch.object(
            release.subprocess,
            "run",
            return_value=_completed(_ls_remote_output(["1.0.0", "1.1.0", "2.0.0"])),
        ) as run:
            tags = release.list_tags()

        self.assertEqual(run.call_count, 1)
        # The fix is to use the git protocol, not the rate-limited/capped REST API.
        self.assertEqual(run.call_args.args[0][0], "git")
        self.assertEqual(sorted(tags), ["1.0.0", "1.1.0", "2.0.0"])
        self.assertNotIn("some-other-tag", tags)

    def test_raises_when_remote_query_fails(self):
        """A failed remote query surfaces as a ReleaseError rather than being
        swallowed into a wrong (empty) version computation.
        """
        with mock.patch.object(
            release.subprocess, "run", return_value=_completed("", returncode=128)
        ):
            with self.assertRaises(release.ReleaseError):
                release.list_tags()


if __name__ == "__main__":
    unittest.main()
