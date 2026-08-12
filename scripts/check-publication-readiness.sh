#!/usr/bin/env bash
# Fail closed before a release job installs Cargo, reads a secret, or reaches the network.
set -euo pipefail

root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)"
fail() { printf 'check-publication-readiness: %s\n' "$*" >&2; exit 1; }

check_wiring() {
  local tree="$1" local_workflow crates_workflow publish_script
  local checkout self_test ordinary secret toolchain metadata network
  local_workflow="$tree/.github/workflows/local-release.yml"
  crates_workflow="$tree/.github/workflows/crates-io.yml"
  publish_script="$tree/scripts/publish-crates-io.sh"

  checkout="$(grep -nF 'uses: actions/checkout@' "$local_workflow" | head -n1 | cut -d: -f1)"
  self_test="$(grep -nF './scripts/check-publication-readiness.sh --self-test' "$local_workflow" | head -n1 | cut -d: -f1)"
  ordinary="$(grep -nE '^[[:space:]]+\./scripts/check-publication-readiness\.sh$' "$local_workflow" | head -n1 | cut -d: -f1)"
  secret="$(grep -nF 'secrets.FLUX_EXCHANGE_' "$local_workflow" | head -n1 | cut -d: -f1)"
  toolchain="$(grep -nF 'uses: dtolnay/rust-toolchain@' "$local_workflow" | head -n1 | cut -d: -f1)"
  [ -n "$checkout" ] && [ -n "$self_test" ] && [ -n "$ordinary" ] && [ -n "$secret" ] && [ -n "$toolchain" ] \
    && [ "$checkout" -lt "$self_test" ] && [ "$self_test" -lt "$ordinary" ] \
    && [ "$ordinary" -lt "$secret" ] && [ "$ordinary" -lt "$toolchain" ] \
    || { printf '%s\n' 'check-publication-readiness: local-release preflight does not run readiness first after checkout and before secrets/tools' >&2; return 1; }

  checkout="$(grep -nF 'uses: actions/checkout@' "$crates_workflow" | head -n1 | cut -d: -f1)"
  self_test="$(grep -nF './scripts/check-publication-readiness.sh --self-test' "$crates_workflow" | head -n1 | cut -d: -f1)"
  ordinary="$(grep -nE '^[[:space:]]+\./scripts/check-publication-readiness\.sh$' "$crates_workflow" | head -n1 | cut -d: -f1)"
  secret="$(grep -nF 'secrets.CARGO_REGISTRY_TOKEN' "$crates_workflow" | head -n1 | cut -d: -f1)"
  toolchain="$(grep -nF 'uses: dtolnay/rust-toolchain@' "$crates_workflow" | head -n1 | cut -d: -f1)"
  [ -n "$checkout" ] && [ -n "$self_test" ] && [ -n "$ordinary" ] && [ -n "$secret" ] && [ -n "$toolchain" ] \
    && [ "$checkout" -lt "$self_test" ] && [ "$self_test" -lt "$ordinary" ] \
    && [ "$ordinary" -lt "$secret" ] && [ "$ordinary" -lt "$toolchain" ] \
    || { printf '%s\n' 'check-publication-readiness: crates.io workflow does not run readiness first after checkout and before tokens/tools' >&2; return 1; }

  ordinary="$(grep -nE '^\./scripts/check-publication-readiness\.sh( \|\| exit 1)?$' "$publish_script" | head -n1 | cut -d: -f1)"
  metadata="$(grep -nE '^[[:space:]]*cargo metadata ' "$publish_script" | head -n1 | cut -d: -f1)"
  network="$(grep -nE '^[[:space:]]*curl ' "$publish_script" | head -n1 | cut -d: -f1)"
  [ -n "$ordinary" ] && [ -n "$metadata" ] && [ -n "$network" ] \
    && [ "$ordinary" -lt "$metadata" ] && [ "$ordinary" -lt "$network" ] \
    || { printf '%s\n' 'check-publication-readiness: publish script does not repeat readiness before Cargo metadata and network access' >&2; return 1; }
}

