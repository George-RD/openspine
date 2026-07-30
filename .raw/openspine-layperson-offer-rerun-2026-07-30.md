# OpenSpine layperson-first offer rerun

Date: 2026-07-30
Status: Approved copy and positioning input for the website and README
Method: Growth Arsenal `grandslam-offer` phases 0–5, followed by the `business-copy-style` paired-evaluation path
Trigger: Direct reader feedback that the current hero was technically correct but did not make a lay reader want or understand the product

## Executive finding

The previous positioning decision was directionally right and hierarchically wrong.

It correctly established that OpenSpine is the installed system, Lyra is the assistant, and the governed runtime is the reason the product differs. It then put that product hierarchy and runtime vocabulary in the first paragraph. A low reading-grade score showed that the sentences were short. It did not show that a new reader understood why the product mattered.

The revised order is:

1. **Desire:** a personal AI that can do useful work.
2. **Hesitation:** giving one AI broad access to an inbox, files, and accounts.
3. **Plain-English result:** each task can reach only what it needs.
4. **Current proof:** one selected Gmail thread, one reviewed draft, sending blocked.
5. **Product shape:** OpenSpine is the system; Lyra is the assistant.
6. **Mechanism:** credentials, task permissions, gates, and tests stay outside the model.
7. **Trade-off:** OpenClaw and Hermes do far more today; OpenSpine puts hard task limits ahead of breadth.

The product position remains **trust-first personal AI**. The copy changes from architecture-first to buyer-first.

## Research brief

### Product truth

OpenSpine is a self-hosted personal AI system. It ships with Lyra as the assistant a user talks to. The runtime verifies requests, builds short-lived task permissions, keeps connector credentials outside contained workers, gates AI-requested effects, and records the result.

The public alpha is narrow. Through a verified Telegram channel, Lyra can read one Gmail thread selected by the owner, draft a reply, and create the exact approved draft. Email sending is denied.

OpenSpine does not currently run OpenClaw, Hermes, or arbitrary third-party assistants through a finished compatibility layer.

### Current market context

OpenClaw and Hermes make the appeal of personal agents easy to understand. They offer messaging channels, memory, tools, skills, browser or terminal work, and scheduled automation. Their official material also documents configurable security controls and explicit trust assumptions.

OpenSpine should not claim that those projects ignore security. Its useful distinction is narrower:

> Mature personal agents optimise for broad capability and add controls around it. OpenSpine starts with the task boundary and grows capability inside it.

Primary sources reviewed:

- OpenClaw product site: https://openclaw.ai/
- OpenClaw security policy: https://github.com/openclaw/openclaw/blob/main/SECURITY.md
- Hermes Agent README: https://github.com/NousResearch/hermes-agent/blob/main/README.md
- Hermes Agent security policy: https://github.com/NousResearch/hermes-agent/blob/main/SECURITY.md
- OpenSpine README, threat claims, product context, comparison, and current source tree

### Direct reader evidence

The user read the current hero and reported that it still did not speak to a lay person who had seen the OpenClaw-style personal-agent hype and might want OpenSpine instead.

This feedback carries more weight than the previous self-evaluation. The prior evaluation measured sentence mechanics and product hierarchy, but it did not test whether the reader recognised their own desire and hesitation.

## Phase 0: discovery and personas

### Primary reader

**The interested but cautious personal-agent user.**

They have seen what OpenClaw, Hermes, or similar agents can do. They want an assistant that can handle useful work, but pause when setup reaches the main inbox, customer data, files, infrastructure, or other real accounts.

They do not begin with the words `runtime`, `authority`, `scope`, or `task grant`. Their question is simpler:

> Can I let this help without giving it access to everything?

### Current adopter qualification

The current alpha still requires a technical self-hoster who can use Docker, configure Telegram, connect Gmail OAuth, and follow a repository quickstart. This is an adoption constraint, not the language the hero should use to explain the problem.

The previous work conflated **who can install the alpha today** with **how any reader first understands the offer**.

### Secondary reader

**The agent product builder.**

They need one testable place to decide what each task may do when prompts, models, skills, and external content change.

### Personas

#### Maya: the curious adopter

