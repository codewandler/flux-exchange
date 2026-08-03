# Managed credential storage on AWS Secrets Manager

## Decision

Public Fly deployments will bind the existing `connector_secrets::SecretStore` port to AWS Secrets
Manager. The backend adapter belongs in `connector-secrets`, the crate that owns the port and its
batch semantics; `exchange-host::CredentialStore` constructs that released adapter without exposing
AWS SDK types to routes or the server. It is not a credential API, an operation transport, or a
second request-building path. The server selects exactly one credential store at startup. Settings,
grants, audit evidence, and every other non-secret store retain their current meanings and
locations.

The workload authenticates to AWS with a Fly Machine OIDC token and AWS STS
`AssumeRoleWithWebIdentity`. No AWS access key is placed in a repository, image, Fly secret, or
ordinary configuration. The trust policy admits one Fly application, while the role and KMS policy
admit only the deployment's secret prefix and required operations.

AWS Secrets Manager versions one secret at a time and does not provide a transaction across
secrets. This design therefore commits each existing `SecretBatch` through one versioned scope
manifest. Immutable value objects become visible only when that manifest advances. The first public
deployment has one Machine and one writer; horizontal writers are not supported by this backend
until a distributed compare-and-swap or transactional owner exists.

This document is the design for X-97. It does not claim that the adapter, migration, release, vendor
rotation, or old-volume destruction has happened.

## Boundaries

The implementation must preserve these boundaries:

- `CredentialStore` continues to wrap an `Arc<dyn SecretStore>`. It may construct the released
  `connector-secrets` AWS binding; routes, connection lifecycle, invocation, and channel
  supervision do not learn AWS types.
- The principal supplies the tenant. A request cannot supply an AWS name, prefix, tenant, secret
  ARN, version, or KMS key.
- Connector execution still ends in `connector_pack` and the connector-declared Flux runtime. The
  Secrets Manager client only implements the existing storage port.
- The adapter exposes no operation other than the existing `get`, `put`, `delete`, `references`,
  and atomic `apply` contract.
- No read falls back to the file store and no write is mirrored to it. Exactly one backend is active.
- Names, manifests, tags, metrics, and logs contain addresses and state, never credential values.

The future dependency allow-list change updates the existing `connector-secrets` entry: its optional
AWS client transports the existing secret-store operations to a managed store; it cannot dispatch a
connector operation or construct a vendor request. The client and its dependencies are optional
behind one narrowly named feature propagated by `exchange-host`, and the composing server enables
that feature for the public image. `exchange-host` does not add a direct AWS SDK dependency.

## Upstream prerequisite

`SecretBatch` exposes its scope but deliberately keeps its mutation list private to
`connector-secrets`. An adapter implemented in `exchange-host` could implement point reads and
writes, but it could not interpret moves, puts, and deletes to implement atomic `apply`. Reaching
into private state, replaying route behavior, or declaring `apply` unsupported would violate the
existing port or X-97's acceptance.

The AWS adapter must therefore land in `connector-secrets`, alongside its file, memory, and Vault
bindings, where it can apply the checked batch to a manifest without adding another public
credential mutation API. It should retain the crate's testable transport seam: store semantics are
tested against a scripted AWS transport, while SDK wiring is feature-gated. Exchange then consumes
a published crates.io release—never a sibling `path` or `git` dependency—and propagates its
`aws-secrets-manager` feature through `exchange-host` to the server composition. Updating that
connector release follows this repository's engine-line rule: connector and Flux pins move together
and the seam/lockfile tests prove one 0.54 engine line.

## Workload identity

