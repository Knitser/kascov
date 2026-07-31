/* The holder bot's security rests on three pure things: the phrase binds both
   the account and the address, Discord's signature is actually checked, and a
   challenge expires. All three are testable without a network. */
import test from 'node:test';
import assert from 'node:assert/strict';
import { generateKeyPairSync, sign as edSign } from 'node:crypto';

import {
  balanceFor, challengeIsUsable, challengePhrase, verifyDiscordSignature,
} from '../scripts/discord-holder-bot.mjs';

const ADDR = 'kaspa:qzlxrxar9w4z2n5ynmr8n356hawma24k44dedxfuvqfyn0dprn2egpyf9wul9';

test('the phrase binds the Discord account AND the address', () => {
  const a = challengePhrase('111', ADDR, 'abcd');
  // change either half and it is a different phrase, so a signature over one
  // cannot be replayed for the other
  assert.notEqual(a, challengePhrase('222', ADDR, 'abcd'));
  assert.notEqual(a, challengePhrase('111', 'kaspa:qother', 'abcd'));
  assert.notEqual(a, challengePhrase('111', ADDR, 'dcba'));
  assert.match(a, /^kascov verify: 111 kaspa:.+ abcd$/);
});

/* ---------------------------------------------------- discord signatures */

const ed = () => generateKeyPairSync('ed25519');
const rawPub = (pub) => pub.export({ format: 'der', type: 'spki' }).subarray(12).toString('hex');
const signIt = (priv, ts, body) => edSign(null, Buffer.from(ts + body), priv).toString('hex');

test('a genuine Discord signature verifies', () => {
  const { publicKey, privateKey } = ed();
  const ts = '1700000000';
  const body = '{"type":1}';
  assert.ok(verifyDiscordSignature(body, signIt(privateKey, ts, body), ts, rawPub(publicKey)));
});

test('a tampered body is rejected', () => {
  const { publicKey, privateKey } = ed();
  const ts = '1700000000';
  const sig = signIt(privateKey, ts, '{"type":1}');
  assert.ok(!verifyDiscordSignature('{"type":2}', sig, ts, rawPub(publicKey)));
});

test('a replayed signature under a different timestamp is rejected', () => {
  const { publicKey, privateKey } = ed();
  const body = '{"type":1}';
  const sig = signIt(privateKey, '1700000000', body);
  assert.ok(!verifyDiscordSignature(body, sig, '1700000001', rawPub(publicKey)));
});

test('another key cannot forge an interaction', () => {
  const a = ed();
  const b = ed();
  const ts = '1700000000';
  const body = '{"type":1}';
  assert.ok(!verifyDiscordSignature(body, signIt(b.privateKey, ts, body), ts, rawPub(a.publicKey)));
});

test('a body with emoji verifies, even split across chunks', () => {
  // The bug this pins: a real interaction carries the channel name, and this
  // server's channel is "✅ | verify-holdings". Rebuilding the body by
  // concatenating DECODED chunks corrupts any multi-byte character that lands
  // on a chunk boundary, so the signature check fails and Discord reports
  // "the application didn't respond". A PING is pure ASCII and never shows it.
  const { publicKey, privateKey } = ed();
  const ts = '1700000000';
  const body = Buffer.from(JSON.stringify({
    type: 2,
    channel: { name: '✅ | verify-holdings' },
    member: { user: { global_name: '0xKnitser 🐉' } },
  }), 'utf8');
  const sig = edSign(null, Buffer.concat([Buffer.from(ts), body]), privateKey).toString('hex');

  assert.ok(verifyDiscordSignature(body, sig, ts, rawPub(publicKey)));

  // Now split mid-character, the way a TCP chunk boundary would, and prove
  // the byte-preserving path still verifies while naive concatenation cannot.
  const cut = body.indexOf(Buffer.from('✅', 'utf8')) + 1;
  const chunks = [body.subarray(0, cut), body.subarray(cut)];
  assert.ok(verifyDiscordSignature(Buffer.concat(chunks), sig, ts, rawPub(publicKey)));
  const naive = chunks.map((c) => c.toString()).join('');
  assert.ok(!verifyDiscordSignature(naive, sig, ts, rawPub(publicKey)));
});

test('garbage never throws, it just fails', () => {
  const { publicKey } = ed();
  const pub = rawPub(publicKey);
  // Discord probes with deliberately bad signatures when saving the endpoint;
  // every one of these must be a quiet false, never a crash.
  assert.ok(!verifyDiscordSignature('{}', 'not-hex', '1', pub));
  assert.ok(!verifyDiscordSignature('{}', 'ab', '1', pub));
  assert.ok(!verifyDiscordSignature('{}', 'a'.repeat(128), '1', pub));
  assert.ok(!verifyDiscordSignature('{}', 'a'.repeat(128), '1', 'short-key'));
  assert.ok(!verifyDiscordSignature('{}', '', '', ''));
});

/* --------------------------------------------------------- challenges */

test('a challenge expires, and belongs to exactly one account', () => {
  const now = 1_000_000_000;
  const fresh = { user: '111', address: ADDR, nonce: 'x', issued_ms: now };
  assert.ok(challengeIsUsable(fresh, '111', now + 60_000));
  assert.ok(!challengeIsUsable(fresh, '222', now + 60_000));        // not yours
  assert.ok(!challengeIsUsable(fresh, '111', now + 16 * 60_000));   // too old
  assert.ok(!challengeIsUsable(undefined, '111', now));
  assert.ok(!challengeIsUsable(null, '111', now));
});

/* ------------------------------------------------------------ balances */

const HOLDINGS = [
  { token_id: 'aa', balance: 100 },
  { token_id: 'bb', balance: 250 },
  { token_id: 'aa', balance: 5 },
];

test('the role counts only its own token, across every cell', () => {
  assert.equal(balanceFor(HOLDINGS, 'aa'), 105);
  assert.equal(balanceFor(HOLDINGS, 'bb'), 250);
  assert.equal(balanceFor(HOLDINGS, 'cc'), 0);
});

test('no token configured means any holding counts', () => {
  assert.equal(balanceFor(HOLDINGS, ''), 355);
});

test('a missing or malformed holdings list is zero, never a crash', () => {
  assert.equal(balanceFor(undefined, 'aa'), 0);
  assert.equal(balanceFor(null, 'aa'), 0);
  assert.equal(balanceFor([], 'aa'), 0);
  assert.equal(balanceFor([{ token_id: 'aa' }], 'aa'), 0);
});

/* ------------------------------------------- the signing page's completion */

test('a nonce finds exactly its own challenge', async () => {
  const { pendingByNonce } = await import('../scripts/discord-holder-bot.mjs');
  const pending = {
    '111': { user: '111', address: ADDR, nonce: 'aaaa', issued_ms: 1 },
    '222': { user: '222', address: 'kaspa:qother', nonce: 'bbbb', issued_ms: 1 },
  };
  assert.equal(pendingByNonce(pending, 'aaaa').userId, '111');
  assert.equal(pendingByNonce(pending, 'bbbb').userId, '222');
  // A wrong or absent nonce must resolve to nothing rather than the first entry
  assert.equal(pendingByNonce(pending, 'cccc'), null);
  assert.equal(pendingByNonce(pending, ''), null);
  assert.equal(pendingByNonce(pending, undefined), null);
  assert.equal(pendingByNonce({}, 'aaaa'), null);
});
