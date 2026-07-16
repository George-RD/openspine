# identity-store Specification

## Purpose
TBD - created by archiving change implement-identity-store-and-principal. Update Purpose after archive.
## Requirements
### Requirement: Identity resolution MUST be a read-only seam that never binds or mints principals

Identity resolution MUST be a read-only process that resolves an incoming identifier to an identity and an optional principal_id.

Identity resolution MUST NOT perform any database writes.

An unknown identifier MUST resolve to RelationshipKind::Unknown with confidence 0 and principal_id None, and MUST NOT create any new identity or relationship records.

#### Scenario: Owner resolves successfully

Given the owner sends an authenticated Telegram message
When identity resolution runs
Then the result MUST yield the owner's principal_id.

#### Scenario: Unknown sender resolves to Unknown

Given an unknown sender sends a message
When identity resolution runs
Then the result MUST have RelationshipKind::Unknown
And confidence MUST be 0.0
And principal_id MUST be None
And no new database records MUST be created.

### Requirement: A Principal is a first-class, authority-free record and v1 enforces exactly one owner

A Principal record MUST represent a resolved identity with principal status.

The identity store MUST enforce that at most one Principal can be marked as the owner (`is_owner = true`) at the database layer.

Initialization/bootstrap MUST establish exactly one owner principal in the store, and MUST fail closed if it cannot.

Composition MUST consume a principal_id, and MUST fail closed if the resolved principal_id is absent.

#### Scenario: Idempotent bootstrap establishes exactly one owner

Given an empty store
When owner bootstrap is run
Then the store MUST contain exactly one owner principal
And a second concurrent bootstrap run MUST result in exactly one owner principal.

#### Scenario: Second owner principal insert is rejected

Given an owner principal already exists in the store
When a second owner principal is attempted to be inserted
Then the database MUST reject the insertion.

### Requirement: Identity binding MUST happen only via an audited, owner-approved path

Binding an identifier to an identity MUST require an authenticated owner-principal context at the API boundary.

Every identity binding mutation MUST be audited.

Identity binding MUST NOT be reachable via the agent task path.

#### Scenario: Owner asserts a binding successfully

Given the owner asserts a relationship binding with an owner-principal context
When the binding is executed
Then the identifier and relationship records MUST be created
And an audit record of kind `identity.bound` MUST be appended.

#### Scenario: Binding attempt without owner context is rejected

Given a request to bind an identity is received
When the request lacks an authenticated owner-principal context
Then the request MUST be rejected.

