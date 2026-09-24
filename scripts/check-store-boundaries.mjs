#!/usr/bin/env node
// Conservative lexical placement guards, not Rust name resolution. These scan
// ordinary source spelling (including aliases, references, comments and strings),
// not macro expansion, generated code or dynamically assembled SQL.
import { lstatSync, readdirSync, readFileSync } from 'node:fs';
import { join, relative, resolve, sep } from 'node:path';

function lexicalView(source) {
  let code = '';
  const strings = [];
  let i = 0;
  const mask = (end) => {
    code += source.slice(i, end).replace(/[^\r\n]/g, ' ');
    i = end;
  };
  while (i < source.length) {
    if (source.startsWith('//', i)) {
      const end = source.indexOf('\n', i);
      mask(end < 0 ? source.length : end);
      continue;
    }
    if (source.startsWith('/*', i)) {
      let depth = 1;
      let end = i + 2;
      while (end < source.length && depth > 0) {
        if (source.startsWith('/*', end)) { depth++; end += 2; }
        else if (source.startsWith('*/', end)) { depth--; end += 2; }
        else end++;
      }
      if (depth !== 0) throw new Error('unterminated block comment');
      mask(end);
      continue;
    }
    const raw = /^(?:br|r)(#*)"/.exec(source.slice(i));
    if (raw) {
      const begin = i + raw[0].length;
      const terminator = '"' + raw[1];
      const end = source.indexOf(terminator, begin);
      if (end < 0) throw new Error('unterminated raw string');
      strings.push({ index: i, value: source.slice(begin, end) });
      mask(end + terminator.length);
      continue;
    }
    if (source[i] === '"' || source.startsWith('b"', i)) {
      let end = i + (source[i] === 'b' ? 2 : 1);
      let value = '';
      while (end < source.length && source[end] !== '"') {
        if (source[end] !== '\\') { value += source[end++]; continue; }
        end++;
        const escape = source[end++];
        if (escape === '\n' || escape === '\r') {
          while (/\s/.test(source[end] ?? '') && end < source.length) end++;
        } else if (escape === 'x' && /^[0-9a-f]{2}/i.test(source.slice(end))) {
          value += String.fromCharCode(parseInt(source.slice(end, end + 2), 16));
          end += 2;
        } else if (escape === 'u') {
          const unicode = /^\{([0-9a-f_]+)\}/i.exec(source.slice(end));
          if (!unicode) throw new Error('invalid Unicode string escape');
          value += String.fromCodePoint(parseInt(unicode[1].replaceAll('_', ''), 16));
          end += unicode[0].length;
        } else {
          value += ({ n: '\n', r: '\r', t: '\t', '"': '"', '\\': '\\' })[escape] ?? escape;
        }
      }
      if (end >= source.length) throw new Error('unterminated string');
      strings.push({ index: i, value });
      mask(end + 1);
      continue;
    }
    // Consume character literals, but retain lifetimes such as 'a and '_.
    const character = /^(?:b)?'(?:\\(?:u\{[0-9a-f]+\}|x[0-9a-f]{2}|.)|[^'\\\r\n])'/i.exec(source.slice(i));
    if (character) { mask(i + character[0].length); continue; }
    code += source[i++];
  }
  return { code, strings };
}

function functionRanges(code, name) {
  const ranges = [];
  const pattern = new RegExp('\\bfn\\s+' + name + '\\b', 'g');
  for (const match of code.matchAll(pattern)) {
    const start = code.indexOf('{', match.index);
    if (start < 0) continue;
    let depth = 1;
    let end = start + 1;
    while (end < code.length && depth > 0) {
      if (code[end] === '{') depth++;
      if (code[end] === '}') depth--;
      end++;
    }
    if (depth !== 0) throw new Error('unbalanced function body: ' + name);
    ranges.push({ start, end });
  }
  return ranges;
}

// Exceptions identify a function AND its existing API. Migration functions run
// before Store construction; an entire store/migration filename is not exempt.
const transactionBoundaries = {
  'store/mod.rs': [
    ['with_immediate_tx_mapped', 'transaction_with_behavior'],
    ['with_deferred_read', 'transaction_with_behavior'],
  ],
  'store/migrations.rs': [
    ['apply_single_migration_inner', 'transaction'],
    ['revert_versioned_migrations_for_test', 'transaction'],
  ],
  'store/learned_artifacts.rs': [['migrate_provenance_column', 'unchecked_transaction']],
};

