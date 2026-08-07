import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const SCRIPT_PATH = fileURLToPath(import.meta.url);
const REPOSITORY_ROOT = path.resolve(path.dirname(SCRIPT_PATH), "..");
const MAP_RELATIVE_PATH = "capabilities/capability-map.json";
const LEDGER_RELATIVE_PATH = "openspec/openspine-change-sequence.md";
const ROADMAP_RELATIVE_PATH = "site/src/content/docs/roadmap.md";
const START_MARKER = "<!-- capability-map:start -->";
const END_MARKER = "<!-- capability-map:end -->";
const VALID_STATES = new Set([
  "runtime_landed",
  "wired_into_lyra",
  "product_surface_missing",
  "proof_in_progress",
]);
const PROOF_KINDS = new Set([
  "selected",
  "portability",
  "whole_responsibility",
  "candidate",
]);
const PROOF_STATES = new Set([
  "planned",
  "in_progress",
  "shipped",
  "verified",
]);
const OWNER_TEST_KINDS = new Set(["positive_effect", "fallback", "control"]);
const BLOCKER_ROLES = new Set([
  "architecture contract",
  "execution/review foundations",
  "scoped evidence/matching",
  "proposal-specific evaluation",
]);

// Protocol tokens that a generic, protocol-neutral capability must not be
// described as. The generic capability names the protocol-neutral responsibility
// (e.g. "progressive delegation"); a vertical proof names a specific protocol
// (e.g. "recurring Gmail drafts"). Adding a future protocol is a one-line
// addition here.
//
// This is a bounded heuristic, not a semantic proof: it is a deterministic,
// case-insensitive substring match, so it stops the *accidental* "the Gmail
// proof is the capability" case but can be bypassed by obfuscation (zero-width
// characters, fullwidth forms, a hyphen like "e-mail", or an unlisted protocol
// name). The structural distinctness rules above — capability id and
// owner_outcome must differ from the selected proof's — are the airtight part
// of the anti-conflation gate; the token list is a blunt first line only.
const PROTOCOL_TOKENS = [
  "gmail",
  "email",
  "telegram",
  "slack",
  "imap",
  "smtp",
  "whatsapp",
  "sms",
  "discord",
];

function mentionsProtocol(value) {
  if (typeof value !== "string") return true;
  const lower = value.toLowerCase();
  return PROTOCOL_TOKENS.some((token) => lower.includes(token));
}

function readText(root, relativePath) {
  return fs.readFileSync(path.join(root, relativePath), "utf8");
}

function fileExists(root, relativePath) {
  return fs.existsSync(path.join(root, relativePath));
}

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function stripRustComments(source) {
  return source
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .replace(/\/\/.*$/gm, "");
}

function hasRegisteredRustTest(source, testName) {
  const uncommented = stripRustComments(source);
  const escapedName = escapeRegExp(testName);
  const testPattern = new RegExp(
    `#\\s*\\[\\s*(?:tokio::)?test(?:\\s*\\([^\\]]*\\))?\\s*\\]` +
      `(?:\\s*#\\s*\\[[^\\]]+\\])*` +
      `\\s*(?:pub(?:\\([^)]*\\))?\\s+)?(?:async\\s+)?fn\\s+${escapedName}\\s*\\(`,
    "m",
  );
  return testPattern.test(uncommented);
}

