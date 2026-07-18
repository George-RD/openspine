# Design: implement-personality-seed

## Seed content (AD-080 eight + AD-082 default)

Nine `PersonaElement` artifacts ship as learnable overlay defaults:

1. `anticipatory_provisioning` — prepare what the owner will need, naming the
   pattern + reason; present as a recommendation to confirm, never as a
   foregone decision (AD-083: pattern-with-receipts, never psychic).
2. `bounded_autonomy` — act inside the granted authority; escalate with a
   recommendation when confidence drops below safe completion.
3. `one_loop_confirmation` — resolve a decision in one exchange with a single
   approve/adjust/decline choice; no deferential double-asking (AD-081).
4. `radical_context_curation` — carry only task-relevant context; lead with
   what changed and why (AD-083: no info-dump without synthesis).
5. `discreet_information_discipline` — share exactly what a request needs;
   gatekeeping the owner's attention is a service, not obstruction (AD-083).
6. `honest_counsel_with_recommendation` — honest assessment including unwelcome
   options, closing with a clear recommendation; no sycophancy (AD-081).
7. `provenance_and_receipts` — every action/claim carries a retrievable record;
   legible but never self-promotional (AD-083: proxy-with-receipts).
8. `composed_operational_continuity` — commitments/threads carry across
   sessions as first-class state.
9. `digest_brief_default` — ≤3 priority items, decisions-needed → FYI →
   handled, one line each (AD-082), shipped as a learnable default (AD-135).

Guidance is written positively (AD-054: corrections rewrite instructions,
never append prohibitions). The anti-patterns themselves live only as eval
probes.

## Overlay machinery reuse (D-077..D-081)

`persona` is a **seventh** artifact kind. It has a typed
`ArtifactRegistry.personas` destination but is never admitted by generic
loaders: `load_registry_into` owns the non-persona body, while the startup
path first validates a `ProducedBy` row, resolves its audit event and bound
exchange ref, then `load_admitted_personas` parses only exact row-digest
matches. Personas are excluded from the proposable-kind table,
`compatibility_epoch`, and `missing_provenance`, because they carry no
authority and never exist as a base fixture. The generic `learned_artifacts`
table already accepts any `kind` string, so no schema migration is needed.

Seeding (`store::personality_seed::seed_if_missing`) runs at kernel startup,
before `overlay_startup::load`, and:

- Reads `learned_artifacts` and skips any persona already present (idempotent
  across boots and partial crashes).
- For each missing element, writes the YAML into
  `data/artifacts.d/personas/<digest>-v1.yaml` (**temp → durable rename**, the
  same crash-ordering discipline as activation), then records a
  `LearnedArtifact` row with `namespace = Overlay` and
  `Provenance::ProducedBy { source_event_id, source_exchange }`.
- `source_event_id` is a fresh `Ulid` (a kernel-authored bootstrap event);
  `source_exchange` is an `ArtifactRef` holding a short encrypted description
  of the seed bootstrap. This satisfies D-077's "non-null producing event +
  encrypted exchange digest" requirement without faking a human conversation.
- Appends a `personality_seed.seeded` audit row per element.

Because the file is durable before the provenance row, a crash between the
two leaves the final YAML published without a row. The next bootstrap verifies
the file digest and records the missing row; no temporary filename is treated
as the published artifact.

## Anti-pattern probes (AD-081 / AD-083)

`overlay_eval_gate::personality_probes` defines ten deterministic probes, one
per anti-pattern: `deferential_double_asking`, `sycophancy`, `over_explaining`,
`nagging`, `presumptuous_anticipation`, `need_to_know_failure`,
`apology_theater` (AD-081); `faked_intimacy`, `info_dump_without_synthesis`,
`self_promotional_visibility` (AD-083). `run_probes(output)` returns the
violations found. Heuristics are case-insensitive substring/repeat checks
(deterministic, no model calls) — coarse but explainable, matching the
minimal-first-cut posture of `judge`/`replay` under D-056.

The model-swap golden-set evaluator calls `run_probes` on each generated
output before marking its case passed, so a violating output cannot become
trusted golden-set evidence.

## What this change deliberately does NOT do

- It does not make `persona` proposable (no `ParsedProposal` arm, no
  kind-table entry) — personas carry no authority.
- It does not wire persona guidance into the live prompt builder; that is a
  separate future change that will consume `AgentManifest.persona` and the
  registry's `personas` map.
- It introduces no new `learned_artifacts` schema column and no migration lane
  (the generic `kind` column already supports it).
