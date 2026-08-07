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
    [
      "#[test]",
      "fn owner_can_finish_the_workflow() {}",
      "#[test]",
      "fn owner_positive_effect() {}",
      "#[test]",
      "fn owner_fallback_case() {}",
      "#[test]",
      "fn owner_control_case() {}",
      "#[test]",
      "fn conformance_second_protocol() {}",
      "",
    ].join("\n"),
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
    schema_version: 2,
    capabilities: [
      {
        id: "generic-delegation",
        generic: true,
        blocking_issues: [],
        owner_outcome: "Delegate reusable protocol-neutral responsibility.",
        state: "product_surface_missing",
        runtime_changes: ["landed-change"],
        canonical_specs: ["openspec/specs/example/spec.md"],
        lyra_artifacts: [],
        owner_path_tests: [],
        current_limit: "No owner loop yet.",
      },
      {
        id: "wired-example",
        generic: false,
        blocking_issues: [],
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
        generic: false,
        blocking_issues: [],
        owner_outcome: "Reuse the result next time.",
        state: "product_surface_missing",
        runtime_changes: ["landed-change"],
        canonical_specs: ["openspec/specs/example/spec.md"],
        lyra_artifacts: [],
        owner_path_tests: [],
        current_limit: "No owner surface.",
      },
    ],
    proofs: [
      {
        id: "recurring-gmail-draft",
        capability: "generic-delegation",
        kind: "selected",
        selected: true,
        owner_outcome: "Prepare recurring drafts for one relationship.",
        reason: "Smallest useful progression.",
        scope: "One mailbox, one counterparty, draft only.",
        current_limit: "Not shipped.",
        state: "planned",
        tracking_issue: 130,
        proof_sequence: ["First", "Second", "Third", "Fourth", "Fifth"],
        owner_path_tests: [],
        conformance_tests: [],
      },
      {
        id: "second-protocol",
        capability: "generic-delegation",
        kind: "portability",
        selected: false,
        owner_outcome: "Prove portability across protocols.",
        reason: "Needs second protocol.",
        scope: "Second communication shape.",
        current_limit: "",
        state: "planned",
        tracking_issue: 131,
        owner_path_tests: [],
        conformance_tests: [],
      },
      {
        id: "whole-responsibility",
        capability: "generic-delegation",
        kind: "whole_responsibility",
        selected: false,
        owner_outcome: "Compose whole responsibilities.",
        reason: "Later maturity stage.",
        scope: "Composition object.",
        current_limit: "",
        state: "planned",
        tracking_issue: 132,
        owner_path_tests: [],
        conformance_tests: [],
      },
    ],
  };
}

test("archived change parsing accepts only actual list entries", () => {
  const ids = parseArchivedChangeIds(
    [
      "## Completed / archived",
      "- `done-one` (retroactive support for",
      "  `mentioned-only`)",
      "- `done-two`",
      "",
      "A prose reference to `also-not-done` is not evidence.",
      "## Planned",
      "- `not-done`",
    ].join("\n"),
  );
  assert.deepEqual([...ids], ["done-one", "done-two"]);
});

test("a valid version 2 map passes validation", () => {
  const root = fixtureRoot();
  assert.deepEqual(validateCapabilityMap(root, validMap()), []);
});

test("an unarchived runtime change fails validation", () => {
  const root = fixtureRoot();
  const map = validMap();
  map.capabilities[1].runtime_changes = ["not-landed-change"];
  assert.match(
    validateCapabilityMap(root, map).join("\n"),
    /not in the ledger's Completed \/ archived section/,
  );
});

test("a canonical spec cannot substitute for archived implementation evidence", () => {
  const root = fixtureRoot();
  const map = validMap();
  map.capabilities[1].runtime_changes = [];
  assert.match(
    validateCapabilityMap(root, map).join("\n"),
    /at least one archived runtime change is required/,
  );
});

test("a helper or commented declaration cannot masquerade as an owner-path test", () => {
  const root = fixtureRoot();
  write(
    root,
    "crates/example/tests/owner.rs",
    [
      "// #[tokio::test]",
      "// async fn owner_can_finish_the_workflow() {}",
      "async fn owner_can_finish_the_workflow() {}",
      "",
    ].join("\n"),
  );
  assert.match(
    validateCapabilityMap(root, validMap()).join("\n"),
    /registered owner-path test owner_can_finish_the_workflow was not found/,
  );
});