export function parseArchivedChangeIds(markdown) {
  const heading = "## Completed / archived";
  const start = markdown.indexOf(heading);
  if (start === -1) {
    throw new Error(`Missing ${heading} in ${LEDGER_RELATIVE_PATH}`);
  }
  const afterHeading = markdown.slice(start + heading.length);
  const nextHeading = afterHeading.search(/^##\s+/m);
  const section = nextHeading === -1
    ? afterHeading
    : afterHeading.slice(0, nextHeading);
  return new Set(
    section
      .split("\n")
      .map((line) => line.match(/^- `([^`]+)`(?:\s|$)/))
      .filter(Boolean)
      .map((match) => match[1]),
  );
}

// Validate a proof's owner-path / conformance evidence list. Evidence entries
// are { path, test, kind? } objects and must resolve to registered Rust tests.
function validateTestEntries(root, errors, prefix, entries) {
  for (const entry of entries ?? []) {
    if (typeof entry !== "object" || entry === null) {
      errors.push(`${prefix}: evidence entries must be objects`);
      continue;
    }
    if (entry.issue !== undefined) {
      errors.push(
        `${prefix}: an issue number is never runtime evidence; use an archived change, spec, or named test instead`,
      );
    }
    if (!entry.path || !entry.test) {
      errors.push(`${prefix}: evidence entries require path and test`);
      continue;
    }
    if (entry.kind !== undefined && !OWNER_TEST_KINDS.has(entry.kind)) {
      errors.push(
        `${prefix}: evidence entry kind must be one of ${[...OWNER_TEST_KINDS].join(", ")}`,
      );
    }
    if (!fileExists(root, entry.path)) {
      errors.push(`${prefix}: evidence test file does not exist: ${entry.path}`);
      continue;
    }
    const testSource = readText(root, entry.path);
    if (!hasRegisteredRustTest(testSource, entry.test)) {
      errors.push(
        `${prefix}: registered test ${entry.test} was not found in ${entry.path}`,
      );
    }
  }
}

export function validateCapabilityMap(root, map) {
  const errors = [];
  const ledger = readText(root, LEDGER_RELATIVE_PATH);
  const archivedChanges = parseArchivedChangeIds(ledger);

  if (map.schema_version !== 2) {
    errors.push("schema_version must be 2");
  }
  if (!Array.isArray(map.capabilities) || map.capabilities.length === 0) {
    errors.push("capabilities must be a non-empty array");
    return errors;
  }

  const ids = new Set();
  for (const capability of map.capabilities) {
    const prefix = capability.id ? `capability ${capability.id}` : "capability";

    if (!capability.id || typeof capability.id !== "string") {
      errors.push(`${prefix}: id must be a non-empty string`);
      continue;
    }
    if (ids.has(capability.id)) {
      errors.push(`${prefix}: duplicate id`);
    }
    ids.add(capability.id);

    if (typeof capability.generic !== "boolean") {
      errors.push(`${prefix}: generic must be a boolean`);
    }
    if (!Array.isArray(capability.blocking_issues)) {
      errors.push(`${prefix}: blocking_issues must be an array`);
    } else {
      for (const blocker of capability.blocking_issues) {
        if (!Number.isInteger(blocker?.issue)) {
          errors.push(`${prefix}: each blocking_issue requires an integer issue`);
        }
        if (!BLOCKER_ROLES.has(blocker?.role)) {
          errors.push(
            `${prefix}: blocking issue role must be one of ${[...BLOCKER_ROLES].join(", ")}`,
          );
        }
      }
    }

    if (!VALID_STATES.has(capability.state)) {
      errors.push(`${prefix}: invalid state ${JSON.stringify(capability.state)}`);
    }
    if (!capability.owner_outcome || typeof capability.owner_outcome !== "string") {
      errors.push(`${prefix}: owner_outcome must be a non-empty string`);
    }
    if (!capability.current_limit || typeof capability.current_limit !== "string") {
      errors.push(`${prefix}: current_limit must be a non-empty string`);
    }

    const runtimeChanges = capability.runtime_changes ?? [];
    const canonicalSpecs = capability.canonical_specs ?? [];
    if (runtimeChanges.length === 0) {
      errors.push(`${prefix}: at least one archived runtime change is required`);
    }
    for (const changeId of runtimeChanges) {
      if (Number.isInteger(changeId)) {
        errors.push(
          `${prefix}: an issue number (${changeId}) is never runtime evidence; runtime changes must be archived change ids`,
        );
        continue;
      }
      if (!archivedChanges.has(changeId)) {
        errors.push(
          `${prefix}: runtime change ${changeId} is not in the ledger's Completed / archived section`,
        );
      }
    }
    for (const specPath of canonicalSpecs) {
      if (!fileExists(root, specPath)) {
        errors.push(`${prefix}: canonical spec does not exist: ${specPath}`);
      }
    }

    const artifacts = capability.lyra_artifacts ?? [];
    for (const artifactPath of artifacts) {
      if (!fileExists(root, artifactPath)) {
        errors.push(`${prefix}: Lyra artifact does not exist: ${artifactPath}`);
      }
    }

    const ownerTests = capability.owner_path_tests ?? [];
    for (const ownerTest of ownerTests) {
      if (Number.isInteger(ownerTest)) {
        errors.push(
          `${prefix}: an issue number is never runtime evidence; owner-path tests must name a path and test`,
        );
        continue;
      }
      if (!ownerTest?.path || !ownerTest?.test) {
        errors.push(`${prefix}: owner_path_tests entries require path and test`);
        continue;
      }
      if (ownerTest.kind !== undefined && !OWNER_TEST_KINDS.has(ownerTest.kind)) {
        errors.push(
          `${prefix}: owner-path test kind must be one of ${[...OWNER_TEST_KINDS].join(", ")}`,
        );
      }
      if (!fileExists(root, ownerTest.path)) {
        errors.push(`${prefix}: owner-path test file does not exist: ${ownerTest.path}`);
        continue;
      }
      const testSource = readText(root, ownerTest.path);
      if (!hasRegisteredRustTest(testSource, ownerTest.test)) {
        errors.push(
          `${prefix}: registered owner-path test ${ownerTest.test} was not found in ${ownerTest.path}`,
        );
      }
    }

    if (capability.state === "wired_into_lyra") {
      if (artifacts.length === 0) {
        errors.push(`${prefix}: wired capabilities require at least one Lyra artifact`);
      }
      if (ownerTests.length === 0) {
        errors.push(`${prefix}: wired capabilities require at least one named owner-path test`);
      }
      if (capability.generic === true) {
        const hasPositive = ownerTests.some(
          (test) => test?.kind === "positive_effect",
        );
        const hasFallbackOrControl = ownerTests.some(
          (test) => test?.kind === "fallback" || test?.kind === "control",
        );
        if (!hasPositive) {
          errors.push(
            `${prefix}: a wired generic capability requires at least one positive_effect owner-path test`,
          );
        }
        if (!hasFallbackOrControl) {
          errors.push(
            `${prefix}: a wired generic capability requires at least one fallback or control owner-path test`,
          );
        }
      }
    } else if (ownerTests.length > 0) {
      errors.push(
        `${prefix}: only wired_into_lyra capabilities may claim owner-path tests`,
      );
    }
  }

  // Proofs: replace starter_workflow_candidates.
  const proofs = map.proofs ?? [];
  if (!Array.isArray(map.proofs)) {
    errors.push("proofs must be an array");
  } else if (proofs.length === 0) {
    errors.push("proofs must be a non-empty array");
  }

  const selectedProofs = proofs.filter((proof) => proof.selected === true);
  if (selectedProofs.length !== 1) {
    errors.push(`exactly one proof must be selected; found ${selectedProofs.length}`);
  }
  const proofIds = new Set();
  const byKind = Object.fromEntries([...PROOF_KINDS].map((kind) => [kind, []]));
  for (const proof of proofs) {
    const prefix = proof?.id ? `proof ${proof.id}` : "proof";
    if (!proof?.id || typeof proof.id !== "string") {
      errors.push(`${prefix}: id must be a non-empty string`);
      continue;
    }
    if (proofIds.has(proof.id)) {
      errors.push(`${prefix}: duplicate proof id`);
    }
    proofIds.add(proof.id);

    if (!PROOF_KINDS.has(proof.kind)) {
      errors.push(`${prefix}: invalid proof kind ${JSON.stringify(proof.kind)}`);
    } else {
      byKind[proof.kind].push(proof);
    }
    if (!PROOF_STATES.has(proof.state)) {
      errors.push(`${prefix}: invalid proof state ${JSON.stringify(proof.state)}`);
    }
    if (typeof proof.selected !== "boolean") {
      errors.push(`${prefix}: selected must be a boolean`);
    }
    if (!proof.capability || !ids.has(proof.capability)) {
      errors.push(`${prefix}: proof references unknown capability ${proof.capability}`);
    } else {
      const targetCapability = map.capabilities.find(
        (capability) => capability.id === proof.capability,
      );
      if (targetCapability?.generic === true) {
        // The generic capability and its proofs must be distinct objects: the
        // anti-conflation rule the roadmap change exists to enforce. The
        // generic capability names a protocol-neutral responsibility, never
        // the vertical proof that realizes it.
        if (proof.id === targetCapability.id) {
          errors.push(
            `${prefix}: a generic capability and its proof must be distinct objects (ids must differ)`,
          );
        }
        if (proof.owner_outcome === targetCapability.owner_outcome) {
          errors.push(
            `${prefix}: a generic capability and its proof must not share an owner_outcome`,
          );
        }
        if (mentionsProtocol(targetCapability.id) || mentionsProtocol(targetCapability.owner_outcome)) {
          errors.push(
            `${prefix}: generic capability must be protocol-neutral (id and owner_outcome must not name a specific protocol like gmail/telegram/slack)`,
          );
        }
      }
    }
    if (!proof.owner_outcome || !proof.reason) {
      errors.push(`${prefix}: each proof requires owner_outcome and reason`);
    }

    if (proof.kind === "selected") {
      if (proof.selected !== true) {
        errors.push(`${prefix}: a selected proof must have selected: true`);
      }
      if (!Number.isInteger(proof.tracking_issue)) {
        errors.push(`${prefix}: selected proof requires an integer tracking_issue`);
      }
      if (!Array.isArray(proof.proof_sequence) || proof.proof_sequence.length < 4) {
        errors.push(
          `${prefix}: selected proof requires a proof_sequence with at least four steps`,
        );
      }
      const targetCapability = map.capabilities.find(
        (capability) => capability.id === proof.capability,
      );
      if (
        targetCapability &&
        !["product_surface_missing", "proof_in_progress"].includes(targetCapability.state)
      ) {
        errors.push(
          `${prefix}: selected proof must close a product_surface_missing or proof_in_progress capability`,
        );
      }
    }

    if (proof.kind !== "candidate" && !Number.isInteger(proof.tracking_issue)) {
      errors.push(
        `${prefix}: ${proof.kind} proof requires an integer tracking_issue`,
      );
    }

    // Evidence rules. A shipped/verified *selected* proof must carry named
    // owner-path tests. Portability proof evidence is conformance_tests, and
    // whole-responsibility proof evidence is its dependency on the selected
    // and portability proofs.
    if (proof.kind === "selected") {
      if (proof.state === "shipped" || proof.state === "verified") {
        const evidence = proof.owner_path_tests ?? [];
        if (evidence.length === 0) {
          errors.push(
            `${prefix}: a shipped selected proof requires non-empty owner_path_tests`,
          );
        }
      }
    }
    if (proof.kind === "portability") {
      if (proof.state === "verified") {
        const conformance = proof.conformance_tests ?? [];
        if (conformance.length === 0) {
          errors.push(
            `${prefix}: a verified portability proof requires non-empty conformance_tests (second-protocol evidence)`,
          );
        }
      }
    }
    if (proof.kind === "whole_responsibility" && proof.state === "verified") {
      const selected = selectedProofs[0];
      const portability = byKind.portability[0];
      if (!selected || selected.state !== "shipped") {
        errors.push(
          `${prefix}: whole-responsibility cannot be verified until the selected proof is shipped`,
        );
      }
      if (!portability || portability.state !== "verified") {
        errors.push(
          `${prefix}: whole-responsibility cannot be verified until the portability proof is verified`,
        );
      }
    }

    validateTestEntries(
      root,
      errors,
      prefix,
      proof.owner_path_tests ?? [],
    );
    validateTestEntries(
      root,
      errors,
      prefix,
      proof.conformance_tests ?? [],
    );
  }

  return errors;
}

