#!/usr/bin/env bash
# Static release-policy ratchet plus failing-first mutations for the release workflow.
set -euo pipefail

root="$(git rev-parse --show-toplevel)"
workflow="$root/.github/workflows/local-release.yml"
targets="$root/release-targets.tsv"
fail() { printf 'check-local-release: %s\n' "$*" >&2; exit 1; }

check() {
  local workflow_path="$1" target_path="$2" download_path="$3" expected actual expected_matrix actual_matrix
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
  expected_matrix="$(awk -F '\t' '!/^#/ && NF {print $1 ":" $2}' "$target_path" | LC_ALL=C sort)"
  actual_matrix="$(sed -n '/^[[:space:]]*matrix:/,/^[[:space:]]*runs-on:/p' "$workflow_path" | awk '
    /^[[:space:]]*- runner:/ { runner = $3 }
    /^[[:space:]]*target:/ { print $2 ":" runner; runner = "" }
  ' | LC_ALL=C sort)"
  [ "$actual_matrix" = "$expected_matrix" ] || {
    diff <(printf '%s\n' "$expected_matrix") <(printf '%s\n' "$actual_matrix") >&2 || true
    fail 'workflow native matrix is not the exact target/runner set'
  }
  while IFS=$'\t' read -r target runner format executable; do
    case "$target:$runner:$format:$executable" in
      aarch64-apple-darwin:macos-15:tar.zst:flux-exchange|x86_64-apple-darwin:macos-15-intel:tar.zst:flux-exchange|aarch64-unknown-linux-gnu:ubuntu-24.04-arm:tar.zst:flux-exchange|x86_64-unknown-linux-gnu:ubuntu-24.04:tar.zst:flux-exchange|x86_64-pc-windows-msvc:windows-2025:zip:flux-exchange.exe) ;;
      *) fail "target row violates the native platform contract: $target:$runner:$format:$executable" ;;
    esac
    grep -Fq "target: $target" "$workflow_path" || fail "workflow omits target $target"
    grep -Fq "runner: $runner" "$workflow_path" || fail "workflow omits native runner $runner"
  done < <(awk '!/^#/ && NF' "$target_path")
  grep -Fq 'cargo run --locked -p flux-exchange-release -- verify-staged' "$workflow_path" || fail 'staged assets do not pass the production verifier'
  grep -Fq 'scripts/release-stage.sh' "$workflow_path" || fail 'staged assets bypass the exact archive-set producer'
  grep -Fq 'cargo run --locked -p flux-exchange-release -- verify-published' "$workflow_path" || fail 'immutable public assets do not pass the production verifier'
  grep -Fq 'needs: [gate, preflight, build]' "$workflow_path" || fail 'publication is not gated by full, signer and native-build evidence'
  grep -Fq -- '--draft' "$workflow_path" || fail 'assets are not staged in a draft before exposure'
  grep -Fq 'attest-build-provenance@' "$workflow_path" || fail 'provenance is absent before exposure'
  grep -Fq 'group: flux-exchange-stable-channel' "$workflow_path" || fail 'stable-channel writes are not serialized'
  grep -Fq 'scripts/release-download.sh' "$workflow_path" || fail 'post-publication verification bypasses the one-302 transport'
  grep -Fq 'cargo run --locked -p flux-exchange-release -- verify-compatibility' "$workflow_path" || fail 'native artifacts bypass the production compatibility verifier'
  grep -Fq -- '--executable-sha256 "$executable_sha256"' "$workflow_path" || fail 'compatibility verification is not bound to independently digested executable bytes'
  grep -Fq '"tag": f"refs/tags/{tag}"' "$workflow_path" || fail 'native compatibility expectation is not bound to the exact tag'
  grep -Fq '"version": version' "$workflow_path" || fail 'native compatibility expectation is not bound to the exact version'
  grep -Fq '"source_commit": source_commit' "$workflow_path" || fail 'native compatibility expectation is not bound to the exact source commit'
  grep -Fq '"build_id": build_id' "$workflow_path" || fail 'native compatibility expectation is not bound to the exact build ID'
  for protocol in \
    '"exchange_api": "exchange.api.v1"' \
    '"effective_catalogue_response": "exchange.effective-catalogue-response.v1"' \
    '"invoke_request": "exchange.invoke-request.v1"' \
    '"invoke_response": "exchange.invoke-response.v1"' \
    '"connection_plan": "exchange.connection-plan.v1"' \
    '"supervisor": "exchange.supervisor-ready.v1"'; do
    grep -Fq "$protocol" "$workflow_path" || fail "native compatibility expectation omits $protocol"
  done
  grep -Fq '[ "$GITHUB_REF" = "refs/tags/$RELEASE_TAG" ]' "$workflow_path" || fail 'resumable publication is not bound to the immutable tag ref used by provenance'
  grep -Fq '[ "$GITHUB_SHA" = "$source" ]' "$workflow_path" || fail 'checked-out source is not bound to the workflow provenance SHA'
  if grep -Fq 'gh release download' "$workflow_path"; then
    fail 'workflow uses unbounded authenticated release downloads'
  fi
  grep -Fq 'gh release view "$RELEASE_TAG" --repo "$GITHUB_REPOSITORY" --json isDraft,assets' "$workflow_path" || fail 'immutable resume does not inspect names before downloading bytes'
  if grep -F '$(curl ' "$download_path" | grep -Fv '$(curl --disable ' >/dev/null; then
    fail 'a curl invocation can load ambient configuration'
  fi
  [ "$(grep -Fc 'curl --disable ' "$download_path")" = 3 ] || fail 'release transport has an unexpected curl invocation set'
  grep -Fq -- '--max-filesize 65536' "$download_path" || fail 'initial redirect response body is unbounded'
  grep -Fq -- '--max-filesize "$byte_limit"' "$download_path" || fail 'release transport does not bound response bytes while reading'
  if grep -Eq 'releases/(latest|download/latest)|curl[^\n]*(-L|--location)([[:space:]]|$)' "$workflow_path"; then
    fail 'workflow admits mutable latest or automatic redirects'
  fi
}

