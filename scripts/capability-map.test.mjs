import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import test from "node:test";

import {
  parseArchivedChangeIds,
  renderRoadmapBlock,
  replaceGeneratedBlock,
  validateCapabilityMap,
} from "./capability-map.mjs";

function fixtureRoot() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), "openspine-capability-map-"));
  write(
    root,
    "openspec/openspine-change-sequence.md",
    [
      "# sequence",
      "",
      "## Completed / archived",
      "",
      "- `landed-change`",
      "",
      "## Later",
      "",
      "- `not-landed-change`",
      "",
    ].join("\n"),
  );
  write(root, "openspec/specs/example/spec.md", "# spec\n");
  write(root, "artifacts/lyra/workflows/example.yaml", "id: example\n");
  write(
    root,
    "crates/example/tests/owner.rs",
    "#[test]\nfn owner_can_finish_the_workflow() {}\n",
  );
  return root;
}

function write(root, relativePath, content) {
  const destination = path.join(root, relativePath);
  fs.mkdirSync(path.dirname(destination), { recursive: true });
  fs.writeFileSync(destination, content);
}

function validMap() {
  return {
    schema_version: 1,
    capabilities: [
      {
        id: "wired-example",
        owner_outcome: "Finish one owner workflow.",
        state: "wired_into_lyra",
        runtime_changes: ["landed-change"],
        canonical_specs: ["openspec/specs/example/spec.md"],
        lyra_artifacts: ["artifacts/lyra/workflows/example.yaml"],
        owner_path_tests: [
          {
            path: "crates/example/tests/owner.rs",
            test: "owner_can_finish_the_workflow",
          },
        ],
        current_limit: "One bounded path.",
      },
      {
        id: "missing-example",
        owner_outcome: "Reuse the result next time.",
        state: "product_surface_missing",
        runtime_changes: ["landed-change"],
        canonical_specs: ["openspec/specs/example/spec.md"],
        lyra_artifacts: [],
        owner_path_tests: [],
        current_limit: "No owner surface.",
      },
    ],
    starter_workflow_candidates: [
      {
        id: "next-proof",
        selected: true,
        uses_capabilities: ["missing-example"],
        owner_outcome: "Reuse the bounded result.",
        reason: "It is the smallest useful next proof.",
        task_boundary: "One target and one action.",
        proof_sequence: ["First", "Second", "Third", "Fourth"],
        tracking_issue: 123,
      },
    ],
  };
}

test("archived change parsing stops before the next ledger section", () => {
  const ids = parseArchivedChangeIds(
    [
      "## Completed / archived",
      "- `done-one`",
      "- `done-two`",
      "## Planned",
      "- `not-done`",
    ].join("\n"),
  );
  assert.deepEqual([...ids], ["done-one", "done-two"]);
});

test("a wired capability requires archived implementation, artifacts, and a named owner test", () => {
  const root = fixtureRoot();
  assert.deepEqual(validateCapabilityMap(root, validMap()), []);
});

test("an unarchived runtime change fails validation", () => {
  const root = fixtureRoot();
  const map = validMap();
  map.capabilities[0].runtime_changes = ["not-landed-change"];
  assert.match(
    validateCapabilityMap(root, map).join("\n"),
    /not in the ledger's Completed \/ archived section/,
  );
});

test("a canonical spec cannot substitute for archived implementation evidence", () => {
  const root = fixtureRoot();
  const map = validMap();
  map.capabilities[0].runtime_changes = [];
  assert.match(
    validateCapabilityMap(root, map).join("\n"),
    /at least one archived runtime change is required/,
  );
});

test("a wired capability without an owner-path test fails validation", () => {
  const root = fixtureRoot();
  const map = validMap();
  map.capabilities[0].owner_path_tests = [];
  assert.match(
    validateCapabilityMap(root, map).join("\n"),
    /wired capabilities require at least one named owner-path test/,
  );
});

test("the map selects exactly one next owner-facing proof", () => {
  const root = fixtureRoot();
  const map = validMap();
  map.starter_workflow_candidates.push({
    ...map.starter_workflow_candidates[0],
    id: "duplicate-selection",
  });
  assert.match(
    validateCapabilityMap(root, map).join("\n"),
    /exactly one selected workflow; found 2/,
  );
});

test("roadmap rendering is deterministic and replaces only the generated block", () => {
  const map = validMap();
  const block = renderRoadmapBlock(map);
  assert.match(block, /1 wired into Lyra · 1 known product surfaces missing · 0 runtime-only capabilities/);
  assert.match(block, /Selected next owner-facing proof/);
  assert.match(block, /owner_can_finish_the_workflow/);

  const original = [
    "before",
    "<!-- capability-map:start -->",
    "old",
    "<!-- capability-map:end -->",
    "after",
    "",
  ].join("\n");
  const replaced = replaceGeneratedBlock(original, block);
  assert.ok(replaced.startsWith("before\n<!-- capability-map:start -->"));
  assert.ok(replaced.endsWith("\nafter\n"));
  assert.doesNotMatch(replaced, /\nold\n/);
});
