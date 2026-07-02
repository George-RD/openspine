# OpenSpine OpenSpec change sequence

This sequence decomposes the PRD into reviewable OpenSpec changes.

## Completed / baseline

- `define-openspine-development-process`

## Next changes

### 1. define-core-runtime-schemas

Define OpenSpine runtime object schemas before implementation.

### 2. implement-authority-composition

Implement deny-by-default authority composition and task-grant materialization.

### 3. implement-gate-action-api

Implement the enforcement boundary for all effectful actions.

### 4. implement-telegram-owner-control-slice

Implement the first Lyra owner-control channel.

### 5. implement-selected-thread-email-preview-slice

Implement selected-thread email reply preview with no send and no draft creation.

### 6. implement-digest-bound-draft-approval

Allow Gmail draft creation only after exact digest-bound owner approval.

## Later changes not included in this bundle

- implement-model-gateway
- implement-audit-artifact-store
- implement-shell-containment
- implement-secret-intake
- implement-route-artifact-lifecycle
- implement-agent-manifest-registry
- implement-capability-pack-registry
- implement-memory-policy
- implement-deployment-reference
