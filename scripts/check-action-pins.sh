#!/usr/bin/env bash
#
# check-action-pins.sh — every third-party GitHub Action must be pinned to an immutable commit SHA.
#
# Why this exists (X-30): a step naming a movable tag (`actions/checkout@v4`,
# `dtolnay/rust-toolchain@stable`) hands whoever controls that upstream tag the code that runs in our
# workflows. One of those workflows holds the crown jewels — `crates-io.yml` carries
# `CARGO_REGISTRY_TOKEN` and can publish `codewandler-flux-exchange-host`, and a published version
# cannot be withdrawn. AGENTS.md and both workflows already treat SHA pinning as an invariant; until
# this script existed it was enforced by review, which is to say by whoever happened to look.
#
# The rule: every third-party action reference must name a full 40-char commit SHA and keep the
# human-readable version as a trailing comment (`actions/checkout@<sha> # v6.1.0`), because a pin
# nobody can read is a pin nobody will knowingly bump. Local (`./`-path) actions live in this repo and
# are covered by the tree itself, so they are exempt.
#
#   scripts/check-action-pins.sh              # scan .github/workflows/*.yml
#   scripts/check-action-pins.sh --self-test  # prove the check rejects a tag and accepts a SHA pin
#
# Modelled on ../flux/scripts/check-action-pins.sh — same `pin_ok` shape, same self-test-before-scan
# contract. It differs in one place, and the difference is the point of X-30: the scanner classifies
# each line before judging it, so a COMMENT mentioning the step keyword, or an example inside a
# `run: |` block, is not mistaken for a real action reference. `ci.yml`'s Node step carries a comment
# that had to be written around a naive grep to avoid exactly this; a checker that reports "no
# unpinned actions" because it silently mis-parsed the file is worse than no checker. Both hazards
# are exercised by --self-test.
#
# Exit 0 clean, 1 an unpinned or comment-less action reference (a real failure).
#
set -uo pipefail

cd "$(git rev-parse --show-toplevel)"

fail() { printf '\033[31mFAIL\033[0m %s\n' "$1" >&2; }

