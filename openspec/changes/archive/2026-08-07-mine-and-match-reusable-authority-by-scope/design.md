# Design: Mine and match reusable authority by scope

## `ResolvedActionContext` is reused, not rebuilt

`crates/openspine-schemas/src/resolved_context.rs` already defines the sealed context class this change needs: `ResolvedActionContext` (28 fields, `#[serde(deny_unknown_fields)]`), its `ResolvedActionContextInput`, and its typed `ResolvedActionContextError`. It is exported at `crates/openspine-schemas/src/lib.rs:41` and covered by `crates/openspine-schemas/tests/responsibility_contract.rs`.

It has **zero kernel callers**. `ResolvedActionContext` appears only in its own module and two schema test files. The brief's "construct resolved context at the kernel boundary" therefore means *wiring a finished type into the kernel*, not designing a new one. No parallel context type is introduced, and no field is added to carry connector-specific data.

`try_new` calls `catalog.validated_delegation_contract(action_id, implementation_id)`, which returned `MissingImplementationDescriptor` for every action until #127 registered `gmail.draft.v1`. `email.create_draft` is consequently the first — and currently the only — action for which construction can succeed. The proof is scoped to it deliberately: a second action would need its own descriptor and executor, which is a catalog change, not a matching change.

## Two digests, because one of them is the wrong key

`compatibility_digest()` (`resolved_context.rs:144-159`) is computed over declaration axes only:

`action_id` · `descriptor_version` · `delegation_policy_version` · `implementation_id` · `implementation_version` · `connector_kind` · `executor_id` · `executor_version` · `resolver_id` · `resolver_version` · `effect_destination` · `required_scope_dimensions` · `egress_class` · `output_channels`

It deliberately excludes every **instance** axis: `connector_instance_id`, `account_identity_digest`, `target_refs`, `counterparty_identity_id`, `workflow_id`, `task_shape_digest`.

That exclusion is correct for its purpose and disqualifying for the other. Using `compatibility_digest` as the scope key would make two different Gmail accounts — same descriptor, same executor, same resolver — produce the *same* digest and collide into one pattern. The change's own invariant forbids exactly that. So:

| Digest | Computed over | Answers |
| --- | --- | --- |
| `compatibility_digest()` (exists) | declaration axes | "has the machinery under this responsibility changed?" — the **drift epoch** |
| `reviewed_scope_digest()` (new) | the values named by `required_scope_dimensions` | "is this the same reviewed instance?" — the **scope key** |

`reviewed_scope_digest()` is sealed the same way as its sibling: `digest_of(&serde_json::json!({…}))` with field order matching the dimension order in `validate_required_dimensions` (`resolved_context.rs:195-232`), so two contexts that agree on every required dimension produce byte-identical pre-images. A standing rule stores **both**.

### Reviewed values are stored, not only their digest

The rule persists the required dimension set, the individual reviewed value per dimension, and the derived digest. Storing only the digest would be smaller and wrong for two reasons:

1. `responsibility-contract` already requires that comparison "MUST return the exact changed dimensions". An opaque digest can only answer same/different.
2. Narrowing is a first-class owner intent in the sibling review change. Narrowing one dimension must not force the owner to re-review the rest, which requires per-dimension comparability.

The digest is the fast-path match key; the values are the evidence and the mismatch explanation. They are kept consistent by deriving the digest from the stored values, never by storing them independently — a stored digest that disagrees with its stored values is the corrupt-binding case the canonical spec already fails closed on.

## Matching: exactly one, before any budget moves

Today's lookup is `active_standing_rule_for_action(&ActionId, now)` — action-keyed and single-valued, so "which rule?" has never been a question. It becomes one.

```
resolve context  →  candidate rules active for context.action_id at `now`
                 →  retain those whose compatibility epoch AND reviewed scope both match
                 →  0 → ordinary owner approval
                    1 → admit; only now reserve quota+rate / schedule dark window
                    2+ → FAIL CLOSED; reserve nothing, schedule nothing
```

Three properties of that ordering are load-bearing:

- **Ambiguity is refusal, not arbitration.** Two rules matching one context means the owner approved two overlapping responsibilities and the kernel cannot know which budget the effect should spend. Picking the narrowest, the newest, or the first is a policy the owner never reviewed. Falling back to ordinary approval is the only answer that consumes no authority.
- **Matching precedes reservation.** Selection is pure and side-effect-free; nothing is written until exactly one rule is chosen. A mismatch or an overlap therefore cannot leave a reserved row, and cannot mint a dark-window pending exception either — which matters because #131 will cap those per scoped rule.
- **Selection stays inside the existing atomic boundary.** Quota and rate are reserved in one `BEGIN IMMEDIATE` (D-050); the rule identity chosen by matching is bound to that same transaction, so a concurrent activation cannot swap the rule between selection and reservation. This is the same TOCTOU class `consult_and_reserve` already closes for the action key, extended to the scope key.

Budgets remain per rule. Two disjoint rules on `email.create_draft` hold two independent quota and rate windows; there is no aggregate per-action counter to pool them.

## Drift restores approval before the effect, never after

Every axis the brief names is already covered by one of the two digests:

| Drift | Detected by |
| --- | --- |
| descriptor, executor, resolver, policy version | `compatibility_digest` |
| connector instance, account identity, counterparty, target, workflow, task shape | `reviewed_scope_digest` |

A rule whose stored epoch or scope no longer equals the freshly resolved context stops matching. Because matching runs before reservation and before effect, the fallback to ordinary approval happens **pre-effect** by construction — there is no window in which a drifted rule admits an effect and the drift is noticed afterwards. The kernel never remaps a rule onto a successor connector or account; an unresolvable connector/account is a construction failure, not a substitution.

