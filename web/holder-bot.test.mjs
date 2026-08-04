/* The holder bot's security rests on pure things: the phrase binds both
   the account and the address, Discord's signature is actually checked, a
   challenge expires, a ballot is one per holder, and a DM fires only on a
   change the holder agreed to hear about. All testable without a network. */
import test from 'node:test';
import assert from 'node:assert/strict';
import { generateKeyPairSync, sign as edSign } from 'node:crypto';

import {
  balanceFor, challengeIsUsable, challengePhrase, verifyDiscordSignature,
  leftTheServer,
  VOTE_DUST_FLOOR, VOTE_MAX_AGE_MS, voteEligibility, roundIsOpen, castBallot,
  tallyCounts, buildTally, isOperator, parseSlate,
  balanceBucket, alertsEnabled, cursorDiff, deliverableAlerts,
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

/* -------------------------------------------------- leave-server cleanup */

test('only an explicit 404 means someone left', () => {
  assert.ok(leftTheServer(404));
  // everything else proves nothing about the member, and a lookup that
  // failed must never delete anyone's record.
  assert.ok(!leftTheServer(200));
  assert.ok(!leftTheServer(403));
  assert.ok(!leftTheServer(429));
  assert.ok(!leftTheServer(500));
  assert.ok(!leftTheServer(undefined));
  assert.ok(!leftTheServer(null));
});

/* ---------------------------------------------------------- the audit vote */

test('a holder below the dust floor cannot vote, and the floor is >=', () => {
  assert.ok(!voteEligibility(undefined).ok);                       // not verified
  assert.ok(!voteEligibility(null).ok);
  const below = voteEligibility({ balance: VOTE_DUST_FLOOR - 1 });
  assert.ok(!below.ok);
  assert.match(below.reason, /100/);                               // says the floor
  assert.ok(voteEligibility({ balance: VOTE_DUST_FLOOR }).ok);     // exactly at it
  assert.ok(voteEligibility({ balance: 5000 }).ok);
});

const openRound = () => ({
  id: 'round-1', status: 'open', options: ['token-a', 'token-b', 'token-c'],
  opened_ms: 0, closes_ms: VOTE_MAX_AGE_MS, closed_ms: null, ballots: {},
});

test('a second ballot replaces the first, never duplicates', () => {
  const round = openRound();
  const one = castBallot(round, 'u1', 1, 10);
  assert.ok(one.ok);
  assert.equal(one.label, 'token-a');
  const two = castBallot({ ...round, ballots: one.ballots }, 'u1', 3, 20);
  assert.ok(two.ok);
  // still exactly one ballot, and it is the LATER one
  assert.deepEqual(two.ballots, { u1: 2 });
  assert.deepEqual(tallyCounts({ ...round, ballots: two.ballots }), [0, 0, 1]);
});

test('a closed or expired round rejects ballots', () => {
  const round = openRound();
  assert.ok(!castBallot({ ...round, status: 'closed' }, 'u1', 1, 10).ok);
  // five days ran out: the clock closes it even if no operator ever did
  assert.ok(!castBallot(round, 'u1', 1, VOTE_MAX_AGE_MS).ok);
  assert.ok(castBallot(round, 'u1', 1, VOTE_MAX_AGE_MS - 1).ok);
  // and a choice off the slate is refused, open or not
  assert.ok(!castBallot(round, 'u1', 4, 10).ok);
  assert.ok(!castBallot(round, 'u1', 0, 10).ok);
  assert.ok(!castBallot(round, 'u1', 'x', 10).ok);
});

test('the public tally counts ballots and names nobody', () => {
  const round = { ...openRound(), ballots: { 111222333: 0, 444555666: 0, 777888999: 1 } };
  const tally = buildTally(round, 10);
  assert.equal(tally.round, 'round-1');
  assert.equal(tally.status, 'open');
  assert.deepEqual(tally.options, [
    { label: 'token-a', votes: 2 },
    { label: 'token-b', votes: 1 },
    { label: 'token-c', votes: 0 },
  ]);
  assert.equal(tally.total_ballots, 3);
  // the rules ride with the counts, so the file cannot be quoted without them
  assert.match(JSON.stringify(tally.rules), /never decides a verdict/);
  assert.match(JSON.stringify(tally.rules), /at least 100 KASCOV/);
  assert.match(JSON.stringify(tally.rules), /audit anything at any time/);
  // and no voter identity survives into the file
  const json = JSON.stringify(tally);
  assert.ok(!json.includes('111222333'));
  assert.ok(!json.includes('"ballots"'));
});

test('a frozen round keeps its counts after the ballots are gone', () => {
  const round = { ...openRound(), status: 'closed', closed_ms: 50, counts: [2, 1, 0] };
  delete round.ballots;
  assert.deepEqual(tallyCounts(round), [2, 1, 0]);
  const tally = buildTally(round, 60);
  assert.equal(tally.status, 'closed');
  assert.equal(tally.options[0].votes, 2);
  assert.ok(tally.closed);
});

test('the operator gate never opens on empty ids', () => {
  assert.ok(isOperator('1', '1', ''));       // the guild owner
  assert.ok(isOperator('2', '1', '2'));      // the explicit operator env
  assert.ok(!isOperator('3', '1', '2'));     // anyone else
  assert.ok(!isOperator('', '', ''));        // absent ids authorize nobody
  assert.ok(!isOperator(undefined, undefined, ''));
});

test('a slate is 2 to 6 trimmed labels, or nothing', () => {
  assert.deepEqual(parseSlate('a | b |c'), ['a', 'b', 'c']);
  assert.deepEqual(parseSlate(' a |  | b '), ['a', 'b']); // empties dropped
  assert.equal(parseSlate('only-one'), null);
  assert.equal(parseSlate('a|b|c|d|e|f|g'), null);
  assert.equal(parseSlate(''), null);
  assert.equal(parseSlate(undefined), null);
});

/* ------------------------------------------------------------ the watchtower */

test('the balance bucket has exactly three rooms', () => {
  assert.equal(balanceBucket(0), 'zero');
  assert.equal(balanceBucket(-5), 'zero');
  assert.equal(balanceBucket(undefined), 'zero');
  assert.equal(balanceBucket(1), 'dust');
  assert.equal(balanceBucket(VOTE_DUST_FLOOR - 1), 'dust');
  assert.equal(balanceBucket(VOTE_DUST_FLOOR), 'above');
});

test('records from before the watchtower default to OFF; new ones say ON explicitly', () => {
  assert.equal(alertsEnabled({ alerts: true }), true);
  assert.equal(alertsEnabled({}), false);            // pre-watchtower record
  assert.equal(alertsEnabled({ alerts: false }), false);
  assert.equal(alertsEnabled(undefined), false);
});

const BEFORE = { phases: { aa: 'bonding', bb: 'bonding' }, bucket: 'above' };
const AFTER = { phases: { aa: 'graduated', bb: 'bonding' }, bucket: 'above' };

test('a phase change fires once, and only once', () => {
  const rec = { alerts: true, cursor: BEFORE };
  const fired = deliverableAlerts(rec, AFTER);
  assert.deepEqual(fired, [{ kind: 'phase', token_id: 'aa', from: 'bonding', to: 'graduated' }]);
  // once the cursor moves, the same snapshot is silence
  assert.deepEqual(deliverableAlerts({ alerts: true, cursor: AFTER }, AFTER), []);
});

test('no change fires nothing, including the very first snapshot', () => {
  assert.deepEqual(deliverableAlerts({ alerts: true, cursor: BEFORE }, BEFORE), []);
  // no cursor yet: nothing to differ from, so standing it up messages nobody
  assert.deepEqual(deliverableAlerts({ alerts: true }, AFTER), []);
});

test('crossing zero or the dust floor is one alert', () => {
  const cross = deliverableAlerts(
    { alerts: true, cursor: { phases: {}, bucket: 'dust' } },
    { phases: {}, bucket: 'zero' },
  );
  assert.deepEqual(cross, [{ kind: 'balance', from: 'dust', to: 'zero' }]);
  assert.deepEqual(cursorDiff({ phases: {}, bucket: 'dust' }, { phases: {}, bucket: 'above' }),
    [{ kind: 'balance', from: 'dust', to: 'above' }]);
});

test('an unreachable holder stays silent', () => {
  assert.deepEqual(deliverableAlerts({ alerts: true, unreachable: true, cursor: BEFORE }, AFTER), []);
});

test('opting out suppresses everything', () => {
  assert.deepEqual(deliverableAlerts({ alerts: false, cursor: BEFORE }, AFTER), []);
  assert.deepEqual(deliverableAlerts({ cursor: BEFORE }, AFTER), []); // never opted in
});