- Has watched personal-agent demos and wants the same practical help.
- Has not connected a main inbox because broad access feels reckless.
- Current workaround: keep AI in chat and do account work manually.
- Main objection: “Is this a real assistant or a security framework I have to assemble?”
- Dream outcome: “Let it do the job without letting it wander through everything else.”

#### Sam: the cautious power user

- Has tried or runs a mature personal agent in a disposable or limited environment.
- Uses approvals, sandboxes, or separate accounts, but still finds the effective permission surface hard to reason about.
- Current workaround: approve constantly or avoid sensitive workflows.
- Main objection: “OpenSpine looks safer, but it does far less and takes more work.”
- Dream outcome: “Give each job a limit I can inspect and test.”

#### Priya: the agent builder

- Builds products or workflows that touch customer or company systems.
- Has permissions spread across prompts, tool code, channel rules, sandbox settings, and approval hooks.
- Current workaround: custom policy and logging around every integration.
- Main objection: “Why add another runtime instead of hardening my stack?”
- Dream outcome: “Keep task authority true even when the model and prompt change.”

## Phase 1: starving crowd

Niche:

> People actively interested in capable personal agents who stop at broad access to sensitive accounts, plus builders facing the same boundary in products.

| Indicator | Score | Evidence and limitation |
|---|---:|---|
| Pain | 8/10 | The desire is active, but the blocker is often fear and uncertainty rather than an existing paid workflow. Pain becomes acute at the moment of connecting real accounts. |
| Purchasing power | 7/10 | The audience already spends money or time on models, hardware, VPSs, and self-hosting. OpenSpine is free, so setup effort is the effective price. |
| Easy to target | 9/10 | The audience gathers around personal-agent repositories, self-hosting communities, local AI, security discussions, Hacker News, Reddit, and GitHub. |
| Growth | 9/10 | Personal-agent capability, integrations, and public interest continue to expand. More useful access makes the authority problem more important. |

**Total: 33/40. Green light.**

Critical market decision:

Do not target “people who care about AI security” in the abstract. Target people who already want a personal agent and are stalled at the account-access decision.

## Phase 2: pricing

OpenSpine has no monetary price today. The effective price is:

- Docker and self-hosting setup;
- Telegram and model-provider setup;
- Gmail OAuth and mailbox configuration;
- copied thread IDs;
- a narrow workflow compared with mature alternatives;
- time spent evaluating an evolving alpha.

### Position

**High-trust, high-effort alpha.**

OpenSpine should not compete as a cheaper OpenClaw or Hermes. It cannot win on feature count or onboarding today. It can earn adoption from people who value a harder task boundary enough to accept the setup.

### 10x challenge

To justify ten times the current setup effort, OpenSpine would need several useful guarded workflows, a much faster install, a clear first-run path, and evidence from external users.

Copy cannot solve that product gap. It can stop wasting the value already present by making the reason to care obvious.

### 1/10 challenge

At one tenth of the effort, a visitor would be able to see or run the boundary without connecting a real account. That remains product work, not a copy claim.

## Phase 3: value equation

| Variable | Score | Reason |
|---|---:|---|
| Dream outcome | 9/10 | A useful personal AI that can work with real accounts without open-ended reach is highly attractive. |
| Perceived likelihood | 6/10 | Named tests, contained workers, explicit denials, and the working Gmail path provide proof. Alpha status and little external adoption evidence reduce confidence. |
| Time delay | 3/10 | The first useful result follows significant setup. |
| Effort and sacrifice | 3/10 | The user gives up breadth, polished onboarding, and convenience. |

### Copy implications

Maximise the dream outcome by naming the personal-agent desire first.

Raise likelihood by placing the Gmail example and named proof beside the promise.

Do not pretend copy lowers setup time or effort. Qualify the alpha clearly.

### Fastest honest win

The current proof is:

1. choose one Gmail thread;
2. Lyra drafts a reply;
3. review the exact text and target;
4. create only the approved draft;
5. keep sending blocked.

This belongs in the first viewport because it turns “hard limits” into a scene a normal reader can picture.

## Phase 4: problem-to-solution stack

