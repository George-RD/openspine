# Design: Core runtime schemas

## Context

OpenSpine receives events, verifies sources, resolves identity, selects routes, composes authority, issues task grants, runs agents/workflows, mediates effects through gate(), and audits outcomes.

The first implementation risk is inconsistent shape. This change defines the schema layer only.

## Schema format

Use YAML or JSON-serializable objects as the implementation-neutral schema representation.

The initial implementation may define these as Markdown examples plus JSON Schema, Zod, Pydantic, TypeScript types, or another typed layer. The important constraint is that each object has:

- stable `id` where relevant;
- `kind` or equivalent type discriminator where useful;
- `schema_version`;
- lifecycle state where the object can be activated/deactivated;
- explicit references rather than hidden coupling.

## Runtime schema groups

### Event and authenticity

- `event_envelope`
- source verification fields
- replay protection fields
- actor hints
- target refs
- lane and trust context

### Identity

- `identity_record`
- verified identifiers
- relationships
- confidence
- `identity_resolution`

Identity records store knowledge. They do not grant authority.

### Routing

- `route_artifact`
- `route_resolution`
- `route_conflict`

Routes are declarative artifacts. LLMs may not resolve route conflicts that affect authority.

### Authority

- `authority_source`
- `authority_composition_input`
- `authority_composition_result`
- `task_grant`

Task grant is the only live authority object presented to a running workflow.

### Execution boundary

- `action_request`
- `gate_decision`
- `approval_record`

Every effectful action is represented as an action request and mediated through gate().

### Connectors and model calls

- `connector_account`
- `account_role`
- `model_request`
- `model_response_ref`

Private-context model calls must be represented as model gateway requests.

### Artifacts and audit

- `artifact_ref`
- `audit_event`
- encrypted/hash reference fields
- lifecycle states

Private payloads must be referenced, not stored as raw audit text.

## Trade-offs

| Option | Benefit | Cost |
|---|---|---|
| Schemas first | Reduces ambiguity before code | Less immediate product progress |
| Connector first | Faster visible demo | Risks hardcoding authority semantics |
| Full JSON Schema now | Strong validation | May slow early iteration |
| Typed examples first | Faster | Requires later formalization |

## Decision

Define typed schema requirements and examples first. Use implementation types in a later change once the language/runtime stack is selected.
