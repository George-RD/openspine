# Tasks: Implement nerve subscribers

## 1. Declaration schema (schemas crate)

- [x] Add versioned nerve declarations, five type/measure pairings, scope/tier predicates, bounded provenance, issuance id, and structured payloads.
- [x] Wire `pub mod nerve;` into `openspine-schemas/src/lib.rs`.
- [x] Add complete five-type declaration round-trip and pure scope/threshold/retirement tests.

## 2. Kernel store module

- [x] Add kernel-owned advisee limits plus registration, budget, decay, issuance, and reaction tables through `store/nerve.rs::ensure_schema`.
- [x] Create `openspine-kernel/src/store/nerve.rs` with authoritative registration, scope→filter validation, atomic windowed admission/retirement predicate, issuance authentication, idempotent reaction decay, and opaque class digests.
- [x] Wire `pub(crate) mod nerve;` into `openspine-kernel/src/store/mod.rs`; bind each namespaced nerve consumer to a fresh exact event-bus checkpoint/filter transactionally.
- [x] Add production replay dispatch over each registered nerve's exact persisted filter; typed handlers run before checkpoint advancement.
- [x] Seed kernel-owned advisee limits from active manifests as a full snapshot, subtracting overlapping denied classes conservatively.
- [x] Add AD-034 owner-control ingestion screening with atomic `event.received` + structured `manipulation_signal.detected` append and a typed screener handler.
- [x] Revalidate current limits at dispatch time, revoke stale registration state, and durably enqueue gate-visible delivery metadata in the admission transaction.

## 3. Interjection admission

- [x] Implement store-backed admission: validate payload/provenance/type pairing and threshold, atomically reset/debit only positive non-retired budgets, issue an id, then return the interjection.
- [x] Ensure advisor objections carry concern class + cited clause + bounded suggested rewrite + provenance + forced gate visibility, with no answer field.

## 4. Tests

- [x] Wider-scope nerve is unregistrable (registration returns `ScopeExceedsAdvisee`).
- [x] Valid scope/filter registers; kernel-owned tier exceeds-advisee is rejected; persisted filter/checkpoint binding is verified.
- [x] Advisor interjection carries structured objection fields (concern class + cited clause), store-issued id, opaque class, and no answer/rewrite.
- [x] Threshold/provenance/payload mismatch does not admit; budget exhaustion and zero budget do not admit.
- [x] Five distinct ignored store-issued reactions retire a class; engaged/annoyed counters persist; duplicate reactions are idempotent.
- [x] Advisor hints are always `gate_visible` structured messages, including when caller requests ambient delivery.
- [x] Cross-connection concurrent admission spends exactly one unit; rollover is exercised.
- [x] Registered replay exposes declaration type and event to the typed handler while preserving idempotent checkpoints.

## 5. Verification
- [x] `cargo fmt --all --check` clean across changed crates.
- [x] `cargo clippy --workspace --all-targets -- -D warnings` clean.
- [x] `cargo test --workspace` green (493 tests).
- [x] `bash scripts/check-file-sizes.sh` green (all `*.rs` <= 500 lines).
- [x] `/Users/george/repos/openspine/node_modules/.bin/openspec validate implement-nerve-subscribers --strict` green.