check_tree() {
  python3 - "$1" <<'PY'
import hashlib
import json
import os
from pathlib import Path
import re
import sys
import tomllib

root = Path(sys.argv[1])


def refuse(message):
    print(f"check-publication-readiness: {message}", file=sys.stderr)
    raise SystemExit(1)


# X-137 deliberately contracts the platform and regenerates only interim fixtures. A tag must stay
# stopped until X-138 proves the retained provider recovery and X-139 records the final candidate-
# derived authority/fixture identity. CI exercises this check through its synthetic self-test; only
# publication paths invoke ordinary mode while either content contract remains incomplete.
for story_id, filename in (
    ("X-138", "X-138-bind-provider-recovery-and-native-c515-evidence.md"),
    ("X-139", "X-139-canonicalize-release-native-evidence.md"),
):
    try:
        story = (root / "docs/stories" / filename).read_text(encoding="utf-8")
    except OSError as error:
        refuse(f"cannot read publication prerequisite {story_id}: {error}")
    status = re.search(r"(?m)^status: ([a-z-]+)$", story)
    if status is None or status.group(1) != "done":
        refuse(f"interim Linux contraction is not publishable before {story_id} is done")


registry = "registry+https://github.com/rust-lang/crates.io-index"

try:
    authority_bytes = (root / "crates/exchange-release/native-evidence-v1.json").read_bytes()
    authority = json.loads(authority_bytes)
except (OSError, json.JSONDecodeError) as error:
    refuse(f"cannot read the canonical native-evidence authority: {error}")
authority_canonical = json.dumps(authority, ensure_ascii=False, separators=(",", ":"), sort_keys=True).encode()
if authority_bytes != authority_canonical or authority.get("schema") != "exchange.native-evidence.v1":
    refuse("native-evidence authority is not canonical schema v1 JSON")
upstream = [item for item in authority.get("authorities", {}).values() if item.get("class") == "inherited_upstream"]
if len(upstream) != 1 or not isinstance(upstream[0].get("package"), dict):
    refuse("native-evidence authority lacks one inherited upstream package identity")
upstream_package = upstream[0]["package"]

try:
    manifest = tomllib.loads((root / "Cargo.toml").read_text(encoding="utf-8"))
    lock = tomllib.loads((root / "Cargo.lock").read_text(encoding="utf-8"))
except (OSError, tomllib.TOMLDecodeError) as error:
    refuse(f"cannot read the committed Cargo selection: {error}")

dependency = manifest.get("workspace", {}).get("dependencies", {}).get("connector-secrets")
if not isinstance(dependency, dict):
    refuse("workspace connector-secrets is not one direct registry dependency table")
if dependency.get("package") != "codewandler-connector-secrets":
    refuse("workspace connector-secrets does not name the published provider crate")
if dependency.get("version") not in {"0.23", "0.23.0", "=0.23.0"}:
    refuse("workspace connector-secrets does not select the 0.23.0 release line")
if any(key in dependency for key in ("path", "git", "registry")):
    refuse("workspace connector-secrets uses a path, git, or alternate-registry source")

packages = lock.get("package", [])
connector_dependencies = sorted(
    value["package"] for value in manifest.get("workspace", {}).get("dependencies", {}).values()
    if isinstance(value, dict) and str(value.get("package", "")).startswith("codewandler-connector-")
)
if not connector_dependencies or len(connector_dependencies) != len(set(connector_dependencies)):
    refuse("workspace connector-family selection is absent or duplicated")
for name in connector_dependencies:
    selected = [package for package in packages if package.get("name") == name]
    if len(selected) != 1:
        refuse(f"Cargo.lock contains {len(selected)} instances of {name}, want exactly one")
    package = selected[0]
    if package.get("version") != "0.23.0":
        refuse(f"Cargo.lock selects {name} {package.get('version')!r}, want 0.23.0")
    if package.get("source") != registry or not re.fullmatch(r"[0-9a-f]{64}", package.get("checksum", "")):
        refuse(f"Cargo.lock does not authenticate crates.io bytes for {name} 0.23.0")
    if name == upstream_package.get("name") and package.get("checksum") != upstream_package.get("registry_sha256"):
        refuse("Cargo.lock connector-secrets checksum disagrees with its canonical upstream authority")

obsolete = root / "tests/fixtures/exchange-release-v1"
if obsolete.exists() or obsolete.is_symlink():
    refuse("the obsolete tests/fixtures/exchange-release-v1 candidate is still present")

old_identities = (
    "exchange.release-channel.v1",
    "exchange.release-manifest.v1",
    "exchange.compatibility.v1",
    "exchange.supervisor-ready.v1",
    "exchange.connection-plan.v1",
)
producer_paths = [
    root / ".github/workflows/local-release.yml",
    root / "scripts/check-local-release.sh",
    root / "scripts/release-stage.sh",
    root / "crates/exchange-release/src",
    root / "crates/exchange-release/examples/generate_fixtures.rs",
    root / "crates/exchange-server/src/protocol_identity.rs",
    root / "crates/exchange-server/src/supervisor.rs",
]
producer_files = []
for path in producer_paths:
    if path.is_dir():
        producer_files.extend(candidate for candidate in path.rglob("*") if candidate.is_file())
    elif path.is_file():
        producer_files.append(path)
for path in producer_files:
    try:
        text = path.read_text(encoding="utf-8")
    except UnicodeDecodeError:
        continue
    for identity in old_identities:
        if identity in text:
            refuse(f"obsolete producer identity {identity} remains in {path.relative_to(root)}")

fixture_root = root / "tests/fixtures/exchange-release-v2"
fixture_manifest = fixture_root / "fixture-set.json"
if fixture_root.is_symlink():
    refuse("the v2 fixture root is a symlink")
try:
    fixture_bytes = fixture_manifest.read_bytes()
    fixture = json.loads(fixture_bytes)
except (OSError, json.JSONDecodeError) as error:
    refuse(f"cannot read the v2 fixture-set manifest: {error}")
canonical = json.dumps(fixture, ensure_ascii=False, separators=(",", ":"), sort_keys=True).encode()
if fixture_bytes != canonical:
    refuse("the v2 fixture-set manifest is not canonical JSON")
if fixture.get("schema") != "exchange.release-fixture-set.v2":
    refuse("the fixture-set schema is not exchange.release-fixture-set.v2")
if not re.fullmatch(r"[0-9a-f]{40}", fixture.get("exchange_commit", "")):
    refuse("fixture-set exchange_commit is not one lowercase commit identity")

declared = fixture.get("files")
if not isinstance(declared, dict) or list(declared) != sorted(declared):
    refuse("fixture-set files must be one lexically ordered path-to-SHA-256 object")
actual = {}
for directory, directories, files in os.walk(fixture_root, followlinks=False):
    directories.sort()
    files.sort()
    directory_path = Path(directory)
    for name in directories:
        path = directory_path / name
        if path.is_symlink():
            refuse(f"fixture inventory contains a symlinked directory: {path.relative_to(fixture_root)}")
    for name in files:
        path = directory_path / name
        if path == fixture_manifest:
            continue
        if path.is_symlink() or not path.is_file():
            refuse(f"fixture inventory contains a non-regular file: {path.relative_to(fixture_root)}")
        relative = path.relative_to(fixture_root).as_posix()
        actual[relative] = hashlib.sha256(path.read_bytes()).hexdigest()
if declared != actual:
    missing = sorted(set(actual) - set(declared))
    stale = sorted(set(declared) - set(actual))
    changed = sorted(path for path in set(actual) & set(declared) if actual[path] != declared[path])
    refuse(f"fixture inventory mismatch (unlisted={missing[:3]}, absent={stale[:3]}, changed={changed[:3]})")

protocols = {
    "exchange_api": "exchange.api.v1",
    "effective_catalogue_response": "exchange.effective-catalogue-response.v1",
    "invoke_request": "exchange.invoke-request.v1",
    "invoke_response": "exchange.invoke-response.v1",
    "connection_plan": "exchange.connection-plan.v2",
    "local_management": "exchange.local-management.v1",
    "service_account_handoff": "exchange.service-account-handoff.v1",
    "supervisor": "exchange.supervisor-ready.v2",
}
canonical_documents = {
    "positive/flux-exchange-release-trust.json": "exchange.release-trust.v1",
    "positive/flux-exchange-release-channel.json": "exchange.release-channel.v2",
    "positive/flux-exchange-release-manifest.json": "exchange.release-manifest.v2",
    "compatibility.json": "exchange.compatibility.v2",
    "readiness-linux.json": "exchange.supervisor-ready.v2",
}
for relative, schema in canonical_documents.items():
    try:
        document = json.loads((fixture_root / relative).read_bytes())
    except (OSError, json.JSONDecodeError) as error:
        refuse(f"cannot read canonical fixture {relative}: {error}")
    if document.get("schema") != schema:
        refuse(f"canonical fixture {relative} has schema {document.get('schema')!r}, want {schema}")
    if relative.endswith("release-trust.json"):
        if "protocols" in document:
            refuse("release trust must not grow a protocol compatibility claim")
    elif relative.endswith("release-channel.json"):
        releases = document.get("releases")
        if not isinstance(releases, list) or not releases or any(release.get("protocols") != protocols for release in releases if isinstance(release, dict)) or any(not isinstance(release, dict) for release in releases):
            refuse(f"canonical fixture {relative} does not carry the exact eight-protocol identity in every release")
    elif document.get("protocols") != protocols:
        refuse(f"canonical fixture {relative} does not carry the exact eight-protocol identity")

required_trust_cases = {
    "trust-rollback", "trust-equivocation", "minisign-key-malformed",
    "minisign-key-wrong-length", "minisign-key-wrong-algorithm",
    "minisign-key-embedded-id-disagreement", "minisign-key-reused",
    "minisign-key-reused-within-role", "minisign-key-reused-with-root", "role-confusion",
    "key-id-empty", "key-id-overlong", "key-id-slash", "key-id-double-hyphen",
    "key-id-leading-punctuation", "key-id-trailing-punctuation", "key-id-nonascii",
    "key-id-uppercase", "trust-future-issued", "root-threshold-failure",
    "trust-signature-missing", "trust-signature-substituted", "release-threshold-failure",
    "channel-threshold-failure", "channel-signature-missing", "channel-signature-substituted",
    "manifest-signature-missing", "manifest-signature-key-id-disagree",
    "github-initial-trust", "key-id-substituted",
}
cases = fixture.get("cases")
if not isinstance(cases, list):
    refuse("fixture-set cases is not a list")
case_ids = [case.get("id") for case in cases if isinstance(case, dict)]
if len(case_ids) != len(set(case_ids)) or None in case_ids:
    refuse("fixture-set case ids are absent or duplicated")
missing_cases = sorted(required_trust_cases - set(case_ids))
if missing_cases:
    refuse(f"v2 fixtures dropped trust-v1 refusal cases: {missing_cases}")

def has_gap(value):
    if isinstance(value, dict):
        return any(key == "gap" or has_gap(item) for key, item in value.items())
    if isinstance(value, list):
        return any(has_gap(item) for item in value)
    return value == "gap"


if has_gap(authority):
    refuse("native-evidence authority contains a publication gap")
targets = authority.get("targets")
target_sets = authority.get("target_sets")
bindings = authority.get("bindings")
release_evidence = authority.get("release_evidence")
obligations = authority.get("obligations")
if not all(isinstance(value, dict) and value for value in (targets, target_sets, bindings, release_evidence, obligations)):
    refuse("native-evidence authority omits targets, sets, bindings, release evidence or obligations")
target_ids = set(targets)
for set_id, selected in target_sets.items():
    if not isinstance(selected, list) or selected != sorted(set(selected)) or not set(selected) <= target_ids:
        refuse(f"native target set {set_id!r} is empty, duplicated or unknown")
used_bindings = set()
used_release_evidence = set()
expected_native = []
for obligation_id, obligation in obligations.items():
    if not isinstance(obligation, dict) or obligation.get("target_set") not in target_sets:
        refuse(f"native obligation {obligation_id!r} has no authoritative target set")
    binding_ids = obligation.get("bindings")
    inherited_ids = obligation.get("release_evidence")
    adversarial = obligation.get("adversarial_cases")
    if not isinstance(binding_ids, list) or not isinstance(inherited_ids, list) or not isinstance(adversarial, list) or not adversarial:
        refuse(f"native obligation {obligation_id!r} is incomplete")
    evidence = []
    covered = set()
    for binding_id in binding_ids:
        binding = bindings.get(binding_id)
        if not isinstance(binding, dict) or binding.get("target_set") not in target_sets:
            refuse(f"native obligation {obligation_id!r} names an invalid Cargo binding")
        selected = target_sets[binding["target_set"]]
        cargo_target = binding.get("cargo_target")
        if not isinstance(cargo_target, dict) or not binding.get("exact_test"):
            refuse(f"native binding {binding_id!r} lacks one exact Cargo test")
        test_target = "lib" if cargo_target.get("kind") == "lib" else cargo_target.get("name")
        if not test_target:
            refuse(f"native binding {binding_id!r} lacks one Cargo target")
        evidence.append({
            "targets": selected,
            "test_target": test_target,
            "exact_test": binding["exact_test"],
        })
        covered.update(selected)
        used_bindings.add(binding_id)
    for evidence_id in inherited_ids:
        inherited = release_evidence.get(evidence_id)
        if not isinstance(inherited, dict) or inherited.get("target_set") not in target_sets:
            refuse(f"native obligation {obligation_id!r} names invalid inherited evidence")
        covered.update(target_sets[inherited["target_set"]])
        used_release_evidence.add(evidence_id)
    if covered != set(target_sets[obligation["target_set"]]):
        refuse(f"native obligation {obligation_id!r} does not exactly cover its target set")
    expected_native.append({
        "id": obligation_id,
        "authority": obligation.get("authority"),
        "evidence": evidence,
        "release_evidence": inherited_ids,
    })
if used_bindings != set(bindings) or used_release_evidence != set(release_evidence):
    refuse("native authority contains unreferenced Cargo or inherited evidence")
native_identity = hashlib.sha256(authority_canonical).hexdigest()
if fixture.get("native_evidence_sha256") != native_identity:
    refuse("fixture-set does not name the canonical native-evidence authority identity")
if fixture.get("native_cases") != expected_native:
    refuse("fixture-set native cases are not the exact authority-derived projection")
native_ids = {case["id"] for case in expected_native}

required_contract_cases = {
    "positive-linux", "positive-signer-overlap",
    "integer-over-jcs-safe", "decimal-noncanonical", "id-or-basename-unsafe",
    "minisign-key-malformed", "minisign-key-reused", "channel-floor-survives-rotation",
    "higher-channel-no-compatible", "higher-channel-target-fails", "same-number-different-bytes",
    "expiry-equality-stopped", "readiness-bind-domain", "readiness-start-kind",
    "provenance-client-input",
}
missing_contract_cases = sorted(required_contract_cases - set(case_ids) - set(native_ids))
if missing_contract_cases:
    refuse(f"v2 fixtures omit minimum contract cases: {missing_contract_cases}")

print("PASS publication readiness: registry 0.23, v2 producers/fixtures, eight protocols and authority-derived native evidence")
PY
}

