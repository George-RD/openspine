import assert from "node:assert/strict";
import test from "node:test";

import { migrateCapabilityMap } from "./capability-map-migration.mjs";

function versionOneMap() {
  return {
    schema_version: 1,
    capabilities: [
      {
        id: "selected-gmail-draft",
        owner_outcome: "Choose one Gmail thread, review the exact reply, and create only the approved draft.",
        state: "wired_into_lyra",
        runtime_changes: ["implement-digest-bound-draft-approval"],
        canonical_specs: ["openspec/specs/digest-bound-draft-approval/spec.md"],
        lyra_artifacts: [],
        owner_path_tests: [],
        current_limit: "Approval per draft.",
      },
      {
        id: "recurring-draft-responsibility",
        owner_outcome: "Let Lyra prepare narrowly defined recurring Gmail drafts after the responsibility has been reviewed.",
        state: "product_surface_missing",
        runtime_changes: ["implement-standing-rules"],
        canonical_specs: [],
        lyra_artifacts: [],
        owner_path_tests: [],
        current_limit: "No owner loop yet.",
      },
    ],
    starter_workflow_candidates: [
      {
        id: "recurring-gmail-draft",
        selected: true,
        uses_capabilities: ["selected-gmail-draft", "recurring-draft-responsibility"],
        owner_outcome: "Let Lyra prepare recurring drafts for one known relationship.",
        reason: "Smallest useful progression.",
        task_boundary: "One mailbox, one counterparty, draft only.",
        proof_sequence: ["First", "Second", "Third", "Fourth", "Fifth"],
        tracking_issue: 130,
      },
      {
        id: "research-and-brief",
        selected: false,
        uses_capabilities: ["recurring-draft-responsibility"],
        owner_outcome: "Delegate a private-context research brief.",
        reason: "Larger owner surface.",
      },
    ],
  };
}

test("a version 1 map migrates to a structurally valid version 2 map", () => {
  const migrated = migrateCapabilityMap(versionOneMap());

  assert.equal(migrated.schema_version, 2);
  assert.ok(Array.isArray(migrated.capabilities));
  assert.ok(Array.isArray(migrated.proofs));

  for (const capability of migrated.capabilities) {
    assert.equal(capability.generic, false);
    assert.deepEqual(capability.blocking_issues, []);
  }

  const selected = migrated.proofs.find((proof) => proof.kind === "selected");
  assert.ok(selected);
  assert.equal(selected.selected, true);
  assert.equal(selected.capability, "selected-gmail-draft");
  assert.equal(selected.scope, "One mailbox, one counterparty, draft only.");
  assert.equal(selected.tracking_issue, 130);
  assert.deepEqual(selected.proof_sequence, [
    "First",
    "Second",
    "Third",
    "Fourth",
    "Fifth",
  ]);
  assert.equal(selected.state, "planned");
  assert.deepEqual(selected.owner_path_tests, []);

  const candidates = migrated.proofs.filter((proof) => proof.kind === "candidate");
  assert.equal(candidates.length, 1);
  assert.equal(candidates[0].id, "research-and-brief");
  assert.equal(candidates[0].selected, false);
  assert.equal(candidates[0].capability, "recurring-draft-responsibility");
});

test("migrating a version 2 map is a no-op", () => {
  const migrated = migrateCapabilityMap(versionOneMap());
  const again = migrateCapabilityMap(migrated);
  assert.strictEqual(again, migrated);
});

test("the migration produces the shape the validator accepts as a valid v2 base", () => {
  const migrated = migrateCapabilityMap(versionOneMap());
  assert.equal(migrated.schema_version, 2);
  assert.equal(migrated.proofs.filter((proof) => proof.selected === true).length, 1);
  assert.ok(migrated.proofs.every((proof) => proof.capability !== undefined));
});