if [ "${1:-}" = --self-test ]; then
  scratch="$(mktemp -d "${TMPDIR:-/tmp}/flux-exchange-release-check.XXXXXX")"
  trap 'rm -rf -- "$scratch"' EXIT
  cp "$workflow" "$scratch/workflow.yml"
  cp "$targets" "$scratch/targets.tsv"
  cp "$root/scripts/release-download.sh" "$scratch/release-download.sh"
  check "$scratch/workflow.yml" "$scratch/targets.tsv" "$scratch/release-download.sh"
  sed -i.bak '/aarch64-apple-darwin/d' "$scratch/targets.tsv"
  if (check "$scratch/workflow.yml" "$scratch/targets.tsv" "$scratch/release-download.sh" >/dev/null 2>&1); then fail 'self-test accepted a missing platform'; fi
  cp "$targets" "$scratch/targets.tsv"
  printf '%s\n' 'riscv64-unknown-linux-gnu ubuntu-24.04 tar.zst flux-exchange' >>"$scratch/targets.tsv"
  if (check "$scratch/workflow.yml" "$scratch/targets.tsv" "$scratch/release-download.sh" >/dev/null 2>&1); then fail 'self-test accepted an undeclared platform'; fi
  cp "$targets" "$scratch/targets.tsv"
  sed -i.bak '/^[[:space:]]*runs-on: \${{ matrix.runner }}/i\
          - runner: ubuntu-24.04\
            target: riscv64-unknown-linux-gnu' "$scratch/workflow.yml"
  if (check "$scratch/workflow.yml" "$scratch/targets.tsv" "$scratch/release-download.sh" >/dev/null 2>&1); then fail 'self-test accepted an extra native build matrix target'; fi
  cp "$workflow" "$scratch/workflow.yml"
  sed -i.bak 's/--max-redirs 0/--location/' "$scratch/workflow.yml"
  # The workflow delegates transport to release-download.sh, so mutating its required call is the
  # static failure that proves this check is wired to the policy boundary.
  sed -i.bak '/scripts\/release-download.sh/d' "$scratch/workflow.yml"
  if (check "$scratch/workflow.yml" "$targets" "$scratch/release-download.sh" >/dev/null 2>&1); then fail 'self-test accepted transport-policy removal'; fi
  cp "$workflow" "$scratch/workflow.yml"
  sed -i.bak '/verify-staged/d' "$scratch/workflow.yml"
  if (check "$scratch/workflow.yml" "$targets" "$scratch/release-download.sh" >/dev/null 2>&1); then fail 'self-test accepted staged-verifier removal'; fi
  cp "$workflow" "$scratch/workflow.yml"
  sed -i.bak '/verify-compatibility/d' "$scratch/workflow.yml"
  if (check "$scratch/workflow.yml" "$targets" "$scratch/release-download.sh" >/dev/null 2>&1); then fail 'self-test accepted native production compatibility-verifier removal'; fi
  cp "$workflow" "$scratch/workflow.yml"
  sed -i.bak '/exchange\.invoke-response\.v1/d' "$scratch/workflow.yml"
  if (check "$scratch/workflow.yml" "$targets" "$scratch/release-download.sh" >/dev/null 2>&1); then fail 'self-test accepted a missing native protocol binding'; fi
  cp "$workflow" "$scratch/workflow.yml"
  printf '%s\n' '          gh release download "$RELEASE_TAG" --repo "$GITHUB_REPOSITORY"' >>"$scratch/workflow.yml"
  if (check "$scratch/workflow.yml" "$targets" "$scratch/release-download.sh" >/dev/null 2>&1); then fail 'self-test accepted an unbounded authenticated immutable-resume download'; fi
  cp "$workflow" "$scratch/workflow.yml"
  sed -i.bak '/GITHUB_REF.*refs\/tags/d' "$scratch/workflow.yml"
  if (check "$scratch/workflow.yml" "$targets" "$scratch/release-download.sh" >/dev/null 2>&1); then fail 'self-test accepted provenance tag-ref binding removal'; fi
  cp "$root/scripts/release-download.sh" "$scratch/release-download.sh"
  sed -i.bak '/--max-filesize/d' "$scratch/release-download.sh"
  if (check "$workflow" "$targets" "$scratch/release-download.sh" >/dev/null 2>&1); then fail 'self-test accepted receive-time byte-bound removal'; fi
  cp "$root/scripts/release-download.sh" "$scratch/release-download.sh"
  sed -i.bak '0,/curl --disable /s//curl /' "$scratch/release-download.sh"
  if (check "$workflow" "$targets" "$scratch/release-download.sh" >/dev/null 2>&1); then fail 'self-test accepted curl configuration loading'; fi
  printf 'PASS check-local-release self-test\n'
  exit 0
fi

check "$workflow" "$targets" "$root/scripts/release-download.sh"
printf 'PASS local release workflow and five-target contract\n'
