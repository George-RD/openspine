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
]);

function readText(root, relativePath) {
  return fs.readFileSync(path.join(root, relativePath), "utf8");
}

function fileExists(root, relativePath) {
  return fs.existsSync(path.join(root, relativePath));
}

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
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
    [...section.matchAll(/`([^`]+)`/g)].map((match) => match[1]),
  );
}

export function validateCapabilityMap(root, map) {
  const errors = [];
  const ledger = readText(root, LEDGER_RELATIVE_PATH);
  const archivedChanges = parseArchivedChangeIds(ledger);

  if (map.schema_version !== 1) {
    errors.push("schema_version must be 1");
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
    if (runtimeChanges.length === 0 && canonicalSpecs.length === 0) {
      errors.push(`${prefix}: at least one runtime change or canonical spec is required`);
    }
    for (const changeId of runtimeChanges) {
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
      if (!ownerTest?.path || !ownerTest?.test) {
        errors.push(`${prefix}: owner_path_tests entries require path and test`);
        continue;
      }
      if (!fileExists(root, ownerTest.path)) {
        errors.push(`${prefix}: owner-path test file does not exist: ${ownerTest.path}`);
        continue;
      }
      const testSource = readText(root, ownerTest.path);
      const testPattern = new RegExp(
        `\\b(?:async\\s+)?fn\\s+${escapeRegExp(ownerTest.test)}\\s*\\(`,
      );
      if (!testPattern.test(testSource)) {
        errors.push(
          `${prefix}: named owner-path test ${ownerTest.test} was not found in ${ownerTest.path}`,
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
    } else if (ownerTests.length > 0) {
      errors.push(
        `${prefix}: only wired_into_lyra capabilities may claim owner-path tests`,
      );
    }
  }

  const candidates = map.starter_workflow_candidates ?? [];
  const selected = candidates.filter((candidate) => candidate.selected === true);
  if (selected.length !== 1) {
    errors.push(
      `starter_workflow_candidates must contain exactly one selected workflow; found ${selected.length}`,
    );
  }
  for (const candidate of candidates) {
    if (!candidate.id || !candidate.owner_outcome || !candidate.reason) {
      errors.push("each starter workflow candidate requires id, owner_outcome, and reason");
    }
    for (const capabilityId of candidate.uses_capabilities ?? []) {
      if (!ids.has(capabilityId)) {
        errors.push(
          `starter workflow ${candidate.id} references unknown capability ${capabilityId}`,
        );
      }
    }
    if (candidate.selected === true) {
      if (!Number.isInteger(candidate.tracking_issue)) {
        errors.push(`selected starter workflow ${candidate.id} requires tracking_issue`);
      }
      if (!Array.isArray(candidate.proof_sequence) || candidate.proof_sequence.length < 4) {
        errors.push(
          `selected starter workflow ${candidate.id} requires a proof_sequence with at least four steps`,
        );
      }
      const selectedCapabilities = (candidate.uses_capabilities ?? [])
        .map((capabilityId) => map.capabilities.find((item) => item.id === capabilityId))
        .filter(Boolean);
      if (!selectedCapabilities.some((capability) => capability.state === "product_surface_missing")) {
        errors.push(
          `selected starter workflow ${candidate.id} must close at least one product_surface_missing capability`,
        );
      }
    }
  }

  return errors;
}

function humanState(state) {
  return {
    wired_into_lyra: "Wired into Lyra",
    product_surface_missing: "Product surface missing",
    runtime_landed: "Runtime landed",
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

export function renderRoadmapBlock(map) {
  const counts = Object.fromEntries(
    [...VALID_STATES].map((state) => [
      state,
      map.capabilities.filter((capability) => capability.state === state).length,
    ]),
  );
  const selected = map.starter_workflow_candidates.find(
    (candidate) => candidate.selected === true,
  );

  const lines = [
    START_MARKER,
    "## Capability map",
    "",
    "This table is generated from [`capabilities/capability-map.json`](https://github.com/George-RD/openspine/blob/main/capabilities/capability-map.json). CI checks its runtime change IDs against the archived implementation ledger, verifies every evidence path, and requires each **Wired into Lyra** claim to name a real owner-path test.",
    "",
    `**Current count:** ${counts.wired_into_lyra} wired into Lyra · ${counts.product_surface_missing} known product surfaces missing · ${counts.runtime_landed} runtime-only capabilities`,
    "",
    "| Owner outcome | State | Repository proof | Current limit |",
    "|---|---|---|---|",
  ];

  for (const capability of map.capabilities) {
    lines.push(
      `| ${tableCell(capability.owner_outcome)} | **${humanState(capability.state)}** | ${evidenceMarkdown(capability)} | ${tableCell(capability.current_limit)} |`,
    );
  }

  lines.push(
    "",
    "### Selected next owner-facing proof",
    "",
    `**${selected.owner_outcome}**`,
    "",
    selected.reason,
    "",
    `**Boundary:** ${selected.task_boundary}`,
    "",
    "**Proof sequence:**",
    "",
    ...selected.proof_sequence.map((step, index) => `${index + 1}. ${step}`),
    "",
    `Implementation is tracked in [issue #${selected.tracking_issue}](https://github.com/George-RD/openspine/issues/${selected.tracking_issue}).`,
    "",
    END_MARKER,
  );

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

export function checkRepository(root = REPOSITORY_ROOT, { write = false } = {}) {
  const map = JSON.parse(readText(root, MAP_RELATIVE_PATH));
  const errors = validateCapabilityMap(root, map);
  if (errors.length > 0) {
    throw new Error(`Capability map validation failed:\n- ${errors.join("\n- ")}`);
  }

  const roadmapPath = path.join(root, ROADMAP_RELATIVE_PATH);
  const currentRoadmap = fs.readFileSync(roadmapPath, "utf8");
  const expectedRoadmap = replaceGeneratedBlock(
    currentRoadmap,
    renderRoadmapBlock(map),
  );

  if (write) {
    fs.writeFileSync(roadmapPath, expectedRoadmap);
    return;
  }
  if (expectedRoadmap !== currentRoadmap) {
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