## The third executor caller

#127 delivered two admission sources holding a digest-bound `ActionRequest`: per-instance approval (`handle_create_approved_draft`) and the D-117 headless approved lane (`handle_headless_approved`). Both call the shared `gmail.create_draft` executor. The generic shell dispatch path was left failing closed because it receives `payload: Option<&serde_json::Value>` and cannot supply the `payload_ref`/`target_ref`/`target_digest` the executor re-derives against (D-153).

Scope-matched admission becomes the third caller, and it is the *right* third caller precisely because it does not reconstruct anything from shell input: the context it hands over is the one the kernel resolved and the owner reviewed. The executor is unchanged; its re-derivations stay load-bearing and are re-run against kernel-owned values, exactly as for the other two sources.

### `EffectOutcome` → reservation

| Outcome | Reservation | Why |
| --- | --- | --- |
| `Executed` | finalize | the effect happened; the budget was spent |
| `DeliveryUnknown` | retain (stays `reserved`), fence stays open | releasing budget for a write that may have landed under-counts real effects |
| `RefusedPreEffect` | cancel | proven pre-effect; #127 semantics unchanged |
| `FailedAfterAttempt` | cancel | no confirmed effect; #127 semantics unchanged |
| `NoExecutor` (dispatch) | cancel | #127 semantics unchanged |

`DeliveryUnknown` retaining the reservation is the deliberately conservative direction: it may over-count a write that never landed, and it can never under-count one that did. The existing fired-token rule is unchanged — a one-use token is re-armed only after its cancellation *succeeds*.

**What "retain" means precisely.** The usage rows stay `reserved`: neither cancelled nor finalized. They keep counting against quota and rate, because every headroom query counts `status IN ('reserved', 'committed')`, and the rule's lapse clock and AD-010 drift trigger are still advanced so a responsibility that keeps saturating through ambiguous outcomes is still surfaced for owner re-review. Nothing then settles the row automatically — it ages out of its trailing window like any other spent unit. There is deliberately **no** fence reconciler today: `resolve_pending_draft_write` flips only the fence row and never touches `standing_rule_usage`. Leaving the reservation `reserved` rather than `committed` is what keeps a future reconciler *able* to release it, since a cancel can only delete a row that is still `reserved`. That reconciler is follow-up work, not current behaviour.

### Known limitation: silent thread participants

The reviewed scope binds the thread's participant set through `BoundParameters` and the drafted recipient through `TargetDigest`, both kernel-resolved from the read-only thread fetch. That set is derived from the `From` header of each message in the thread, because `GmailMessage` carries only `from` — `parse_thread` never reads `To` or `Cc`. A counterparty who is CC'd onto a reviewed thread but never posts is therefore invisible to the kernel and cannot enter the reviewed scope, so the rule keeps matching.

This is acceptable to land because of what a scope-matched admission can actually do: `email.send` is in `denied_actions` (`artifacts/lyra/policies/global.yaml`), so the only admitted effect is `email.create_draft` — an unsent draft in the owner's own mailbox. A silent CC receives nothing; the owner still sees and sends the draft. Closing the gap means extracting `To`/`Cc` in the Gmail thread parser, which is D-042 parser territory and a separate change.

## Authority, containment, audit, failure modes

- **Authority.** A scoped rule is still a composition *input*, never a live authority object (D-007). Every admitted task still mints a fresh task grant and crosses `gate()`. The rule narrows when approval is required; it never widens what the grant permits.
- **Containment.** Every value in the reviewed scope is kernel-resolved. The shell supplies an intent and cannot supply, widen, or select a scope dimension — a shell-chosen scope would be self-granted authority.
- **Audit.** The admitting rule id, rule version, and both digests are recorded with the admission, so an auditor can reconstruct which reviewed responsibility spent which budget. An ambiguous-overlap refusal is itself durable evidence: it is an owner-actionable signal that two approved responsibilities collide.
- **Failure modes.** Construction failure (missing descriptor, unresolvable connector/account, missing required dimension, unbound counterparty) fails closed before review and before admission. Match ambiguity fails closed. Drift fails closed. A corrupt persisted binding — stored values disagreeing with the stored digest — fails closed as an invalid scope rather than matching on either half.
- **Prompt injection.** Nothing in the reviewed scope originates in model output or connector content. Injected text can influence the *intent*, which still faces ordinary approval unless the kernel's own resolution lands inside a reviewed scope; it cannot influence the scope itself.

## Rejected alternatives

| Option | Why rejected |
| --- | --- |
| Match on `compatibility_digest` alone | Excludes every instance axis, so two different accounts collide into one pattern — a direct violation of the change's invariant. |
| Store only `reviewed_scope_digest`, no values | Cannot report the exact changed dimensions (already required) and makes single-dimension narrowing impossible without full re-review. |
| Resolve match ambiguity by a rule (narrowest / newest / first) | Picks a budget the owner never reviewed. Any tie-break is unreviewed policy; refusal is the only fail-closed answer. |
| Add a Gmail-specific scope type | Couples the autonomy model to one connector and duplicates matching per protocol. D-146 chose generic dimensions precisely to avoid this. |
| Aggregate one budget per `ActionId` across rules | Pools budget between distinct responsibilities — explicitly forbidden by the invariant. |
| Release the reservation on `DeliveryUnknown` | Under-counts effects that actually landed. Retaining over-counts at worst, which is the safe direction. |
| Reconstruct digest-bound context inside the generic shell dispatcher | Re-creates the class of bug #127 removed: the executor's re-derivations are only meaningful against a kernel-resolved context, not a shell-handed one. |