if [ "${1:-}" = "--self-test" ]; then
  scratch="$(mktemp -d "${TMPDIR:-/tmp}/flux-exchange-publication-readiness.XXXXXX")"
  trap 'rm -rf -- "$scratch"' EXIT
  python3 - "$scratch" "$root" <<'PY'
import hashlib
import json
from pathlib import Path
import sys

root = Path(sys.argv[1])
source_root = Path(sys.argv[2])
(root / "scripts").mkdir(parents=True)
(root / ".github/workflows").mkdir(parents=True)
(root / "crates/exchange-release/src").mkdir(parents=True)
(root / "crates/exchange-release/examples").mkdir(parents=True)
(root / "crates/exchange-server/src").mkdir(parents=True)
(root / "docs/stories").mkdir(parents=True)
authority_bytes = (source_root / "crates/exchange-release/native-evidence-v1.json").read_bytes()
authority = json.loads(authority_bytes)
(root / "crates/exchange-release/native-evidence-v1.json").write_bytes(authority_bytes)
for filename in (
    "X-138-bind-provider-recovery-and-native-c515-evidence.md",
    "X-139-canonicalize-release-native-evidence.md",
):
    (root / "docs/stories" / filename).write_text("---\nstatus: done\n---\n", encoding="utf-8")
for relative in (
    ".github/workflows/local-release.yml", "scripts/check-local-release.sh",
    "scripts/release-stage.sh", "crates/exchange-release/src/model.rs",
    "crates/exchange-release/examples/generate_fixtures.rs",
    "crates/exchange-server/src/protocol_identity.rs", "crates/exchange-server/src/supervisor.rs",
):
    (root / relative).write_text("exchange.release-channel.v2\n", encoding="utf-8")