test("a wired capability without an owner-path test fails validation", () => {
  const root = fixtureRoot();
  const map = validMap();
  map.capabilities[1].owner_path_tests = [];
  assert.match(
    validateCapabilityMap(root, map).join("\n"),
    /wired capabilities require at least one named owner-path test/,
  );
});

test("a generic capability that IS the selected proof fails validation", () => {
  const root = fixtureRoot();
  const map = validMap();
  // Conflate: the generic capability is the recurring Gmail draft, and the
  // selected proof is the same object.
  map.capabilities[0].id = "recurring-gmail-draft";
  map.capabilities[0].owner_outcome =
    "Let Lyra prepare recurring drafts for one known relationship.";
  const selected = map.proofs.find((proof) => proof.selected === true);
  selected.id = "recurring-gmail-draft";
  selected.capability = "recurring-gmail-draft";
  selected.owner_outcome =
    "Let Lyra prepare recurring drafts for one known relationship.";

  const errors = validateCapabilityMap(root, map).join("\n");
  assert.match(errors, /generic capability and its proof must be distinct objects/);
});

test("a generic capability and a proof sharing an owner_outcome fail validation", () => {
  const root = fixtureRoot();
  const map = validMap();
  const generic = map.capabilities.find((capability) => capability.generic === true);
  const selected = map.proofs.find((proof) => proof.selected === true);
  selected.owner_outcome = generic.owner_outcome;

  assert.match(
    validateCapabilityMap(root, map).join("\n"),
    /generic capability and its proof must not share an owner_outcome/,
  );
});

test("a generic capability named after a protocol fails validation", () => {
  const root = fixtureRoot();
  const map = validMap();
  const generic = map.capabilities.find((capability) => capability.generic === true);

  // id names Gmail.
  generic.id = "gmail-draft-capability";
  for (const proof of map.proofs) proof.capability = "gmail-draft-capability";
  assert.match(
    validateCapabilityMap(root, map).join("\n"),
    /generic capability must be protocol-neutral/,
  );

  // owner_outcome describes a protocol.
  const map2 = validMap();
  map2.capabilities[0].id = "progressive-delegation";
  for (const proof of map2.proofs) proof.capability = "progressive-delegation";
  map2.capabilities[0].owner_outcome =
    "Let Lyra draft recurring emails for one relationship.";
  assert.match(
    validateCapabilityMap(root, map2).join("\n"),
    /generic capability must be protocol-neutral/,
  );
});

test("the generic capability and a proof are distinct schema objects", () => {
  const root = fixtureRoot();
  const map = validMap();
  const generic = map.capabilities.find((capability) => capability.generic === true);
  const selected = map.proofs.find((proof) => proof.selected === true);
  assert.ok(generic);
  assert.ok(selected);
  assert.notEqual(generic.id, selected.id);
  assert.equal(selected.capability, generic.id);
  // The generic capability passes the protocol-neutral check.
  assert.doesNotMatch(generic.id, /gmail|email|telegram|slack/);
  assert.deepEqual(validateCapabilityMap(root, map), []);
});

test("the map selects exactly one next owner-facing proof", () => {
  const root = fixtureRoot();
  const map = validMap();
  map.proofs.push({
    ...map.proofs[0],
    id: "duplicate-selection",
  });
  assert.match(
    validateCapabilityMap(root, map).join("\n"),
    /exactly one proof must be selected; found 2/,
  );
});

test("a shipped proof without owner-path tests fails validation", () => {
  const root = fixtureRoot();
  const map = validMap();
  map.proofs[0].state = "shipped";
  assert.match(
    validateCapabilityMap(root, map).join("\n"),
    /a shipped selected proof requires non-empty owner_path_tests/,
  );
});

test("a wired generic capability requires positive-effect and fallback/control owner-path tests", () => {
  const root = fixtureRoot();
  const map = validMap();
  const generic = map.capabilities.find((capability) => capability.generic === true);
  generic.state = "wired_into_lyra";
  generic.lyra_artifacts = ["artifacts/lyra/workflows/example.yaml"];
  // Re-point the selected proof to a still-missing capability so this test
  // isolates the wiring rule rather than the selected-proof target rule.
  map.proofs[0].capability = "missing-example";

  // Only an uncategorized test (or only runtime changes) cannot satisfy it.
  generic.owner_path_tests = [
    { path: "crates/example/tests/owner.rs", test: "owner_positive_effect" },
  ];
  assert.match(
    validateCapabilityMap(root, map).join("\n"),
    /wired generic capability requires at least one fallback or control owner-path test/,
  );

  // Categorized positive_effect + fallback passes.
  generic.owner_path_tests = [
    {
      path: "crates/example/tests/owner.rs",
      test: "owner_positive_effect",
      kind: "positive_effect",
    },
    {
      path: "crates/example/tests/owner.rs",
      test: "owner_fallback_case",
      kind: "fallback",
    },
  ];
  assert.deepEqual(validateCapabilityMap(root, map), []);
});

