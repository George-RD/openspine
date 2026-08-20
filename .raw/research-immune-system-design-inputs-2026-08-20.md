# Research: Immune-System Design Inputs (Wayfinder Ticket #190)

**Date**: 2026-08-20  
**Target Lane**: Immune System (Prompt-Injection Defense for Governed Agent Kernel)  
**Governing Promise**: *External content fills parameters, never gives instructions.* (`CONTEXT.md` §2.4)  
**Primary Sources Evaluated**:
- **CaMeL**: Debenedetti, E. et al., *Defeating Prompt Injections by Design*, arXiv:2503.18813 (Google DeepMind & ETH Zurich, March 2025). [Code](https://github.com/google-research/camel-prompt-injection)
- **FIDES**: Costa, A. et al., *Securing AI Agents with Information-Flow Control*, arXiv:2505.23643 (Microsoft Research, May 2025). [Code](https://github.com/microsoft/fides)
- **Progent**: Shi, W. et al., *Progent: Securing AI Agents with Privilege Control*, arXiv:2504.11703 (April 2025).
- **RTBAS**: Zhong, W. et al., *RTBAS: Defending LLM Agents Against Prompt Injection and Privacy Leakage*, arXiv:2502.08966 (CMU Parallel Data Lab, February 2025).
- **AgentDojo & Adaptive Evaluations**: Debenedetti, E., Zhang, J. et al., *AgentDojo: A Dynamic Environment to Evaluate Attacks and Defenses for LLM Agents*, arXiv:2406.13352 / NeurIPS 2024; Narisetty, P. et al., *Adaptive Evaluation of Out-of-Band Defenses Against Prompt Injection in LLM Agents*, arXiv:2606.26479 (June 2026); OpenAI/Anthropic/DeepMind/ETH *The Attacker Moves Second* (2025).

---

## 1. Executive Summary & Design Premise

Prompt injection is fundamentally an architectural flaw resulting from the **von Neumann confusion of code and data** inside Large Language Models: instructions and untrusted content share a single unstructured token stream. In-band defenses (system prompt admonitions, adversarial fine-tuning, output classifiers) fail against adaptive adversaries (>90% attack success rate across 12 published in-band defenses under adaptive optimization; *The Attacker Moves Second*, 2025).

For OpenSpine, prompt-injection defense cannot be an LLM reasoning task. It must be an **out-of-band kernel invariant** enforced in Rust. 

The core thesis across CaMeL, FIDES, Progent, and RTBAS is that **security boundaries belong in the deterministic execution runtime, not the model's weights**. This research deconstructs these four systems into actionable design inputs for OpenSpine's three planned immune-system mechanisms:
1. Capability-derived tool catalogs (pre-inference attenuation);
2. Disclosure gating on external egress (runtime gate mediation);
3. Provenance labels with typed identity (hybrid compile-time typing + runtime ledger tracking).

---

## 2. Deliverable 1: Static-vs-Runtime Enforcement Split for a Rust Kernel

### 2.1 System-by-System Deconstruction

| System | Pre-Inference / Static Checks | Runtime / Dynamic Execution Checks |
| :--- | :--- | :--- |
| **CaMeL** (DeepMind) | **Static Plan Generation & AST Parsing**: Privileged LLM translates trusted user goals into a restricted Python AST. Static schema definitions (`output_schema = PydanticModel`) constrain the Quarantined LLM's return types. Tools are declared as callable Python functions with typed signatures. | **Custom AST Interpreter & Capability Propagation**: Interpreter traverses AST node-by-node, tracking capability tags and data provenance on every variable. Evaluates security policies at tool-call dispatch (e.g. `send_email(recipient=address)` asserts `address` holds a `TrustedRecipient` capability). Gated approval fallback on policy violations. |
| **FIDES** (Microsoft) | **Static Policy & Lattice Declarations**: Declarative definition of information-flow lattice: Confidentiality ($C$) and Integrity ($I$) labels on principals, data sources, and tools. Tool interfaces statically declare clearance requirements ($C_{req}$) and integrity preconditions ($I_{req}$). | **Dynamic Information Flow Monitor (DIFC)**: Attaches runtime labels $L=(C,I)$ to context variables. Propagates taint across tool outputs and LLM steps. Checks clearance/integrity before every tool invocation. Enforces *selective hiding* (replacing secrets with opaque handles `Ref<Id>`) and *constrained inspection* (schema projection) dynamically. |
| **Progent** | **Tool Permission Matrix & Policy Compilation**: Static specification of tool privileges per agent role via a Domain-Specific Policy Language (DSL). Base whitelist of reachable tools is compiled prior to execution. | **Programmable Reference Monitor & Fallback Execution**: Interposes on every tool invocation $a_t = (\text{tool}, \text{args})$. Evaluates dynamic state variables, argument constraints (e.g. `recipient in user.contacts`), and state-machine transitions. On policy violation, blocks execution or executes a pre-defined fallback action $a_{fallback}$ returning an observation string $o_t \in \mathcal{O}$. |
| **RTBAS** (CMU) | **Source/Sink Classification**: Static categorization of tools into Taint Sources (e.g. web search, inbox read) and Taint Sinks (e.g. file write, email send, shell execution). | **Message-Level Labeling & Dual Dependency Screeners**: Intercepts tool return values. Attaches $(\ell_I, \ell_C)$ labels to context messages $m$. Dual dependency screeners (LM-as-Judge and attention saliency) evaluate if low-integrity context influenced sink arguments. Enforces redacted history $M^\diamond$ before sink calls. |

---

### 2.2 OpenSpine Architectural Placement

OpenSpine's Rust kernel cleanly divides into **Pre-Inference Projection** and **Runtime Gate Mediation**:

```
 ┌─────────────────────────────────────────────────────────────────────────┐
 │                       PRE-INFERENCE (Static / Setup)                    │
 │                                                                         │
 │  1. TaskGrant Resolution (openspine-authority)                          │
 │     └─ Policy + Route + Identity -> Derived ToolCatalog                 │
 │  2. Schema/Type Definition (openspine-schemas)                          │
 │     └─ UntrustedContext<T>, SelectionToken, Bounded Schemas             │
 └────────────────────────────────────┬────────────────────────────────────┘
                                      │ Filtered Tool Catalog + Prompt
                                      ▼
 ┌─────────────────────────────────────────────────────────────────────────┐
 │                       WORKER / ORCHESTRATION LAYER                      │
 │  Worker LLM / Orchestrator (untrusted reasoning)                        │
 │  Quarantined Extraction (bounded schema decoding)                       │
 └────────────────────────────────────┬────────────────────────────────────┘
                                      │ Proposed Effect / Gated Action
                                      ▼
 ┌─────────────────────────────────────────────────────────────────────────┐
 │                       RUNTIME EXECUTION (Kernel Gate)                   │
 │                                                                         │
 │  1. Disclosure Gating & Sink Mediation (openspine-gate)                 │
 │     └─ gate(ActionRequest) -> Pre-effect Policy + Recipient Binding     │
 │  2. Provenance Ledger & Audit (openspine-kernel)                        │
 │     └─ AuditMeta + Digest verification + SelectionToken consumption     │
 └─────────────────────────────────────────────────────────────────────────┘
```

#### 1. Capability-Derived Tool Catalogs: **Pre-Inference (Per-Turn / Per-Session Projection)**
- **Where it belongs**: Pre-inference catalog projection in `openspine-authority` and `openspine-shell`.
- **How it works**:
  - The worker LLM is never presented with a global catalog of all system tools.
  - The kernel resolves the active `TaskGrant` (derived by deterministic intersection of route, identity, workflow, pack, and policy; PRD §8, CLAIM-10).
  - From this `TaskGrant`, the kernel projects an attenuated JSON Schema / tool definition list sent to the worker.
  - If a tool is not in the grant (e.g., `host.filesystem` for a customer-facing `Bell` conversation), its tool definition does not exist in the prompt. The worker model cannot even hallucinate or be tricked into constructing a call to a non-existent tool.
  - *Theoretical basis*: FORGE minimal permission grants + Biba integrity isolation.

#### 2. Disclosure Gating on External Egress: **Runtime (Kernel Gate & Sink Mediation)**
- **Where it belongs**: Runtime mediation inside `openspine-gate` (`gate()` function; CLAIM-13, CLAIM-17, CLAIM-20).
- **How it works**:
  - Even within an allowed tool (e.g. `email.draft_reply` or `webhook.send`), the kernel intercepts the action before execution.
  - Runtime checks enforce:
    1. **Target Binding**: The recipient/destination must match the bound counterparty identity or user-selected target, not an arbitrary address extracted from untrusted body text (D-048, CLAIM-04, CLAIM-18).
    2. **Egress Policy Check**: Explicit denial of high-risk actions (e.g. autonomous final send is denied regardless of grant; CLAIM-17).
    3. **Approval Gating**: Transitions to `ApprovalRequired` if parameters touch unverified external channels or widening is requested (CLAIM-12, CLAIM-15).
  - *Theoretical basis*: Progent reference monitor + FIDES sink clearance checks.

#### 3. Provenance Labels with Typed Identity: **Hybrid (Static Typing + Runtime Ledger)**
- **Where it belongs**: 
  - **Static Type Invariant**: `openspine-schemas` (Rust type system enforces wrapped untrusted containers and unforgeable token types).
  - **Runtime Provenance Tracking**: `openspine-kernel` Store/Audit ledger (tracking event IDs, artifact digests, and counterparty bindings).
- **How it works**:
  - *Static layer*: Rust's type system prevents raw untrusted string payloads from being coerced into trusted instructions. APIs require `UntrustedContext<T>` or `SelectionToken` rather than `String`.
  - *Runtime layer*: Every piece of external content enters the kernel wrapped in an `EventEnvelope` with an immutable `EventId`, `ChannelId`, and SHA-256 digest. When an action is proposed, the gate verifies that any referenced entities carry valid provenance chains linked to the current grant (CLAIM-08, CLAIM-24, CLAIM-26).
  - *Theoretical basis*: CaMeL capability tags + FIDES DIFC labels $(C, I)$ + Biba integrity model.

---

## 3. Deliverable 2: What the Quarantine Boundary Returns

### 3.1 Exact Types and Return Shapes in Reference Systems

```
┌────────────────────────────────────────────────────────────────────────┐
│ UNTRUSTED EXTERNAL DATA (Email body, Webpage, User Chat, Tool Output)  │
└───────────────────────────────────┬────────────────────────────────────┘
                                    │
                                    ▼
┌────────────────────────────────────────────────────────────────────────┐
│ QUARANTINE / MEDIATION BOUNDARY                                        │
└───────────────────────────────────┬────────────────────────────────────┘
                                    │
                                    │ Exact Return Shape Crosses Boundary
                                    ▼
┌────────────────────────────────────────────────────────────────────────┐
│ WHAT CROSSES BACK INTO PLANNING CONTEXT:                               │
│                                                                        │
│ CaMeL:    output_schema instance (Pydantic / typed scalar) OR           │
│           Tagged AST Variable Reference ($var_i)                       │
│                                                                        │
│ FIDES:    Opaque Reference Ref<Id> (Selective Hiding) OR               │
│           Projected Schema Value tagged with (C, I) label               │
│                                                                        │
│ Progent:  Observation string o ∈ O (success/error text) from tool/     │
│           fallback action a_fallback evaluated by Reference Monitor     │
│                                                                        │
│ RTBAS:    Labeled Message m^(li,lc) in Trajectory + Redacted History   │
│           M♢ + Screener Verdict (li,lc) from LM-Judge/Attention        │
└────────────────────────────────────────────────────────────────────────┘
```

#### 1. CaMeL (Google DeepMind, arXiv:2503.18813)
- **Quarantine Boundary Mechanism**: The Privileged LLM issues queries to the Quarantined LLM constrained by an explicit `output_schema` parameter:
  ```python
  address = query_quarantined_llm(
      "Find Bob's email address in [email]",
      output_schema=EmailStr  # or a Pydantic BaseModel subclass
  )
  ```
- **Exact Return Shape**:
  - The Quarantined LLM returns a structured JSON instance strictly deserializable into `output_schema` (e.g. `EmailStr`, `int`, `Date`, bounded `BaseModel`).
  - In the custom Python AST interpreter, untrusted variables are tracked as capability-tagged node values; the Privileged LLM refers to variables via variable identifiers (`address`, `$var_1`).
  - **Negative Guarantee**: No arbitrary natural language instructions or unconstrained executable Python strings can cross from the Quarantined LLM into the Privileged LLM's prompt.

#### 2. FIDES (Microsoft Research, arXiv:2505.23643)
- **Quarantine Boundary Mechanism**: *Selective Hiding* and *Constrained Inspection*.
- **Exact Return Shape**:
  - *Selective Hiding*: Untrusted or confidential tool outputs are stored in kernel-managed state and injected into the agent context as opaque object references:
    $$\text{Ref}\langle id \rangle \quad \text{with label } L = (C, I)$$
  - *Constrained Inspection*: When reasoning requires inspecting content, FIDES executes a constrained inspection query returning a typed projection (e.g., boolean flag, enum code) tagged with low integrity $I_{untrusted}$.
  - *Declassification / Endorsement*: Label changes require explicit policy rules; low-integrity data cannot be unpacked into high-integrity planning context without endorsement.

#### 3. Progent (Shi et al., arXiv:2504.11703)
- **Quarantine / Mediation Boundary Mechanism**: Programmable Reference Monitor mediating actions $a_t = (\text{tool\_name}, \text{args})$.
- **Exact Return Shape**:
  - Tool execution returns an observation string:
    $$o_t \in \mathcal{O}$$
  - When the agent proposes an action $a_t$ violating policy $\mathcal{P}$, the reference monitor intercepts it and either:
    1. Returns an error observation string $o_{err} \in \mathcal{O}$ rejecting the call;
    2. Rewrites the invocation to a fallback action $a_{fallback} \in \mathcal{A}$ with policy-vetted parameters (e.g. human confirmation prompt or localized query), returning the fallback observation $o_{fallback} \in \mathcal{O}$.
  - State tracking updates dynamic policy variables from observation strings according to domain rules.

#### 4. RTBAS (Zhong et al., arXiv:2502.08966)
- **Quarantine Boundary Mechanism**: Message-Level Taint Labeling and Redacted History.
- **Exact Return Shape**:
  - Tool outputs enter the agent's interaction trajectory as labeled messages:
    $$m_t^{(\ell_I, \ell_C)}, \quad \ell_I \in \{\text{High}, \text{Low}\}, \quad \ell_C \in \{\text{Public}, \text{Private}\}$$
  - Before invoking a sensitive sink, RTBAS passes the context through dependency screeners returning a screener verdict $(\ell_I, \ell_C)$ indicating whether low-integrity messages influenced sink arguments.
  - Generates a redacted history view $M^\diamond$ where unauthorized/private tokens are masked prior to dispatch.

---

### 3.2 Design Implications for Rust Types at OpenSpine's Gate

OpenSpine's kernel enforces that untrusted external content **never enters the planning instruction stream as raw strings**. The boundary must be reified in Rust types across `openspine-schemas` and `openspine-gate`:

```rust
/// Rust type design inputs for `openspine-schemas` and `openspine-gate`

/// 1. Untrusted context wrapper: Prevents raw string interpolation into system instructions.
/// Wire format serializes inside explicit, randomized delimiter envelopes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UntrustedContext<T> {
    pub source_event_id: EventId,
    pub source_channel: ChannelId,
    pub payload_digest: Sha256Digest,
    pub value: T,
}

/// 2. Opaque handle for external entities (email threads, attachments, CRM objects).
/// The worker/orchestrator receives only the token ID, never the raw address/URL.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SelectionToken {
    pub token_id: Uuid,
    pub grant_id: GrantId,
    pub target_digest: Sha256Digest,
    pub single_use: bool,
    pub expires_at: Timestamp,
}

/// 3. Bounded, typed output schemas for quarantined extraction tasks.
/// Extraction models decode into closed enums or bounded scalar fields.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExtractedIntent {
    ScheduleMeeting {
        time_slot: Iso8601DateTime,
        topic_enum: BoundedTopicCode,
    },
    AcknowledgeReceipt,
    EscalateToHuman {
        reason_code: EscalationReason,
    },
}

/// 4. Gate Action Request with typed parameter provenance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatedActionRequest {
    pub action_id: ActionId,
    pub grant_token: TaskToken,
    pub parameters: ActionParameters,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "param_type", rename_all = "snake_case")]
pub enum ActionParameters {
    /// Safe: Target proved via unforgeable selection token bound to current grant.
    TargetedEmailReply {
        selection_token: SelectionToken,
        draft_body: UntrustedContext<String>,
    },
    /// Safe: Strictly bounded enum value.
    UpdateTicketStatus {
        ticket_id: SelectionToken,
        new_status: TicketStatusEnum,
    },
}
```

#### Key Rules for OpenSpine Gate Types:
1. **Opaque References over Raw Pointers**: A worker managing an email reply never receives the raw `To: victim@example.com` string to pass into `send_email`. It receives a `SelectionToken` minted during the inbound event. The kernel resolves the token to the real recipient at execution time (CLAIM-08, CLAIM-23).
2. **Untrusted Envelope Serialization**: When `UntrustedContext<String>` is formatted for an LLM prompt (e.g. drafting an email reply), the template engine automatically wraps it in randomized boundary markers (`<untrusted_content id="...">...</untrusted_content>`), fulfilling CLAIM-04 and CLAIM-07.
3. **Bounded Decoding for Classification**: Workflow transitions must consume structured enums (`TicketStatusEnum`, `ExtractedIntent`) parsed by strict JSON-schema decoding, not free-form text.

---

## 4. Deliverable 3: The Branch-Steering Residual & Tradeoffs

### 4.1 What Remains Exploitable: The Mechanics of Branch Steering

Even if an agent cannot inject instructions into system prompts, cannot escalate privileges, and cannot forge parameter addresses, **untrusted data can still influence boolean and branching decisions** in the model's control flow:

```
                      [ Untrusted External Input ]
                                   │
                                   ▼
                   Does this text trigger a branch?
                                  / \
                                 /   \
                             YES/     \NO
                               /       \
                              ▼         ▼
                      [ Action A ]   [ Action B ]
```

When control flow branches on tainted data, an attacker can steer execution between validly granted actions:
1. **Action Suppression (Denial of Service)**: The attacker crafts content that tricks the model's classification branch into choosing a no-op or error path (e.g. "Do not reply; customer requested cancellation").
2. **In-Grant Action Forcing**: If a grant allows both `crm.lookup` and `support.escalate`, tainted input can force unnecessary escalation, consuming human operator bandwidth or financial credits.
3. **Semantic Content Shaping**: While the target recipient is locked by a `SelectionToken`, the text *inside* an email draft or ticket update is shaped by the attacker's prompt (e.g. social engineering the human recipient who reviews the draft).
4. **Implicit Flow Leakage (Side-Channel)**: If branching on a secret value produces observable external differences (e.g. execution timing, distinct error codes, or differing tool call counts), information flows from confidential context to external observers.

---

### 4.2 How Existing Systems Handle (or Punt on) Branch Steering

| System | Branch-Steering Strategy | What it Handles | What it Punts On / Limitations |
| :--- | :--- | :--- | :--- |
| **CaMeL** (DeepMind) | **Control-Flow Dependency Policy**: Tracks execution context taint. If an `if` condition evaluates an untrusted expression, the Program Counter (PC) becomes tainted. Sensitive tool calls inside that block require explicit policy clearance. | Prevents untrusted data from executing privileged actions by hiding inside conditional blocks. | Punts on termination/timing channels and semantic degradation of permitted branches. Policy configuration burden on developer. |
| **FIDES** (Microsoft) | **Conservative Context Propagation & Dual Policy ($P_T$ / $P_F$)**: Runtime conservatively propagates conversation/context labels to all LLM outputs. Policy $P_T$ checks the tool-call label for trusted context (enforcing integrity non-interference), while Policy $P_F$ checks argument confidentiality only. | $P_T$ prevents low-integrity untrusted inputs from steering execution to sensitive tools. | Explicitly accepts implicit-flow leakage under an "explicit secrecy" model: argument confidentiality is enforced, but control-flow branching on confidential data is permitted to avoid over-tainting. |
| **RTBAS** (CMU) | **Dual Dependency Screeners**: Uses LM-as-Judge and attention saliency detection to determine if a branch decision was caused by adversarial input. | Catches blatant prompt-steering diversions during benchmark tasks. | Probabilistic detection layer (LM Judge); susceptible to sophisticated adaptive phrasing that mimics legitimate task conditions. |
| **Progent** | **State-Machine Preconditions & Fallbacks**: Enforces rigid allowable state transitions. Actions must satisfy explicit preconditions regardless of LLM reasoning. | Restricts branching to valid domain state-machine edges. | Cannot determine whether the choice of valid edge was semantically justified or maliciously induced. |
---

### 4.3 The Honest Residual OpenSpine Must Accept or Design Around

OpenSpine must formally document and accept three residual risks while containing them:

```
┌─────────────────────────────────────────────────────────────────────────┐
│                    THE RESIDUAL THREAT LANDSCAPE                        │
├───────────────────────────────────┬─────────────────────────────────────┤
│ RESIDUAL RISK                     │ OPENSPINE MITIGATION / CONTAINMENT  │
├───────────────────────────────────┼─────────────────────────────────────┤
│ 1. In-Grant Action Selection      │ Declarative State Machines (AD-044) │
│    (Attacker steers choice among  │ Workflows live in kernel; agents    │
│    permitted grant actions)       │ only step valid state transitions.  │
├───────────────────────────────────┼─────────────────────────────────────┤
│ 2. Semantic Content Smuggling     │ Local Draft Preview & Approval UX   │
│    (Attacker shapes text inside   │ No final autonomous send (CLAIM-17, │
│    a validly targeted draft)      │ D-048); owner inspects payload.     │
├───────────────────────────────────┼─────────────────────────────────────┤
│ 3. Denial of Service / Drop       │ Dead-Letter Auditing & Timeout      │
│    (Attacker tricks model into    │ Unattended workhorse loops audit    │
│    dropping or skipping a task)   │ drops; task board logs AckSkip.     │
└───────────────────────────────────┴─────────────────────────────────────┘
```

1. **Workflows as Declarative State Machines (AD-044)**:
   - Control flow orchestration must not be free-form Python or unconstrained agent loops.
   - Workflows live in the kernel as deterministic state machines. The worker LLM only provides structured classification tokens to advance edges. If an edge is invalid in the current state, `gate()` rejects the transition.
2. **Draft Preview without Autonomous Egress (CLAIM-17, D-048)**:
   - The kernel accepts that an attacker might convince the model to draft an embarrassing or hostile response.
   - However, **final email send is denied by policy** (CLAIM-17). The drafted payload is staged for owner inspection with full diff and provenance audit (CLAIM-24). The blast radius is contained to a draft artifact.
3. **Bounded Selection Tokens (CLAIM-08, CLAIM-23)**:
   - An attacker cannot steer an email to a new address because the recipient parameter is bound to the verified incoming session token. The attacker can at most steer the content sent back to *themselves*.

---

### 4.4 Published Benchmark Metrics: Utility vs. Security Tradeoffs

| Defense System | Benchmark | Clean Utility (No Attack) | Attack Success Rate (Defended) | Baseline ASR (No Defense) | Notes & Source Citation |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **CaMeL** | AgentDojo | **77.0%** task success | **~0.0%** (Provable for defended policies; 67% secure completion on hard suite) | 84.0% utility / ~30–45% ASR | 7.0 percentage-point utility cost for deterministic capability protection. (arXiv:2503.18813) |
| **FIDES** | AgentDojo / MS Bench | **+16.0%** relative utility boost with reasoning models | **0.0%** policy-violating attacks | ~25–40% ASR | Information-flow decomposition actually aids reasoning models in task completion. (arXiv:2505.23643) |
| **Progent** | AgentDojo / ToolBench | **~98.0%** relative utility | **4.2%** (down from 25.8%); **2.6%** under adaptive attacks | 25.8%–41.2% ASR | 6x reduction in attack success rate with negligible utility loss. (Shi et al., arXiv:2504.11703; Narisetty et al. arXiv:2606.26479) |
| **RTBAS** | AgentDojo | **~98.0%** relative utility (2% loss) | **0.0%** targeted attacks | ~30–40% ASR | Evaluated with LM-as-Judge and attention dependency screeners. (Zhong et al., arXiv:2502.08966) |
| **MELON** | AgentDojo | **68.7%** utility | **0.32%** ASR | ~30–40% ASR | Masked re-execution monitoring (ICML 2025; arXiv:2502.05174). 2x API cost overhead. |
| **In-Band Defenses** (Prompt Guards / Align) | Adaptive Red-Team | Variable (~75–85%) | **>90.0%** (100% human red-team bypass) | 25–45% ASR | In-band defenses collapse entirely under adaptive, defense-aware optimization. (*The Attacker Moves Second*, 2025) |

---

## 5. Summary of Design Inputs for OpenSpine Implementation

1. **Pre-Inference (`openspine-authority`)**:
   - Statically derive and project an attenuated `ToolCatalog` from the active `TaskGrant` before prompt formatting. The model cannot see ungranted tools.
2. **Quarantine Types (`openspine-schemas`)**:
   - Replace raw string parameters with `UntrustedContext<T>` (wrapped with randomized delimiters on wire).
   - Use `SelectionToken` for all external entity references (emails, documents, thread IDs), binding target addresses to kernel-verified tokens.
   - Require bounded enum decoding for all model classification outputs that advance workflow state.
3. **Runtime Gate Mediation (`openspine-gate`)**:
   - `gate()` strictly validates `SelectionToken` ownership and grant lineage.
   - Enforce hard denial on autonomous egress (CLAIM-17) and route all draft modifications through immutable digest auditing (CLAIM-24).
4. **Branch Containment (`openspine-kernel`)**:
   - Enforce workflows as declarative state machines (AD-044), preventing arbitrary control-flow branching.
   - Accept the residual of within-grant semantic steering by requiring human approval / preview for all consequential effects.