(root / ".github/workflows/local-release.yml").write_text('''exchange.release-channel.v2
- uses: actions/checkout@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
  with:
    ref: tag
- name: readiness
  run: |
    ./scripts/check-publication-readiness.sh --self-test
    ./scripts/check-publication-readiness.sh
- name: secret
  env:
    KEY: ${{ secrets.FLUX_EXCHANGE_CHANNEL_SIGNING_KEY_B64 }}
- uses: dtolnay/rust-toolchain@bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
''', encoding="utf-8")
(root / ".github/workflows/crates-io.yml").write_text('''- uses: actions/checkout@aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa
- name: readiness
  run: |
    ./scripts/check-publication-readiness.sh --self-test
    ./scripts/check-publication-readiness.sh
- name: token
  env:
    TOKEN: ${{ secrets.CARGO_REGISTRY_TOKEN }}
- uses: dtolnay/rust-toolchain@bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
''', encoding="utf-8")
(root / "scripts/publish-crates-io.sh").write_text('''#!/usr/bin/env bash
./scripts/check-publication-readiness.sh || exit 1
cargo metadata --offline
curl https://crates.io
''', encoding="utf-8")

(root / "Cargo.toml").write_text('''[workspace]\n[workspace.dependencies]\nconnector-secrets = { package = "codewandler-connector-secrets", version = "0.23" }\n''')
upstream = next(item for item in authority["authorities"].values() if item["class"] == "inherited_upstream")["package"]
lock = "version = 4\n\n"
lock += f'[[package]]\nname = "{upstream["name"]}"\nversion = "{upstream["version"]}"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\nchecksum = "{upstream["registry_sha256"]}"\n\n'
(root / "Cargo.lock").write_text(lock, encoding="utf-8")

