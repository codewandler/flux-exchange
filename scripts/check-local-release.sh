#!/usr/bin/env bash
# Static release-policy ratchet plus failing-first mutations for the release workflow.
set -euo pipefail

root="$(git rev-parse --show-toplevel)"
workflow="$root/.github/workflows/local-release.yml"
targets="$root/release-targets.tsv"
fail() { printf 'check-local-release: %s\n' "$*" >&2; exit 1; }

check() {
  local workflow_path="$1" target_path="$2" expected actual
  expected='aarch64-apple-darwin aarch64-unknown-linux-gnu x86_64-apple-darwin x86_64-pc-windows-msvc x86_64-unknown-linux-gnu'
  actual="$(awk -F '\t' '!/^#/ && NF {print $1}' "$target_path" | LC_ALL=C sort | xargs)"
  [ "$actual" = "$expected" ] || fail "release target set is not the exact five-platform contract: $actual"
  dist_targets="$(python3 - "$root/dist-workspace.toml" <<'PY'
import sys, tomllib
with open(sys.argv[1], "rb") as source:
    print(" ".join(sorted(tomllib.load(source)["dist"]["targets"])))
PY
)"
  [ "$dist_targets" = "$expected" ] || fail "dist-workspace target set differs from the Flux five-platform contract: $dist_targets"
  [ "$(awk -F '\t' '!/^#/ && NF {print $1}' "$target_path" | sort | uniq -d | wc -l)" = 0 ] || fail 'release target set contains duplicates'
  while IFS=$'\t' read -r target runner format executable; do
    case "$target:$runner:$format:$executable" in
      aarch64-apple-darwin:macos-15:tar.zst:flux-exchange|x86_64-apple-darwin:macos-15-intel:tar.zst:flux-exchange|aarch64-unknown-linux-gnu:ubuntu-24.04-arm:tar.zst:flux-exchange|x86_64-unknown-linux-gnu:ubuntu-24.04:tar.zst:flux-exchange|x86_64-pc-windows-msvc:windows-2025:zip:flux-exchange.exe) ;;
      *) fail "target row violates the native platform contract: $target:$runner:$format:$executable" ;;
    esac
    grep -Fq "target: $target" "$workflow_path" || fail "workflow omits target $target"
    grep -Fq "runner: $runner" "$workflow_path" || fail "workflow omits native runner $runner"
  done < <(awk '!/^#/ && NF' "$target_path")
  [ "$(grep -Ec '^[[:space:]]+target: (aarch64|x86_64)' "$workflow_path")" = 5 ] || fail 'workflow matrix is not closed at five targets'
  grep -Fq 'cargo run --locked -p flux-exchange-release -- verify-staged' "$workflow_path" || fail 'staged assets do not pass the production verifier'
  grep -Fq 'scripts/release-stage.sh' "$workflow_path" || fail 'staged assets bypass the exact archive-set producer'
  grep -Fq 'cargo run --locked -p flux-exchange-release -- verify-published' "$workflow_path" || fail 'immutable public assets do not pass the production verifier'
  grep -Fq 'needs: [gate, preflight, build]' "$workflow_path" || fail 'publication is not gated by full, signer and native-build evidence'
  grep -Fq -- '--draft' "$workflow_path" || fail 'assets are not staged in a draft before exposure'
  grep -Fq 'attest-build-provenance@' "$workflow_path" || fail 'provenance is absent before exposure'
  grep -Fq 'group: flux-exchange-stable-channel' "$workflow_path" || fail 'stable-channel writes are not serialized'
  grep -Fq 'scripts/release-download.sh' "$workflow_path" || fail 'post-publication verification bypasses the one-302 transport'
  if grep -Eq 'releases/(latest|download/latest)|curl[^\n]*(-L|--location)([[:space:]]|$)' "$workflow_path"; then
    fail 'workflow admits mutable latest or automatic redirects'
  fi
}

if [ "${1:-}" = --self-test ]; then
  scratch="$(mktemp -d "${TMPDIR:-/tmp}/flux-exchange-release-check.XXXXXX")"
  trap 'rm -rf -- "$scratch"' EXIT
  cp "$workflow" "$scratch/workflow.yml"
  cp "$targets" "$scratch/targets.tsv"
  check "$scratch/workflow.yml" "$scratch/targets.tsv"
  sed -i.bak '/aarch64-apple-darwin/d' "$scratch/targets.tsv"
  if (check "$scratch/workflow.yml" "$scratch/targets.tsv" >/dev/null 2>&1); then fail 'self-test accepted a missing platform'; fi
  cp "$targets" "$scratch/targets.tsv"
  printf '%s\n' 'riscv64-unknown-linux-gnu ubuntu-24.04 tar.zst flux-exchange' >>"$scratch/targets.tsv"
  if (check "$scratch/workflow.yml" "$scratch/targets.tsv" >/dev/null 2>&1); then fail 'self-test accepted an undeclared platform'; fi
  cp "$workflow" "$scratch/workflow.yml"
  sed -i.bak 's/--max-redirs 0/--location/' "$scratch/workflow.yml"
  # The workflow delegates transport to release-download.sh, so mutating its required call is the
  # static failure that proves this check is wired to the policy boundary.
  sed -i.bak '/scripts\/release-download.sh/d' "$scratch/workflow.yml"
  if (check "$scratch/workflow.yml" "$targets" >/dev/null 2>&1); then fail 'self-test accepted transport-policy removal'; fi
  cp "$workflow" "$scratch/workflow.yml"
  sed -i.bak '/verify-staged/d' "$scratch/workflow.yml"
  if (check "$scratch/workflow.yml" "$targets" >/dev/null 2>&1); then fail 'self-test accepted staged-verifier removal'; fi
  printf 'PASS check-local-release self-test\n'
  exit 0
fi

check "$workflow" "$targets"
printf 'PASS local release workflow and five-target contract\n'
