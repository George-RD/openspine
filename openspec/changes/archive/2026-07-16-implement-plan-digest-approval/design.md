# Design: Plan digest-bound approval

## Authority boundary

The plan is a payload, not a new authority object. The existing `ApprovalRecord` remains the sole persisted approval shape and `gate()` remains the sole mediation path. The plan digest is placed in the request's `payload_ref.digest`, exactly like an email draft payload digest. This avoids a second approval lifecycle that storage and kernel dispatch could not load.

## Canonical plan payload

`Plan` contains the ordered `Vec<PlanStep>`. Each `PlanStep` contains:

- `action: ActionId`, identifying the effectful operation;
- `arguments: serde_json::Value`, the canonical structured arguments that bind exact execution identity; and
- `summary: String`, the owner-facing text that is additive to the execution binding.

`Plan::digest()` serializes the complete versioned plan object to JSON and
calls `openspine_schemas::digest::digest_of`. That function applies the D-028
canonical-JSON transform (recursive key sorting, no insignificant whitespace,
UTF-8) before SHA-256. Array order and `schema_version` are part of the
approval. Data-handling steps are ordinary `PlanStep` values and cannot be
omitted from the digest.

## One-loop question and kernel response

`PlanApprovalQuestion::new` deterministically renders the schema version,
action id, summary, and canonical structured arguments for every step. The
kernel persists those canonical plan bytes in an ordinary pending
`ActionRequest` before sending the question. `approve_plan:<id>` is routed
through the verified owner callback path; the kernel loads and deserializes
the artifact, re-derives `Plan::digest()`, compares it to the request and
question-bound digest, and only then persists the existing `ApprovalRecord`.
The carrier has no approval constructor and cannot mint authority.

## Mutation refusal

At execution, the current plan artifact's digest is carried in `ActionRequest.payload_ref.digest`. `gate()` looks up the existing approval by request id and compares both payload and target digests. An unchanged plan reaches `Allow`. If any step or structured argument changes, the current payload digest differs from the approved digest and `gate()` returns `Deny { reason: ApprovalDigestMismatch }`. Because an existing non-matching approval is a denial rather than `ApprovalRequired`, the agent cannot obtain a deferential second ask through mutation.

The approved-plan resolver records and announces the plan but does not
execute its steps; workflow-state-machine and worker-runtime changes own
those semantics. `dispatch_plan_preview` refuses a Telegram approval button
when the complete rendering would truncate, sending only a plain notice.

## Verification strategy

Schema tests cover deterministic canonical hashing, key-order normalization, order/step/argument/data-handling mutation, strict deserialization, carrier construction, and approval matching. Gate tests exercise the public `gate()` function with a plan digest as payload, then replace that digest with a mutated plan and assert `ApprovalDigestMismatch`.