fixtures = root / "tests/fixtures/exchange-release-v2"
(fixtures / "positive").mkdir(parents=True)
protocols = {
    "exchange_api": "exchange.api.v1", "effective_catalogue_response": "exchange.effective-catalogue-response.v1",
    "invoke_request": "exchange.invoke-request.v1", "invoke_response": "exchange.invoke-response.v1",
    "connection_plan": "exchange.connection-plan.v2", "local_management": "exchange.local-management.v1",
    "service_account_handoff": "exchange.service-account-handoff.v1", "supervisor": "exchange.supervisor-ready.v2",
}
documents = {
    "positive/flux-exchange-release-trust.json": {"schema": "exchange.release-trust.v1"},
    "positive/flux-exchange-release-channel.json": {"schema": "exchange.release-channel.v2", "releases": [{"protocols": protocols}]},
    "positive/flux-exchange-release-manifest.json": {"schema": "exchange.release-manifest.v2", "protocols": protocols},
    "compatibility.json": {"schema": "exchange.compatibility.v2", "protocols": protocols},
    "readiness-linux.json": {"schema": "exchange.supervisor-ready.v2", "protocols": protocols},
}
for relative, document in documents.items():
    path = fixtures / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(document, separators=(",", ":"), sort_keys=True), encoding="utf-8")

