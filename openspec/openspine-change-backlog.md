# OpenSpine OpenSpec change backlog

This backlog lists recommended OpenSpec changes after `define-openspine-development-process`.

It is not runtime authority.

## Near-term sequence

### 1. define-core-runtime-schemas

Purpose:

Convert the PRD’s core runtime concepts into typed schemas.

Scope:

- event envelope;
- source verification fields;
- identity resolution output;
- route artifact;
- route resolution result;
- agent manifest;
- workflow manifest;
- capability pack;
- authority composer input/output;
- task grant;
- action request;
- gate decision;
- selection token;
- model request;
- approval record;
- audit event;
- artifact lifecycle state.

Non-goals:

- no Telegram implementation;
- no Gmail implementation;
- no model calls;
- no real connector execution.

### 2. implement-authority-composition

Purpose:

Implement deny-by-default authority composition and task-grant creation.

Scope:

- merge rules;
- explicit deny precedence;
- approval-required precedence;
- allowed action intersection;
- lane/connector/account-role constraints;
- task-grant materialization;
- tests for conflicting authority sources.

Non-goals:

- no real connector calls;
- no final email send;
- no runtime agent orchestration.

### 3. implement-gate-action-api

Purpose:

Create the first gate-mediated action boundary.

Scope:

- typed action requests;
- task-grant validation;
- allowed/denied/approval-required results;
- audit event emission;
- deny tests.

Non-goals:

- no broad connector implementation;
- no autonomous workflow activation.

### 4. implement-telegram-owner-control-slice

Purpose:

Build the first usable Lyra owner control channel.

Scope:

- Telegram bot connector;
- configured owner Telegram ID verification;
- `telegram.owner.message`;
- owner identity;
- deterministic route to `main_assistant_agent`;
- owner-control capability pack;
- owner-control task grant;
- status response;
- setup/proposal stubs;
- audit metadata.

Non-goals:

- no email access;
- no external communication ingestion;
- no infrastructure actions.

### 5. implement-selected-thread-email-preview-slice

Purpose:

Build the first guarded external communication workflow.

Scope:

- Gmail / Google Workspace owner mailbox setup;
- selected-thread token;
- `email.thread.selected`;
- selected-thread read only;
- no attachments;
- email content as untrusted data;
- `email_reply_drafter`;
- model gateway wrapping;
- local preview;
- audit metadata.

Non-goals:

- no inbox-wide read;
- no final send;
- no autonomous draft creation;
- no Outlook/IMAP/AgentMail yet.

### 6. implement-digest-bound-draft-approval

Purpose:

Allow creating a Gmail draft only after exact digest-bound approval.

Scope:

- immutable draft artifact;
- approval record;
- payload digest;
- target digest;
- Gmail create-draft action;
- approval invalidation on mutation;
- draft delete/revert helper.

Non-goals:

- no final send;
- no autonomous approval;
- no recipient changes after approval.
