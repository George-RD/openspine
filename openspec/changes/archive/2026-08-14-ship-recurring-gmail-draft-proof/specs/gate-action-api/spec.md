## ADDED Requirements

### Requirement: Catalog non-delegability MUST be explicit

The action catalog MUST carry the non-delegable action set consumed by reusable-authority validation and manifest rendering. `email.send` MUST be a catalog-owned non-delegable action. A standing-rule or owner-review manifest MUST NOT restate or override that catalog denial, and an attempted reusable delegation for `email.send` MUST fail closed before effect dispatch. The existing gate audit and opaque no-executor boundary remain unchanged.

#### Scenario: Final send is catalog-denied

- **WHEN** a proposal or capability pack attempts to include `email.send` in reusable authority
- **THEN** the catalog-backed judge MUST return the typed non-delegable refusal
- **AND** the manifest MUST render the catalog denial without an action-specific validation arm
- **AND** no send effect MUST execute

Test: `catalog_email_send_is_non_delegable`