test("a portability proof cannot be verified without conformance evidence", () => {
  const root = fixtureRoot();
  const map = validMap();
  map.proofs[1].state = "verified";
  assert.match(
    validateCapabilityMap(root, map).join("\n"),
    /verified portability proof requires non-empty conformance_tests/,
  );

  // With a registered conformance test it passes.
  map.proofs[1].conformance_tests = [
    {
      path: "crates/example/tests/owner.rs",
      test: "conformance_second_protocol",
      kind: "positive_effect",
    },
  ];
  assert.deepEqual(validateCapabilityMap(root, map), []);
});

test("whole-responsibility maturity depends on shipped proof and verified portability", () => {
  const root = fixtureRoot();
  const map = validMap();

  // Whole-responsibility verified while selected is not shipped fails.
  map.proofs[0].state = "planned";
  map.proofs[2].state = "verified";
  map.proofs[2].owner_path_tests = [
    { path: "crates/example/tests/owner.rs", test: "owner_positive_effect" },
  ];
  assert.match(
    validateCapabilityMap(root, map).join("\n"),
    /whole-responsibility cannot be verified until the selected proof is shipped/,
  );

  // Now ship the selected proof and verify portability; whole-responsibility
  // may be verified.
  map.proofs[0].state = "shipped";
  map.proofs[0].owner_path_tests = [
    { path: "crates/example/tests/owner.rs", test: "owner_positive_effect" },
  ];
  map.proofs[1].state = "verified";
  map.proofs[1].conformance_tests = [
    {
      path: "crates/example/tests/owner.rs",
      test: "conformance_second_protocol",
      kind: "positive_effect",
    },
  ];
  assert.deepEqual(validateCapabilityMap(root, map), []);
});

test("an issue number is never runtime evidence", () => {
  const root = fixtureRoot();
  const map = validMap();
  map.capabilities[1].runtime_changes = [128];
  assert.match(
    validateCapabilityMap(root, map).join("\n"),
    /an issue number \(128\) is never runtime evidence/,
  );
});

test("blocking_issues require an integer issue and a known role", () => {
  const root = fixtureRoot();
  const map = validMap();
  const generic = map.capabilities.find((capability) => capability.generic === true);
  generic.blocking_issues = [{ issue: 128, role: "scoped evidence/matching" }];
  assert.deepEqual(validateCapabilityMap(root, map), []);

  generic.blocking_issues = [{ issue: 128, role: "not a role" }];
  assert.match(
    validateCapabilityMap(root, map).join("\n"),
    /blocking issue role must be one of/,
  );
});

test("roadmap rendering is deterministic, groups blockers by role, and never renders issues as proof", () => {
  const map = validMap();
  const generic = map.capabilities.find((capability) => capability.generic === true);
  generic.blocking_issues = [
    { issue: 128, role: "scoped evidence/matching" },
    { issue: 129, role: "execution/review foundations" },
    { issue: 133, role: "proposal-specific evaluation" },
  ];
  const block = renderRoadmapBlock(map);
  assert.match(block, /1 wired into Lyra · 2 known product surfaces missing · 0 runtime-only capabilities · 0 proof in progress/);
  assert.match(block, /Scoped evidence\/matching.*#128/);
  assert.match(block, /Execution\/review foundations.*#129/);
  assert.match(block, /Proposal-specific evaluation.*#133/);
  assert.match(block, /Selected proof.*#130/);
  assert.match(block, /Portability proof.*#131/);
  assert.match(block, /Whole-responsibility progression.*#132/);
  // Issue numbers must not appear in the "Repository proof" column: the only
  // evidence links in the table body are change/spec/test links.
  assert.doesNotMatch(block, /\| .*#13\d/);
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

test("a shipped selected proof without verified portability renders as first shipped proof", () => {
  const map = validMap();
  map.proofs[0].state = "shipped";
  map.proofs[0].owner_path_tests = [
    { path: "crates/example/tests/owner.rs", test: "owner_positive_effect" },
  ];
  map.proofs[1].state = "planned";
  const block = renderRoadmapBlock(map);
  assert.match(block, /first shipped proof/);
  assert.doesNotMatch(block, /works across communication protocols/);
});
