#!/usr/bin/env node
// Store invariant #4 (#262/#271): conservative placement check, not a Rust/SQL
// parser or a proof of audit pairing. Dynamic/concatenated SQL is out of scope;
// SQL-looking comments can be flagged too. Runtime pairing still belongs to
// the audited Store APIs. Node is already required by check.sh and CI.
import { lstatSync, readdirSync, readFileSync } from 'node:fs';
import { join, relative, resolve, sep } from 'node:path';

const approvedPaths = new Set([
  'store/identity.rs',
  'store/audited_effect.rs',
  'store/effect_settlement.rs',
  'store/pending_draft.rs',
  'failure_surfacing/tests.rs',
]);

// Match ordinary literal SQLite INSERT [OR ...] / REPLACE forms. SQL comments
// are whitespace, and the optional schema and table may be quoted. Whole-file
// matching is essential: line-oriented grep silently misses multiline SQL.
// The final boundary must not mistake an effect-named schema for the table.
const trivia = String.raw`(?:\s|/\*[\s\S]*?\*/|--[^\r\n]*(?:\r?\n|$))`;
const identifierChar = String.raw`[\w$\u0080-\uFFFF]`;
const identifier = String.raw`(?:${identifierChar}+|"(?:""|[^"])+"|'(?:''|[^'])+'|\x60(?:\x60\x60|[^\x60])+\x60|\[[^\]]+\])`;
const names = '(?:pending_draft_writes|identities|principals)';
// Unquoted names need a word boundary; a closing quote is already a delimiter.
const table = String.raw`(?:${names}(?!${identifierChar})|"${names}"|'${names}'|\x60${names}\x60|\[${names}\])`;
const effectInsert = new RegExp(
  String.raw`(?<!${identifierChar})(?:INSERT(?:${trivia}+OR${trivia}+(?:ROLLBACK|ABORT|FAIL|IGNORE|REPLACE))?|REPLACE)` +
  String.raw`${trivia}+INTO(?!${identifierChar})${trivia}*(?:${identifier}${trivia}*\.${trivia}*)?${table}(?!${trivia}*\.)`,
  'i',
);

function containsEffectInsert(source) {
  // Also inspect common escaped spellings in ordinary Rust strings. Retain
  // the raw spelling so normalization never hides a previously visible match.
  const unescaped = source
    .replace(/\\\r?\n\s*/g, '')
    .replace(/\\([nrt"])/g, (_, code) => ({ n: '\n', r: '\r', t: '\t', '"': '"' })[code]);
  return effectInsert.test(source) || effectInsert.test(unescaped);
}

function scan(root, directory = root) {
  const offenders = [];
  for (const name of readdirSync(directory).sort()) {
    const file = join(directory, name);
    const path = relative(root, file).split(sep).join('/');
    const stat = lstatSync(file);
    // Do not silently omit linked source or let a link escape the scan root.
    if (stat.isSymbolicLink()) throw new Error(`symlinked source is not supported: ${path}`);
    if (stat.isDirectory()) {
      offenders.push(...scan(root, file));
    } else if (name.endsWith('.rs')) {
      if (!stat.isFile()) throw new Error(`not a regular source file: ${path}`);
      const source = readFileSync(file, 'utf8');
      if (containsEffectInsert(source) && !approvedPaths.has(path)) offenders.push(path);
    }
  }
  return offenders;
}

try {
  if (process.argv.length !== 3) throw new Error('usage: check-effect-writes.mjs <source-root>');
  const root = resolve(process.argv[2]);
  if (!lstatSync(root).isDirectory()) throw new Error(`not a source directory: ${root}`);
  const offenders = scan(root);
  if (offenders.length !== 0) {
    console.error('FAIL: effect-table INSERT outside the audit-paired allowlist (ticket #262):');
    for (const path of offenders) console.error(`  ${path}`);
    console.error('  Route effect writes through Store::with_audited_effect / begin_effect / settle_effect.');
    console.error('  New fixture exceptions require an explicit source-relative allowlist entry.');
    process.exitCode = 1;
  }
} catch (error) {
  console.error(`FAIL: effect-table scan failed (ticket #262): ${error.message}`);
  process.exitCode = 1;
}