function humanState(state) {
  return {
    wired_into_lyra: "Wired into Lyra",
    product_surface_missing: "Product surface missing",
    runtime_landed: "Runtime landed",
    proof_in_progress: "Proof in progress",
  }[state];
}

function evidenceMarkdown(capability) {
  const pieces = [];
  for (const changeId of capability.runtime_changes ?? []) {
    pieces.push(
      `[${changeId}](https://github.com/George-RD/openspine/blob/main/openspec/openspine-change-sequence.md)`,
    );
  }
  if ((capability.runtime_changes ?? []).length === 0) {
    for (const specPath of capability.canonical_specs ?? []) {
      pieces.push(
        `[${path.basename(path.dirname(specPath))}](https://github.com/George-RD/openspine/blob/main/${specPath})`,
      );
    }
  }
  for (const test of capability.owner_path_tests ?? []) {
    pieces.push(
      `[${test.test}](https://github.com/George-RD/openspine/blob/main/${test.path})`,
    );
  }
  return pieces.join("<br />");
}

function tableCell(value) {
  return value.replaceAll("|", "\\|").replaceAll("\n", " ");
}

function blockerGroupLabel(role) {
  return {
    "architecture contract": "Architecture contract",
    "execution/review foundations": "Execution/review foundations",
    "scoped evidence/matching": "Scoped evidence/matching",
    "proposal-specific evaluation": "Proposal-specific evaluation",
  }[role];
}

