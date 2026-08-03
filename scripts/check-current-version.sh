#!/usr/bin/env bash
#
# Check the small, explicit set of prose that claims to describe the CURRENT build.
#
# Historical changelog headings and sentences such as "since X-57, v0.9.0+" are deliberately not
# scanned: those versions date an event and must remain true after a release. The five expressions
# below instead identify the status sentence in each contributor/user index and the published
# exchange-host crate's front-page doc comment. If another current-version claim is added, add it to
# `claims` with an exact extraction rule; do not widen this into a grep for every version-shaped
# string, because that would make accurate history fail the gate.
#
# Usage:
#   scripts/check-current-version.sh
#   scripts/check-current-version.sh --self-test
set -uo pipefail

cd "$(git rev-parse --show-toplevel)"

fail() { printf '\033[31mFAIL\033[0m %s\n' "$1" >&2; }

manifest_version() {
  awk '/^\[workspace\.package\]/{p=1;next} /^\[/{p=0} p' Cargo.toml \
    | sed -nE 's/^version *= *"([^"]+)".*/\1/p' | head -1
}

extract() {
  sed -nE "$1"
}

claim_agrees() {
  [ "$1" = "$2" ]
}

if [ "${1:-}" = "--self-test" ]; then
  sample='## [0.14.3] - 2026-08-03
Since X-57, v0.9.0+.
> **Status: v0.15.0 — current.**'
  got="$(printf '%s\n' "$sample" | extract 's/^> \*\*Status: v([0-9]+\.[0-9]+\.[0-9]+) —.*/\1/p')"
  [ "$got" = "0.15.0" ] || {
    fail "self-test: current claim read as '$got', want 0.15.0"
    exit 1
  }
  if claim_agrees "$got" "0.14.3"; then
    fail "self-test: a stale current claim compared equal to the manifest"
    exit 1
  fi
  historical="$(printf '%s\n' "$sample" | extract 's/^## \[([0-9]+\.[0-9]+\.[0-9]+)\].*/\1/p')"
  [ "$historical" = "0.14.3" ] || {
    fail "self-test: the historical control was not present"
    exit 1
  }
  printf '\033[32mPASS\033[0m self-test: a stale current claim is detected without treating history as current\n'
  exit 0
fi

expected="$(manifest_version)"
if [ -z "$expected" ]; then
  fail "could not read [workspace.package].version from Cargo.toml"
  exit 2
fi

# label|file|sed expression. Each expression must select exactly one current-state sentence.
claims=(
  'contributor status|AGENTS.md|s/^\*\*v([0-9]+\.[0-9]+\.[0-9]+)\. The service.*/\1/p'
  'user status|README.md|s/^> \*\*Status: v([0-9]+\.[0-9]+\.[0-9]+) —.*/\1/p'
  'roadmap status|docs/roadmap.md|s/^_As of [^_]+:_ \*\*v([0-9]+\.[0-9]+\.[0-9]+) —.*/\1/p'
  'story-board status|docs/stories/README.md|s/^\*\*v([0-9]+\.[0-9]+\.[0-9]+) —.*/\1/p'
  'published host crate docs|crates/exchange-host/src/lib.rs|s/^\/\/! \*\*v([0-9]+\.[0-9]+\.[0-9]+),.*/\1/p'
)

failed=0
echo "== current prose versions vs [workspace.package].version ($expected) =="
for spec in "${claims[@]}"; do
  IFS='|' read -r label file expression <<<"$spec"
  versions="$(extract "$expression" <"$file")"
  count="$(printf '%s\n' "$versions" | sed '/^$/d' | wc -l)"
  if [ "$count" -ne 1 ]; then
    fail "$label: expected exactly one current-version claim in $file, found $count"
    failed=1
    continue
  fi
  if ! claim_agrees "$versions" "$expected"; then
    fail "$label: $file claims v$versions, manifest is $expected"
    failed=1
  else
    printf '\033[32mPASS\033[0m %s (%s)\n' "$label" "$file"
  fi
done

exit "$failed"