trust_cases = '''trust-rollback trust-equivocation minisign-key-malformed minisign-key-wrong-length minisign-key-wrong-algorithm minisign-key-embedded-id-disagreement minisign-key-reused minisign-key-reused-within-role minisign-key-reused-with-root role-confusion key-id-empty key-id-overlong key-id-slash key-id-double-hyphen key-id-leading-punctuation key-id-trailing-punctuation key-id-nonascii key-id-uppercase trust-future-issued root-threshold-failure trust-signature-missing trust-signature-substituted release-threshold-failure channel-threshold-failure channel-signature-missing channel-signature-substituted manifest-signature-missing manifest-signature-key-id-disagree github-initial-trust key-id-substituted positive-linux positive-signer-overlap integer-over-jcs-safe decimal-noncanonical id-or-basename-unsafe channel-floor-survives-rotation higher-channel-no-compatible higher-channel-target-fails same-number-different-bytes expiry-equality-stopped readiness-bind-domain readiness-start-kind provenance-client-input'''.split()
native = []
for case_id, obligation in authority["obligations"].items():
    evidence = []
    for binding_id in obligation["bindings"]:
        binding = authority["bindings"][binding_id]
        target = binding["cargo_target"]
        evidence.append({
            "targets": authority["target_sets"][binding["target_set"]],
            "test_target": "lib" if target["kind"] == "lib" else target["name"],
            "exact_test": binding["exact_test"],
        })
    native.append({
        "id": case_id,
        "authority": obligation["authority"],
        "evidence": evidence,
        "release_evidence": obligation["release_evidence"],
    })
files = {}
for path in sorted(candidate for candidate in fixtures.rglob("*") if candidate.is_file()):
    relative = path.relative_to(fixtures).as_posix()
    files[relative] = hashlib.sha256(path.read_bytes()).hexdigest()
