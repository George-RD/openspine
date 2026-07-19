# Change: Implement disclosure policy

## Why
Private context in an outbound query is itself an effect. Relationship-scoped policy and immutable briefcase provenance must make disclosure checks deterministic and auditable.

## What Changes
- Add a strict `DisclosurePolicy` keyed by relationship and disclosure class.
- Generalize query text before egress while checking immutable classified provenance.
- Persist owner-approved scoped policies and carve-outs alongside one independent D-107 standing-rule envelope per `(relationship, disclosure_class, egress_class)` scope.
- Bind prepared queries to their grant and kernel-derived provenance, and reserve the disclosure envelope budget before rated egress.
- Block uncovered disclosure classes and route an owner-only AD-133 question through the canonical escalation surface; owner answers resolve by pending-question id and use the kernel-stored blocked-query digest.

## Impact
Touches the schema crate, kernel disclosure boundary, policy storage, migrations, and owner-question escalation payloads. Existing task grants remain the only live authority objects.
