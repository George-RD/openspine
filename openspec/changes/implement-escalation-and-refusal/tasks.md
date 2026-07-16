# Tasks: implement-escalation-and-refusal

## 1. OpenSpec artifacts

- [x] 1.1 Write proposal.md
- [x] 1.2 Write design.md (integrated chokepoint; separate worker/owner channels; reusable generic router)
- [x] 1.3 Write specs/escalation-and-refusal/spec.md (ADDED Requirements)
- [x] 1.4 Write specs/core-runtime-schemas/spec.md (ADDED thread_id Requirements and MAC integrity)
- [x] 1.5 Validate with `openspec validate implement-escalation-and-refusal --strict`

## 2. Schema types (openspine-schemas)

- [x] 2.1 Create `src/escalation.rs`: canonical deferral, typed gate notice, tagged producer payload, generic escalation event, `surface_denial()`
- [x] 2.2 Register `pub mod escalation;` in `lib.rs`
- [x] 2.3 Add serde-default `thread_id: Option<String>` to `EventEnvelope`
- [x] 2.4 Add serde-default `thread_id: Option<String>` to `TaskGrant`
- [x] 2.5 Include TaskGrant thread binding in RootAuthority MAC commitment
- [x] 2.6 Unit tests: every DenialReason, no-leak deferral, generic payload, thread_id serde and MAC mutation

## 3. Construction site updates

- [x] 3.1 Add `thread_id: None` to every `TaskGrant { }` literal
- [x] 3.2 Add `thread_id: None` to every `EventEnvelope { }` literal

## 4. Kernel integration (openspine-kernel)

- [x] 4.1 Create `src/escalation.rs`: generic `route_escalation`, deterministic owner message, dormant binding resolver
- [x] 4.2 Register `mod escalation;` in `main.rs`
- [x] 4.3 Keep counterparty classification kernel-owned in `ActionCatalog` (email.send only; unknown fails closed)
- [x] 4.4 Wire `POST /v1/actions` denial branch: classify, adapt to typed event, route/deliver, audit, return canonical deferral
- [x] 4.5 Extend `ActionResponseBody` with optional `counterparty_deferral`
- [x] 4.6 End-to-end tests: counterparty denial → exact canonical response + owner delivery + typed audit; non-counterparty denial → enum only; no policy sentinel leak; dormant binding

## 5. Local gate

- [x] 5.1 `cargo fmt && cargo fmt --check`
- [x] 5.2 `cargo clippy --workspace --all-targets -- -D warnings`
- [x] 5.3 `cargo test --workspace`
- [x] 5.4 `bash scripts/check-file-sizes.sh`
- [x] 5.5 `openspec validate implement-escalation-and-refusal --strict`

- [x] 6.1 Write IMPLEMENTATION-NOTES.md with exact gate results and deviations