export function renderRoadmapBlock(map) {
  const counts = Object.fromEntries(
    [...VALID_STATES].map((state) => [
      state,
      map.capabilities.filter((capability) => capability.state === state).length,
    ]),
  );
  const selected = map.proofs.find((proof) => proof.selected === true);
  const portability = map.proofs.find((proof) => proof.kind === "portability");
  const wholeResponsibility = map.proofs.find(
    (proof) => proof.kind === "whole_responsibility",
  );
  const generic = map.capabilities.find((capability) => capability.generic === true);

  const countLine = [
    `**Current count:** ${counts.wired_into_lyra} wired into Lyra · ${counts.product_surface_missing} known product surfaces missing · ${counts.runtime_landed} runtime-only capabilities · ${counts.proof_in_progress} proof in progress`,
  ].join("");

  const lines = [
    START_MARKER,
    "## Capability map",
    "",
    "This table is generated from [`capabilities/capability-map.json`](https://github.com/George-RD/openspine/blob/main/capabilities/capability-map.json). CI checks its runtime change IDs against the archived implementation ledger, verifies every evidence path, and requires each **Wired into Lyra** claim to name real owner-path tests. Issue numbers are blockers, never repository proof.",
    "",
    countLine,
    "",
    "| Owner outcome | State | Repository proof | Current limit |",
    "|---|---|---|---|",
  ];

  for (const capability of map.capabilities) {
    lines.push(
      `| ${tableCell(capability.owner_outcome)} | **${humanState(capability.state)}** | ${evidenceMarkdown(capability)} | ${tableCell(capability.current_limit)} |`,
    );
  }

  if (generic) {
    lines.push(
      "",
      `### ${generic.owner_outcome}`,
      "",
      `**State:** ${humanState(generic.state)} (generic capability)`,
      "",
      "**Landed substrate:**",
      "",
      ...generic.canonical_specs.map(
        (specPath) =>
          `- [${path.basename(path.dirname(specPath))}](https://github.com/George-RD/openspine/blob/main/${specPath})`,
      ),
    );

    if (generic.blocking_issues && generic.blocking_issues.length > 0) {
      const byRole = {};
      for (const blocker of generic.blocking_issues) {
        (byRole[blocker.role] ??= []).push(blocker.issue);
      }
      lines.push("", "**Blockers:**", "");
      for (const role of Object.keys(byRole).sort()) {
        const links = byRole[role]
          .map(
            (issue) =>
              `[#${issue}](https://github.com/George-RD/openspine/issues/${issue})`,
          )
          .join(", ");
        lines.push(`- **${blockerGroupLabel(role)}:** ${links}`);
      }
    }

    if (selected) {
      lines.push(
        "",
        `**Selected proof:** [recurring Gmail drafts (#${selected.tracking_issue})](https://github.com/George-RD/openspine/issues/${selected.tracking_issue})`,
      );
    }
    if (portability) {
      lines.push(
        "",
        `**Portability proof:** [second communication shape (#${portability.tracking_issue})](https://github.com/George-RD/openspine/issues/${portability.tracking_issue})`,
      );
    }
    if (wholeResponsibility) {
      lines.push(
        "",
        `**Whole-responsibility progression:** [#${wholeResponsibility.tracking_issue}](https://github.com/George-RD/openspine/issues/${wholeResponsibility.tracking_issue})`,
      );
    }

    // Public-copy rule: a shipped selected proof that is not verified for
    // portability is a "first shipped proof", never cross-protocol.
    if (
      selected?.state === "shipped" &&
      (!portability || portability.state !== "verified")
    ) {
      lines.push(
        "",
        "The selected proof is the **first shipped proof**; portability across a second communication protocol remains unproven.",
      );
    }
  }

  if (selected) {
    lines.push(
      "",
      "### Selected proof",
      "",
      `**${selected.owner_outcome}**`,
      "",
      selected.reason,
      "",
      `**Boundary:** ${selected.scope}`,
      "",
      "**Proof sequence:**",
      "",
      ...selected.proof_sequence.map((step, index) => `${index + 1}. ${step}`),
      "",
      `Implementation is tracked in [issue #${selected.tracking_issue}](https://github.com/George-RD/openspine/issues/${selected.tracking_issue}).`,
    );
  }

  lines.push("", END_MARKER);

  return `${lines.join("\n")}\n`;
}

