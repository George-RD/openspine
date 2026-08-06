# External assessment: AutoGPT platform vs OpenSpine (ChatGPT Pro, pasted by owner 2026-08-06)

> Provenance: produced by ChatGPT Pro for the owner as a follow-up to the 2026-08-06
> OpenClaw/Hermes comparison; pasted into the roadmap session verbatim. Assessed the
> current `autogpt_platform`, not the deprecated `classic/` loop. OpenSpine's verdict
> on this document is recorded in D-150..D-152.

# Verdict

**AutoGPT does not replace OpenSpine unless you are willing to give up OpenSpine's core trust model.**

It does, however, implement much of the **capability-building layer OpenSpine is missing**:

* typed reusable blocks;
* graph-based workflow composition;
* natural-language agent construction;
* graph validation and repair;
* simulated dry runs;
* schedules and triggers;
* credentials onboarding;
* sub-agent composition;
* marketplace/versioning;
* substantial user-facing product infrastructure.

So the better interpretation is:

> **AutoGPT already solves much of the "Lego" part. OpenSpine is attempting to solve the "secure" part.**

The strongest target would be **an AutoGPT-style capability builder over an OpenSpine-style authority kernel**.

I assessed the current `autogpt_platform`, not the original AutoGPT loop under `classic/`, which the repository now describes as deprecated and outside its supported security scope.

---

# What AutoGPT is now

Current AutoGPT is closer to an AI-native **Zapier/n8n agent platform** than the original autonomous terminal agent.

Its principal abstraction is a versioned graph:

```text
inputs / triggers
       ↓
typed blocks connected by links
       ↓
sub-agents, logic, model calls and integrations
       ↓
outputs / external effects
```

The platform includes a visual builder, deployment and monitoring, schedules, webhooks, a marketplace and an AutoPilot interface for constructing agents through conversation. Agents are explicitly treated as automated workflows, with blocks representing integrations, transformations, models, scripts and control logic.

The graph model already contains much of what I suggested OpenSpine call a `CapabilityBundle`:

* versioned graph identity;
* nodes and typed links;
* graph-level input and output schemas;
* subgraphs;
* schedules;
* external triggers;
* credential requirements;
* graph settings;
* fork/version lineage.

## It already has the AI capability author

AutoGPT's agent-generation guide tells AutoPilot to:

1. search for an existing similar agent;
2. sample the user's real data;
3. discover suitable blocks;
4. discover reusable sub-agents;
5. generate a graph JSON;
6. validate it;
7. repair it;
8. save it;
9. run a dry-run;
10. inspect failures and iterate until it works.

It can also edit existing agents, compose sub-agents, add webhooks, configure schedules and use MCP tools.

That is very close to the capability-author loop discussed for OpenSpine:

```text
understand job
→ discover available bricks
→ compose workflow
→ validate
→ simulate
→ save and run
```

OpenSpine does not currently have anything comparably complete on this side.

---

# Does AutoGPT "self-build capability"?

It depends on what **capability** means.

## 1. New composition from existing bricks: yes

AutoGPT is already good at this.

A model can discover typed blocks, connect them into a graph, validate the graph, correct type/wiring errors, compose existing agents as sub-agents and dry-run the result. Blocks expose Pydantic input/output schemas, credential requirements, tests, mocks, costs, webhook definitions and execution methods.

This is the most important result of the comparison:

> **The basic "AI assembles software Lego into a new working automation" idea is not naive. AutoGPT has implemented it.**

## 2. New privileged brick or connector: not safely

AutoGPT contains an experimental block-generation path where an LLM writes a complete Python block, the system installs it into the backend, imports it and tests it.

But the corresponding installation block is disabled and explicitly described as providing remote code execution on the server.

That confirms the same boundary identified for OpenSpine:

```text
Self-compose trusted bricks      feasible
Self-create unprivileged logic   feasible in a sandbox
Self-create trusted connector    dangerous
Self-declare new authority       unacceptable
```

AutoGPT has solved **assembly** much more than it has solved **safe manufacture of new privileged bricks**.

## 3. Learn new workflows from repeated experience: only partly

AutoGPT now has a "dream" pipeline that:

* consolidates existing memories;
* derives tentative findings, rules, preferences and plans;
* sanitizes the proposed operations;
* writes consolidated facts as active memory;
* writes novel findings as tentative memory;
* demotes stale or contradicted relationships.

