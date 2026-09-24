// Exercise the actual shell gates in isolated source trees. Each bypass must
// fail on its own; another offender must never hide a missing detection.
import test from 'node:test';
import assert from 'node:assert/strict';
import { mkdtempSync, mkdirSync, copyFileSync, existsSync, writeFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { spawnSync } from 'node:child_process';

function run(gate, path, source) {
  const root = mkdtempSync(join(tmpdir(), 'openspine-boundaries-'));
  try {
    mkdirSync(join(root, 'scripts'));
    for (const name of ['check-store-encapsulation.sh', 'check-owner-proof-mint.sh',
      'check-effect-writes.mjs', 'check-store-boundaries.mjs']) {
      if (existsSync(`scripts/${name}`)) copyFileSync(`scripts/${name}`, join(root, 'scripts', name));
    }
    const file = join(root, 'crates/openspine-kernel/src', path);
    mkdirSync(dirname(file), { recursive: true });
    writeFileSync(file, source);
    const result = spawnSync('bash', [join(root, 'scripts', gate)], { encoding: 'utf8' });
    assert.ifError(result.error);
    return { status: result.status, output: result.stdout + result.stderr };
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
}

const txCases = [
  ['plain transaction', 'fn rogue() { conn.transaction(); }'],
  ['unchecked transaction', 'fn rogue() { conn.unchecked_transaction(); }'],
  ['qualified method reference', 'fn rogue() { let start = Connection::transaction; }'],
  ['connection alias', 'use rusqlite::Connection as Db; fn rogue() { Db::transaction(conn); }'],
  ['multiline method', 'fn rogue() { conn.\n transaction_with_behavior(mode); }'],
  ['comment split method', 'fn rogue() { conn./* gap */transaction(); }'],
  ['raw identifier method', 'fn rogue() { conn.r#transaction(); }'],
  ['transaction constructor', 'fn rogue() { rusqlite::Transaction::new(conn, mode); }'],
  ['transaction type alias', 'use rusqlite::{Transaction as Tx}; fn rogue() { Tx::new(conn, mode); }'],
  ['transaction type declaration', "type Tx<'a> = rusqlite::Transaction<'a>; fn rogue() {}"],
  ['absolute transaction type declaration', "type Tx<'a> = ::rusqlite::Transaction<'a>; fn rogue() { Tx::new(conn, mode); }"],
  ['qualified transaction constructor', "fn rogue() { <rusqlite::Transaction<'_>>::new(conn, mode); }"],
  ['savepoint constructor', 'fn rogue() { conn.savepoint(); }'],
  ['savepoint type constructor', 'fn rogue() { rusqlite::Savepoint::new(conn); }'],
  ['named savepoint constructor', 'fn rogue() { rusqlite::Savepoint::with_name(conn, "nested"); }'],
  ['savepoint type alias', 'use rusqlite::Savepoint as Point; fn rogue() { Point::new(conn); }'],
  ['raw BEGIN', 'fn rogue() { conn.execute_batch("BEGIN IMMEDIATE; SELECT 1; COMMIT"); }'],
  ['escaped BEGIN', String.raw`fn rogue() { conn.execute_batch("BEGIN\nTRANSACTION;"); }`],
  ['unicode escaped BEGIN', String.raw`fn rogue() { conn.execute_batch("\u{42}EGIN IMMEDIATE;"); }`],
  ['commented SQL BEGIN', 'fn rogue() { conn.execute_batch(r#"/* start */ BEGIN;"#); }'],
  ['later statement BEGIN', 'fn rogue() { conn.execute_batch("SELECT 1; BEGIN EXCLUSIVE;"); }'],
  ['quoted SQL line comment before BEGIN', `fn rogue() { conn.execute_batch("SELECT '--'; BEGIN IMMEDIATE; SELECT 1; COMMIT"); }`],
  ['quoted SQL block comment before BEGIN', `fn rogue() { conn.execute_batch("SELECT '/*'; BEGIN; SELECT '*/'; COMMIT"); }`],
  ['escaped SQL quote before BEGIN', `fn rogue() { conn.execute_batch("SELECT 'it''s --'; BEGIN IMMEDIATE;"); }`],
  ['quoted SQL identifier before BEGIN', String.raw`fn rogue() { conn.execute_batch("SELECT \"--\"; BEGIN;"); }`],
  ['quoted SAVEPOINT name', String.raw`fn rogue() { conn.execute_batch("SAVEPOINT \"--\";"); }`],
  ['raw SAVEPOINT', 'fn rogue() { conn.execute_batch("SAVEPOINT isolated;"); }'],
];
for (const [label, source] of txCases) {
  test(`transaction guard rejects ${label}`, () => {
    const result = run('check-store-encapsulation.sh', 'store/rogue.rs', source);
    assert.equal(result.status, 1, result.output);
    assert.match(result.output, /store\/rogue\.rs/);
  });
}
for (const [path, name, method] of [
  ['store/mod.rs', 'with_immediate_tx_mapped', 'transaction_with_behavior(mode)'],
  ['store/mod.rs', 'with_deferred_read', 'transaction_with_behavior(mode)'],
  ['store/migrations.rs', 'apply_single_migration_inner', 'transaction()'],
  ['store/migrations.rs', 'revert_versioned_migrations_for_test', 'transaction()'],
  ['store/learned_artifacts.rs', 'migrate_provenance_column', 'unchecked_transaction()'],
]) {
  test(`transaction guard permits exact existing boundary ${name}`, () => {
    const result = run('check-store-encapsulation.sh', path, `fn ${name}() { conn.${method}; }`);
    assert.equal(result.status, 0, result.output);
  });
  test(`transaction guard rejects another function in ${path} (${name})`, () => {
    const result = run('check-store-encapsulation.sh', path, `fn ${name}() {} fn rogue() { conn.${method}; }`);
    assert.equal(result.status, 1, result.output);
    assert.ok(result.output.includes(path), result.output);
  });
}
test('transaction guard ignores documentation and non-transaction SQL', () => {
  const result = run('check-store-encapsulation.sh', 'store/read.rs',
    '// conn.transaction() and BEGIN IMMEDIATE\nfn read() { conn.execute_batch("SELECT 1"); }');
  assert.equal(result.status, 0, result.output);
});
test('transaction guard ignores a direct expect diagnostic', () => {
  const result = run('check-store-encapsulation.sh', 'store/read.rs',
    'fn read() { operation().expect("begin"); operation().expect_err("begin"); }');
  assert.equal(result.status, 0, result.output);
});
test('transaction guard ignores transaction words inside SQL quoted values and comments', () => {
  const result = run('check-store-encapsulation.sh', 'store/read.rs',
    `fn read() { conn.execute_batch("SELECT '; BEGIN;', '--', '/*'; -- BEGIN;\\n/* SAVEPOINT x; */ SELECT 1;"); }`);
  assert.equal(result.status, 0, result.output);
});

const mintCases = [
  ['direct', 'fn rogue() { OwnerVerifiedProof::mint(); }'],
  ['import alias', 'use crate::identity::OwnerVerifiedProof as Proof; fn rogue() { Proof::mint(); }'],
  ['type alias', 'type Proof = crate::identity::OwnerVerifiedProof; fn rogue() { Proof::mint(); }'],
  ['function pointer', 'fn rogue() { let mint = OwnerVerifiedProof::mint; mint(); }'],
  ['qualified alias', 'fn rogue() { <Proof>::mint(); }'],
  ['raw identifier', 'fn rogue() { Proof::r#mint(); }'],
  ['comment and newline', 'fn rogue() { Proof::/* gap */\n mint(); }'],
  ['Self inside impl', 'impl Proof { fn rogue() { Self::mint(); } }'],
];
for (const [label, source] of mintCases) {
  test(`owner proof guard rejects ${label}`, () => {
    const result = run('check-owner-proof-mint.sh', 'rogue.rs', source);
    assert.equal(result.status, 1, result.output);
    assert.match(result.output, /rogue\.rs/);
  });
}
for (const path of ['telegram.rs', 'nested/telegram.rs']) {
  test(`owner proof exact adapter path ${path}`, () => {
    const result = run('check-owner-proof-mint.sh', path, 'fn verified() { Proof::mint(); }');
    assert.equal(result.status, path === 'telegram.rs' ? 0 : 1, result.output);
  });
}
test('owner proof guard allows tests and ignores prose', () => {
  const result = run('check-owner-proof-mint.sh', 'identity.rs',
    '// OwnerVerifiedProof::mint()\nfn test_new() { Proof::test_new(); let text = "Proof::mint()"; }');
  assert.equal(result.status, 0, result.output);
});
