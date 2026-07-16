# Spec: Selected-thread email preview slice

## MODIFIED Requirements

### Requirement: Email workflow MUST require a trusted selected-thread token

Selected-thread email drafting MUST require a valid selected-thread token from a
trusted owner selection path. For the `email.read_selected_thread` action,
`gate()` itself validates the selection token — bound to the requesting grant,
exists, correct token type, not expired — for the token-requiring catalog entry
(see gate-action-api "Selection-token validation MUST occur inside gate() for
token-requiring actions"); the atomic single-use consume remains at dispatch.

#### Scenario: Valid selected-thread token exists

Given the owner selects a Gmail thread through a trusted selection path
When the email workflow starts
Then the event MUST include a valid selection token
And the workflow MAY read only the selected thread.

#### Scenario: Shell provides thread ID directly

Given a shell or agent provides a Gmail thread ID directly
When no trusted selection token exists
Then selected-thread read authority MUST NOT be granted.

#### Scenario: Gate validates the selection token for the read action

Given an `email.read_selected_thread` request carrying a selection token
When `gate()` evaluates the request
Then `gate()` MUST validate the token's grant-binding, existence, type, and expiry
And MUST deny if any check fails.

### Requirement: Selection tokens MUST be single-use, expiring, and scope-fixed

Selection tokens MUST be minted only by the kernel, never by the shell. A token's
validity — bound to its grant, exists, correct type, not expired — MUST be
validated by `gate()` for catalog-marked token-requiring actions (see gate-action-api
"Selection-token validation MUST occur inside gate() for token-requiring actions").
Consuming a token MUST remain atomic (at-most-once) at dispatch, after `gate()`
returns allow, so `gate()` stays pure. A token's scope (no attachments, bounded
message count) MUST be fixed at mint time and MUST NOT be widened by the shell.

#### Scenario: Token reused after consumption

Given a selection token has already been consumed once
When the shell attempts to use the same token again
Then the kernel MUST reject the request as a bad request, not a fresh read.

#### Scenario: Token used after expiry

Given a selection token whose `expires_at` has passed
When the shell attempts to use it
Then the kernel MUST reject the request.

#### Scenario: Token used by a foreign grant

Given a selection token bound to grant A
When a request authenticated under grant B references that token
Then the kernel MUST reject the request.

#### Scenario: Gate denies a missing or wrong-type token

Given an `email.read_selected_thread` request whose selection token is missing or of the wrong type
When `gate()` evaluates the request
Then `gate()` MUST deny the request before dispatch.
