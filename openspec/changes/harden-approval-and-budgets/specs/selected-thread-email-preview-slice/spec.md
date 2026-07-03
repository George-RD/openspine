# Spec: Selected-thread email preview slice

## ADDED Requirements

### Requirement: Selection tokens MUST be single-use, expiring, and scope-fixed

Selection tokens MUST be minted only by the kernel, never by the shell.
Consuming a token MUST be atomic (at-most-once). A token's `expires_at`
MUST be enforced at use. A token's scope (no attachments, bounded message
count) MUST be fixed at mint time and MUST NOT be widened by the shell.

This requirement is already fully implemented by
`dispatch_read_selected_thread`; this delta brings the capability spec up
to date with existing, tested behaviour rather than describing new work.

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
