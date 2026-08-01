#!/usr/bin/env bash
#
# check-crate-versions.sh — the workspace version and the `exchange-host` pin must be the same number.
#
# Why this exists (X-30): `Cargo.toml` states this workspace's version twice.
#
#   [workspace.package]
#   version = "0.3.0"                                  # what the published crate IS
#
#   [workspace.dependencies]
#   exchange-host = { path = …, version = "0.3.0" }    # what a consumer resolving it from crates.io GETS
#
# The `path` half is what the workspace builds against, so a local `cargo build` is green no matter
# what the `version` half says — the two only meet at `cargo publish`, which strips `path` and ships
# the `version` requirement to crates.io. Bump one and forget the other and the published crate
# depends on a version of itself that does not exist, or on an older one, and the first person to
# find out is a consumer. AGENTS.md § Publishing contract already says to move them together; this is
# what makes "together" a machine's job rather than a reviewer's memory.
#
# This runs at PR time. It is deliberately NOT the tag check in `crates-io.yml`, and does not replace
# it: that one compares the pushed tag against the manifest and can only run at release. A tag can be
# pushed at a commit no pull request touched, so both checks stay — this one so a mismatch is caught
# where it is cheap to fix, that one so it cannot reach crates.io.
#
#   scripts/check-crate-versions.sh              # check Cargo.toml
#   scripts/check-crate-versions.sh --self-test  # prove the check catches a mismatch
#
# Named for, and shaped after, ../flux/scripts/check-crate-versions.sh. The rule it enforces is
# different — that workspace guards independently-versioned crates against a stale version, this one
# guards a single version stated in two places — but the contract is the same: self-test first, so
# the check has proved it can fail before it is trusted to pass.
#
# Exit 0 clean, 1 the two numbers disagree (a real failure), 2 a number could not be read at all.
#
set -uo pipefail

cd "$(git rev-parse --show-toplevel)"

fail() { printf '\033[31mFAIL\033[0m %s\n' "$1" >&2; }

# The version in `[workspace.package]`. $1 is the manifest text. Section-scoped on purpose: plenty of
# other tables in this file carry a `version = "…"` key, and the first one in the file is not
# reliably ours.
workspace_version() {
  printf '%s\n' "$1" \
    | awk '/^\[workspace\.package\]/{p=1;next} /^\[/{p=0} p' \
    | sed -nE 's/^version *= *"([^"]+)".*/\1/p' | head -1
}

# The `version = "…"` inside the `exchange-host` entry of `[workspace.dependencies]`. The entry is an
# inline table on one line, which is how it is written today and how the sibling workspaces write
# theirs; a multi-line rewrite would read as absent, and `--self-test` covers that case so the
# failure is a named one rather than a silent pass.
host_pin_version() {
  printf '%s\n' "$1" \
    | awk '/^\[workspace\.dependencies\]/{p=1;next} /^\[/{p=0} p' \
    | sed -nE 's/^exchange-host *=.*[{,][[:space:]]*version *= *"([^"]+)".*/\1/p' | head -1
}

# --self-test: the failing-first proof. A checker that has never been shown failing is not evidence
# that the tree is clean, so before reading the real manifest we make it read three synthetic ones —
# agreeing, disagreeing, and missing the pin entirely.
if [ "${1:-}" = "--self-test" ]; then
  agree='[workspace.package]
version = "0.3.0"
edition = "2021"

[workspace.dependencies]
exchange-host = { path = "crates/exchange-host", package = "codewandler-flux-exchange-host", version = "0.3.0" }
serde = { version = "9.9.9" }'

  got="$(workspace_version "$agree")"
  [ "$got" = "0.3.0" ] || { fail "self-test: workspace version read as '$got', want 0.3.0"; exit 1; }
  got="$(host_pin_version "$agree")"
  # The decoy matters: `serde = { version = "9.9.9" }` sits in the same table, and a checker that
  # grabs the first `version` it sees in `[workspace.dependencies]` would report 9.9.9 here.
  [ "$got" = "0.3.0" ] || { fail "self-test: exchange-host pin read as '$got', want 0.3.0 (a sibling dependency's version was picked up)"; exit 1; }

  # The rule itself: a bumped workspace version with a forgotten pin must compare unequal.
  disagree="${agree/version = \"0.3.0\"$'\n'edition/version = \"0.4.0\"$'\n'edition}"
  ws="$(workspace_version "$disagree")"
  pin="$(host_pin_version "$disagree")"
  [ "$ws" = "0.4.0" ] || { fail "self-test: the bumped workspace version read as '$ws', want 0.4.0"; exit 1; }
  [ "$ws" != "$pin" ] || { fail "self-test: a bumped workspace version compared equal to a stale pin"; exit 1; }

  # An unreadable pin must be reported, not silently treated as agreeing.
  missing='[workspace.package]
version = "0.3.0"

[workspace.dependencies]
exchange-host = { path = "crates/exchange-host" }'
  [ -z "$(host_pin_version "$missing")" ] || { fail "self-test: a missing pin was read as a version"; exit 1; }

  printf '\033[32mPASS\033[0m self-test: a forgotten pin is detectable, an agreeing pair is recognized\n'
  exit 0
fi

echo "== [workspace.package].version vs the exchange-host pin in [workspace.dependencies] =="

manifest="$(cat Cargo.toml)"
ws="$(workspace_version "$manifest")"
pin="$(host_pin_version "$manifest")"

if [ -z "$ws" ]; then
  fail "could not read [workspace.package].version from Cargo.toml"
  exit 2
fi
if [ -z "$pin" ]; then
  fail "could not read the exchange-host 'version' from [workspace.dependencies] in Cargo.toml"
  echo "The entry must carry a version alongside its path, or a published crate has no requirement" >&2
  echo "to hand consumers: exchange-host = { path = \"crates/exchange-host\", …, version = \"$ws\" }" >&2
  exit 2
fi

if [ "$ws" != "$pin" ]; then
  fail "[workspace.package].version is $ws but the exchange-host pin is $pin"
  echo >&2
  echo "These are two places holding one number. Move them together in Cargo.toml — the path" >&2
  echo "dependency hides the difference locally, and 'cargo publish' is where it stops being hidden." >&2
  exit 1
fi

printf '\033[32mPASS\033[0m workspace version and the exchange-host pin agree (%s)\n' "$ws"