Tentative findings can be promoted automatically after they are retrieved into useful context at least once; unused tentative findings are eventually superseded.

This is interesting adaptive memory, but it is not the same as:

```text
observe repeated job
→ construct a new agent graph
→ evaluate it
→ propose its effects and scope
→ receive owner approval
→ activate it as a new responsibility
```

I did not find that complete longitudinal capability-growth loop in the current repository. AutoPilot builds and edits agents when asked. Dream evolves memory. Those two systems do not yet appear to form a governed self-modifying agent system.

---

# AutoGPT versus OpenSpine

| Dimension                             |  AutoGPT |                           OpenSpine |
| ------------------------------------- | -------: | ----------------------------------: |
| Capability composition                | **9/10** |                                4/10 |
| Natural-language capability authoring | **9/10** |                                2/10 |
| Typed executable workflow model       | **8/10** |                                5/10 |
| Integrations and reusable bricks      | **9/10** |                                2/10 |
| Product and builder UX                | **9/10** |                                2/10 |
| Scheduling, triggers and deployment   | **8/10** |          5/10 runtime, 2/10 product |
| Adaptive memory                       | **7/10** |                                5/10 |
| Per-task least privilege              |     3/10 |                            **9/10** |
| Credential isolation from agent code  |     4/10 |                            **9/10** |
| Governed authority growth             |     4/10 | **9/10 architecture**, 4/10 shipped |
| Current practical completeness        | **9/10** |                                3/10 |
| Product licensing freedom             |     4/10 |                           **10/10** |

The projects are optimised around almost opposite centres:

```text
AutoGPT:
Make it easy to assemble and operate agents.

OpenSpine:
Make it hard for agents to exceed reviewed authority.
```

---

# The material security difference

AutoGPT has legitimate security controls:

* encrypted credential storage;
* user/team/organisation credential scopes;
* access-control checks;
* human review blocks;
* optional sensitive-action review;
* credential stripping from exported graphs;
* SSRF protections in relevant blocks;
* dry-run simulation;
* tests and marketplace review.

But it uses a conventional **trusted automation platform** model rather than an OpenSpine-style capability-security model.

## Credentials enter block execution

A block declares a credential field, and the executor passes the resolved credential object into its Python `run()` implementation.

The SMTP block, for example, receives the username and password, unwraps the `SecretStr` values, logs into the SMTP server and sends the message directly.

That means trusted block code sits inside the credential trust boundary.

OpenSpine's intended model is different:

```text
AutoGPT

graph executor
   ├── block code
   ├── decrypted credential
   └── direct external effect
```

```text
OpenSpine

model-driven worker
   ├── task token
   └── action intent
          ↓
trusted kernel
   ├── resolves account and target
   ├── checks grant and policy
   ├── uses credential
   └── dispatches effect
```

OpenSpine's shell receives only the kernel endpoint and task token, while the kernel retains connector and provider credentials.

## Review is optional and comparatively coarse

AutoGPT blocks have an `is_sensitive_action` Boolean. It defaults to false.

A sensitive block is reviewed only when:

```text
block.is_sensitive_action
AND graph.sensitive_action_safe_mode
```

The graph-level sensitive-action safe mode defaults to **false**. The frontend uses the same default and describes the disabled state as allowing sensitive blocks to proceed automatically. Direct block execution also explicitly skips the review path because it lacks graph execution context.

Human review can approve, reject or edit the complete block input. Auto-approval is keyed to a node within a graph execution and then applies to current inputs for subsequent executions of that node.

This is useful workflow review. It is not equivalent to OpenSpine's proposed contract covering:

* canonical action semantics;
* connector implementation and version;
* connector instance;
* account;
* target;
* counterparty;
* payload and target digests;
* reviewed scope;
* quotas and expiry;
* policy compatibility;
* executor and resolver readiness.

## Blocks are the trusted computing base

AutoGPT's executor service runs block Python. Adding a block means adding Python code to the backend. Some blocks individually implement SSRF checks, file limits and other protections, but a faulty trusted block can still access whatever its process and credential inputs permit.

OpenSpine is trying to make the model-driven component structurally unable to hold credentials or execute arbitrary connector effects. That is a stronger but much more restrictive design.

---

# AutoGPT's dry run is useful, but not an authority proof

AutoGPT's dry-run path is strong product engineering. For most blocks, an LLM simulates the block from:

* block description;
* source code;
* input schema;
* output schema;
* actual inputs.

Real external APIs and effects are normally not invoked.

This is well suited for:

* checking graph connectivity;
* generating representative output shapes;
* finding missing inputs;
* testing whether the workflow is coherent;
* improving the AI builder loop.

But it does not establish that:

* a live request matches an approved account and counterparty;
* an effect will be idempotent;
* changed targets fall back before execution;
* an overlapping rule is rejected;
* budget reservations remain atomic;
* a connector implementation cannot bypass reviewed scope.

The simulator even deliberately returns the last parseable output after repeated schema-conformance failures rather than failing the entire dry run. That is a reasonable UX trade-off, but it means "dry run passed" should not be treated as a security or correctness proof.

OpenSpine's planned proposal-specific evaluator is intended to replay the exact proposed scope against positive and adversarial contexts, with verdicts bound to the proposal and runtime compatibility versions.

---

# Would AutoGPT alone satisfy your actual objective?

## Yes, if the objective is this

> I want to tell an AI what automation I need, have it build a graph from integrations, test it, schedule it and improve my workflows.

AutoGPT appears much closer to that outcome today than OpenSpine.

For low-risk or owner-supervised workflows, it may already be the practical answer.

Examples:

* research and summarize;
* transform files;
* monitor websites;
* generate reports;
* retrieve and combine data;
* draft content;
* post notifications;
* schedule recurring workflows;
* orchestrate existing agents.

## Probably, if you accept this trust model

> The self-hosted AutoGPT backend and its installed blocks are trusted with my credentials and accounts, while sensitive effects use optional review controls.

That is a common and workable automation-platform trust model.

## No, if the objective remains this

> Lyra should be able to learn and construct more capability over time without the model, generated workflow or worker process ever obtaining broad standing access—and without learning silently becoming permission.

AutoGPT does not provide the equivalent of OpenSpine's:

* one short-lived task grant as the only live authority;
* kernel-owned trusted scope resolution;
* model-independent effect gate;
* credentials withheld from the worker;
* scope-bound reusable authority;
* proposal-specific authority evaluation;
* separate competence and authority lifecycles.

That is not a small configuration difference. It is a different execution architecture.

---

# What AutoGPT should replace in OpenSpine

AutoGPT should substantially change the **build-versus-borrow decision** for OpenSpine.

I would not continue inventing all of these concepts from first principles:

| Missing OpenSpine component     | AutoGPT analogue                         |
| ------------------------------- | ---------------------------------------- |
| `CapabilityBundle`              | Agent graph                              |
| Typed executable workflow steps | Blocks and typed links                   |
| Capability-author agent         | AutoPilot                                |
| Brick discovery                 | `find_block`                             |
| Existing capability reuse       | Library agents and `AgentExecutorBlock`  |
| Static validation               | `validate_agent_graph`                   |
| Automated graph repair          | `fix_agent_graph`                        |
| Behaviour preview               | Dry-run simulator                        |
| Capability store                | Library and Marketplace                  |
| Capability versioning           | Graph and listing versions               |
| Triggers                        | Webhook blocks                           |
| Recurring execution             | Schedules                                |
| Connector setup UX              | Credential schemas and integration cards |
| Experience consolidation        | Dream pipeline                           |
| Builder UI                      | Visual flow editor                       |

AutoGPT is strong evidence that OpenSpine's low-level route/workflow/pack objects should be treated as a **compiler target**, not as the interface Lyra directly authors.

---

# What OpenSpine would add to AutoGPT's block model

A secure OpenSpine brick would need more than AutoGPT's current:

```text
input schema
output schema
credential type
run()
is_sensitive_action
```

It would need:

```text
canonical action
effect kind
reversibility
destination / visibility
connector implementation identity
kernel-owned target resolver
required reviewed-scope dimensions
idempotency semantics
delivery-unknown semantics
budget bounds
credential handle, never credential bytes
evaluation fixtures
compatibility version
```

That is what the recently added OpenSpine action and implementation descriptors are moving towards.

A graph could then compose those secure bricks freely, while the graph itself remains non-authoritative.

---

# Viable architecture options

## Option A — Replace OpenSpine with AutoGPT

