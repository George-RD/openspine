# Proposal: Implement selected-thread email preview slice

## Summary

Implement Lyra's first guarded external communication workflow: selected-thread email reply preview.

## What Changes

This change adds the minimum selected-thread email workflow:

- Gmail / Google Workspace owner-mailbox connector;
- trusted selected-thread path;
- selection token;
- `email.thread.selected` event envelope;
- specialist `email_reply_drafter`;
- selected-thread-only read;
- no attachments;
- model gateway request wrapping;
- local/owner preview;
- no final send;
- audit refs.

## Why

Email is high-risk external content. The PRD intentionally limits early email work to selected-thread drafting and preview, avoiding inbox-wide reads, attachments, and final send.

## Affected layer

Both OpenSpine core and Lyra product.

## Authority sensitivity

Authority-sensitive.

This change handles private owner mailbox content and external communication data.

## Goals

- Read only a user-selected email thread.
- Treat email content as untrusted data.
- Draft a reply through a specialist workflow.
- Present preview to owner.
- Prevent final send.
- Audit private payloads by refs/hashes.

## Non-goals

- Do not read inbox-wide email.
- Do not read attachments.
- Do not send email.
- Do not create Gmail drafts yet unless separately approved in a later change.
- Do not support Outlook, IMAP, or AgentMail yet.

## Decision-log check

This change preserves the accepted decisions in `.raw/openspine-decision-log.md`.

If implementation reveals a need to weaken, reverse, or materially refine an accepted decision, update the decision log before completing the change.