| Reader problem | Copy or product response | Plain-English result |
|---|---|---|
| “I do not know what this is.” | Lead with “personal AI” and name Lyra in the first paragraph. | It is an assistant system, not an abstract security library. |
| “Why would I choose this over a mature agent?” | State the trade-off directly. | Fewer features today; harder task limits that can be inspected and tested. |
| “What does a task limit mean?” | Use one selected Gmail thread as the example. | The AI cannot quietly switch to another thread. |
| “Does the AI hold my account keys?” | Explain credential separation after the outcome. | It cannot print or reuse a key it never received. |
| “Is this another safety promise?” | Put named tests and failure scenes close to the claim. | The limit is tied to something the reader can run and try to break. |
| “Can it do everything OpenClaw can?” | Reject feature-parity implications. | No. The alpha is deliberately narrow. |
| “Is Lyra a separate product?” | State the relationship once in plain language. | OpenSpine is the system; Lyra is the assistant included with it. |

### Trim and stack

Keep above the fold:

- the desired personal-agent outcome;
- the broad-access hesitation;
- one-task-at-a-time limits;
- Lyra as the assistant;
- the current Gmail proof;
- an action that shows the limits before asking for setup.

Move below the first explanation:

- runtime;
- authority;
- task grants;
- capability packs;
- policy composition;
- pre-gate path qualifications;
- full competitive nuance.

Cut from the hero:

- architecture as the opening subject;
- the phrase “model-driven worker” before the reader understands the job;
- `scope` as the first explanation of a task limit;
- category claims that sound like middleware.

## Phase 5: enhancement

### Bonuses, scarcity, and urgency

Not applicable. OpenSpine is an open-source alpha. Invented scarcity, countdowns, or value anchors would reduce trust.

### Risk reversal

A commercial money-back guarantee is not relevant. OpenSpine’s useful risk reversal is falsifiability:

> Do not take the limit on trust. Run the failure and inspect the test.

This is not a universal security guarantee. It is a bounded invitation to verify named claims.

### Offer name

Keep **OpenSpine**. Do not invent a campaign name that competes with the product and Lyra.

### Final offer statement

> A self-hosted personal AI for people who want useful account access without open-ended reach.

## Adversarial review and gate

Independent model-review tooling was not available in this connector session. The user’s direct feedback is external reader evidence. The remaining lenses below were run as separate structured passes and should not be misrepresented as independent humans.

### Sceptical marketer

- **Critical issue:** the previous hero started with internal product structure, so the reader had to understand the architecture before recognising the problem.
- **Issue key:** `audience-before-architecture`
- **Fix:** lead with personal-agent desire and broad-access hesitation.
- **Secondary issue:** “trust-first” and “hard limits” can become category adjectives unless one current scene appears beside them.
- **Fix:** put the Gmail proof in the first viewport.

### Business strategist

- **Critical issue:** copy could imply OpenSpine is ready to replace a mature agent on feature breadth.
- **Issue key:** `replacement-overclaim`
- **Fix:** state that OpenClaw and Hermes offer far more today and frame OpenSpine as a deliberate trade.
- **Operational issue:** setup and workflow breadth remain the main value leaks. Do not treat copy as their solution.

### Maya

- The previous copy sounded like a system diagram.
- The new direction answers the question she actually has: “Can it help without seeing everything?”
- She still needs the Gmail example to believe this is more than a principle.

### Sam

- Values the explicit trade-off more than a generic “safer agent” claim.
- Wants the tests and exact denial path immediately after the simple explanation.
- Rejects any implication that OpenSpine already has mature-agent breadth.

### Priya

- The simple hero is useful, but the mechanism must remain below it.
- The architecture section should still name credential separation, short-lived task permissions, and the effect gate.

### Gate result

The repeated causal issue is `audience-before-architecture`. It is fixed by the revised hierarchy.

`replacement-overclaim` is fixed by an explicit comparison sentence and alpha qualification.

No critical issue remains for the copy change. Product work on setup and broader useful workflows remains open.

## Paired copy evaluation

### Frozen baseline

> Give your AI real work. Not the master key.
>
> OpenSpine is the system you install. Lyra is the assistant you talk to. The runtime keeps your account keys away from the model. Each task gets a short-lived scope. The model-driven worker cannot reach your accounts beyond that scope.