function transactionOffenses(path, { code, strings }) {
  const allowed = (transactionBoundaries[path] ?? []).flatMap(([name, api]) =>
    functionRanges(code, name).map((range) => ({ ...range, api })));
  const offenses = [];
  const APIs = /(?:\.|::)\s*(?:r#)?(transaction_with_behavior|unchecked_transaction|transaction|savepoint_with_name|savepoint|new_unchecked)\b/g;
  for (const match of code.matchAll(APIs)) {
    if (!allowed.some(({ start, end, api }) =>
      match.index > start && match.index < end && match[1] === api)) {
      offenses.push({ index: match.index, reason: 'raw transaction API: ' + match[1] });
    }
  }
  // Detect explicit constructors and reject local renaming of transaction types.
  // Connection aliases remain covered by the API-name check above.
  for (const pattern of [
    /\b(?:Transaction|Savepoint)\s*(?:::\s*<[^;{}]*>)?\s*::\s*(?:r#)?(?:new|with_name)\b/g,
    /<\s*(?:\w+\s*::\s*)*(?:Transaction|Savepoint)(?:\s*<[^;{}]*>)?\s*>\s*::\s*(?:r#)?(?:new|with_name)\b/g,
    /\b(?:Transaction|Savepoint)\s+as\s+\w+/g,
    /\btype\s+\w+(?:\s*<[^;={}]*>)?\s*=\s*(?:::\s*)?(?:\w+\s*::\s*)*(?:Transaction|Savepoint)\b/g,
  ]) {
    for (const match of code.matchAll(pattern)) {
      offenses.push({ index: match.index, reason: 'transaction constructor or type alias' });
    }
  }
  for (const literal of strings) {
    // A direct Result diagnostic is never the SQL argument. In particular,
    // `.expect("begin")` occurs in unrelated OAuth tests.
    if (/\.\s*(?:expect|expect_err)\s*\(\s*$/.test(code.slice(0, literal.index))) continue;
    const sql = literal.value.replace(/\/\*[\s\S]*?\*\//g, ' ').replace(/--[^\r\n]*/g, ' ');
    if (/(?:^|;)\s*(?:BEGIN(?:\s+(?:DEFERRED|IMMEDIATE|EXCLUSIVE))?(?:\s+TRANSACTION)?\s*(?:;|$)|SAVEPOINT\s+\S)/i.test(sql)) {
      offenses.push({ index: literal.index, reason: 'literal SQL transaction opening' });
    }
  }
  return offenses;
}

function offenses(mode, path, view) {
  if (mode === 'transactions') return transactionOffenses(path, view);
  if (path === 'telegram.rs') return [];
  // Conservatively reserve every ::mint reference for adapters, so a renamed
  // OwnerVerifiedProof, a function pointer, <Alias>::mint or Self::mint cannot
  // bypass the guard. An unrelated future mint API requires deliberate review.
  return [...view.code.matchAll(/::\s*(?:r#)?mint\b/g)]
    .map((match) => ({ index: match.index, reason: 'mint reference outside channel adapter' }));
}

function scan(mode, root, directory = root) {
  const found = [];
  for (const name of readdirSync(directory).sort()) {
    const file = join(directory, name);
    const path = relative(root, file).split(sep).join('/');
    const stat = lstatSync(file);
    if (stat.isSymbolicLink()) throw new Error('symlinked source: ' + path);
    if (stat.isDirectory()) found.push(...scan(mode, root, file));
    else if (name.endsWith('.rs')) {
      if (!stat.isFile()) throw new Error('not a regular source: ' + path);
      const source = readFileSync(file, 'utf8');
      for (const offense of offenses(mode, path, lexicalView(source))) {
        const line = source.slice(0, offense.index).split('\n').length;
        found.push(path + ':' + line + ': ' + offense.reason);
      }
    }
  }
  return found;
}

try {
  const [mode, sourceRoot] = process.argv.slice(2);
  if (!['transactions', 'owner-proofs'].includes(mode) || !sourceRoot || process.argv.length !== 4) {
    throw new Error('usage: check-store-boundaries.mjs <transactions|owner-proofs> <source-root>');
  }
  const root = resolve(sourceRoot);
  if (!lstatSync(root).isDirectory()) throw new Error('not a source directory: ' + root);
  const found = scan(mode, root);
  if (found.length) {
    console.error('FAIL: ' + mode + ' placement boundary (#208):');
    found.forEach((entry) => console.error('  ' + entry));
    process.exitCode = 1;
  }
} catch (error) {
  console.error('FAIL: Store boundary scan (#208): ' + error.message);
  process.exitCode = 1;
}
