# counterparty-key-model Specification

## Purpose
TBD - created by archiving change implement-counterparty-key-model. Update Purpose after archive.
## Requirements
### Requirement: Explicit counterparty scopes MUST use distinct payload keys

For every non-system identity passed to scoped artifact storage, the store MUST
lazily generate a random 256-bit payload key unique to that scope. It MUST
store the key only in wrapped form under the kernel master key. The versioned
wrapped-key format MUST authenticate a domain label and the scope id as AEAD
associated data. Payload bytes MUST be encrypted with the scoped payload key,
not directly with the master key.

Existing `put/get` callers MUST continue to use `SYSTEM_SCOPE`; explicit scoped
operations MUST pass scope separately, and `ArtifactRef` MUST remain
digest-only.

#### Scenario: Two counterparty scopes get distinct keys

Given two distinct non-system identity scopes
When each stores payloads through `put_scoped`
Then the store MUST hold a distinct 256-bit payload key per scope
And one scope's key MUST NOT decrypt the other scope's payload.

#### Scenario: A scoped key persists across reopen

Given a scope that has never stored a payload
When its first payload is stored
Then a fresh random 256-bit key MUST be durably persisted in wrapped form
And reopening the key ring with the same master key MUST recover that key.

#### Scenario: Associated data rejects scope and key substitution

Given a wrapped key file or format-3 blob created for scope A
When its bytes are substituted into scope B's path, with the blob scope header
rewritten to B where applicable
Then unwrap or payload decryption MUST fail
Because the key's domain-and-scope associated data and the blob's
tag-and-scope associated data MUST authenticate the original scope.

### Requirement: Crypto-erasure MUST permanently close a counterparty scope

Erasing a non-system counterparty MUST durably record the closed scope, create
a marker-only filesystem tombstone, and physically delete its wrapped payload
key and temporary aliases. Once erasure returns, no maintained plaintext key
copy MAY survive. Reads, writes, and erasure for the same scope MUST be
serialized so a racing read cannot return plaintext after erasure completes.
Later writes MUST NOT recreate a key for the closed scope.

`SYSTEM_SCOPE` is reserved for existing callers, legacy/internal provenance,
and migrated format-1 blobs. The counterparty-erasure boundary and key ring
MUST reject attempts to erase it without changing its key, tombstone, payloads,
learned artifacts, or audit history.

#### Scenario: A stored payload becomes unreadable after erasure

Given a counterparty scope with a stored private payload
When that counterparty is erased
Then reading the payload MUST fail with a decrypt or missing-key error
And its ciphertext blob MAY remain on disk without making plaintext
recoverable.

#### Scenario: A fresh store cannot read an erased payload

Given a counterparty payload whose scope has been erased
When a fresh artifact-store instance opens the same data directory with the
same master key
Then the wrapped key MUST still be absent
And reading the pre-erasure payload MUST fail.

#### Scenario: A closed scope cannot be recreated

Given a counterparty scope that was erased, including a scope that had never
created a key
When `put_scoped` later attempts to store payload bytes for that scope
Then the write MUST fail with the terminal erased-scope error
And no new wrapped key MUST be created.

#### Scenario: Reserved system scope erasure is rejected

Given payloads or provenance stored under `SYSTEM_SCOPE`
When counterparty erasure is requested for `SYSTEM_SCOPE`
Then the request MUST be rejected
And the system key, payload readability, provenance, closure markers, and audit
history MUST remain unchanged.

#### Scenario: Startup reconciles a committed erasure after a crash

Given the database closure marker and erasure audit committed before a crash
And the crash occurred before the filesystem tombstone or key deletion
completed
When the kernel starts again
Then it MUST replay the durable database marker into the key ring before
serving
And it MUST durably create the tombstone and delete any remaining wrapped key
without appending a duplicate erasure audit event.

### Requirement: The audit hash chain MUST still verify after crypto-erasure

Crypto-erasure MUST append at most one `counterparty.erased` event per scope
without mutating or deleting earlier audit rows. The event MUST bind the erased
counterparty id and digest-safe references for the exact invalidated
`(kind, artifact_id, version)` identities, and MUST contain no learned payload
plaintext.

#### Scenario: Chain verification passes after erasure

Given an audit log with an intact hash chain
When a counterparty is erased
Then `verify_audit_chain` MUST return true
And exactly one counterparty-bound erasure event MUST remain present across
idempotent retries.

### Requirement: Derived artifacts MUST be terminally invalidated through recorded provenance

Erasure MUST match learned overlay artifacts by the erased identity's exact
`Provenance::ProducedBy.source_scope`, not by digest alone. In the same
transaction as the durable closure marker and erasure audit, every matching
artifact MUST become `Erased`, its pending reconfirmation MUST be consumed and
cleared, and its exact identity MUST be returned by the primitive. No later
compatibility transition or learned-artifact write MAY revive or replace a row
derived from the closed scope.

`SYSTEM_SCOPE` MAY appear in `ProducedBy` for legacy or internal sources even
though counterparty erasure MUST reject that reserved scope.

#### Scenario: Exactly the producing scope's artifacts are invalidated

Given learned artifacts whose `ProducedBy.source_scope` values name two
different counterparties, including artifacts with identical source digests
When one counterparty is erased
Then every non-erased artifact produced by that scope MUST become terminal
`Erased`
And artifacts produced by the other scope MUST remain unchanged.

#### Scenario: Stale reconfirmation cannot revive terminal erasure

Given a learned artifact with a pending reconfirmation request
When its recorded source scope is erased before the owner responds
Then erasure MUST consume the request and clear its pending reconfirmation
fields
And a stale approval or later reconfirmation-required transition MUST NOT
change the artifact from `Erased`.

#### Scenario: Learned metadata cannot be recorded after scope closure

Given a durable closure marker for a counterparty
When any learned-artifact insert or replacement records
`ProducedBy.source_scope` for that counterparty
Then the database MUST reject the write atomically.

### Requirement: Earlier artifact and key formats MUST migrate safely

On startup, each pre-AD-140 flat format-1 blob
`[nonce:12][ciphertext]` MUST be decrypted with the legacy master key,
digest-verified, and rewritten as current format 3 under `SYSTEM_SCOPE` at
`<SYSTEM_SCOPE>/<sha256-hex>`. The target MUST be durable before the source is
removed, and retry after a crash MUST verify an existing target before cleanup.

Recovered format-2 scoped blobs
`[tag=2][scope:16][nonce:12][ciphertext]` MUST remain readable. A successful
read MUST verify the plaintext digest and rewrite the blob as associated-data-
bound format 3. Legacy unversioned wrapped-key files MUST likewise remain
readable and migrate to the versioned associated-data-bound key format.

#### Scenario: A flat format-1 blob migrates under system scope

Given a valid pre-AD-140 flat blob, including one whose nonce begins with a
current or recovered format tag byte
When startup migration runs
Then the payload MUST be re-encrypted as format 3 under `SYSTEM_SCOPE`
And its digest and plaintext readability MUST be preserved
And rerunning migration MUST not count or rewrite it again.

#### Scenario: Format-1 migration recovers after target publication

Given a crash left both a valid scoped migration target and its flat source
When migration runs again
Then it MUST verify the target decrypts to the expected digest before removing
the source
And it MUST not count cleanup as a new migration.

#### Scenario: A recovered format-2 blob upgrades on read

Given a valid format-2 scoped blob and its payload key
When `get_scoped` reads it
Then the original plaintext MUST be returned after digest verification
And the on-disk blob MUST be rewritten as associated-data-bound format 3.

