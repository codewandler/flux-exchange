#!/usr/bin/env python3
"""Hermetic contract tests for the bounded Fleet story-evidence receipt."""

from __future__ import annotations

import importlib.util
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import unittest

SCRIPT = Path(__file__).with_name("story_evidence.py")
spec = importlib.util.spec_from_file_location("story_evidence", SCRIPT)
assert spec is not None and spec.loader is not None
story_evidence = importlib.util.module_from_spec(spec)
spec.loader.exec_module(story_evidence)


class StoryEvidenceTest(unittest.TestCase):
    def test_free_form_transcript_is_unbounded_but_receipt_is_bound(self) -> None:
        transcript = (b"compiler output must not be retained\n" * 100_000) + b"done"
        evidence = story_evidence.output_evidence(transcript)
        receipt = story_evidence.receipt(
            story="X-141",
            commit="a" * 40,
            profile="evidence",
            commands=[
                {
                    "argv": ["python3", "scripts/test_story_evidence.py"],
                    "duration_ms": 1,
                    "outcome": {"kind": "passed"},
                    "stdout": evidence,
                    "stderr": story_evidence.output_evidence(b""),
                }
            ],
            outcome={"kind": "passed"},
        )
        encoded = story_evidence.encode_receipt(receipt)

        self.assertGreater(len(transcript), story_evidence.RECEIPT_MAX_BYTES)
        self.assertLessEqual(len(encoded), story_evidence.RECEIPT_MAX_BYTES)
        self.assertEqual(receipt["story"], "X-141")
        self.assertEqual(receipt["commit"], "a" * 40)
        self.assertEqual(receipt["profile"], "evidence")
        self.assertEqual(receipt["outcome"]["kind"], "passed")
        self.assertEqual(evidence["bytes"], len(transcript))
        self.assertEqual(len(evidence["sha256"]), 64)
        self.assertNotIn("compiler output", encoded.decode())

    def test_pass_fail_timeout_signal_and_oversized_output_are_typed(self) -> None:
        passing = story_evidence.run_command(
            [sys.executable, "-c", "print('ok')"], Path.cwd(), 1.0
        )
        failing = story_evidence.run_command(
            [sys.executable, "-c", "raise SystemExit(7)"], Path.cwd(), 1.0
        )
        timed_out = story_evidence.run_command(
            [sys.executable, "-c", "import time; time.sleep(2)"], Path.cwd(), 0.02
        )
        signaled = story_evidence.run_command(
            [
                sys.executable,
                "-c",
                "import os, signal; os.kill(os.getpid(), signal.SIGTERM)",
            ],
            Path.cwd(),
            1.0,
        )
        oversized = story_evidence.run_command(
            [sys.executable, "-c", "print('x' * 100_000)"], Path.cwd(), 1.0
        )

        self.assertEqual(passing["outcome"], {"kind": "passed"})
        self.assertEqual(failing["outcome"], {"kind": "failed", "exit_code": 7})
        self.assertEqual(timed_out["outcome"]["kind"], "timed_out")
        self.assertEqual(signaled["outcome"], {"kind": "signaled", "signal": signal.SIGTERM})
        self.assertGreater(oversized["stdout"]["bytes"], story_evidence.RECEIPT_MAX_BYTES)
        self.assertEqual(set(oversized["stdout"]), {"bytes", "sha256"})

    def test_unknown_profile_is_a_typed_terminal_receipt(self) -> None:
        refusal = story_evidence.refusal_receipt("X-141", "anything", "unknown_profile")
        self.assertEqual(refusal["outcome"], {"kind": "unknown_profile"})
        self.assertEqual(refusal["commands"], [])
        self.assertLessEqual(
            len(story_evidence.encode_receipt(refusal)), story_evidence.RECEIPT_MAX_BYTES
        )

    def test_checkout_must_be_clean_and_match_story(self) -> None:
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            subprocess.run(["git", "init", "-q", "-b", "test/story/X-141"], cwd=root, check=True)
            subprocess.run(["git", "config", "user.email", "test@example.invalid"], cwd=root, check=True)
            subprocess.run(["git", "config", "user.name", "Test"], cwd=root, check=True)
            story_dir = root / "docs" / "stories"
            story_dir.mkdir(parents=True)
            (story_dir / "X-141-fixture.md").write_text("fixture\n", encoding="utf-8")
            subprocess.run(["git", "add", "."], cwd=root, check=True)
            subprocess.run(["git", "commit", "-qm", "fixture"], cwd=root, check=True)

            checkout = story_evidence.validate_checkout(root, "X-141")
            self.assertEqual(checkout["branch"], "test/story/X-141")
            self.assertEqual(len(checkout["commit"]), 40)

            (root / "dirty").write_text("dirty\n", encoding="utf-8")
            with self.assertRaisesRegex(story_evidence.CheckoutRefusal, "dirty_checkout"):
                story_evidence.validate_checkout(root, "X-141")
            (root / "dirty").unlink()

            with self.assertRaisesRegex(story_evidence.CheckoutRefusal, "mismatched_checkout"):
                story_evidence.validate_checkout(root, "X-999")

    def test_profiles_are_closed_and_do_not_run_integrated_or_stateful_actions(self) -> None:
        serialized = json.dumps(story_evidence.PROFILES, sort_keys=True)
        for forbidden in (
            "--workspace",
            "flux board",
            "fleet",
            "tmux",
            "publish",
            "release",
            "git push",
            "worktree",
        ):
            self.assertNotIn(forbidden, serialized)
        self.assertEqual(
            story_evidence.PROFILES["evidence"][0]["argv"],
            ["python3", "scripts/test_story_evidence.py"],
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)