Fly publishes an OIDC issuer per organization, accepts a caller-selected audience, and defines a
Machine subject as `<organization>:<application>:<machine>`. Its AWS integration can obtain a token
at Machine initialization, write it to `/.fly/oidc_token`, and set
`AWS_WEB_IDENTITY_TOKEN_FILE`, `AWS_ROLE_ARN`, and `AWS_ROLE_SESSION_NAME` for an AWS SDK process.
See [Fly OIDC](https://fly.io/docs/security/openid-connect/) and Fly's
[OIDC cloud-role walkthrough](https://fly.io/blog/oidc-cloud-roles/).

The AWS IAM OIDC provider and role trust are provisioned outside this process. The trust is
app-scoped, not merely organization-scoped:

```json
{
  "Effect": "Allow",
  "Principal": {
    "Federated": "arn:aws:iam::<account>:oidc-provider/oidc.fly.io/<organization>"
  },
  "Action": "sts:AssumeRoleWithWebIdentity",
  "Condition": {
    "StringEquals": {
      "oidc.fly.io/<organization>:aud": "sts.amazonaws.com"
    },
    "StringLike": {
      "oidc.fly.io/<organization>:sub": "<organization>:<application>:*"
    }
  }
}
```

The concrete provider-key spelling must be generated from the issuer recorded by IAM and checked
against Fly's current example during provisioning. The audience is exactly `sts.amazonaws.com`; the
subject admits every Machine identity for this application and no other application. AWS validates
the signed token and role trust and returns temporary credentials. The SDK credential provider
caches and refreshes them; `AssumeRoleWithWebIdentity` does not require a signed request. See AWS's
[temporary credential flow](https://docs.aws.amazon.com/IAM/latest/UserGuide/id_credentials_temp_request.html),
[web-identity SDK settings](https://docs.aws.amazon.com/sdkref/latest/guide/access-assume-role-web.html),
and [Rust credential providers](https://docs.aws.amazon.com/sdk-for-rust/latest/dg/credproviders.html).

The adapter constructs the web-identity credential provider explicitly; it does not use the SDK's
general default chain, which could select environment, shared-file, process, or instance
credentials. When this backend is selected, startup refuses `AWS_ACCESS_KEY_ID`,
`AWS_SECRET_ACCESS_KEY`, `AWS_SESSION_TOKEN`, `AWS_PROFILE`, and
`AWS_SHARED_CREDENTIALS_FILE`. The accepted deployment inputs are non-secret: region, role ARN,
OIDC token-file path supplied by Fly, deployment prefix, and KMS key ARN. Missing or contradictory
identity inputs refuse startup; there is no file-store fallback.

## Names and stored documents

An operator assigns an immutable deployment identifier and prefix `P`, for example:

```text
flux-exchange/<deployment-id>/credentials/v1
```

The adapter validates `P` at startup. AWS secret names accept ASCII letters, digits, and
`/_+=.@-`, and are limited to 512 characters. The adapter never truncates or hashes a rejected
address. It also appends a fixed final component so no name ends in a hyphen followed by six
characters, which AWS warns can be confused with its ARN suffix. See
[CreateSecret](https://docs.aws.amazon.com/secretsmanager/latest/apireference/API_CreateSecret.html)
and [what a secret contains](https://docs.aws.amazon.com/secretsmanager/latest/userguide/whats-in-a-secret.html).

The mapping is deterministic and derived only from `TenantLayout`:

```text
scope head H(s)  = P/manifests/tenants/<tenant>/<authority>/head
value V(r, tx)   = P/objects/<TenantLayout.render(r)>/<transaction-uuid>/value
cutover control  = P/control/cutover
```

The complete name must pass the AWS grammar and length check before a request is sent. Failure is
`StoreError::Layout`. The canonical address, UUID, description, and idempotency token contain no
value-derived data. The adapter sends no tags, so the workload does not need `TagResource`. In
particular, it records no digest of a credential because a digest can become an offline oracle for
a low-entropy secret.

Each value secret contains only the credential bytes in `SecretBinary`. Exchange's existing value
limit remains stricter than AWS's 65,536-byte request limit. Each scope head contains a versioned,
value-free manifest:

```text
schema version
canonical tenant and authority
generation and transaction UUID
canonical credential address -> {value secret ARN, exact VersionId}
```

The manifest must parse into the scope named by `H(s)`, every key must be a canonical address in
that same scope, and every referenced ARN must be under `P/objects/`. Unknown schema versions,
duplicate addresses, a scope mismatch, or a reference outside the prefix refuses the whole read.
No partial manifest is served.

The full ARN returned by `CreateSecret` is stored because AWS appends six random characters to an
ARN. Reads specify both the immutable object's ARN and its exact `VersionId`; they never accept
whatever happens to be `AWSCURRENT`. AWS documents the default stage and exact-version behavior in
[GetSecretValue](https://docs.aws.amazon.com/secretsmanager/latest/apireference/API_GetSecretValue.html).

## Store semantics and atomic batches

Every process holds a mutex per `(tenant, authority)` scope. This is sufficient only because the
public deployment has one Machine and therefore one backend writer. Startup must refuse a
multi-Machine managed-backend configuration. This is a deployment invariant, not a claim that an
in-process mutex is distributed coordination.

On first use, the adapter reads the scope's `AWSCURRENT` head and validates it. Its per-scope state
then retains that exact head `VersionId`; a local commit replaces the retained ID under the same
lock. `get(reference)` reads that exact head version, resolves the exact object ARN and version, and
returns its `SecretBinary`. `references(scope)` returns the manifest keys. A genuinely missing head
is an empty scope; a present head that omits a requested address is `NotFound`.

`put`, `delete`, and `apply` use the same transaction:

1. Lock the scope and read its current validated manifest.
2. Allocate one transaction UUID. Create an immutable value object for each put, using a stable
   `ClientRequestToken` for idempotency, and read it back by the returned exact version.
3. Apply all moves, puts, and deletes to a new manifest in memory. The existing `SecretBatch`
   single-scope rule remains authoritative.
4. Create the head if the scope is new, or add one head version with `PutSecretValue`. Advancing
   that one secret to `AWSCURRENT` is the commit point.
5. After commit, schedule value objects no longer referenced by the head for deletion with AWS's
   minimum seven-day recovery window. Cleanup is idempotent and retried.

Before step 4, readers can only observe the old manifest. After it, a reader can observe either the
old or new complete manifest, never a mixture: AWS explicitly documents eventual consistency, so a
stage change may remain briefly stale at an endpoint. The writer retains the exact committed head
version and will not acknowledge success until both that version and its `AWSCURRENT` stage read
back within a bounded deadline. On restart, startup similarly waits for the cutover and head
versions it is required to serve. See AWS's
[eventual-consistency warning](https://docs.aws.amazon.com/secretsmanager/latest/userguide/troubleshoot.html).

A pre-commit error leaves the prior logical store intact and schedules newly created unreachable
objects for cleanup. A timeout while updating the head is ambiguous, so the adapter resolves it by
reading the transaction's head version and `AWSCURRENT` state before returning. It never retries an
unknown commit as a new transaction. If stage convergence cannot be established, the call returns
`Conflict` and the scope enters a read-only degraded state until reconciliation proves which
complete generation is current. Later mutations refuse rather than treating the ambiguous result as
safe to retry.

Before the manifest write, a durable, value-free cleanup journal on the existing non-secret volume
records the transaction, prior head generation, newly allocated object ARNs, and objects that would
be retired. Recovery reads the head to decide whether the transaction committed: an uncommitted
intent deletes only its new objects; a committed intent deletes only retired objects. This closes
the crash window between commit and queueing cleanup.

Post-commit cleanup failure does not turn a committed mutation into an error that a caller might
retry unsafely. It emits a value-free pending-cleanup event and retries from that work list. Delete
removes an address from the manifest first; re-creating the address uses a new object name and
cannot race a scheduled deletion.

AWS documents `ClientRequestToken` idempotency and the `AWSCURRENT`/`AWSPREVIOUS` movement in
[PutSecretValue](https://docs.aws.amazon.com/secretsmanager/latest/apireference/API_PutSecretValue.html).
It also warns that sustained writes more frequent than one per ten minutes can exhaust retained
versions. Credential administration is expected to be low-volume, but the adapter must measure head
version pressure, alert before the quota, and refuse safely rather than discard history or weaken
atomicity.

This manifest is necessary because Secrets Manager versions an individual secret and offers no
cross-secret commit. One mutable AWS secret per credential would not implement the existing atomic
`apply` contract. Conversely, one scope-sized document containing all values would couple every
credential read and can exceed AWS's value limit, so manifests contain references, not values.

## IAM, KMS, and audit policy

The workload role is limited to the deployment prefix and exactly these Secrets Manager actions:

- `secretsmanager:CreateSecret`
- `secretsmanager:GetSecretValue`
- `secretsmanager:PutSecretValue`
- `secretsmanager:DeleteSecret`

It receives no `ListSecrets`, batch-get, resource-policy, restore, replication, rotation-policy, IAM,
or KMS-administration action. Deterministic names and manifests remove the need to list the account.
`DeleteSecret` is required only for post-commit object cleanup and eventual decommissioning; if
operations are split later, a janitor role may remove it from the serving process.

The Secrets Manager resource is the exact account, region, and prefix ARN pattern, including the
service's random ARN suffix. `CreateSecret`, whose resource does not yet exist, is additionally
constrained by `secretsmanager:Name` matching `P/*` and
`secretsmanager:KmsKeyArn` matching the one configured key. An explicit deny requires that key.
AWS lists both keys for `CreateSecret` in its
[service-authorization reference](https://docs.aws.amazon.com/service-authorization/latest/reference/list_secretsmanager.html).
A policy test must prove that the same role is denied outside this deployment prefix.

`DeleteSecret` is allowed only with a recovery window of at least seven days and is explicitly
denied when `ForceDeleteWithoutRecovery` is true. Those restrictions use AWS's
`secretsmanager:RecoveryWindowInDays` and `secretsmanager:ForceDeleteWithoutRecovery` condition
keys. No workload call can permanently erase a secret immediately.

Secrets use one customer-managed symmetric KMS key. The workload receives only
`kms:Decrypt` and `kms:GenerateDataKey` on that exact key, constrained with `kms:ViaService` to
`secretsmanager.<region>.amazonaws.com` and with the Secrets Manager encryption context restricted
to the deployment ARN prefix. Provisioning owns key policy, OIDC provider, role, CloudTrail, and
automatic KMS key rotation; the workload owns none of them. AWS describes the envelope-encryption
permissions and these conditions in
[Secrets Manager encryption](https://docs.aws.amazon.com/secretsmanager/latest/userguide/security-encryption.html)
and gives least-privilege examples in
[Secrets Manager IAM policies](https://docs.aws.amazon.com/secretsmanager/latest/userguide/auth-and-access_iam-policies.html).

CloudTrail records all Secrets Manager API calls and is retained outside the Machine. AWS omits
`SecretString` and `SecretBinary` from CloudTrail, but Exchange must still avoid SDK debug dumps,
request/response bodies, JWTs, and source error chains that can carry material. Logs and audit events
may contain the canonical address, transaction UUID, stable error category, AWS request ID, counts,
and outcome only. See
[CloudTrail monitoring](https://docs.aws.amazon.com/secretsmanager/latest/userguide/monitoring-cloudtrail.html)
and AWS's [Secrets Manager best practices](https://docs.aws.amazon.com/secretsmanager/latest/userguide/best-practices.html).

## Error contract

The adapter translates AWS and validation failures into the existing store vocabulary:

| Condition | `StoreError` result |
| --- | --- |
| Requested address absent from a valid manifest; AWS object genuinely absent | `NotFound` |
| IAM or KMS access denial, invalid/expired web identity, rejected STS credentials | `Denied` |
| DNS, connect, TLS, timeout, SDK dispatch, throttling, or retryable AWS 5xx | `Unreachable` |
| Invalid prefix/name or canonical address cannot be represented within 512 characters | `Layout` |
| Manifest generation/precondition race or unresolved transaction ownership | `Conflict` |
| Corrupt document, decryption failure, wrong value field, scope/ARN mismatch, invalid AWS request | `Backend` |
| Required atomic or identity guarantee cannot be provided by the selected deployment | `Unsupported` |

The adapter may retry an AWS-documented retryable failure within a bounded request deadline. A
startup or request never maps denial to absence and never repairs malformed state. Caller-facing
and logged reasons are stable codes, not credential data, token data, AWS response bodies, or full
SDK `Debug` output.

## SDK and Rust 1.88

The latest AWS SDK release is not compatible with this workspace's Rust 1.88 floor. As checked for
this design on 2026-08-03, `aws-sdk-secretsmanager` 1.111.0 declares Rust 1.94.1, while 1.99.0
declares Rust 1.88.0. `aws-config` 1.8.13 also declares Rust 1.88. The primary crate manifests are:

- [`aws-sdk-secretsmanager` 1.111.0](https://docs.rs/crate/aws-sdk-secretsmanager/1.111.0/source/Cargo.toml)
- [`aws-sdk-secretsmanager` 1.99.0](https://docs.rs/crate/aws-sdk-secretsmanager/1.99.0/source/Cargo.toml)
- [`aws-config` 1.8.13](https://docs.rs/crate/aws-config/1.8.13/source/Cargo.toml)

The upstream `connector-secrets` implementation starts with exact, optional direct pins:

```toml
aws-config = { version = "=1.8.13", default-features = false, features = ["rt-tokio", "rustls"] }
aws-sdk-secretsmanager = { version = "=1.99.0", default-features = false, features = ["default-https-client", "rt-tokio", "rustls"] }
```

The actual feature names and complete resolved graph must be verified against those source
manifests during implementation. The released connector manifest and this repository's committed
lockfile are both part of the pin: upstream CI and Exchange CI must build the managed-backend feature
with Rust 1.88 and `--locked`. If a transitive crate selected by Cargo has a higher MSRV, pin the
matching compatible release set or stop with a blocker. Do not raise the workspace `rust-version`
to make the adapter compile. Default features such as SSO and credential-process support stay
disabled unless the explicit web-identity provider proves they are required.

## Configuration and startup

The future selector has two explicit values, conceptually `file` and `aws-secrets-manager`. AWS mode
requires region, immutable deployment prefix, KMS key ARN, role ARN, and Fly's web-identity token
file. File settings in AWS mode, AWS settings in file mode, an absent cutover marker after migration,
an invalid prefix, a failed identity probe, or a denied/unreachable head read refuse startup.

Startup performs only value-free identity and control-plane validation; it does not create a probe
credential or repair a missing marker. The cutover control document records schema, state, measured
address count, source inventory identity, release commit, and timestamps, never values or digests.
The server serves credentials only when the selected backend and marker state agree.

Non-secret settings and grants remain on their existing stores. Moving credentials neither copies
nor reinterprets those files.

## Migration and cutover

Migration is an explicit maintenance operation, not startup magic:

1. Enter maintenance mode. Freeze credential mutation and all invocation or channel work that could
   consume credentials. Confirm one file-store writer and one complete source generation.
2. Inventory the source through the file store's public `paths`/reference surface, parse every path
   with `TenantLayout`, group by scope, and record canonical addresses and counts only.
3. With the file backend still active, copy each scope into immutable AWS objects and a prepared
   manifest. The AWS cutover control remains `prepared`, so the server cannot serve it.
4. Read every value back through its exact ARN/version and compare bytes in memory. Verify every
   scope's address set and count against the measured source. Persist only pass/fail, addresses,
   counts, request IDs, and timestamps.
5. Write a `ready` cutover document containing the schema, complete inventory count, source
   generation, release commit, and transaction identifier. Keep maintenance mode on.
6. Deploy a reviewed version with the explicit AWS selector. Startup requires the matching `ready`
   document and inventory. Exercise managed read, write, delete/rotation, restart persistence,
   cross-prefix denial, and refusal while the backend is unavailable.
7. Mark the cutover committed, allow new writes, and begin vendor rotation. Never consult or update
   the old file store again.

There is no dual-read or dual-write phase. Until step 6 the file store is authoritative and AWS is
invisible; at step 6 one explicit selector changes authority. Before the first AWS-backed write,
rollback means selecting the unchanged file store under maintenance. After an AWS-backed write or
vendor rotation, rollback cannot reactivate the stale file: it is a new managed-to-file migration
into a fresh store, with the same inventory, verification, maintenance, and explicit cutover rules.

An interruption in steps 2–5 leaves the file store authoritative. Re-running uses stable transaction
identifiers, verifies existing AWS objects, and cleans abandoned objects; it does not infer success.
An interruption in step 6 leaves maintenance mode and startup refusal in place until an operator
chooses one verified backend.

## Verification plan

Implementation begins with failing tests for:

- canonical prefix/name mapping, maximum lengths, scope validation, and rejection rather than
  truncation;
- missing, denied, throttled, unavailable, malformed, and KMS-decryption error mapping;
- exact tenant and deployment-prefix isolation, including an IAM integration test denied against a
  sibling prefix;
- `put`, idempotent delete, references, move, and multi-operation `SecretBatch` semantics;
- fault injection before value creation, after object creation, before manifest commit, during an
  ambiguous commit timeout, and after commit during cleanup;
- delayed `AWSCURRENT` visibility, same-process read-your-write behavior through the exact head
  version, and refusal of mutation while a commit remains ambiguous;
- restart with a new adapter instance reading the committed manifest and exact object versions;
- concurrent same-scope calls under the supported single-writer model and startup refusal for a
  multi-writer deployment;
- migration interruption at every phase, exact inventory/read-after-write checks, unchanged source,
  explicit selector behavior, and absence of fallback;
- a sentinel credential, OIDC token, and AWS-shaped key absent from every captured log, error,
  metric, audit event, manifest, name, tag, and migration record;
- the optional feature/dependency graph, the no-second-request-path allow-list rationale, and a
  locked Rust 1.88 build.

The store contract, transaction, fault-injection, redaction, and scripted-transport tests live with
the adapter in `connector-secrets`. This repository adds failing-first composition, configuration,
migration, prefix-policy, restart, and live tests through the unchanged `SecretStore` port. Both
gates must be green on the released connector version; an upstream-only test run is not X-97
completion.

AWS sandbox tests provision an isolated prefix and KMS key and destroy their fixtures after
assertions. Live acceptance additionally requires a versioned Fly release proving managed
read/write/rotation, restart persistence, backend-unavailable refusal, and completed old-store
decommission. None of those live checks is satisfied by this design alone.

## Rotation and decommission

After cutover, rotate every migrated vendor credential through its connector-specific, canonical
administrative path. Secrets Manager's automatic rotation cannot be assumed to understand arbitrary
vendor credentials. Record address, time, actor, and outcome without either old or new value, and
verify the old vendor credential is rejected before marking that address rotated.

Once every address is rotated and the rollback deadline has passed:

1. Remove the old credential directory and all sibling temporary files. Because the Fly volume also
   holds non-secret stores, copy only the explicitly retained non-secret data to a fresh volume,
   verify it, attach it, and destroy the old volume rather than claiming a directory unlink erased
   its blocks.
2. Inventory and destroy every other old copy, CI artifact, operator export, and migration scratch
   file. Record each snapshot or backup retention deadline and keep X-97 open until all have expired.
3. Delete abandoned AWS value objects using the documented recovery window and monitor completion.
   AWS deletion is asynchronous, individual versions cannot be deleted, and the ordinary recovery
   window is at least seven days; see
   [DeleteSecret](https://docs.aws.amazon.com/secretsmanager/latest/apireference/API_DeleteSecret.html).
4. Remove the file-backend deployment option after its explicit rollback window. Review the Fly
   OIDC trust, IAM prefix, CloudTrail retention, and KMS rotation evidence.

Do not schedule destruction of the active KMS key while any current secret or required recovery
copy depends on it. Revoking the workload role or key is an emergency refusal mechanism, not data
repair. Vendor rotation makes retained pre-cutover copies unusable, but it does not waive the duty
to destroy them on the recorded timeline.

## Rejected alternatives

- **Static AWS access keys:** move the bootstrap credential into Fly configuration and violate the
  workload-identity requirement.
- **Organization-wide Fly trust:** lets another application in the organization assume the role.
- **One mutable AWS secret per address:** cannot make the existing multi-address batch atomic.
- **One scope document containing all values:** couples reads and rotations and can exceed the AWS
  value-size limit.
- **Dual reads, dual writes, or automatic fallback:** make authority ambiguous and can silently
  resurrect stale credentials.
- **Parsing the file-store directory directly:** bypasses its canonical address and mode checks.
- **Using the latest SDK or raising the workspace MSRV:** breaks the published Rust 1.88 contract;
  the compatible client line is pinned and tested instead.
- **An Exchange-owned vendor adapter:** creates the second request path this repository forbids.
