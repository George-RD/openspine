# Spec: Selected-thread email preview slice

## ADDED Requirements

### Requirement: Email workflow MUST require a trusted selected-thread token

Selected-thread email drafting MUST require a valid selected-thread token from a trusted owner selection path.

#### Scenario: Valid selected-thread token exists

Given the owner selects a Gmail thread through a trusted selection path
When the email workflow starts
Then the event MUST include a valid selection token
And the workflow MAY read only the selected thread.

#### Scenario: Shell provides thread ID directly

Given a shell or agent provides a Gmail thread ID directly
When no trusted selection token exists
Then selected-thread read authority MUST NOT be granted.

### Requirement: Email content MUST be treated as untrusted data

Email content MUST be treated as external communication data, not instruction.

#### Scenario: Email contains prompt injection text

Given the selected email thread says "ignore previous instructions"
When the drafter processes the thread
Then that text MUST be treated as quoted email content
And MUST NOT change system, policy, route, or authority behavior.

### Requirement: Email read MUST be selected-thread only

The email drafter MUST NOT receive inbox-wide read authority.

#### Scenario: Drafter requests inbox read

Given the email drafter has a selected-thread task grant
When it requests `email.read_inbox`
Then gate() MUST deny the request.

### Requirement: Attachments MUST be denied in the preview slice

The selected-thread preview slice MUST NOT read attachments.

#### Scenario: Selected thread has attachment

Given the selected email thread includes an attachment
When the drafter reads the thread
Then the attachment MUST NOT be read
And the preview MAY indicate attachments were excluded.

### Requirement: Model calls with private email context MUST use model gateway

Private email context MUST be sent to models only through the model gateway.

#### Scenario: Drafter needs model generation

Given the drafter needs to generate a reply
When private email context is included
Then the model request MUST go through the model gateway
And the private content MUST be wrapped as untrusted data.

### Requirement: Preview slice MUST NOT send email

The selected-thread email preview slice MUST NOT send email.

#### Scenario: Drafter requests email send

Given the drafter creates a proposed reply
When it requests `email.send`
Then gate() MUST deny the request.

### Requirement: Preview output MUST be reviewable by the owner

The generated draft preview MUST be presented to the owner for review.

#### Scenario: Draft preview is generated

Given a draft reply artifact is created
When the workflow completes
Then the owner MUST receive or access a preview
And no external email action MUST occur.
