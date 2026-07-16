# Proposal: Plan digest-bound approval

## Dependencies

- AD-011 (one-loop conversational approval).
- D-011 (approval must be digest-bound).
- D-028 (canonical JSON digest pre-image).
- D-045 (WYSIWYS refusal when the owner cannot see the full payload).
- Existing `digest-bound-draft-approval` capability and `ApprovalRecord`/`gate()` payload-digest path.

## Impact

- **Surface:** OpenSpine core runtime schemas and gate integration; no Lyra-only behavior.
- **Runtime authority:** Yes, this tightens the existing digest-bound approval boundary; it does not create a new authority object or bypass task grants.
- **Private data:** The plan may describe private-data handling, but only canonical digests and protected artifact references cross the approval boundary; raw private content is not added to approval records.
- **External communication:** No new external communication; existing gate-mediated actions retain their current requirements.
- **Connector access:** No new connector or connector scope; plan steps reuse existing action ids and connector policy.
- **System operations:** No changes to deployment, filesystem, shell containment, or process management.

## Problem/Context

The existing approval boundary is exercised by email-body-shaped payloads. Plans contain multiple ordered effectful steps, including data-handling steps, and a deferential second confirmation must not be able to approve a plan different from the one shown in the clarifying question. A prose-only step description is insufficient: changing a recipient, time, or other execution argument while retaining the same prose would leave the approval under-bound.

## Proposed Solution

Introduce a canonical typed plan payload consisting of an ordered list of
`PlanStep` values. Each step carries its action, canonical structured
arguments, and additive owner-facing summary. `Plan::digest()` includes
`schema_version` and uses the existing canonical-JSON `digest_of` convention.
The kernel persists canonical plan bytes as an ordinary pending
`ActionRequest` payload, presents a complete question with an
`approve_plan:<id>` callback, re-derives the plan digest from artifact-store
bytes at callback time, persists the existing `ApprovalRecord`, re-runs
`gate()`, and records resolution without executing arbitrary step actions.

## Acceptance Criteria

- A plan approval binds the digest of the complete ordered step list.
- Data-handling steps participate in the same digest as every other effectful step.
- Changing, adding, removing, or reordering a step changes the digest.
- Changing structured execution arguments changes the digest even when the summary is unchanged.
- The question carrier holds the computed digest; the kernel re-derives it from canonical stored bytes before persisting `ApprovalRecord`.
- `gate()` allows the unchanged approved plan and denies a mutated plan with `ApprovalDigestMismatch`.

## Out of Scope

- A live plan-producing route. A candidate `plan_approval_pack` fixture declares
 `plan.propose` and `plan.execute`, but no shipped route references it; later
 workflow/producer changes own activation and first production use.
- Execution semantics for individual plan steps; the approved-plan resolver records and announces the approved plan only.
- A new approval enum or parallel gate path; existing ActionRequest/ApprovalRecord storage is reused.
- Changes to email draft approval semantics.
- Execution of calendar, reminder, search, or data-handling connectors.
- Multi-message or paginated owner-review UX beyond the existing truncation refusal.
