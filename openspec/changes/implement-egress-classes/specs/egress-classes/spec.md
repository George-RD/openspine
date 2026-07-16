# Egress classes Specification Delta

## ADDED Requirements

### Requirement: The connector registry MUST type and protect egress endpoints

The connector registry MUST own the endpoint-to-egress-class rating for every registered egress endpoint. It MUST expose the AD-060 classes `search`, `forum-browse`, and `web-form-post`, and a conflicting registration for an existing endpoint MUST be rejected without changing the existing class.

#### Scenario: Registered web endpoints expose stable classes

Given the kernel connector registry is initialized
When the caller looks up `web.search`, `web.forum_browse`, and `web.form_submit`
Then the registry MUST return `search`, `forum-browse`, and `web-form-post` respectively
And an unrated action MUST return no egress class.

#### Scenario: A conflicting registration cannot downgrade an endpoint

Given `web.form_submit` is already rated `web-form-post`
When a caller attempts to register it as `search`
Then registration MUST return a structured conflict
And the registry MUST continue to return `web-form-post`.

### Requirement: Capability packs MUST reference allowed egress classes

A capability pack MAY declare an `allowed_egress_classes` list. Authority composition MUST copy that list into the resulting task grant, and the grant-chain root MAC MUST cover the list so post-sealing mutation is invalid.

#### Scenario: Pack egress classes become live grant authority

Given an active capability pack declares only `search`
When authority composition mints a task grant
Then the task grant MUST carry only `search` in its allowed egress classes
And adding another class after sealing MUST make grant verification fail.

### Requirement: The gate MUST enforce registry-rated egress classes

For every action that the trusted connector-registry classifier rates with an egress class, `gate()` MUST require that class in the task grant's allowed egress classes. The classifier MUST be a required trusted gate input; request payloads MUST NOT be the source of endpoint classification. A missing class MUST produce the structured `egress_class_not_granted` denial.

#### Scenario: Search-class pack cannot submit a web form

Given a task grant allows `web.form_submit` as an action but grants only the `search` egress class
And the trusted registry rates `web.form_submit` as `web-form-post`
When the shell requests `web.form_submit` at the gate
Then the decision MUST be denied with `egress_class_not_granted`
And the form endpoint MUST NOT be dispatched.

#### Scenario: Search-class pack may query search

Given a task grant allows `web.search` and grants the `search` egress class
And the trusted registry rates `web.search` as `search`
When the shell requests `web.search` at the gate
Then the decision MUST be `Allow`.
