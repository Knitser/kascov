// The WebAssembly build, end to end, on the same real chain bytes as the
// command's own tests (crates/kascov-decode/tests/cli.rs). Build the module
// first:
//
//   cargo build -p kascov-decode-wasm --target wasm32-unknown-unknown --release
//   node --test crates/kascov-decode-wasm/js/kascov-decode.test.mjs
//
// With KASCOV_DECODE_BIN set to a built kascov-decode command, it also checks
// that the module and the command give the same answer for every program
// fixture in the crate.

import { test } from 'node:test';
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { readFile, readdir } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import { join } from 'node:path';
import { load } from './kascov-decode.mjs';

const here = (rel) => fileURLToPath(new URL(rel, import.meta.url));
const WASM = process.env.KASCOV_DECODE_WASM
  ?? here('../../../target/wasm32-unknown-unknown/release/kascov_decode_wasm.wasm');
const FIXTURES = here('../../kascov-decode/fixtures/');
const hexOf = async (rel) => (await readFile(join(FIXTURES, rel))).toString('hex');

const PARENT_HASH = '7cec521a9350d30408dae91466aeedc31fa0f34b43d30d6d23c06f650f333820';
const kd = await load(await readFile(WASM));
const parent = await hexOf('recovery/kcom_2671_parent_56fc521e_0.bin');
const witness = await hexOf('recovery/kcom_witness_7d5f35f0.bin');

test('name reads a program', () => {
  const v = kd.name(parent);
  assert.equal(v.bytes, 2671);
  assert.equal(v.blake2b_256, PARENT_HASH);
  assert.equal(v.p2sh_script, `aa20${PARENT_HASH}87`);
  assert.equal(v.template, 'KaspaCom · token');
});

test('hash takes a prefix and whitespace', () => {
  const half = parent.length / 2;
  const v = kd.hash(`0x${parent.slice(0, half)}\n  ${parent.slice(half)}\n`);
  assert.equal(v.blake2b_256, PARENT_HASH);
});

test('check confirms the program a real spend revealed', () => {
  const v = kd.check(witness, `aa20${PARENT_HASH}87`);
  assert.equal(v.spk_matches, true);
  assert.equal(v.push_only, true);
  assert.equal(v.program.template, 'KaspaCom · token');
  assert.equal(kd.check(witness).spk_matches, null);
});

test('check refuses a program the output did not commit to', () => {
  assert.equal(kd.check(witness, `aa20${'00'.repeat(32)}87`).spk_matches, false);
});

test('bad input throws instead of trapping, and the module keeps working', () => {
  assert.throws(() => kd.name('zz'), /the program is not hex/);
  assert.throws(() => kd.name(''), /the program is empty/);
  assert.throws(() => kd.check(witness, '51'), /not P2SH/);
  assert.equal(kd.hash(parent).bytes, 2671);
});

test('disasm lists every instruction', () => {
  const lines = kd.disasm(parent).split('\n');
  assert.match(lines[0], /^2671 bytes, \d+ instructions$/);
  assert.match(lines[1], /^0000  /);
});

test('many calls free what they allocate', () => {
  const before = kd.name(parent);
  for (let i = 0; i < 300; i++) kd.check(witness, `aa20${PARENT_HASH}87`);
  assert.deepEqual(kd.name(parent), before);
});

test('the module and the command agree on every program fixture', { skip: !process.env.KASCOV_DECODE_BIN }, async () => {
  const files = (await readdir(FIXTURES, { recursive: true })).filter((f) => f.endsWith('.bin')).sort();
  assert.ok(files.length > 250, `only ${files.length} fixtures found`);
  for (const f of files) {
    const hex = await hexOf(f);
    const cli = JSON.parse(execFileSync(process.env.KASCOV_DECODE_BIN, ['name', '-'], { input: hex, maxBuffer: 64 << 20 }));
    assert.deepEqual(kd.name(hex), cli, f);
  }
});
