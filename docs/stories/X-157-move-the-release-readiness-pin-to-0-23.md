---
id: X-157
title: "The release-readiness pin follows the connector line to 0.23"
status: done
priority: 0
areas: [build, release]
note: "check-publication-readiness.sh and native-evidence-v1.json pin the connector crates and connector-secrets' checksum to 0.20.0; X-146 (0.21) and X-155 (0.23) moved past it invisibly because no release ran the release-only check. The v0.18.0 publish failed on exactly this. Supply-chain checksum authority — verify against the crates.io index, never the lockfile"
---

# The release-readiness pin follows the connector line to 0.23

## Goal

The release path publishes again. `scripts/check-publication-readiness.sh` and its authority
`crates/exchange-release/native-evidence-v1.json` pin the connector family — and
connector-secrets' registry checksum — to `0.20.0`. X-146 moved the workspace to 0.21 and X-155 to
0.23; neither ran the release-only readiness check, so the drift was invisible until the v0.18.0
publish refused with `check-publication-readiness: workspace connector-secrets does not select the
0.20.0 release line`. Move the pin to 0.23.0, with the checksum taken from the crates.io index.

## Acceptance

- [x] The version expectations in `scripts/check-publication-readiness.sh` move from `0.20.0` to
      `0.23.0` (the direct-dependency accept-set at ~line 104 and the Cargo.lock version check at
      ~line 121), and every `0.20.0`/`0.20` self-test fixture inside the same script moves with
      them so `--self-test` still passes. Grep the whole script for `0.20` and leave none behind
      that names the connector line.
- [x] The `inherited_upstream` authority in `crates/exchange-release/native-evidence-v1.json`
      carries `codewandler-connector-secrets` `0.23.0` and its **`registry_sha256` taken from the
      crates.io sparse index**, not copied from `Cargo.lock` (a checksum copied from the file under
      test proves only that the file agrees with itself). The index value is
      `360225fcfbd3af81248eb4fa449175a4909651612abdf5ffd9a54a09c35a2e14` — re-fetch it yourself
      (`curl -s https://index.crates.io/co/de/codewandler-connector-secrets | ... vers==0.23.0`)
      and quote the command. The JSON must stay canonical (the check reserialises it and refuses a
      non-canonical body): `schema` `exchange.native-evidence.v1`, RFC 8785-style compact form.
- [x] `bash scripts/check-publication-readiness.sh --self-test` passes, then
      `bash scripts/check-publication-readiness.sh` passes against the real tree — quote both.
- [x] The other release-policy checkers still pass: `bash scripts/check-crate-versions.sh`,
      `bash scripts/check-current-version.sh`, and `bash scripts/check-local-release.sh` if it
      reads either file. State what ran.
- [x] A failing-first note: capture the current refusal message first (it is the seeded failure),
      then green — this is the story's proof, since the checkers are their own tests.

## Progress

- 2026-08-12: Filed when the v0.18.0 crates.io publish refused on the stale connector pin. The
  workspace has been on a newer connector line than this check expects since X-146; this is the
  first release to exercise the path and catch it.

## Notes

- Write set: `scripts/check-publication-readiness.sh`, `crates/exchange-release/native-evidence-v1.json`.
  Release-policy only; no product code, no manifest, no lockfile.
- The regular `ci.yml` gate does not run this check (it is release-only, per the Publishing
  contract), which is why X-155's green gate did not catch it. Do not "fix" that by moving the
  check into the normal gate — its cost model is deliberate; just correct the pin.
