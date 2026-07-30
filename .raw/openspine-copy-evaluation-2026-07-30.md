# OpenSpine paired copy evaluation

Date: 2026-07-30
Method: Growth Arsenal `business-copy-style` paired-evaluation path
Scope: landing hero and README opening

## Brief

- **Primary reader:** technical self-hoster or agent power user who wants a personal agent to touch real accounts but is unwilling to accept broad, hard-to-inspect authority.
- **Secondary reader:** agent builder who needs permissions enforced outside the model.
- **Three-second understanding:** OpenSpine is the assistant system; Lyra is the assistant; the runtime keeps the task inside hard limits.
- **Primary action:** inspect the working boundary.
- **Product truth that may not change:** OpenSpine is a reusable governed runtime; Lyra is the default package; the public alpha is narrow; arbitrary third-party assistant compatibility is not shipped; email sending is denied.
- **Wrong fit:** reader expecting feature parity with mature personal agents or consumer-grade setup.

## Frozen landing baseline

> Let AI use your tools. Keep the keys.
>
> OpenSpine sits between the model and your email or apps. Each task gets only the access it needs. It checks every action, asks you when approval is needed, and records the result. The model never sees raw credentials.

## Landing candidate

> Give your AI real work. Not the master key.
>
> OpenSpine is the system you install. Lyra is the assistant you talk to. The runtime keeps your account keys away from the model. Each task gets a short-lived scope. The model-driven worker cannot reach your accounts beyond that scope.

## Deterministic comparison

The metrics use the same heuristic and default gates as Growth Arsenal: Flesch-Kincaid grade at or below 6, zero em dashes, zero Tier-1 AI vocabulary, and average sentence length at or below 15 words.

| Landing signal | Baseline | Candidate | Candidate minus baseline |
|---|---:|---:|---:|
| Words | 46 | 48 | +2 |
| Sentences | 6 | 7 | +1 |
| Average words per sentence | 7.7 | 6.9 | -0.8 |
| Flesch-Kincaid grade | 4.1 | 3.8 | -0.3 |
| Em dashes | 0 | 0 | 0 |
| Tier-1 AI vocabulary | 0 | 0 | 0 |
| Hard gate | Pass | Pass | No regression |

The final sentence states the worker boundary rather than claiming that every runtime action crosses the effect gate. OpenSpine has a small, enumerated set of owner-selected metadata reads before grant composition. Those trusted paths are separately classified and audited.

## Reader lenses

### Skimmer

**Keep candidate.** It names the product relationship in the first three sentences: OpenSpine is the installed system, Lyra is the assistant, and the runtime protects the account boundary. The baseline can be read as middleware added to an assistant the reader already owns.

Strongest baseline line worth preserving: “Keep the keys.” The candidate carries the same idea through “Not the master key” and the explicit account-key sentence.

### Right-fit sceptic

**Keep candidate, with proof immediately below it.** “Hard limits” and “master key” could become generic security language without the live trace, current alpha statement, named failure scenes, and test ledger. The revised page supplies those.

Strongest objection to the baseline: it explains where OpenSpine sits but not what the user actually installs or who the assistant is.

Strongest objection to the candidate: “real work” is broader than the current Gmail alpha. The page therefore states the narrow current workflow before claiming broader product breadth.

### Wrong-fit reader

**Keep candidate page.** The new README and final landing section explicitly reject users who need the broadest feature set or consumer-grade onboarding. The baseline was understandable but did not give the wrong-fit reader a clear reason to leave.

### Mechanism reader

**Keep candidate.** Both versions communicate credential separation and task limits. The candidate adds the missing product hierarchy without removing the mechanism. A reader can now explain the distinction as: “Lyra proposes the work; the OpenSpine runtime decides what may happen.”

## Paired rubric

Score: `0 = fails`, `1 = partial`, `2 = clear`.

| Dimension | Baseline | Candidate | Reason |
|---|---:|---:|---|
| Target-audience recognition | 1 | 2 | Candidate speaks to the point where an installed personal agent receives real account access. |
| Category clarity | 1 | 2 | Baseline sounds like middleware; candidate names the system and assistant. |
| Mechanism clarity | 2 | 2 | Both explain credential separation and task limits. |
| Specificity | 2 | 2 | Both use account keys and task scope. |
| Action clarity | 2 | 2 | Baseline led to setup; candidate leads to the boundary, then setup. Both actions are visible. |
| Trust and claim discipline | 2 | 2 | Candidate keeps the alpha limit and proof ledger close to the promise. |
| Wrong-fit rejection | 1 | 2 | Candidate surfaces narrow workflow breadth and technical setup. |
| Voice and memorability | 2 | 2 | Both headlines are memorable; candidate better matches the actual product shape. |
| **Total** | **13** | **16** | The important win is category clarity, not the numeric total. |

## README opening comparison

The README candidate adds the missing product hierarchy and explicit non-compatibility boundary while remaining within the deterministic gates. The metrics below measure the explanatory body and alpha statement, excluding the bold category line in both variants.

| README signal | Baseline | Candidate |
|---|---:|---:|
| Words | 66 | 86 |
| Sentences | 6 | 10 |
| Average words per sentence | 11.0 | 8.6 |
| Flesch-Kincaid grade | 5.9 | 5.7 |
| Em dashes | 0 | 0 |
| Tier-1 AI vocabulary | 0 | 0 |
| Hard gate | Pass | Pass |

The extra words earn their place because they answer the user's actual confusion: OpenSpine does not currently run OpenClaw, Hermes, or another assistant; Lyra is the supported assistant path.

## Decision

**Adopt the candidate direction.**

The baseline mechanism was correct and should not be discarded. It now appears lower in the hierarchy, after the user understands what OpenSpine is and why real account access creates the problem.

## Limitations

- The author and evaluator are the same AI system, so this is structured paired judgement rather than an independent blind human panel.
- No user comprehension test, conversion data, or external adopter interview exists yet.
- The comparison names OpenClaw and Hermes from current official documentation. Their capabilities and security controls will continue to change.
- The next evidence that could reverse this decision is repeated reader confusion about the “OpenSpine system / Lyra assistant” hierarchy or evidence that the target audience expects a pure runtime library rather than an installed assistant system.