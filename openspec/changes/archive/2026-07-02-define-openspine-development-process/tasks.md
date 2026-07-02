# Tasks: Define OpenSpine development process

## 1. Add development-process spec

- [x] Create `openspec/changes/define-openspine-development-process/specs/openspine-development-process/spec.md`.
- [x] Add requirement that OpenSpec artifacts do not grant OpenSpine runtime authority.
- [x] Add requirement that every change classifies affected layer: OpenSpine core, Lyra product, both, or development tooling.
- [x] Add requirement that authority-sensitive changes are explicitly marked.
- [x] Add requirement that security-sensitive changes include verification tasks.
- [x] Add requirement that decision-log consistency is checked.
- [x] Add requirement that PRD-derived work is split into implementation slices.
- [x] Add requirement that OpenSpec archive preserves rationale.
- [x] Add requirement that tool-specific skills avoid drift.

## 2. Add development-process design

- [x] Create `openspec/changes/define-openspine-development-process/design.md`.
- [x] Explain the OpenSpec/OpenSpine boundary.
- [x] Explain the OpenSpine/Lyra boundary.
- [x] Define the OpenSpine development lifecycle.
- [x] Define authority-sensitive change handling.
- [x] Define why the default `spec-driven` schema is acceptable for now.
- [x] List recommended next implementation slices.
- [x] Document trade-offs and failure modes.

## 3. Add proposal

- [x] Create `openspec/changes/define-openspine-development-process/proposal.md`.
- [x] Explain why this process change is needed before implementation.
- [x] Define goals, non-goals, scope, risks, and next implementation slices.
- [x] State that this change does not implement runtime code.

## 4. Strengthen OpenSpec config

- [x] Update `openspec/config.yaml` with OpenSpine project context.
- [x] Add proposal rules for layer classification, authority sensitivity, non-goals, and decision-log checks.
- [x] Add spec rules for testable requirements and runtime authority boundaries.
- [x] Add design rules for authority, containment, audit, and failure modes.
- [x] Add task rules for verification and decision-log updates.

## 5. Review consistency with existing PRD and decision log

- [x] Confirm terminology: OpenSpine is substrate; Lyra is product.
- [x] Confirm OpenSpec is development/change-management layer, not runtime.
- [x] Confirm future implementation slices match PRD phase plan.
- [x] Confirm no task in this change implements runtime behavior.

## 6. Prepare future changes

- [x] Create a backlog entry or note for `define-core-runtime-schemas`.
- [x] Create a backlog entry or note for `implement-authority-composition`.
- [x] Create a backlog entry or note for `implement-gate-action-api`.
- [x] Create a backlog entry or note for `implement-telegram-owner-control-slice`.
- [x] Create a backlog entry or note for `implement-selected-thread-email-preview-slice`.

## 7. Archive readiness

- [x] Run `openspec status --change "define-openspine-development-process"`.
- [x] Verify proposal, specs, design, and tasks are complete.
- [x] Archive only after review.