export function replaceGeneratedBlock(markdown, block) {
  const start = markdown.indexOf(START_MARKER);
  const end = markdown.indexOf(END_MARKER);
  if (start === -1 || end === -1 || end < start) {
    throw new Error(
      `${ROADMAP_RELATIVE_PATH} must contain ${START_MARKER} and ${END_MARKER}`,
    );
  }
  const afterEnd = end + END_MARKER.length;
  return `${markdown.slice(0, start)}${block.trimEnd()}${markdown.slice(afterEnd)}`;
}

// Regenerate the generated block in the public roadmap markdown file. This
// script's only write target is site/src/content/docs/roadmap.md; the
// capability map never writes into the change sequence, whose file header
// states it holds only the change decomposition.
function insertOrReplaceGeneratedBlock(fileText, block) {
  const start = fileText.indexOf(START_MARKER);
  const end = fileText.indexOf(END_MARKER);
  if (start !== -1 && end !== -1 && end > start) {
    const afterEnd = end + END_MARKER.length;
    return `${fileText.slice(0, start)}${block.trimEnd()}${fileText.slice(afterEnd)}`;
  }
  const anchor = "## Canon sources";
  const anchorIndex = fileText.indexOf(anchor);
  if (anchorIndex !== -1) {
    return (
      fileText.slice(0, anchorIndex) +
      `${block.trimEnd()}\n\n` +
      fileText.slice(anchorIndex)
    );
  }
  return `${fileText.trimEnd()}\n\n${block.trimEnd()}\n`;
}