### Candidate

> Let a personal AI do the job. Keep the rest of your accounts out of reach.
>
> OpenSpine comes with Lyra, the assistant you talk to. You choose the task. Lyra can use only what that task needs. Anything else stays out of reach.

### Deterministic comparison

The Growth Arsenal heuristic gives:

| Signal | Baseline | Candidate |
|---|---:|---:|
| Words | 48 | 43 |
| Sentences | 7 | 6 |
| Average words per sentence | 6.9 | 7.2 |
| Flesch-Kincaid grade | 3.8 | 2.6 |
| Em dashes | 0 | 0 |
| Tier-1 AI vocabulary | 0 | 0 |
| Hard gate | Pass | Pass |

The mechanical result does not choose the winner. Both pass. The candidate earns replacement because it fixes the reader-recognition failure without weakening the bounded claim.

### Reader panel

#### Skimmer

Keep the candidate. It says what kind of thing this is, names the desired job, and states the account-access limit without requiring `runtime`, `scope`, or `worker` knowledge.

#### Right-fit sceptic

Keep the candidate only with the Gmail proof and honest comparison beside it. Without those, “out of reach” is an unsupported promise.

#### Wrong-fit reader

The full page correctly rejects someone who wants the widest tool list or consumer-grade onboarding. The candidate itself is clearer rather than more exclusionary.

#### Mechanism reader

The baseline explains more of the mechanism in the hero. Preserve that information in the next section, not in the opening paragraph.

### Rubric

Score: `0 = fails`, `1 = partial`, `2 = clear`.

| Dimension | Baseline | Revised page | Reason |
|---|---:|---:|---|
| Target-audience recognition | 1 | 2 | The revised page starts from the desire and hesitation. |
| Category clarity | 2 | 2 | Both identify a personal AI system and Lyra. |
| Mechanism clarity | 2 | 2 | The revised page moves the mechanism down rather than removing it. |
| Specificity | 2 | 2 | The Gmail scene and account boundary remain concrete. |
| Action clarity | 2 | 2 | The primary action shows the limits before setup. |
| Trust and claim discipline | 2 | 2 | The alpha and named proof remain adjacent. |
| Wrong-fit rejection | 2 | 2 | The feature and setup trade-offs remain explicit. |
| Voice and memorability | 2 | 2 | The new line is simpler and tied to the user’s concern. |
| **Total** | **15** | **16** | The material win is reader recognition, not the one-point total. |

## Approved copy direction

### Badge

**Self-hosted personal AI · hard limits · alpha**

### Headline

**Let a personal AI do the job.\nKeep the rest of your accounts out of reach.**

### Supporting copy

**OpenSpine comes with Lyra, the assistant you talk to. You choose the task. Lyra can use only what that task needs. Anything else stays out of reach.**

### Recognition line

**Seen the personal-agent promise? OpenSpine is for the part that makes people pause: broad access to inboxes, files, and accounts.**

### Current-proof line

**Choose one Gmail thread. Lyra drafts a reply. Only the exact draft you approve is created. Sending stays blocked.**

### Competitive bridge

**OpenClaw and Hermes offer far more features today. OpenSpine makes a different trade: each task gets hard limits before the AI can reach your accounts.**

### Primary action

**See the limits in action**

## Process correction

The previous copy workflow treated these as nearly the same:

- low reading grade;
- short sentences;
- category clarity;
- layperson comprehension;
- buyer recognition.

They are not the same.

A new process rule follows from this failure:

> For an architecture-led product, the first copy gate must ask whether a reader can name the desire and hesitation without repeating any architecture term.

If they cannot, plain wording has not yet produced plain understanding.

## Limitations

- One real reader supplied direct feedback; there is no broader human comprehension test yet.
- The structured reviewer roles were produced by the same AI system and are not independent reviewers.
- No conversion, installation, or adoption data exists for either hero.
- OpenClaw and Hermes will continue to change; comparisons must be rechecked before future revisions.
- Product friction remains. Better copy cannot make the alpha a feature-complete personal assistant.
