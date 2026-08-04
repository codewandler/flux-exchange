#!/usr/bin/env bash
# Refuse a staged archive directory that is not exactly the five provider-owned basenames.
set -euo pipefail

fail() { printf 'release-check-assets: %s\n' "$*" >&2; exit 1; }
root="$(git rev-parse --show-toplevel)"

check() {
  local directory="$1" version="$2" expected actual suffix target format
  [ -d "$directory" ] || fail "asset directory does not exist: $directory"
  expected="$(while IFS=$'\t' read -r target _runner format _executable; do
    [ "$format" = zip ] && suffix=.zip || suffix=.tar.zst
    printf 'flux-exchange-%s-%s%s\n' "$version" "$target" "$suffix"
    printf 'asset-%s.json\n' "$target"
  done < <(awk '!/^#/ && NF' "$root/release-targets.tsv") | LC_ALL=C sort)"
  actual="$(find "$directory" -mindepth 1 -maxdepth 1 -type f -printf '%f\n' | LC_ALL=C sort)"
  [ "$actual" = "$expected" ] || {
    diff <(printf '%s\n' "$expected") <(printf '%s\n' "$actual") >&2 || true
    fail 'staged archive set is not the exact five-target set'
  }
}

if [ "${1:-}" = --self-test ]; then
  scratch="$(mktemp -d "${TMPDIR:-/tmp}/flux-exchange-assets.XXXXXX")"
  trap 'find "$scratch" -type f -delete; rmdir "$scratch"' EXIT
  version=1.2.3
  while IFS=$'\t' read -r target _runner format _executable; do
    [ "$format" = zip ] && suffix=.zip || suffix=.tar.zst
    : >"$scratch/flux-exchange-${version}-${target}${suffix}"
    : >"$scratch/asset-${target}.json"
  done < <(awk '!/^#/ && NF' "$root/release-targets.tsv")
  check "$scratch" "$version"
  missing="$scratch/flux-exchange-${version}-aarch64-apple-darwin.tar.zst"
  unlink "$missing"
  if (check "$scratch" "$version" >/dev/null 2>&1); then fail 'self-test accepted a deleted platform archive'; fi
  : >"$missing"
  : >"$scratch/connector-plugin"
  if (check "$scratch" "$version" >/dev/null 2>&1); then fail 'self-test accepted an undeclared plugin executable'; fi
  unlink "$scratch/connector-plugin"
  mv "$missing" "$scratch/renamed-executable.tar.zst"
  if (check "$scratch" "$version" >/dev/null 2>&1); then fail 'self-test accepted a renamed release asset'; fi
  printf 'PASS release archive-set self-test\n'
  exit 0
fi

[ "$#" = 2 ] || fail 'usage: release-check-assets.sh <directory> <version>|--self-test'
check "$1" "$2"
printf 'PASS exact five-target staged archive set\n'
