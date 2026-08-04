#!/usr/bin/env bash
# Run each provider-declared native release verdict through its exact real process test.
set -euo pipefail

root="$(git rev-parse --show-toplevel)"
fixture="$root/tests/fixtures/exchange-release-v2/fixture-set.json"
fail() { printf 'release-native-fixtures: %s\n' "$*" >&2; exit 1; }

inventory() {
  python3 - "$fixture" "${1:-}" <<'PY'
import json
import sys

with open(sys.argv[1], encoding="utf-8") as source:
    fixture = json.load(source)
target = sys.argv[2]
for case in fixture["native_cases"]:
    for evidence in case["evidence"]:
        if not target or target in evidence["targets"]:
            print("\t".join((case["id"], evidence["test_target"], evidence["exact_test"])))
PY
}

if [ "${1:-}" = --self-test ]; then
  expected='four-form-secret-sentinel-process-scan
production-root-inherited-environment
windows-production-root-unsafe-metadata
c515-server-lifetime-lease
expiry-equality-live
supervisor-death-normal-responsive-unix
supervisor-death-normal-wedged-unix
supervisor-death-sigkill-responsive-unix
supervisor-death-sigkill-wedged-unix
supervisor-death-terminate-responsive-windows
supervisor-death-terminate-wedged-windows
unix-inherited-abi
windows-inherited-abi'
  actual="$(inventory | cut -f1 | LC_ALL=C sort -u)"
  [ "$actual" = "$(printf '%s\n' "$expected" | LC_ALL=C sort)" ] || fail 'native case inventory is not the exact thirteen-case set'
  [ "$(inventory | wc -l | tr -d ' ')" = 19 ] || fail 'native process evidence is not the exact nineteen-test mapping'
  for target in \
    aarch64-apple-darwin \
    aarch64-unknown-linux-gnu \
    x86_64-apple-darwin \
    x86_64-pc-windows-msvc \
    x86_64-unknown-linux-gnu; do
    [ -n "$(inventory "$target")" ] || fail "target $target has no native process evidence"
  done
  printf 'PASS release native fixture mapping self-test\n'
  exit 0
fi

[ "$#" = 1 ] || fail 'usage: release-native-fixtures.sh <release-target>'
target="$1"
case "$target" in
  aarch64-apple-darwin|aarch64-unknown-linux-gnu|x86_64-apple-darwin|x86_64-unknown-linux-gnu)
    expected_test_target=supervised_unix
    ;;
  x86_64-pc-windows-msvc)
    expected_test_target=supervised_windows
    ;;
  *) fail "unsupported release target $target" ;;
esac

# The portable self-test rejects any mapping other than the reviewed exact IDs, targets and test
# names before a field from the fixture manifest can influence a Cargo invocation.
cargo run --quiet --locked -p flux-exchange-release -- self-test "$root/tests/fixtures/exchange-release-v2" >/dev/null
count=0
while IFS=$'\t' read -r case_id test_target exact_test; do
  [ -n "$case_id" ] || continue
  case "$test_target" in
    x134_sentinel_evidence)
      [ "$case_id" = four-form-secret-sentinel-process-scan ] \
        || fail "native case $case_id cannot use the four-form sentinel test target"
      test_args=(--test x134_sentinel_evidence)
      ;;
    local_state_regressions)
      [ "$case_id" = production-root-inherited-environment ] \
        || fail "native case $case_id cannot use the production-root test target"
      test_args=(--test local_state_regressions)
      ;;
    windows_native_root_poisoning)
      [ "$target" = x86_64-pc-windows-msvc ] \
        && [ "$case_id" = windows-production-root-unsafe-metadata ] \
        || fail "native case $case_id cannot use the Windows root-poisoning test target on $target"
      test_args=(--test windows_native_root_poisoning)
      ;;
    credential_store_process_lease)
      [ "$case_id" = c515-server-lifetime-lease ] \
        || fail "native case $case_id cannot use the credential-store lease test target"
      test_args=(--test credential_store_process_lease)
      ;;
    "$expected_test_target") test_args=(--test "$test_target") ;;
    lib)
      [ "$target" = x86_64-pc-windows-msvc ] || fail "lib evidence is not admitted for $target"
      test_args=(--lib)
      ;;
    *) fail "native case $case_id names wrong-platform test target $test_target" ;;
  esac
  cargo_args=(
    test -p flux-exchange
    --features native-root-test-seam,supervisor-test-wedge,supervisor-test-bind-refusal
    --locked --target "$target" "${test_args[@]}"
  )
  listing="$(cargo "${cargo_args[@]}" -- --list --format terse)"
  [ "$(printf '%s\n' "$listing" | grep -Fxc "$exact_test: test")" = 1 ] || {
    fail "native case $case_id does not resolve to exactly one test named $exact_test"
  }
  cargo "${cargo_args[@]}" -- --exact "$exact_test" --nocapture
  count=$((count + 1))
done < <(inventory "$target")
[ "$count" -gt 0 ] || fail "release target $target executed zero native fixture cases"
printf 'PASS %s native release fixture evidence (%s exact tests)\n' "$target" "$count"
