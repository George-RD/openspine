# Spec: Gate action API

## ADDED Requirements

### Requirement: Gate MUST verify authenticated grant caveat chains offline

`gate()` MUST verify the grant's Macaroons-simple HMAC chain offline using a
key from `GateContext`, without parent-grant database lookups. Verification
replays: root-authority commitment, then each ordered caveat, then the
instance bind (`id`, `parent_grant_id`, `mode`). Invalid MAC, reordered,
removed, or altered caveats, or altered root authority fields MUST deny with
reason `caveat_widening`.

After authentication, `gate()` MUST evaluate the request against effective
authority: immutable root allow/deny/approval lists and limits/expiry,
attenuated by caveats (action allowlists, earlier expiry, output-channel
allowlists, unchanged bound parameters). A child MUST only have appended
caveats relative to its chain; caveats are the attenuation proof (AD-101).
The presented grant remains the only live authority object (D-007); parent
lineage fields are not a second allow/deny source.

#### Scenario: Valid sealed root allow

Given a live root grant with a valid MAC and an allowed action
When gate evaluates that action
Then the decision MUST be allow.

#### Scenario: Tampered or reordered caveats are rejected

Given a grant whose caveat bytes are reordered, removed, or whose root
authority fields were edited after sealing
When gate evaluates any action
Then gate MUST deny with reason `caveat_widening`.

#### Scenario: Action outside an action_allowlist caveat is not granted

Given a sealed grant whose root allows `openspine.status.read` and
`lyra.ui.preview` and that carries an `action_allowlist` caveat of only
`openspine.status.read`
When gate evaluates `lyra.ui.preview`
Then gate MUST deny (not granted or caveat_widening)
And MUST NOT allow the action.

### Requirement: Shadow-mode grants MUST return a non-executable decision

When `mode = shadow` and the normal decision path would return `allow` or
`approval_required`, `gate()` MUST return `effect_suppressed` instead — a
decision that MUST NOT be treated as executable success by any dispatch or
effect path. Deny decisions under shadow remain `deny`. Live grants MUST
never produce `effect_suppressed`. Shadow mode MUST NOT widen authority.

#### Scenario: Shadow allow becomes effect_suppressed

Given a shadow grant that would allow `openspine.status.read` if live
When gate evaluates that action
Then the decision MUST be `effect_suppressed`
And MUST NOT be `allow`.

#### Scenario: Shadow deny remains deny

Given a shadow grant that does not allow `email.send`
When gate evaluates `email.send`
Then the decision MUST be `deny`.

#### Scenario: Dispatch does not execute on effect_suppressed

Given a shadow grant for which gate returns `effect_suppressed` for an action
When the kernel action dispatch path handles the outcome
Then it MUST NOT invoke the action's effect handler
And MUST NOT perform the external side effect.
