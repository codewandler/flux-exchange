#!/usr/bin/env bash
#
# X-140: keep the contributor workflow executable by a plain agent with the installed Flux CLI.
# The command probes run only against a temporary copy of the story board; this checkout is read-only.
#
set -uo pipefail

fail() { printf '\033[31mFAIL\033[0m %s\n' "$1" >&2; }

contains_literal() {
  local file="$1" text="$2" description="$3"
  if ! grep -Fq -- "$text" "$file"; then
    fail "$file: missing $description"
    return 1
  fi
}

schema_has() {
  local schema="$1" operation="$2" family="$3"
  if ! printf '%s' "$schema" | grep -Eq '"name"[[:space:]]*:[[:space:]]*"'"$operation"'"'; then
    fail "installed $family schema has no $operation operation"
    return 1
  fi
}

cd "$(git rev-parse --show-toplevel)"
failures=0

for file in AGENTS.md docs/README.md docs/stories/README.md; do
  if grep -Eq '/track:[[:alnum:]_-]+' "$file"; then
    fail "$file: stale private /track command remains"
    failures=$((failures + 1))
  fi
done

for command in \
  'flux board --root . schema --output json' \
  'flux board --root . next --limit 1 --output json' \
  'flux board --root . get X-140 --output json' \
  'flux board --root . transition X-140 in-progress' \
  'flux board --root . evidence X-140' \
  'flux board --root . done X-140' \
  'flux board --root . check --output json' \
  'flux board --root . sync'; do
  contains_literal AGENTS.md "$command" "copyable Board command: $command" || failures=$((failures + 1))
done

for command in \
  'flux fleet message main' \
  'flux fleet inspect worker WORKER --limit 50 --output json' \
  'flux fleet inspect result RESULT --limit 20 --output json' \
  'flux fleet events --limit 100 --follow --output ndjson' \
  'flux fleet handoff WAVE exchange/X-140'; do
  contains_literal AGENTS.md "$command" "copyable Fleet command: $command" || failures=$((failures + 1))
done

for word in accepted delivered completed; do
  contains_literal AGENTS.md "$word" "Fleet acknowledgement state $word" || failures=$((failures + 1))
done
contains_literal AGENTS.md 'dashboard is a snapshot' 'dashboard snapshot warning' || failures=$((failures + 1))
contains_literal AGENTS.md 'tmux is an operator view, never IPC' 'tmux IPC prohibition' || failures=$((failures + 1))
contains_literal AGENTS.md 'does not mean the main agent or worker answered' 'accepted-state warning' || failures=$((failures + 1))
contains_literal AGENTS.md 'Linux-only' 'Linux-only contributor runtime boundary' || failures=$((failures + 1))
contains_literal README.md 'Linux-only' 'Linux-only public runtime boundary' || failures=$((failures + 1))
contains_literal .github/workflows/ci.yml './scripts/check-agent-workflow.sh' 'ordinary CI gate invocation' || failures=$((failures + 1))

fixture="$(mktemp -d)"
trap 'rm -rf "$fixture"' EXIT
mkdir -p "$fixture/docs"
cp -R docs/stories "$fixture/docs/stories"

board_schema="$(flux board --root "$fixture" schema --output json)" || {
  fail 'installed Board schema command failed against the hermetic fixture'
  exit 1
}
fleet_schema="$(flux fleet schema --output json)" || {
  fail 'installed Fleet schema command failed'
  exit 1
}

for operation in transition evidence done sync; do
  schema_has "$board_schema" "$operation" Board || failures=$((failures + 1))
done
for operation in message handoff; do
  schema_has "$fleet_schema" "$operation" Fleet || failures=$((failures + 1))
done

flux board --root "$fixture" next --limit 1 --output json >/dev/null || {
  fail 'documented Board next command failed against the hermetic fixture'
  failures=$((failures + 1))
}
flux board --root "$fixture" get X-140 --output json >/dev/null || {
  fail 'documented Board get command failed against the hermetic fixture'
  failures=$((failures + 1))
}
flux board --root "$fixture" check --output json >/dev/null || {
  fail 'documented Board check command failed against the hermetic fixture'
  failures=$((failures + 1))
}

[ "$failures" -eq 0 ] || exit 1
printf '\033[32mPASS\033[0m installed Board/Fleet workflow is copyable, hermetic and Track-free\n'
