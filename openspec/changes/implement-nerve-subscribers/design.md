# Design: Implement nerve subscribers

## Approach

Nerves are declared, typed event-bus subscribers (AD-130), not ad hoc sidecars. A declaration binds the existing `EventSubscriptionFilter`, a measure, speak threshold, hard budget/window, model tier, and data scope no broader than the advisee. The kernel store owns registration, durable budget admission, and reaction decay; interjection payloads are returned only after a successful atomic budget debit.

### 1. Declaration schema — `openspine-schemas/src/nerve.rs`

The schemas crate remains pure data + pure checks (no I/O).

- `NerveType`: `Advisor`, `Injector`, `Screener`, `Miner`, `MetaCognition`.
- `ModelTier`: ordered `Cheap < Standard < Strong`; registration rejects a tier above the authoritative advisee maximum.
- `NerveMeasure`: `Legibility`, `SkillMatch`, `ManipulationTag`, `SystemicPattern`, `SecondOrderHealth`.
- `NerveScope`: open-vocabulary `data_classes` and `data_scopes`; `contains` enforces set containment so the advisee is the superset.
- `SpeakThreshold`: severity floor plus confidence floor in `[0,1]`; `validate` rejects impossible confidence values.
- `NerveBudget`: `window_kind`, `window_seconds`, and `suggestions_max`. The window start is persisted as epoch nanoseconds and resets usage when the duration elapses.
- `NerveDeclaration`: ULID, advisee id/max tier, existing event-bus filter, measure, threshold, budget, model tier, and scope. `is_scope_within` and `is_tier_within` are pure predicates used by registration.
- `InterjectionProvenance`: required pattern and source references.
- `AdvisorObjection`: concern class, cited clause, suggested rewrite. There is deliberately no answer field (AD-112).
- `ScreenerTag`: manipulation class and tagged aggregate (AD-034).
- `NerveInterjection`: nerve/advisee identity, type, severity, provenance, `gate_visible`, and optional type-specific payload. The kernel forces all advisor interjections gate-visible, so callers cannot route them through ambient context.
- `NerveDeclaration::evaluate_admission`: pure retirement + threshold check. It returns no interjection and spends no budget.

### 2. Kernel store — `openspine-kernel/src/store/nerve.rs`

The new module creates these tables with `CREATE TABLE IF NOT EXISTS` in `ensure_schema`:

```sql
CREATE TABLE IF NOT EXISTS nerve_registrations (
    nerve_id TEXT PRIMARY KEY,
    advisee_id TEXT NOT NULL,
    declaration_json TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS nerve_interjection_budgets (
    nerve_id TEXT NOT NULL,
    window_kind TEXT NOT NULL,
    window_started_ns INTEGER NOT NULL,
    used INTEGER NOT NULL DEFAULT 0,
    max INTEGER NOT NULL,
    PRIMARY KEY (nerve_id, window_kind)
);
CREATE TABLE IF NOT EXISTS nerve_decay (
    nerve_id TEXT NOT NULL,
    class TEXT NOT NULL,
    ignored_count INTEGER NOT NULL DEFAULT 0,
    retired INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (nerve_id, class)
);
```

`register_nerve(declaration, advisee_scope, advisee_max_tier)` validates scope, tier, threshold, and window, then atomically inserts declaration, budget, and an initial event-bus checkpoint bound to the declaration's exact filter. A wider-scope nerve is therefore unregistrable with no registration row.

`load_nerve` reads the canonical declaration. `record_reaction` stores only class counters: the fifth `Ignored` reaction sets `retired`; `Engaged` and `Annoyed` do not increase the ignored count. `class_retired` reads that durable flag.

### 3. End-to-end admission

`admit_interjection` is the only store API that returns an emit-ready interjection. It:

1. Validates required provenance and type-specific advisor/screener payload.
2. Loads the canonical registered declaration.
3. Checks durable class retirement and calls pure `evaluate_admission`.
4. Resets a matured budget window, then executes one conditional `UPDATE ... SET used = used + 1 WHERE used < max`.
5. Constructs and returns `NerveInterjection` only after the update changes one row.

Thus a rejected threshold, retired class, or exhausted budget returns no interjection and consumes no unit. Advisor output is always structured and gate-visible; cross-scope hints never become ambient context.

### 4. Event-bus and storage discipline

The declaration's `subscription_filter` is exactly the archived `EventSubscriptionFilter`. Registration transactionally binds the nerve ULID as a consumer checkpoint using the event-bus helper; no second events table or broker exists. Checkpoints advance later through `IdempotentConsumer` replay.

Registration JSON contains only policy/declaration metadata. Decay rows contain only class and counters. Interjection text and private payloads are not stored in SQLite audit or nerve tables (D-012); provenance sources are references, not captured message bodies.

### 5. File layout

| File | Role |
|------|------|
| `openspine-schemas/src/nerve.rs` | declaration, scope, threshold, payload, pure checks |
| `openspine-schemas/src/lib.rs` | public module registration |
| `openspine-kernel/src/store/nerve.rs` | durable registration, bus binding, admission, budget, decay |
| `openspine-kernel/src/store/nerve_tests.rs` | scope, structure, budget, checkpoint, decay tests |
| `openspine-kernel/src/store/event_bus.rs` | transactional consumer/filter binding helper |
| `openspine-kernel/src/store/mod.rs` | schema initialization + module wiring |

No changes to `gate()`, grant/caveat chains, the action catalog, D-055 effect-path characterization, or D-068 failure taxonomy.

## Key decisions

- Closed enums make type, measure, tier, and severity changes deliberate and reviewed.
- Scope containment is checked against authoritative advisee scope at registration, preventing an advisor covert channel (AD-051).
- Admission combines pure deterministic filters with a single atomic debit; budget spend is never a caller convention.
- Budget windows persist start time and reset after their declared duration, avoiding accidental lifetime caps.
- Decay bookkeeping is durable; ignored classes retire at five ignores (AD-052).
- No interjection body is persisted in SQLite; the audit/D-012 boundary remains plaintext-free.

## Alternatives considered

- A free-string scope or ambient cross-scope hint was rejected because containment and gate visibility would not be enforceable.
- A read-then-compare budget was rejected because concurrent callers could double-spend.
- A second event store/live broker was rejected because AD-105 defines the audit ledger plus typed replay as the bus.

## Authority sensitivity

This is authority-sensitive containment and audit-adjacent work. Tests cover scope rejection, tier rejection, event-bus checkpoint/filter binding, threshold and provenance rejection, atomic budget exhaustion, forced advisor gate visibility, and durable ignored-class retirement.
