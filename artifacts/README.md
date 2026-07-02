# Lyra artifacts

Declarative OpenSpine artifacts (routes, agent manifests, capability packs,
workflows, policies) for the Lyra product, transcribed from
`.raw/openspine-prd-v9.md` §6/§10/§11/§12 and validated by
`crates/openspine-schemas/tests/fixtures.rs`.

These files define runtime objects. **They do not activate authority.** A
route, agent manifest, or capability pack only *contributes candidate
permissions and constraints* — the only live authority object is a task
grant, produced at runtime by `openspine-authority::compose_authority`
(PRD §9, decision D-007). Nothing here is executable on its own.

Two fixtures — `workflows/*.yaml` and `policies/global.yaml` — have no
literal YAML block in the PRD (unlike routes/agents/packs); each file's
header comment records that it is a grounded design choice for the
`define-core-runtime-schemas` change, not a verbatim transcription.
