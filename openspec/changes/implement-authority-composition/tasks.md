# Tasks: Implement authority composition

## 1. Composer interface

- [x] Define authority composition input type (`AuthorityInput<'a>`, borrowed-reference bundle, not a persisted schema — see `define-core-runtime-schemas`'s tasks.md deferral note).
- [x] Define authority composition result type (`AuthorityOutcome`: `Granted(Box<TaskGrant>) | Denied { reason } | Ambiguous { fallback_route }`).
- [x] Define task grant output type (`openspine_schemas::grant::TaskGrant`, extended in Step 1; `task_token` and artifact `version` fields added here as this change surfaced the need for them).
- [x] Include allowed, denied, and approval-required action sets (`TaskGrant::{allowed_actions,denied_actions,approval_required_actions}`).

## 2. Merge logic

- [x] Implement deny-by-default start state.
- [x] Gather candidate allows from route, workflow, agent manifest, and capability pack — **route excluded**: PRD §9's role table says routes contribute "selection and constraints", not permissions, and the Route schema carries no action list; routes select which agent/workflow/pack apply, they don't independently gate actions. Recorded as a clarification of design.md, not a decision-log entry.
- [x] Intersect with global and user/session policy — an **empty** policy allow-list is treated as "no additional narrowing" rather than "deny everything" (see `compose.rs`'s module doc comment); this product's global policy fixture only ever carries denies.
- [x] Apply lane, connector, account-role, data-class, channel, and task constraints — data-classification implemented (`pack.constraints.data_classification_max`); lane/connector/account-role/channel are already enforced by `resolve_route`'s `when` match before `compose_authority` ever runs on the winning route.
- [x] Apply explicit deny precedence.
- [x] Apply approval-required precedence.
- [x] Materialize task grant.

## 3. Tests

- [x] Test that no action is allowed without candidate allow (`no_candidate_allow_means_action_is_not_granted`).
- [x] Test explicit deny overrides allow (`explicit_deny_overrides_allow`).
- [x] Test approval-required overrides plain allow (`approval_required_overrides_plain_allow`).
- [x] Test identity alone grants no authority (`spoofed_owner_id_without_verified_source_is_denied` — `cargo test -p openspine-authority spoofed_owner`).
- [x] Test connector/account role alone grants no authority (`route::tests::gmail_connector_authenticated_alone_does_not_match_the_selected_thread_route` — spec.md's exact scenario: Gmail-connector/account-role authenticated but no `email.thread.selected` selection event, denied at route resolution).
- [x] Test main assistant does not inherit specialist workflow authority (`main_assistant_grant_never_inherits_email_drafter_authority`).
- [x] Test selected-thread email grant excludes inbox-wide read (`email_grant_excludes_inbox_wide_read_matching_prd_12_2` — also the D-034 regression for the `email.create_draft` id).
- [x] Test authority widening requires approval (`widening_via_a_proposed_pack_requires_approval_first`; also `quarantined_artifact_cannot_participate_in_a_grant` and `a_deny_route_is_never_composed`).

## 4. Documentation

- [x] Document authority composition rule (module doc comment on `compose_authority`, including the design.md-ambiguity resolution).
- [x] Document task grant as the final authority object (`TaskGrant` doc comment, incl. the `task_token` redaction warning for Step 4).
- [x] Link to relevant decision-log entries (D-006, D-007, D-013, D-028, D-032, D-034 referenced inline).

## 5. Validation

- [x] Run unit tests (20 tests in `openspine-authority`: 9 unit + 11 compose integration; `openspine-schemas`'s 57 tests remain green, unaffected).
- [x] Run `scripts/check.sh implement-authority-composition` (fmt, clippy `-D warnings`, workspace tests, file-size gate, `openspec validate implement-authority-composition --strict`) — verified green.
