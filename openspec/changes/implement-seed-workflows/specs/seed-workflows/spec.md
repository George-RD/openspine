# Seed workflow set

## ADDED Requirements

### Requirement: Minimal seed workflow set ships as overlay artifacts

The kernel MUST ship exactly four seed workflow manifests — `owner_control_conversation_seed`, `email_draft_with_approval_seed`, `research_and_brief_seed`, and `customer_service_intake_seed` — as overlay artifacts (AD-153), never as kernel/base fixtures. Each MUST be a valid `WorkflowManifest` state machine in the D-087..D-090 declarative shape with an `initial_state`, uniquely-identified `states`, and directed `transitions` whose `from`/`to` ids reference declared states. On a fresh install the kernel MUST materialize any absent seed into the overlay directory and register it as an `Overlay`-namespace learned artifact; it MUST NOT overwrite an existing seed file, so owner edits survive.

#### Scenario: All four seeds parse and validate as state machines

- **WHEN** the embedded seed manifests are parsed and validated
- **THEN** all four deserialize successfully, declare an initial state, and every transition references a declared state

#### Scenario: Seeds load as overlay-namespace artifacts

- **WHEN** a fresh install runs the overlay startup path after the seeds are materialized
- **THEN** the four seed workflow files exist on disk and each is recorded as an `Overlay`-namespace learned artifact (discoverable through the same loader the kernel uses at startup)

#### Scenario: Materialization runs once per fresh install

- **WHEN** the seed materialization runs, records a persisted marker, and then runs again on a later boot
- **THEN** the second run writes no files, and a seed the owner deleted after the first boot is not re-created

### Requirement: Seed workflows render Mermaid flowcharts

Every seed workflow manifest MUST render its declarative transitions as Mermaid `flowchart TD` syntax with one edge line per transition, so the owner can review or edit the seed's shape.

#### Scenario: A seed renders every transition

- **WHEN** a seed manifest's `to_mermaid` output is produced
- **THEN** it begins with `flowchart TD` and contains one directed edge per declared transition


### Requirement: Email-draft seed gates departure on a digest-bound approval

The `email_draft_with_approval_seed` MUST declare an approval-required state (`awaiting_approval`) whose `approval_action` is `email.create_draft`. Leaving that state MUST require a Store-backed `ActionRequest` and `ApprovalRecord` matching the exact action and immutable payload/target digests; without a matching approved, unexpired approval the transition MUST be rejected (D-087/D-088).

#### Scenario: Departure without approval is rejected

- **WHEN** a run reaches `awaiting_approval` and attempts to leave without an action request id
- **THEN** the transition is rejected with an approval-required error and no state advance occurs

#### Scenario: Valid digest-bound approval permits departure

- **WHEN** the stored request action matches `email.create_draft` and the approved payload and target digests match an unexpired approval
- **THEN** the transition to the approved state completes
