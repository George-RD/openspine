# audit-artifact-store Specification

## Purpose
TBD - created by archiving change backfill-implemented-capability-specs. Update Purpose after archive.
## Requirements
### Requirement: The audit log MUST be append-only and hash-chained

Each audit row's `hash` MUST equal `sha256(prev_hash || canonical_json(meta))`,
where `meta` is the row's own fields (excluding `hash` itself) serialized
via canonical JSON. The first row's `prev_hash` MUST be the genesis value
`"sha256:" + 64 zero hex characters` (a fixed, non-secret constant). No
update or delete path MUST exist for audit rows.

#### Scenario: A row is appended

Given at least one existing audit row
When a new audit row is appended
Then its `prev_hash` MUST equal the previous row's `hash`
And its own `hash` MUST be `sha256(prev_hash || canonical_json(meta))`.

(Enforced by `store::tests::first_audit_row_chains_from_genesis` and
`store::tests::second_audit_row_chains_from_first_hash`.)

### Requirement: The kernel MUST verify the audit chain on startup and refuse to start if broken

At startup, the kernel MUST walk the audit chain from genesis,
recomputing each row's hash, and MUST refuse to start (non-zero exit) if
any row's `prev_hash` or `hash` does not match what a clean re-derivation
produces.

#### Scenario: Chain is intact at startup

Given an audit log whose chain has not been tampered with
When the kernel starts
Then chain verification MUST pass
And the kernel MUST proceed to serve requests.

#### Scenario: Chain is broken at startup

Given an audit log row whose `hash` or `meta_json` has been altered
outside the kernel
When the kernel starts
Then chain verification MUST fail
And the kernel MUST exit with an error instead of serving requests.

(Enforced by `store::tests::intact_chain_verifies_true`,
`store::tests::tampered_hash_breaks_verification`, and
`store::tests::tampered_meta_json_breaks_verification`; wired at startup
in `main.rs`.)

### Requirement: Artifact blobs MUST be encrypted and content-addressed by plaintext digest

Every artifact blob MUST be encrypted with AES-256-GCM using a fresh
random nonce per blob, and MUST be stored under a path derived from the
SHA-256 digest of its *plaintext* content — so identical logical content
always resolves to the same reference regardless of when or how many
times it is stored.

#### Scenario: The same content is stored twice

Given the same plaintext bytes are put into the artifact store twice
When both calls complete
Then both MUST resolve to the same content-addressed reference
And the on-disk ciphertext MUST NOT be identical between the two writes
(distinct nonces).

(Enforced by `artifact_store::tests::same_content_is_content_addressed`,
`artifact_store::tests::different_content_is_different_ref`, and
`artifact_store::tests::stored_blob_never_contains_the_plaintext_substring`.)

### Requirement: Reading an artifact MUST re-verify its digest after decryption

`ArtifactStore::get` MUST recompute the digest of the decrypted plaintext
and MUST fail rather than return content whose digest does not match the
requested reference.

#### Scenario: A stored blob is corrupted or tampered with

Given a stored artifact blob has been modified on disk after encryption
When the kernel attempts to read it back
Then decryption or digest re-verification MUST fail
And the kernel MUST NOT return the corrupted content as if it were valid.

(Enforced by `artifact_store::tests::wrong_key_fails_to_decrypt`.)

### Requirement: Task tokens MUST be stored hashed, never in plaintext

`task_grants.task_token` MUST store a hash of the bearer token, and the
token MUST be redacted from any persisted copy of the grant.

This requirement is implemented by `harden-approval-and-budgets` (D-047);
this backfill spec records it as belonging to this capability.

#### Scenario: A task grant is persisted

Given a task grant is minted and persisted
When its row is inspected
Then the stored `task_token` column MUST NOT equal the raw bearer token
And no persisted copy of the grant MUST contain the raw token.

(Enforced by `store::tests::find_task_grant_by_token_rejects_the_raw_hash_value`
and `store::tests::persisted_grant_json_contains_no_task_token`.)

### Requirement: Audit append MUST assign per-aggregate sequence under the store lock

When an audit row is appended, the store MUST assign `aggregate_seq` as one
greater than the current maximum `aggregate_seq` for that row's
`aggregate_id`, using the same connection/lock that performs the insert.

The assigned `aggregate_id` and `aggregate_seq` MUST be stored as columns on
`audit_log` and MUST be included in the hash-chain meta pre-image for the new
row.

#### Scenario: Sequential appends on one aggregate

Given no prior rows for aggregate `system`
When two audit rows are appended without a task grant
Then both MUST use `aggregate_id = "system"`
And their `aggregate_seq` values MUST be 1 then 2.

### Requirement: The store MUST support filtered ordered replay of the audit ledger

The store MUST expose a filtered replay API over `audit_log` that returns
matching rows in ascending global sequence order, optionally starting after a
caller-supplied global sequence watermark.

The store MUST support durable consumer checkpoints keyed by consumer id,
recording the last successfully acked global sequence.

#### Scenario: Replay after watermark skips earlier rows

Given three audit rows with global sequences 1, 2, and 3
When replay is requested with watermark 2 and no filter constraints
Then only the row with global sequence 3 MUST be returned.

