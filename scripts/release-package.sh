#!/usr/bin/env bash
# Create the closed, deterministic platform archive consumed by the release verifier.
set -euo pipefail

fail() { printf 'release-package: %s\n' "$*" >&2; exit 1; }

[ "$#" = 4 ] || fail 'usage: release-package.sh <version> <target> <executable> <output-directory>'
version="$1"
target="$2"
executable="$3"
output_dir="$4"
root="$(git rev-parse --show-toplevel)"

printf '%s' "$version" | grep -Eq '^(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8})$' || fail 'version is not stable SemVer'
row="$(awk -F '\t' -v target="$target" '$1 == target { print; count++ } END { if (count != 1) exit 1 }' "$root/release-targets.tsv")" || fail "target is outside the closed release set: $target"
IFS=$'\t' read -r _target _runner format member <<<"$row"
[ -f "$executable" ] || fail "executable does not exist: $executable"

archive_root="flux-exchange-${version}-${target}"
case "$format" in
  tar.zst) archive="$output_dir/${archive_root}.tar.zst" ;;
  zip) archive="$output_dir/${archive_root}.zip" ;;
  *) fail "unsupported archive format in release-targets.tsv: $format" ;;
esac

mkdir -p "$output_dir"
cargo run --locked -p flux-exchange-release -- package \
  --version "$version" --target "$target" --executable "$executable" \
  --license "$root/LICENSE-APACHE" --license "$root/LICENSE-MIT" --documentation "$root/README.md" \
  --output-directory "$output_dir" >"$output_dir/asset-${target}.json"
[ -f "$archive" ] || fail "release packager did not create the expected archive: $archive"

printf '%s\n' "$archive"