export function checkRepository(root = REPOSITORY_ROOT, { write = false } = {}) {
  const map = JSON.parse(readText(root, MAP_RELATIVE_PATH));
  const errors = validateCapabilityMap(root, map);
  if (errors.length > 0) {
    throw new Error(`Capability map validation failed:\n- ${errors.join("\n- ")}`);
  }

  const block = renderRoadmapBlock(map);

  const targetPath = path.join(root, ROADMAP_RELATIVE_PATH);
  const current = fs.readFileSync(targetPath, "utf8");
  const expected = insertOrReplaceGeneratedBlock(current, block);

  if (write) {
    fs.writeFileSync(targetPath, expected);
    return;
  }
  if (expected !== current) {
    throw new Error(
      `${ROADMAP_RELATIVE_PATH} is stale; run node scripts/capability-map.mjs --write`,
    );
  }
}

async function main() {
  const write = process.argv.slice(2).includes("--write");
  checkRepository(REPOSITORY_ROOT, { write });
  console.log(
    write
      ? "capability-map: validated evidence and regenerated the public roadmap."
      : "capability-map: source, evidence, and public roadmap are consistent.",
  );
}

if (process.argv[1] && path.resolve(process.argv[1]) === SCRIPT_PATH) {
  main().catch((error) => {
    console.error(error.message);
    process.exitCode = 1;
  });
}
