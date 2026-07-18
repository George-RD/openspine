# authority-equivalence-matcher Specification

## Purpose
TBD - created by archiving change implement-authority-equivalence-matcher. Update Purpose after archive.
## Requirements
### Requirement: Kernel computes authority-equivalence classes deterministically from declared action lists

The kernel — never the shell — MUST compute equivalence classes from
composed grants. Two candidates are in one class iff their composed
`(allowed_actions, approval_required_actions, denied_actions,
output_channels, limits)` are identical (AD-147). The class
identity MUST be derived from `compose_authority` output, never from
a shell- or LLM-supplied class label.

#### Scenario: Within-class picks compose an identical grant

- **WHEN** candidates share a composed authority tuple and a
  semantic matcher selects any member of the class
- **THEN** every selected member's grant MUST have identical
  `allowed_actions`, `approval_required_actions`, `denied_actions`,
  `output_channels`, and `limits` to the class baseline, so the
  pick cannot widen authority
  (`test: property_all_authority_dimensions_define_classes_and_identical_grants`)

### Requirement: Class identity equals the composed authority tuple

Class identity MUST be exactly the composed
`(allowed_actions, approval_required_actions, denied_actions,
output_channels, limits)`. Declaration ORDER MUST NOT change
class identity; only the set of composed actions and channels
and the limits matter (AD-147).

#### Scenario: Identical declared sets form one class

- **WHEN** two candidates declare the same action lists (in any
  order) and compose
- **THEN** they MUST fall into exactly one authority class
  (`test: two_identical_inputs_form_one_class`)

#### Scenario: Declaration order does not change class identity

- **WHEN** two candidates declare the same action set in different
  orders
- **THEN** they MUST share one class identity, proving the
  matcher compares composed sets, not declaration order
  (`test: declared_list_order_does_not_change_class`)

### Requirement: The matcher picks only within one class; a cross-class pick is structurally impossible

The semantic matcher MUST receive only a class-scoped view and
MUST return a member of exactly that class. It MUST NOT be
able to return a member of a different class; cross-class
ambiguity MUST resolve by deterministic rule or escalation (AD-147).
The class-scoped selection return type MUST be constructible
only from the chosen class, so a cross-class pick cannot be
expressed by the type.

#### Scenario: Distinct classes escalate and never cross-pick

- **WHEN** more than one known class matches a semantic query
- **THEN** resolution MUST escalate and MUST NOT return a member
  of any class, so a cross-class pick is structurally impossible
  (`test: distinct_inputs_form_separate_classes_and_escalate`)

### Requirement: Class construction is sealed to the kernel composition path

The only public candidate builder MUST run `compose_authority`
(the same kernel function that mints a live grant). A shell or
LLM MUST NOT be able to label an arbitrary `TaskGrant` with a
forged class. An unknown action id MUST surface as a class
construction error, never as a silently-widened class (D-053).

#### Scenario: Unknown action id is a class error, never a widened class

- **WHEN** a candidate's declared action id is not in the canonical
  `ActionCatalog`
- **THEN** class construction MUST fail with a structured error
  and MUST NOT mint a class that smuggles the unknown id into
  authority (`test: compose_denial_is_a_class_error`)

