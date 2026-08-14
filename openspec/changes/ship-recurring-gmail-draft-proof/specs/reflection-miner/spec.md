## MODIFIED Requirements

### Requirement: Repeated approvals MUST be derived from scoped audit evidence

Repeated approvals MAY produce a standing-rule candidate only from kernel-packed audit evidence that supplies typed `OwnerApprovalEvidence` fields plus the observed action, `context_class_digest` from `ReviewedActionScope::derive(&resolved_context)?.context_class_digest()` (the grouping key), and the separate `reviewed_scope_digest` standing-rule key. The resulting `DelegationEvidence::repeated_approvals` value MUST carry its `context_class_digest`, while each evidence row MUST carry the owner principal, unique decision-event ID, request-shape digest, target digest, and payload digest; the scheduled miner MUST retain the observed action as the group key. It MUST group by `(action_id, context_class_digest)`, deduplicate decision-event IDs, and call `DelegationEvidence::repeated_approvals`; it MUST NOT use raw row counts, artifact/action grouping, caller-supplied counts, or a free-text reviewed-scope description. The contract's exact request-shape equality remains required, while payload-digest variation remains allowed.

Provenance copy MUST be derived from the resulting evidence kind and count, so two matching approvals render `2 matching owner approvals`. Rows lacking kernel-resolved context or typed digests MUST be skipped rather than guessed.

#### Scenario: Repeated approvals remain proposed

Given two allowed audit entries for one approved artifact in the scoped slice
When the miner emits an output
Then the output MUST be a proposed standing-rule candidate
And it MUST preserve the observed action ID without activating authority.

Test: `repeated_approval_is_only_a_standing_rule_candidate`, `repeated_approval_requires_kernel_verifiable_evidence`

#### Scenario: Duplicate decision rows do not satisfy the threshold

- **WHEN** two audit rows carry the same decision-event ID
- **THEN** the miner MUST count one unique decision
- **AND** it MUST emit no repeated-approval candidate from those rows alone

#### Scenario: Two context classes cannot form one pattern

- **WHEN** approvals differ in context-class/reviewed-scope digest
- **THEN** the miner MUST keep them in separate groups
- **AND** it MUST emit no candidate that combines them

#### Scenario: Payload variation remains separate from request shape

- **WHEN** approvals share request-shape and context-class digests but have different payload digests
- **THEN** typed repeated-approval construction MAY succeed
- **AND** the payload digests MUST remain separate evidence fields

Test: `scheduled_reflection_miner_tick_mines_repeated_approval`, `scheduled_reflection_miner_duplicate_decision_event_is_not_repeated`, `scheduled_reflection_miner_two_context_classes_do_not_form_one_pattern`, `scheduled_reflection_miner_request_shape_mismatch_is_rejected`
