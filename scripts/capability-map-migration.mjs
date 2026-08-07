// Idempotent schema migration for capabilities/capability-map.json.
//
// v1 → v2:
//   - schema_version 1 → 2
//   - every capability gains `generic: false` and `blocking_issues: []`
//   - `starter_workflow_candidates[]` → `proofs[]`
//   - the previously selected candidate becomes a `selected` proof (its
//     `task_boundary` moves into `scope`); unselected candidates become
//     `candidate` proofs
//   - `uses_capabilities[0]` is preserved as the proof's `capability` reference
//   - proofs gain `state: "planned"` and empty evidence arrays
//
// Re-running the migration on a v2 map is a no-op.
//
// Note on provenance: the checked capabilities/capability-map.json is NOT
// `migrateCapabilityMap(real v1)`. A mechanical migration cannot invent a
// protocol-neutral generic capability out of a vertical one, so the checked
// v2 file is the product of (a) this migration applied to the v1 shape and
// (b) a deliberate authoring step that replaced the old
// `recurring-draft-responsibility` capability with the generic
// `progressive-delegation` capability and corrected the records. The migration
// exists to prove the v1→v2 mechanical transform and to keep the tooling
// honest; authoring the generic capability is a separate, deliberate act.

const VALID_STATES = new Set([
  "runtime_landed",
  "wired_into_lyra",
  "product_surface_missing",
]);

function migrateCandidateToProof(candidate) {
  const proof = {
    id: candidate.id,
    capability: candidate.uses_capabilities?.[0] ?? null,
    owner_outcome: candidate.owner_outcome,
    reason: candidate.reason,
    scope: candidate.task_boundary ?? "",
    current_limit: candidate.current_limit ?? "",
    state: "planned",
    owner_path_tests: [],
    conformance_tests: [],
  };
  if (candidate.selected === true) {
    proof.kind = "selected";
    proof.selected = true;
    if (Array.isArray(candidate.proof_sequence)) {
      proof.proof_sequence = [...candidate.proof_sequence];
    }
    if (Number.isInteger(candidate.tracking_issue)) {
      proof.tracking_issue = candidate.tracking_issue;
    }
  } else {
    proof.kind = "candidate";
    proof.selected = false;
  }
  return proof;
}

export function migrateCapabilityMap(map) {
  if (map.schema_version === 2) {
    return map;
  }

  const capabilities = (map.capabilities ?? []).map((capability) => ({
    ...capability,
    generic: false,
    blocking_issues: [],
  }));

  const proofs = (map.starter_workflow_candidates ?? []).map(
    migrateCandidateToProof,
  );

  return {
    schema_version: 2,
    capabilities,
    proofs,
  };
}

// Re-exported so the validator can share the state vocabulary.
export { VALID_STATES };