# Verdict for a single `uses:` line. Exit 0 = acceptably pinned (or exempt), 1 = violation with a
# reason printed to stdout. Kept as a function so the self-test can exercise it directly, without a
# throwaway workflow file to scan.
pin_ok() {
  local line="$1" ref sha
  # The action reference is the token right after the step keyword.
  ref="$(printf '%s\n' "$line" | sed -nE 's/^[[:space:]-]*uses:[[:space:]]*([^[:space:]]+).*/\1/p')"
  # Not a reference we can parse — the caller's classifier decides what reaches us; treat as clean.
  [ -n "$ref" ] || return 0
  # Local path actions ship in this repo; there is no upstream tag to pin.
  case "$ref" in
    ./*|../*) return 0 ;;
  esac
  # Third-party action: the part after the last `@` must be a full commit SHA.
  sha="${ref##*@}"
  if ! printf '%s' "$sha" | grep -qE '^[0-9a-f]{40}$'; then
    echo "unpinned action (ref is not a 40-char commit SHA): $ref"
    return 1
  fi
  # The version the SHA stands for must survive as a trailing comment, or the pin is unreadable and
  # nobody will ever knowingly bump it.
  if ! printf '%s\n' "$line" | grep -qE '@[0-9a-f]{40}[[:space:]]+#[[:space:]]*\S'; then
    echo "pinned SHA is missing its '# <version>' trailing comment: $ref"
    return 1
  fi
  return 0
}

# Emit the real action references in a workflow, as `<lineno>:<line>`, and nothing else.
#
# This is the part that has to be more than a grep. Two shapes in this repository's own workflows
# mention the step keyword without being a step:
#
#   * a comment — `# ... uses: ... ` — which may be indented to exactly a step's depth;
#   * a line inside a block scalar (`run: |`), where the text is shell, not YAML.
#
# So: track the indent of an open block scalar and skip anything more deeply indented than the key
# that opened it, and drop comment lines before matching. Everything surviving both is a mapping key
# (or list item) named `uses`.
action_lines() {
  awk '
    # Indent of the line, counting leading spaces only (YAML forbids tabs for indentation).
    function indent(s,   i) { i = match(s, /[^ ]/); return i ? i - 1 : length(s) }

    { line = $0 }

    # Blank lines neither open nor close a block scalar.
    line ~ /^[[:space:]]*$/ { next }

    # Inside a block scalar: content is anything indented deeper than the key that opened it.
    in_block {
      if (indent(line) > block_indent) next
      in_block = 0
    }

    # A comment line is prose, whatever it says. This is the case ci.yml had to be written around.
    line ~ /^[[:space:]]*#/ { next }

    # A key whose value is a block scalar (`run: |`, `script: >-`, …) opens one.
    line ~ /^[[:space:]-]*[A-Za-z_-]+:[[:space:]]*[|>][+-]?[[:space:]]*$/ {
      block_indent = indent(line); in_block = 1; next
    }

    # What is left at this point is real YAML structure.
    line ~ /^[[:space:]-]*uses:[[:space:]]/ { print NR ":" line }
  ' "$1"
}

# --self-test: the failing-first proof. It has two halves, because this check has two ways to be
# wrong. `pin_ok` must reject a movable tag and a comment-less SHA (a checker that has not shown it
# catches an unpinned reference is not trusted to report there are none), and `action_lines` must
# find the real references in a synthetic workflow while ignoring the decoys — a commented-out step
# and a documentation example inside `run: |` — that would each make a naive grep cry wolf.
if [ "${1:-}" = "--self-test" ]; then
  green='      - uses: actions/checkout@d23441a48e516b6c34aea4fa41551a30e30af803 # v6.1.0'
  tag='      - uses: actions/checkout@v4'
  branch='        uses: dtolnay/rust-toolchain@master'
  no_comment='      - uses: actions/checkout@d23441a48e516b6c34aea4fa41551a30e30af803'
  local_action='      - uses: ./.github/actions/setup'

  pin_ok "$green" || { fail "self-test: a SHA pin with a version comment was rejected"; exit 1; }
  pin_ok "$local_action" || { fail "self-test: a local ./ action was flagged"; exit 1; }
  if pin_ok "$tag" >/dev/null; then fail "self-test: a movable @tag was accepted"; exit 1; fi
  if pin_ok "$branch" >/dev/null; then fail "self-test: a movable @branch was accepted"; exit 1; fi
  if pin_ok "$no_comment" >/dev/null; then fail "self-test: a SHA pin without its version comment was accepted"; exit 1; fi

  fixture="$(mktemp)"
  trap 'rm -f "$fixture"' EXIT
  cat >"$fixture" <<'YAML'
jobs:
  demo:
    steps:
      # A decoy at step depth. Naive scanners match this:
      # uses: actions/checkout@v4
      - uses: actions/checkout@d23441a48e516b6c34aea4fa41551a30e30af803 # v6.1.0

      - name: Real, and unpinned
        uses: some/action@v3

      - name: A documentation example, not a step
        run: |
          echo "pin it like this:"
          echo "  uses: actions/checkout@<40-char-sha> # v6.1.0"
YAML

  found="$(action_lines "$fixture")"
  # The decoy comment and the `run: |` example must not appear; the two real references must.
  [ "$(printf '%s\n' "$found" | grep -c .)" = 2 ] || {
    fail "self-test: classifier found $(printf '%s\n' "$found" | grep -c .) action reference(s) in the fixture, want 2"
    printf '%s\n' "$found" >&2
    exit 1
  }
  printf '%s\n' "$found" | grep -q 'some/action@v3' || {
    fail "self-test: the classifier missed the real unpinned reference"; exit 1; }
  if printf '%s\n' "$found" | grep -q '<40-char-sha>'; then
    fail "self-test: a block scalar's documentation example was scanned as a step"; exit 1
  fi

  # End to end: the fixture must be *reported* as a violation, not merely parsed.
  violations=0
  while IFS= read -r hit; do
    pin_ok "${hit#*:}" >/dev/null || violations=$((violations + 1))
  done < <(printf '%s\n' "$found")
  [ "$violations" = 1 ] || { fail "self-test: fixture yielded $violations violation(s), want 1"; exit 1; }

  printf '\033[32mPASS\033[0m self-test: tags and branches rejected, SHA+comment pins accepted, comments and run-block examples not mistaken for steps\n'
  exit 0
fi

echo "== third-party action pins in .github/workflows =="
violations=0
checked=0
for wf in .github/workflows/*.yml .github/workflows/*.yaml; do
  [ -f "$wf" ] || continue
  while IFS= read -r hit; do
    [ -n "$hit" ] || continue
    lineno="${hit%%:*}"
    line="${hit#*:}"
    checked=$((checked + 1))
    if reason="$(pin_ok "$line")"; then
      :
    else
      fail "$wf:$lineno: $reason"
      violations=$((violations + 1))
    fi
  done < <(action_lines "$wf")
done

if [ "$violations" -gt 0 ]; then
  echo >&2
  echo "$violations unpinned or comment-less action reference(s) above." >&2
  echo "Pin each to a full commit SHA with the version as a trailing comment, e.g." >&2
  echo "  actions/checkout@d23441a48e516b6c34aea4fa41551a30e30af803  followed by  # v6.1.0" >&2
  echo "Resolve a tag with: gh api repos/<owner>/<repo>/git/ref/tags/<tag> --jq .object.sha" >&2
  exit 1
fi

if [ "$checked" -eq 0 ]; then
  fail "no action references found in .github/workflows — the scanner is broken, or the workflows are"
  exit 1
fi

printf '\033[32mPASS\033[0m %s third-party action reference(s), all pinned to a commit SHA\n' "$checked"
