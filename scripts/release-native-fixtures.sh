#!/usr/bin/env bash
# Execute the canonical native authority one exact Cargo binding at a time.
set -euo pipefail

# macOS exposes its temporary directory through a `/var` symlink. Explicit production state must
# refuse that ancestor, while process fixtures need a safe owner root so they test their named
# obligation. Resolve only the runner's temporary base to its physical spelling before any fixture
# creates a child; adversarial root tests still plant and exercise their own unsafe metadata.
if [ "$(uname -s)" = Darwin ]; then
  # The per-user Darwin TMPDIR is long enough that the X-128 owner endpoint can exceed
  # sockaddr_un.sun_path after its run-directory and socket suffixes are appended. `/tmp` has the
  # same symlink issue, but its physical `/private/tmp` spelling is both short and explicit.
  physical_temp="$(cd -- /tmp && pwd -P)"
  export TMPDIR="$physical_temp"
fi

root="$(git rev-parse --show-toplevel)"
fail() { printf 'release-native-fixtures: %s\n' "$*" >&2; exit 1; }
release_cli() {
  cargo run --quiet --locked -p flux-exchange-release -- native-authority "$@"
}
require_list_once() {
  local exact_test="$1"
  local listing="$2"
  [ "$(printf '%s\n' "$listing" | grep -Fxc "$exact_test: test" || true)" = 1 ] \
    || fail "exact test $exact_test was not listed exactly once"
}
require_one_clean_pass() {
  local exact_test="$1"
  local output="$2"
  printf '%s\n' "$output" | grep -Fq "test $exact_test ... ok" \
    || fail "exact test $exact_test did not report its passing test line"
  printf '%s\n' "$output" \
    | grep -Fq 'test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;' \
    || fail "exact test $exact_test was not one-passed/zero-ignored/zero-filtered"
}

if [ "${1:-}" = --self-test ]; then
  [ "$#" = 1 ] || fail 'usage: release-native-fixtures.sh --self-test'
  release_cli validate >/dev/null
  matrix="$(release_cli matrix)"
  [ "$(printf '%s' "$matrix" | jq -cS '.')" = "$matrix" ] \
    || fail 'authority workflow matrix is not canonical JSON'
  while IFS=$'\t' read -r target runner; do
    [ -n "$target" ] && [ -n "$runner" ] || fail 'authority matrix has an empty target or runner'
    [ -n "$(release_cli bindings "$target")" ] \
      || fail "authority target $target selects no Exchange-owned exact test"
  done < <(printf '%s' "$matrix" | jq -er '.[] | [.target,.runner] | @tsv')
  require_list_once exact_test $'other: test\nexact_test: test'
  if (require_list_once exact_test $'exact_test: test\nexact_test: test') >/dev/null 2>&1; then
    fail 'self-test accepted a duplicated exact test listing'
  fi
  require_one_clean_pass exact_test $'test exact_test ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out;'
  if (require_one_clean_pass exact_test $'test exact_test ... ok\ntest result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 1 filtered out;') >/dev/null 2>&1; then
    fail 'self-test accepted a filtered exact test execution'
  fi
  printf 'PASS release native authority runner self-test\n'
  exit 0
fi

[ "$#" -ge 1 ] && [ "$#" -le 2 ] \
  || fail 'usage: release-native-fixtures.sh <release-target> [report.json]'
target="$1"
report="${2:-$root/native-evidence-report-$target.json}"
matrix="$(release_cli matrix)"
runner="$(printf '%s' "$matrix" | jq -er --arg target "$target" \
  '.[] | select(.target == $target) | .runner')" \
  || fail "unsupported release target $target"
[ "$(printf '%s' "$matrix" | jq -r --arg target "$target" \
  '[.[] | select(.target == $target)] | length')" = 1 ] \
  || fail "release target $target is absent or duplicated"

bindings="$(mktemp "${TMPDIR:-/tmp}/flux-exchange-native-bindings.XXXXXX")"
report_tmp="$(mktemp "${TMPDIR:-/tmp}/flux-exchange-native-report.XXXXXX")"
trap 'rm -f -- "$bindings" "$report_tmp"' EXIT
release_cli bindings "$target" >"$bindings"
[ -s "$bindings" ] || fail "release target $target selected zero exact tests"
requested_binding="${FLUX_EXCHANGE_NATIVE_BINDING:-}"
if [ -n "$requested_binding" ]; then
  selected="$(awk -F '\t' -v requested="$requested_binding" '$1 == requested' "$bindings")"
  [ "$(printf '%s\n' "$selected" | grep -c . || true)" = 1 ] \
    || fail "diagnostic binding $requested_binding is absent or duplicated for $target"
  printf '%s\n' "$selected" >"$bindings"
fi

while IFS=$'\t' read -r binding_id package kind test_target exact_test features; do
  [ -n "$binding_id" ] && [ -n "$package" ] && [ -n "$exact_test" ] \
    || fail 'authority emitted an incomplete Cargo binding'
  cargo_args=(test -p "$package" --locked --target "$target")
  [ -z "$features" ] || cargo_args+=(--features "$features")
  case "$kind" in
    lib) cargo_args+=(--lib) ;;
    test) cargo_args+=(--test "$test_target") ;;
    *) fail "authority emitted unknown Cargo target kind $kind" ;;
  esac
  if ! listing="$(cargo "${cargo_args[@]}" -- --list --format terse 2>&1)"; then
    printf '%s\n' "$listing" >&2
    fail "listing failed for $binding_id"
  fi
  require_list_once "$exact_test" "$listing"
  if ! result="$(cargo "${cargo_args[@]}" -- --exact "$exact_test" --nocapture 2>&1)"; then
    printf '%s\n' "$result" >&2
    fail "execution failed for $binding_id"
  fi
  printf '%s\n' "$result"
  require_one_clean_pass "$exact_test" "$result"
done <"$bindings"

if [ -n "$requested_binding" ]; then
  printf 'PASS diagnostic target=%s runner=%s binding=%s\n' \
    "$target" "$runner" "$requested_binding"
  exit 0
fi

source_commit="$(git -C "$root" rev-parse HEAD)"
release_cli report "$target" "$source_commit" >"$report_tmp"
mkdir -p "$(dirname "$report")"
mv "$report_tmp" "$report"
printf 'PASS target=%s runner=%s authority=%s report=%s\n' \
  "$target" "$runner" "$(jq -er '.authority_sha256' "$report")" "$report"
