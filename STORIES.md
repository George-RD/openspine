# The six users

The stories decisions cite. Each user's failure modes sharpen what a decision
must prevent. Vocabulary: [CONTEXT.md](CONTEXT.md); the decision test that
consumes these stories: [DIRECTION.md](DIRECTION.md).

## Lyra (real — #114, #123)

Trusted owner's assistant. Authority grows only by explicit ratification.

Failure modes:
- Learning silently becomes permission.
- A quoted email body is treated as owner instruction.
- Delegation state is wrong after a crash.

## Bell (planned)

Multi-tenant AI receptionist for small businesses, over WhatsApp and web
chat. Conversational principals are unauthenticated strangers; capability is
fixed per tenant, never grows from conversation; identity step-up recomputes
the tool catalog. Bell drives tenancy, WhatsApp (#131), and hostile-input
work. Bell's product specifics stay in its own repo; only its demands on the
vocabulary belong here.

Failure modes:
- A stranger's words steer an in-grant action.
- Cross-tenant leakage.
- Internal data in a reply to a stranger.

## Unattended workhorse (real — overnight loops, #130)

No principal present, internal triggers, zero security overhead for that.

Failure modes:
- Silent death.
- Runaway spend.
- Replay firing an effect twice.

## Auditor (real)

Adversarial reader of the record months later.

Failure modes:
- An effect with no prior audit row.
- The approver of record holding a raw channel id.
- A record only the system can read.

## Outside framework (hypothetical)

Effect governance via the session surface, without packages. If no decision
cites this story, foreign-host mode is recorded dormant and nothing that only
it needs is paid for.

## Delegating owner (fenced — do not build)

Lyra authors packages; the kernel enforces strict attenuation; the owner
approves. In force now regardless: manifests must be machine-authorable,
machine-checkable, and renderable for review.