fixture = {
    "schema": "exchange.release-fixture-set.v2", "exchange_commit": "a" * 40,
    "native_evidence_sha256": hashlib.sha256(authority_bytes).hexdigest(),
    "cases": [{"id": case} for case in dict.fromkeys(trust_cases)], "native_cases": native, "files": files,
}
(fixtures / "fixture-set.json").write_text(json.dumps(fixture, separators=(",", ":"), sort_keys=True), encoding="utf-8")
PY

  tripwire="$scratch/tripwire"
  mkdir "$tripwire"
  marker="$scratch/forbidden-command-ran"
  for command in cargo curl wget gh rustup npm npx pip; do
    printf '#!/usr/bin/env bash\nprintf "%%s\\n" "$0" >>"%s"\nexit 97\n' "$marker" >"$tripwire/$command"
    chmod +x "$tripwire/$command"
  done
  PATH="$tripwire:$PATH" check_tree "$scratch" >/dev/null
  check_wiring "$scratch"
  [ ! -e "$marker" ] || fail "self-test: the readiness check executed a Cargo or network-capable command"

  cp "$scratch/docs/stories/X-138-bind-provider-recovery-and-native-c515-evidence.md" "$scratch/x138.clean"
  sed -i.bak 's/status: done/status: blocked/' "$scratch/docs/stories/X-138-bind-provider-recovery-and-native-c515-evidence.md"
  if check_tree "$scratch" >/dev/null 2>&1; then fail "self-test: accepted the interim platform contraction for publication"; fi
  mv "$scratch/x138.clean" "$scratch/docs/stories/X-138-bind-provider-recovery-and-native-c515-evidence.md"

  expect_refusal() {
    local label="$1"
    shift
    "$@"
    if check_tree "$scratch" >/dev/null 2>&1; then
      fail "self-test: accepted $label"
    fi
  }

  cp "$scratch/Cargo.lock" "$scratch/Cargo.lock.clean"
  expect_refusal "a changed registry checksum" sed -i.bak 's/360225fcfbd3/060225fcfbd3/' "$scratch/Cargo.lock"
  mv "$scratch/Cargo.lock.clean" "$scratch/Cargo.lock"
  cp "$scratch/Cargo.toml" "$scratch/Cargo.toml.clean"
  sed -i.bak 's/version = "0.23"/version = "0.23", path = "..\/provider"/' "$scratch/Cargo.toml"
  if check_tree "$scratch" >/dev/null 2>&1; then fail "self-test: accepted a path provider dependency"; fi
  mv "$scratch/Cargo.toml.clean" "$scratch/Cargo.toml"

  cp "$scratch/.github/workflows/local-release.yml" "$scratch/local-release.clean"
  sed -i.bak '/check-publication-readiness/d' "$scratch/.github/workflows/local-release.yml"
  if check_wiring "$scratch" >/dev/null 2>&1; then fail "self-test: accepted local-release readiness removal"; fi
  mv "$scratch/local-release.clean" "$scratch/.github/workflows/local-release.yml"
  cp "$scratch/.github/workflows/crates-io.yml" "$scratch/crates-io.clean"
  sed -i.bak '/check-publication-readiness/d' "$scratch/.github/workflows/crates-io.yml"
  if check_wiring "$scratch" >/dev/null 2>&1; then fail "self-test: accepted crates.io readiness removal"; fi
  mv "$scratch/crates-io.clean" "$scratch/.github/workflows/crates-io.yml"
  cp "$scratch/scripts/publish-crates-io.sh" "$scratch/publish.clean"
  sed -i.bak '/check-publication-readiness/d' "$scratch/scripts/publish-crates-io.sh"
  if check_wiring "$scratch" >/dev/null 2>&1; then fail "self-test: accepted publish-script readiness removal"; fi
  mv "$scratch/publish.clean" "$scratch/scripts/publish-crates-io.sh"
  mkdir -p "$scratch/tests/fixtures/exchange-release-v1"
  if check_tree "$scratch" >/dev/null 2>&1; then fail "self-test: accepted the obsolete v1 fixture tree"; fi
  rm -rf "$scratch/tests/fixtures/exchange-release-v1"
  cp "$scratch/scripts/release-stage.sh" "$scratch/scripts/release-stage.sh.clean"
  printf '%s\n' 'exchange.release-manifest.v1' >>"$scratch/scripts/release-stage.sh"
  if check_tree "$scratch" >/dev/null 2>&1; then fail "self-test: accepted a v1 producer"; fi
  mv "$scratch/scripts/release-stage.sh.clean" "$scratch/scripts/release-stage.sh"

  fixture="$scratch/tests/fixtures/exchange-release-v2/compatibility.json"
  cp "$fixture" "$fixture.clean"
  printf ' ' >>"$fixture"
  if check_tree "$scratch" >/dev/null 2>&1; then fail "self-test: accepted changed inventoried bytes"; fi
  mv "$fixture.clean" "$fixture"
  extra="$scratch/tests/fixtures/exchange-release-v2/unlisted.json"
  printf '{}\n' >"$extra"
  if check_tree "$scratch" >/dev/null 2>&1; then fail "self-test: accepted an unlisted fixture"; fi
  rm "$extra"

  fixture_set="$scratch/tests/fixtures/exchange-release-v2/fixture-set.json"
  cp "$fixture_set" "$fixture_set.protocol-clean"
  cp "$scratch/tests/fixtures/exchange-release-v2/compatibility.json" "$scratch/compatibility.clean"
  python3 - "$scratch" <<'PY'
