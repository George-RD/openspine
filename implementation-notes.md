# Implementation notes — #221 Record D-174 (provenance labels bound to typed identity)

Docs-only canon entry. Appended `# D-174` to `.raw/openspine-decision-log.md`, between D-173's closing `---` and the `## Change Log` heading.

## Deviations

- **Change Log / Decision Index rows not added.** The file's convention is that every D-entry also gets a `## Change Log` row (and D-entries have an index table). The ticket AC says "No other content in the file is modified" and "touch nothing else in the file", so I added the body entry only. Revisit by adding the two rows if the owner wants full-convention compliance; reversible.

## Discovered edge cases

- **Ticket paraphrase vs spec #220 verbatim decision differ.** Ticket #221 names the decision as "provenance labels bound to typed identity … kernel-minted, immutable, append-only, fail-closed, no LLM judgment". Spec #220's quoted landed decision is "provenance and visibility labels bound to typed identity, consulted deterministically at context assembly and egress; kernel-minted, immutable, fail-closed, no LLM judgment" — note "and visibility labels" and no "append-only". AC requires the Decision to quote **spec #220 verbatim**, so the `## Decision` blockquote uses the spec's exact string; "append-only" is carried in `## Consequences` (grounded in #220 user story 6 + implementation decisions, AD-140 lineage discipline).
- Next free D-number is **D-174** (highest existing heading was `# D-173`; D-052 index row is a known pre-existing gap, not a free slot — not reused).

## Questions for review

- Confirm the "body entry only, no Change Log/Index row" reading of the AC is intended. If not, the two additive rows are a trivial follow-up.

---

Deviations: 1 (Change Log/Index rows omitted per literal AC).
Most likely to be revisited: whether to add the Change Log + Decision Index rows for convention parity.
Edge cases found: 2 (ticket-vs-spec verbatim wording divergence; D-052 index gap is not a free number).
Next session should read first: this file's "Discovered edge cases", then the D-174 entry in `.raw/openspine-decision-log.md`.
Gate: `./scripts/check.sh` passes (docs-only change).
