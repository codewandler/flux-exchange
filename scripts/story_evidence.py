#!/usr/bin/env python3
"""Run one closed Exchange story check and emit a bounded JSON receipt.

This is intentionally not a general command runner. Profiles below are the entire executable
surface; stdout and stderr are reduced to byte counts and SHA-256 digests so compiler, console-build,
and native-evidence transcripts never become Fleet handoff payloads.
"""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import time
from typing import Any

SCHEMA = "flux-exchange.story-evidence.v1"
RECEIPT_MAX_BYTES = 8 * 1024
STORY_ID = re.compile(r"^[A-Z]+-[0-9]+$")

# Closed story-level checks only. The integrated wave remains responsible for the complete gate.
PROFILES: dict[str, list[dict[str, Any]]] = {
    "evidence": [
        {"argv": ["python3", "scripts/test_story_evidence.py"], "timeout_seconds": 30},
    ],
    "host": [
        {
            "argv": ["cargo", "test", "-p", "codewandler-flux-exchange-host"],
            "timeout_seconds": 900,
        },
    ],
    "server": [
        {"argv": ["cargo", "test", "-p", "flux-exchange"], "timeout_seconds": 1800},
    ],
    "console": [
        {"argv": ["npm", "--prefix", "console", "test"], "timeout_seconds": 600},
    ],
    "web": [
        {"argv": ["npm", "--prefix", "web", "run", "build"], "timeout_seconds": 600},
        {"argv": ["npm", "--prefix", "web", "test"], "timeout_seconds": 300},
    ],
}


class CheckoutRefusal(RuntimeError):
    """A pre-execution checkout invariant was not satisfied."""


def git(root: Path, *args: str) -> str:
    completed = subprocess.run(
        ["git", *args],
        cwd=root,
        check=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    return completed.stdout.strip()


def validate_checkout(root: Path, story: str) -> dict[str, str]:
    """Return the exact checkout identity or refuse before a profile can execute."""
    if not STORY_ID.fullmatch(story):
        raise CheckoutRefusal("invalid_story")
    try:
        top = Path(git(root, "rev-parse", "--show-toplevel")).resolve()
        branch = git(root, "branch", "--show-current")
        commit = git(root, "rev-parse", "HEAD")
        dirty = git(root, "status", "--porcelain")
    except (subprocess.CalledProcessError, OSError) as error:
        raise CheckoutRefusal("not_a_checkout") from error

    if top != root.resolve():
        raise CheckoutRefusal("mismatched_checkout")
    if branch == "" or not branch.endswith(f"/story/{story}"):
        raise CheckoutRefusal("mismatched_checkout")
    stories = list((root / "docs" / "stories").glob(f"{story}-*.md"))
    if len(stories) != 1:
        raise CheckoutRefusal("mismatched_checkout")
    if dirty:
        raise CheckoutRefusal("dirty_checkout")
    return {"branch": branch, "commit": commit}


def output_evidence(output: bytes) -> dict[str, Any]:
    """Reduce arbitrary output atomically; no prefix or suffix is retained."""
    return {"bytes": len(output), "sha256": hashlib.sha256(output).hexdigest()}


def run_command(argv: list[str], cwd: Path, timeout_seconds: float) -> dict[str, Any]:
    """Run one fixed argv with bounded lifetime and typed terminal status."""
    started = time.monotonic_ns()
    process = subprocess.Popen(
        argv,
        cwd=cwd,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        start_new_session=True,
    )
    try:
        stdout, stderr = process.communicate(timeout=timeout_seconds)
        if process.returncode == 0:
            outcome: dict[str, Any] = {"kind": "passed"}
        elif process.returncode < 0:
            outcome = {"kind": "signaled", "signal": -process.returncode}
        else:
            outcome = {"kind": "failed", "exit_code": process.returncode}
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGKILL)
        stdout, stderr = process.communicate()
        outcome = {"kind": "timed_out", "timeout_ms": int(timeout_seconds * 1000)}

    return {
        "argv": argv,
        "duration_ms": (time.monotonic_ns() - started) // 1_000_000,
        "outcome": outcome,
        "stdout": output_evidence(stdout),
        "stderr": output_evidence(stderr),
    }


def receipt(
    *,
    story: str,
    commit: str | None,
    profile: str,
    commands: list[dict[str, Any]],
    outcome: dict[str, Any],
) -> dict[str, Any]:
    return {
        "schema": SCHEMA,
        "story": story,
        "commit": commit,
        "profile": profile,
        "commands": commands,
        "outcome": outcome,
    }


def refusal_receipt(story: str, profile: str, kind: str) -> dict[str, Any]:
    return receipt(
        story=story,
        commit=None,
        profile=profile,
        commands=[],
        outcome={"kind": kind},
    )


def encode_receipt(value: dict[str, Any]) -> bytes:
    encoded = (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode()
    if len(encoded) > RECEIPT_MAX_BYTES:
        raise RuntimeError("receipt_size_exceeded")
    return encoded


def emit(value: dict[str, Any]) -> None:
    sys.stdout.buffer.write(encode_receipt(value))


def main(argv: list[str]) -> int:
    if len(argv) != 2:
        story = argv[0] if argv else ""
        profile = argv[1] if len(argv) > 1 else ""
        emit(refusal_receipt(story, profile, "invalid_arguments"))
        return 2

    story, profile = argv
    if profile not in PROFILES:
        emit(refusal_receipt(story, profile, "unknown_profile"))
        return 2

    root = Path.cwd()
    try:
        checkout = validate_checkout(root, story)
    except CheckoutRefusal as refusal:
        emit(refusal_receipt(story, profile, str(refusal)))
        return 2

    commands: list[dict[str, Any]] = []
    overall: dict[str, Any] = {"kind": "passed"}
    for definition in PROFILES[profile]:
        command = run_command(
            definition["argv"], root, float(definition["timeout_seconds"])
        )
        commands.append(command)
        if command["outcome"]["kind"] != "passed":
            overall = command["outcome"]
            break

    emit(
        receipt(
            story=story,
            commit=checkout["commit"],
            profile=profile,
            commands=commands,
            outcome=overall,
        )
    )
    return 0 if overall["kind"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