| Factor                        | Assessment             |
| ----------------------------- | ---------------------- |
| Time to useful system         | **Best**               |
| Capability breadth            | **Best**               |
| UX                            | **Best**               |
| Strict authority architecture | Weakest                |
| Long-term differentiation     | Weak                   |
| Fork burden                   | None if used unchanged |
| Commercial flexibility        | Problematic            |

This makes sense for a personal prototype or conventional internal automation platform.

It does not preserve OpenSpine's key proposition.

## Option B — Fork AutoGPT and retrofit OpenSpine security

This would require:

* removing credentials from block execution;
* replacing every effectful block with an intent adapter;
* routing all effects through OpenSpine;
* disabling generic HTTP, code and MCP escape paths unless separately contained;
* restricting executor network access;
* minting a task grant for each graph execution;
* resolving account, target and counterparty in the kernel;
* binding HITL decisions to immutable action requests;
* adding responsibility budgets, expiry and drift;
* changing graph activation and self-learning semantics.

At that point, a large part of AutoGPT's execution engine would be bypassed or rewritten.

This is probably the worst option: a large permanent fork of a fast-moving, multi-service platform.

## Option C — AutoGPT-style builder, OpenSpine execution kernel

```text
Lyra / capability author
        ↓
AutoGPT-style graph IR
        ↓
OpenSpine capability compiler
        ↓
validated non-authoritative bundle
        ↓
owner reviews effects and scope
        ↓
OpenSpine task grants and action gate
        ↓
connectors
```

This is the strongest architecture.

AutoGPT's graph is used as the **job description and dataflow**, not as authority. Every effectful block compiles to an OpenSpine action intent. Pure computation can execute in a contained worker.

## Option D — AutoGPT as a temporary external builder

A practical experiment would be:

1. create several workflows through AutoGPT AutoPilot;
2. export the graphs;
3. identify the minimum block/graph subset;
4. write a small translator into an OpenSpine `CapabilityBundle`;
5. execute only pure/model steps initially;
6. replace one external effect with an OpenSpine-gated action;
7. compare build effort and user experience.

This would test the architecture without committing to an AutoGPT dependency.

---

# Licensing changes the decision

Most of the modern platform lives under the PolyForm Shield license, not MIT.

Its noncompete clause prohibits using the software to provide a competing product, and defines competition broadly enough to include products with different interfaces, technical platforms and even free offerings.

For personal or internal experimentation, this may be acceptable. For using AutoGPT Platform as the foundation of a public OpenSpine product, I would not assume it is permitted without specific legal advice or a commercial agreement.

That makes the likely strategy:

> **Study and independently reproduce the relevant architecture, rather than making `autogpt_platform` a core OpenSpine dependency.**

---

# Implication for the OpenSpine roadmap

The existing progressive-delegation sequence is still necessary:

* shared real effect executors;
* kernel-resolved scope;
* channel-neutral review;
* proposal-specific evaluation;
* one complete Gmail delegation proof;
* a second protocol proof.

AutoGPT does not make that work obsolete.

It does suggest that OpenSpine should pull forward a parallel **capability construction track**:

1. minimal graph/`CapabilityBundle`;
2. typed block registry;
3. capability-author agent;
4. graph validator;
5. simulated and deterministic evaluation;
6. sub-capability composition;
7. atomic versioning and rollback;
8. a basic visual or generated graph view.

Issue #132 currently owns whole-responsibility composition, but a minimal graph-shaped composition unit should probably land earlier.

---

# Recommendation

**Do not replace OpenSpine with AutoGPT outright.**

Use AutoGPT to correct OpenSpine's abstraction and product direction:

```text
AutoGPT teaches OpenSpine how capabilities should be built.

OpenSpine determines how those capabilities are allowed to act.
```

For immediate personal use, AutoGPT is likely worth running as a separate self-hosted experiment. It can reveal which workflows, integrations and builder interactions actually matter before OpenSpine recreates them.

For the OpenSpine product, the next meaningful architectural move is no longer merely "invent a `CapabilityBundle`." It is:

> **Define an AutoGPT-like typed graph IR, but compile its effectful nodes into OpenSpine action descriptors, kernel-resolved scopes and per-run task grants.**

That would produce the actual "secure Lego" model:

* the AI can assemble existing bricks;
* graphs can be generated, validated, simulated, versioned and shared;
* pure logic runs in contained workers;
* connector credentials remain in the kernel;
* every effect still passes through one authority boundary;
* a generated graph can request responsibility but cannot promote itself.
