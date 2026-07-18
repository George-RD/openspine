# Change: Implement standing rules

## Why
Repeated owner approvals need a bounded, reviewable representation that can be consulted at the live gate without turning shell behavior into authority.

## What Changes
- Add a versioned, revocable standing-rule artifact with expiry and drift-triggered re-review.
- Route standing-rule proposals through the existing artifact proposal, overlay evaluation, approval, and activation ceremony.
- Consult active rules only when the ordinary gate requires approval, atomically enforcing independent quota and rate windows.
- Return remaining rule budget with the action response.
- Implement optional dark-window defaults through durable kernel timers; AD-012 remains leaning and its concretization is recorded as an unnumbered candidate.

## Impact
Touches artifact parsing/activation, action mediation, kernel storage/migrations, durable timer consumption, and the shared schema crate. Task grants remain the only live authority objects.