import hashlib, json, pathlib, sys
root=pathlib.Path(sys.argv[1]); fixtures=root/"tests/fixtures/exchange-release-v2"
document=fixtures/"compatibility.json"; value=json.load(open(document)); del value["protocols"]["local_management"]
document.write_text(json.dumps(value,separators=(",",":"),sort_keys=True))
manifest=fixtures/"fixture-set.json"; inventory=json.load(open(manifest)); inventory["files"]["compatibility.json"]=hashlib.sha256(document.read_bytes()).hexdigest()
manifest.write_text(json.dumps(inventory,separators=(",",":"),sort_keys=True))
PY
  if check_tree "$scratch" >/dev/null 2>&1; then fail "self-test: accepted a seven-protocol canonical fixture"; fi
  mv "$scratch/compatibility.clean" "$scratch/tests/fixtures/exchange-release-v2/compatibility.json"
  mv "$fixture_set.protocol-clean" "$fixture_set"

  cp "$fixture_set" "$fixture_set.clean"
  python3 - "$fixture_set" <<'PY'
import json, sys
path=sys.argv[1]; value=json.load(open(path)); next(case for case in value["native_cases"] if case["evidence"])["evidence"].pop()
open(path,"w").write(json.dumps(value,separators=(",",":"),sort_keys=True))
PY
  if check_tree "$scratch" >/dev/null 2>&1; then fail "self-test: accepted a missing authority-derived native binding"; fi
  mv "$fixture_set.clean" "$fixture_set"

  cp "$fixture_set" "$fixture_set.clean"
  python3 - "$fixture_set" <<'PY'
import json, sys
path=sys.argv[1]; value=json.load(open(path)); next(case for case in value["native_cases"] if case["evidence"])["evidence"][0]["exact_test"]="renamed_or_substituted_test"
open(path,"w").write(json.dumps(value,separators=(",",":"),sort_keys=True))
PY
  if check_tree "$scratch" >/dev/null 2>&1; then fail "self-test: accepted a substituted authority-derived exact test"; fi
  mv "$fixture_set.clean" "$fixture_set"

  authority="$scratch/crates/exchange-release/native-evidence-v1.json"
  cp "$authority" "$authority.clean"
  python3 - "$authority" <<'PY'
import json, sys
path=sys.argv[1]; value=json.load(open(path)); next(iter(value["bindings"].values()))["exact_test"]="renamed_or_substituted_test"
open(path,"w").write(json.dumps(value,separators=(",",":"),sort_keys=True))
PY
  if check_tree "$scratch" >/dev/null 2>&1; then fail "self-test: accepted a substituted canonical Cargo binding"; fi
  mv "$authority.clean" "$authority"

  cp "$authority" "$authority.clean"
  python3 - "$authority" <<'PY'
import json, sys
path=sys.argv[1]; value=json.load(open(path)); next(iter(value["targets"].values()))["runner"] += "-substituted"
open(path,"w").write(json.dumps(value,separators=(",",":"),sort_keys=True))
PY
  if check_tree "$scratch" >/dev/null 2>&1; then fail "self-test: accepted a substituted canonical native runner"; fi
  mv "$authority.clean" "$authority"

  cp "$authority" "$authority.clean"
  python3 - "$authority" <<'PY'
import json, sys
path=sys.argv[1]; value=json.load(open(path)); value["obligations"].pop(next(iter(value["obligations"])))
open(path,"w").write(json.dumps(value,separators=(",",":"),sort_keys=True))
PY
  if check_tree "$scratch" >/dev/null 2>&1; then fail "self-test: accepted a missing canonical native family"; fi
  mv "$authority.clean" "$authority"

  cp "$fixture_set" "$fixture_set.clean"
  python3 - "$fixture_set" <<'PY'
import json, sys
path=sys.argv[1]; value=json.load(open(path)); value["cases"]=[case for case in value["cases"] if case["id"] != "trust-rollback"]
open(path,"w").write(json.dumps(value,separators=(",",":"),sort_keys=True))
PY
  if check_tree "$scratch" >/dev/null 2>&1; then fail "self-test: accepted a dropped trust-v1 refusal"; fi
  mv "$fixture_set.clean" "$fixture_set"

  printf 'PASS self-test: dependency, interim closure, inventory, trust and two-target readiness mutations refused without Cargo or network access\n'
  exit 0
fi

[ "$#" -eq 0 ] || fail 'usage: check-publication-readiness.sh [--self-test]'
check_tree "$root"
check_wiring "$root" || exit 1
