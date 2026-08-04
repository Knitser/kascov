#!/usr/bin/env node
/* kascov holder verification for Discord — zero dependencies.
 *
 * An HTTP interactions endpoint rather than a gateway bot: Discord POSTs here,
 * so there is no socket to babysit, no reconnect logic, and nothing to restart
 * when the network hiccups. Node verifies Ed25519 natively, so this stays
 * dependency-free like the other two bots.
 *
 * THE FLOW
 *   /verify      opens a FORM for the address, then issues a one-time phrase
 *                bound to the Discord user id AND that address, ephemerally.
 *   /confirm     opens a FORM for the signature, checks it via kascov's own
 *                /prove-holding, and grants the role only if the proven
 *                balance is non-zero.
 *   /holdings    public lookup, no proof, no role.
 *   /unverify    drops the role and deletes the record.
 *   /vote        the audit vote: verified holders at or above the dust floor
 *                pick what gets audited next. the tally is published as
 *                counts only, never identities.
 *   /vote-open   operator only: open a round with a slate of 2-6 options.
 *   /vote-close  operator only: close the round and freeze the counts.
 *   /alerts      watchtower DMs on or off.
 *
 *   The 6h recheck keeps two more promises now. A member who LEFT the server
 *   is forgotten exactly as /unverify would forget them, because bot.html
 *   says leaving has that effect and a published sentence is a debt. And the
 *   watchtower DMs holders about changes derived ONLY from already-public
 *   endpoints, so a DM never carries a fact kascov.io did not already show.
 *
 *   The two sensitive commands take no OPTIONS on purpose. An option's value is
 *   echoed back in the command invocation, and the link between a Discord
 *   account and a Kaspa address is the one new fact this bot creates. It is not
 *   the bot's to announce, so it is typed into a form instead.
 *
 * WHAT MAKES THE PROOF WORTH ANYTHING
 *   The nonce is issued HERE and remembered HERE. kascov's endpoint is
 *   deliberately stateless: it answers "did this key sign this string, and what
 *   does it hold". Replaying somebody else's captured proof against it returns
 *   the same public fact it always did and grants nothing, because the thing
 *   that actually decides is this file's record of which nonce went to whom.
 *
 * Setup (never commit these): /home/kascov/.discord-bot-env, chmod 600
 *   DISCORD_APP_ID, DISCORD_PUBLIC_KEY, DISCORD_BOT_TOKEN
 *
 * Run:      node scripts/discord-holder-bot.mjs            # serve
 *           node scripts/discord-holder-bot.mjs --register # register commands
 *           node scripts/discord-holder-bot.mjs --recheck  # re-prove everyone
 */

import { createServer } from 'node:http';
import { createPublicKey, verify as edVerify, randomBytes } from 'node:crypto';
import { readFile, writeFile, rename } from 'node:fs/promises';

const APP_ID = process.env.DISCORD_APP_ID || '';
const PUBLIC_KEY = process.env.DISCORD_PUBLIC_KEY || '';
const BOT_TOKEN = process.env.DISCORD_BOT_TOKEN || '';
const PORT = Number(process.env.KASCOV_BOT_PORT || 8099);
const STATE_PATH = process.env.KASCOV_BOT_STATE || '/home/kascov/kascov-holder-bot.json';
/* Talk to the worker directly. Going out through kascov.io and back would put
   Caddy, TLS and the public internet inside a loop that starts and ends on this
   same box, and would break the moment DNS did. */
const KASCOV = process.env.KASCOV_API || 'http://127.0.0.1:8080';
const NETWORK = process.env.KASCOV_NETWORK || 'mainnet';
/* Public origin, for links a member clicks. Distinct from KASCOV above, which
   is the loopback the service itself calls. */
const SITE = process.env.KASCOV_SITE || 'https://kascov.io';
const ROLE_NAME = process.env.KASCOV_ROLE_NAME || 'verified holder';
/* The coin the role is about. Empty means "any verified token". */
const TOKEN_ID = process.env.KASCOV_ROLE_TOKEN
  || 'c58c826d0aa9cee62f93208718c674883f5c89a8aca4933dc41fb0391539abe2';

/* Where the public audit-vote tally lands. Point it inside the web root so
   Caddy serves it as /vote-tally.json. Unwritable is a logged no-op: a stale
   tally page must never block a ballot. */
/* the public tally must land somewhere the web server actually serves;
   the deploy points this at the web root (vote.html fetches /vote-tally.json) */
const TALLY_PATH = process.env.KASCOV_VOTE_TALLY || './vote-tally.json';
/* A second operator besides the guild owner, for the day the owner account is
   not the one at the keyboard. Empty means the owner alone, never everyone. */
const OPERATOR_ID = process.env.OPERATOR_USER_ID || '';

const CHALLENGE_TTL_MS = 15 * 60 * 1000;
const API = 'https://discord.com/api/v10';
const EPHEMERAL = 64;

/* ------------------------------------------------------------- pure bits */

/** The phrase a wallet signs. Binds the Discord account AND the address, so a
 *  proof issued for one person cannot be replayed by another, and a proof for
 *  one address cannot be pointed at a different one. */
export function challengePhrase(discordUserId, address, nonce) {
  return `kascov verify: ${discordUserId} ${address} ${nonce}`;
}

/** Discord signs every request. Anyone can POST to a public URL, so this is the
 *  only thing separating a real interaction from a stranger with curl.
 *
 *  `rawBody` MUST be the exact bytes Discord sent. Accepts a Buffer for that
 *  reason: rebuilding the body by concatenating decoded chunks corrupts any
 *  multi-byte character that happens to straddle a chunk boundary, and the
 *  signature is over bytes, not over characters. A PING is pure ASCII and
 *  survives that; a real interaction carries channel names and display names
 *  full of emoji, and does not. */
export function verifyDiscordSignature(rawBody, signature, timestamp, publicKeyHex) {
  try {
    if (!/^[0-9a-f]{128}$/i.test(signature) || !/^[0-9a-f]{64}$/i.test(publicKeyHex)) return false;
    // Node needs SPKI DER; Discord gives a raw 32-byte ed25519 key.
    const key = createPublicKey({
      key: Buffer.concat([
        Buffer.from('302a300506032b6570032100', 'hex'),
        Buffer.from(publicKeyHex, 'hex'),
      ]),
      format: 'der',
      type: 'spki',
    });
    const body = Buffer.isBuffer(rawBody) ? rawBody : Buffer.from(rawBody, 'utf8');
    const msg = Buffer.concat([Buffer.from(String(timestamp), 'utf8'), body]);
    return edVerify(null, msg, key, Buffer.from(signature, 'hex'));
  } catch {
    return false;
  }
}

/** A challenge is only good for one account, one address, and fifteen minutes. */
export function challengeIsUsable(pending, discordUserId, nowMs) {
  if (!pending) return false;
  if (pending.user !== discordUserId) return false;
  return nowMs - Number(pending.issued_ms || 0) < CHALLENGE_TTL_MS;
}

/** How much of the role's token this proof shows. Absent token = any holding. */
export function balanceFor(holdings, tokenId) {
  if (!Array.isArray(holdings)) return 0;
  const rows = tokenId ? holdings.filter((h) => h.token_id === tokenId) : holdings;
  return rows.reduce((n, h) => n + Number(h.balance || 0), 0);
}

export const fmt = (n) => Number(n || 0).toLocaleString('en-US');

/* ------------------------------------------------- leave-server cleanup */

/** Only an explicit 404 from the members endpoint proves departure. A 403, a
 *  500, or a network failure proves nothing about the member, and a lookup
 *  that failed must never delete anyone's record. */
export function leftTheServer(status) {
  return status === 404;
}

/* ----------------------------------------------------------- audit vote */

/* The dust floor is POLICY, not physics: 100 KASCOV keeps a wallet of dust
   from being a ballot. Adjustable by commit only, and the number published on
   kascov.io/bot and /vote must follow this constant, never lead it. */
export const VOTE_DUST_FLOOR = 100;
/* A round the operator forgets is not a round that runs forever. */
export const VOTE_MAX_AGE_MS = 5 * 24 * 60 * 60 * 1000;

/* Shipped inside every tally file, so the counts can never be quoted without
   their own limits attached. */
export const VOTE_RULES = [
  'one verified holder, one vote per round; voting again before close replaces the earlier ballot',
  `voting needs a proven balance of at least ${VOTE_DUST_FLOOR} KASCOV, a dust floor that is policy and changes only by commit`,
  'the vote accelerates the audit queue; it never decides a verdict',
  'kascov may audit anything at any time, voted for or not',
];

/** Whether this record may vote. The floor is compared against the balance
 *  the last proof or recheck actually SHOWED, never against a claim. */
export function voteEligibility(record, floor = VOTE_DUST_FLOOR) {
  if (!record) return { ok: false, reason: 'you are not verified. Run `/verify` first.' };
  const balance = Number(record.balance || 0);
  if (balance < floor) {
    return {
      ok: false,
      reason: `the audit vote needs at least ${fmt(floor)} proven $KASCOV and your last proven balance is ${fmt(balance)}.`,
    };
  }
  return { ok: true };
}

/** A round is closed when the operator closed it OR its five days ran out,
 *  whichever comes first. Expiry needs no timer: it is a fact about the
 *  clock, so a ballot after the deadline bounces even if nothing ran. */
export function roundIsOpen(round, nowMs) {
  if (!round || round.status !== 'open') return false;
  return nowMs < Number(round.closes_ms || 0);
}

/** Cast or replace one ballot. Pure: returns a new ballots map, never
 *  mutating its input. One-holder-one-vote is a map key, so a second ballot
 *  from the same holder REPLACES the first by construction. */
export function castBallot(round, userId, choice, nowMs) {
  if (!roundIsOpen(round, nowMs)) {
    return { ok: false, reason: 'this round is closed. Ballots only count while a round is open.' };
  }
  const idx = Number(choice) - 1;
  if (!Number.isInteger(idx) || idx < 0 || idx >= round.options.length) {
    return { ok: false, reason: `pick a number between 1 and ${round.options.length}.` };
  }
  return { ok: true, ballots: { ...round.ballots, [userId]: idx }, label: round.options[idx] };
}

/** Counts per option. An open round counts live ballots; a closed round
 *  carries counts frozen at close, because the ballots themselves are
 *  deleted the moment they stop being needed. */
export function tallyCounts(round) {
  if (Array.isArray(round.counts)) return round.counts;
  const counts = round.options.map(() => 0);
  for (const idx of Object.values(round.ballots || {})) {
    if (Number.isInteger(idx) && idx >= 0 && idx < counts.length) counts[idx] += 1;
  }
  return counts;
}

/** The public tally. Counts only, NEVER voter identities: the file is
 *  world-readable and who voted for what is nobody's business but theirs. */
export function buildTally(round, nowMs) {
  const counts = tallyCounts(round);
  return {
    round: round.id,
    status: roundIsOpen(round, nowMs) ? 'open' : 'closed',
    opened: new Date(Number(round.opened_ms)).toISOString(),
    closes: new Date(Number(round.closes_ms)).toISOString(),
    closed: round.closed_ms ? new Date(Number(round.closed_ms)).toISOString() : null,
    options: round.options.map((label, i) => ({ label, votes: counts[i] })),
    total_ballots: counts.reduce((a, b) => a + b, 0),
    rules: VOTE_RULES,
  };
}

/** The operator gate: the guild owner, or the explicit OPERATOR_USER_ID.
 *  Empty ids never authorize anyone. */
export function isOperator(userId, ownerId, operatorId) {
  if (!userId) return false;
  return userId === ownerId || (Boolean(operatorId) && userId === operatorId);
}

/** A slate is 2 to 6 labels separated by `|`. Anything else is refused
 *  rather than guessed at. */
export function parseSlate(text) {
  const labels = String(text || '').split('|').map((s) => s.trim()).filter(Boolean);
  if (labels.length < 2 || labels.length > 6) return null;
  return labels;
}

/* ----------------------------------------------------------- watchtower */

/** Which side of the two lines a balance sits on. The lines are the only
 *  thing worth a DM: exact numbers wobble with every trade, buckets do not. */
export function balanceBucket(balance, floor = VOTE_DUST_FLOOR) {
  const b = Number(balance || 0);
  if (b <= 0) return 'zero';
  return b < floor ? 'dust' : 'above';
}

/** Alerts opt-in. An ABSENT preference means the record predates the
 *  watchtower, and silence is the only polite default for someone who never
 *  agreed to DMs. New verifications write an explicit true. */
export function alertsEnabled(record) {
  return record?.alerts === true;
}

/** Diff two cursors into the alerts worth sending. Fires only on CHANGE: a
 *  first snapshot has nothing to differ from, so standing the watchtower up
 *  messages nobody. */
export function cursorDiff(prev, next) {
  const alerts = [];
  const before = prev?.phases || {};
  for (const [tokenId, phase] of Object.entries(next?.phases || {})) {
    if (before[tokenId] && phase && before[tokenId] !== phase) {
      alerts.push({ kind: 'phase', token_id: tokenId, from: before[tokenId], to: phase });
    }
  }
  if (prev?.bucket && next?.bucket && prev.bucket !== next.bucket) {
    alerts.push({ kind: 'balance', from: prev.bucket, to: next.bucket });
  }
  /* RESERVED: claim events. When a public claims endpoint exists, the cursor
     grows a `claims` field and this function diffs it exactly like phases.
     Deliberately NO live code path until then: an alert kind that cannot be
     derived from a public endpoint is not allowed to exist here. */
  return alerts;
}

/** The delivery gate. Opt-out and unreachability are decided HERE so every
 *  caller inherits them: a holder who said no, or whose DMs bounced, gets
 *  silence no matter what changed. */
export function deliverableAlerts(record, next) {
  if (!alertsEnabled(record) || record?.unreachable) return [];
  return cursorDiff(record?.cursor, next);
}

/** One alert as one DM line. Every line cites its public page, because a DM
 *  that cannot point at the free site has no business being sent. */
export function alertMessage(alert, names = {}) {
  if (alert.kind === 'phase') {
    const name = names[alert.token_id] || `${String(alert.token_id).slice(0, 8)}…`;
    return `**${name}** moved from ${alert.from} to ${alert.to}: ${SITE}/#/${NETWORK}/token/${alert.token_id}`;
  }
  if (alert.kind === 'balance') {
    const words = {
      zero: 'zero',
      dust: `below the ${fmt(VOTE_DUST_FLOOR)} dust floor`,
      above: `at or above the ${fmt(VOTE_DUST_FLOOR)} floor`,
    };
    return `Your proven $KASCOV balance moved from ${words[alert.from] || alert.from} to ${words[alert.to] || alert.to}.`;
  }
  return '';
}

/* ---------------------------------------------------------------- state */

let state = { pending: {}, verified: {}, rounds: {} };

async function loadState() {
  try {
    state = JSON.parse(await readFile(STATE_PATH, 'utf8'));
    state.pending ||= {};
    state.verified ||= {};
    state.rounds ||= {};
  } catch { /* first run */ }
}

/* Write-then-rename: a crash mid-write must not leave a truncated file that
   would silently forget who is verified. */
async function saveState() {
  const tmp = `${STATE_PATH}.tmp`;
  await writeFile(tmp, JSON.stringify(state), { mode: 0o600 });
  await rename(tmp, STATE_PATH);
}

/* ------------------------------------------------------------- discord */

async function discord(path, opts = {}) {
  const res = await fetch(`${API}${path}`, {
    ...opts,
    headers: {
      authorization: `Bot ${BOT_TOKEN}`,
      'content-type': 'application/json',
      ...(opts.headers || {}),
    },
  });
  if (!res.ok) {
    const body = await res.text();
    throw new Error(`discord ${opts.method || 'GET'} ${path} -> ${res.status}: ${body.slice(0, 200)}`);
  }
  return res.status === 204 ? null : res.json();
}

/* Guild and role are DISCOVERED rather than configured. Two fewer ids for a
   human to look up and mistype, and the role can be renamed without an edit
   here as long as ROLE_NAME follows. */
let cache = { guildId: null, roleId: null, ownerId: null, at: 0 };
async function guildAndRole() {
  if (cache.guildId && Date.now() - cache.at < 600_000) return cache;
  const guilds = await discord('/users/@me/guilds');
  if (!guilds.length) throw new Error('the bot is not in any server yet');
  const guildId = guilds[0].id;
  // The owner id gates /vote-open and /vote-close; discovered, like the role.
  const [roles, guild] = await Promise.all([
    discord(`/guilds/${guildId}/roles`),
    discord(`/guilds/${guildId}`),
  ]);
  const want = ROLE_NAME.toLowerCase();
  const role = roles.find((r) => r.name.toLowerCase() === want);
  if (!role) throw new Error(`no role named "${ROLE_NAME}" in ${guilds[0].name}`);
  cache = { guildId, roleId: role.id, ownerId: guild.owner_id, at: Date.now() };
  return cache;
}

const grantRole = async (userId) => {
  const { guildId, roleId } = await guildAndRole();
  await discord(`/guilds/${guildId}/members/${userId}/roles/${roleId}`, { method: 'PUT' });
};
const revokeRole = async (userId) => {
  const { guildId, roleId } = await guildAndRole();
  await discord(`/guilds/${guildId}/members/${userId}/roles/${roleId}`, { method: 'DELETE' });
};

/** Raw status of the members endpoint: the one call where a 404 is an ANSWER
 *  (they left) rather than an error, so it cannot go through discord(). */
async function memberStatus(guildId, userId) {
  const res = await fetch(`${API}/guilds/${guildId}/members/${userId}`, {
    headers: { authorization: `Bot ${BOT_TOKEN}` },
    signal: AbortSignal.timeout(15_000),
  });
  await res.text(); // drain the body so the socket is reusable
  return res.status;
}

/** Open (or reuse) the DM channel and send. Returns 'sent'; 'unreachable'
 *  when the member's privacy settings refuse bot DMs, which is a decision to
 *  respect, not an error to retry; or 'failed' for anything transient. */
async function sendDm(userId, content) {
  const post = (path, body) => fetch(`${API}${path}`, {
    method: 'POST',
    headers: { authorization: `Bot ${BOT_TOKEN}`, 'content-type': 'application/json' },
    body: JSON.stringify(body),
    signal: AbortSignal.timeout(15_000),
  });
  try {
    const ch = await post('/users/@me/channels', { recipient_id: userId });
    if (!ch.ok) return ch.status === 403 ? 'unreachable' : 'failed';
    const channel = await ch.json();
    const msg = await post(`/channels/${channel.id}/messages`, { content });
    if (msg.ok) return 'sent';
    return msg.status === 403 ? 'unreachable' : 'failed';
  } catch {
    return 'failed';
  }
}

/* ---------------------------------------------------------------- kascov */

async function proveHolding(address, message, signature) {
  const res = await fetch(`${KASCOV}/data/${NETWORK}/prove-holding`, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify({ address, message, signature }),
    signal: AbortSignal.timeout(15_000),
  });
  if (!res.ok) throw new Error(`kascov ${res.status}`);
  return res.json();
}

async function holdingsOf(address) {
  const res = await fetch(`${KASCOV}/data/${NETWORK}/addr/${encodeURIComponent(address)}`, {
    signal: AbortSignal.timeout(15_000),
  });
  if (!res.ok) throw new Error(`kascov ${res.status}`);
  return res.json();
}

/* --------------------------------------------------------- vote runtime */

/** The round the tally file describes: the most recently opened, whether it
 *  is still open or already frozen. */
function currentRound() {
  const rounds = Object.values(state.rounds || {});
  if (!rounds.length) return null;
  return rounds.reduce((a, b) => (Number(a.opened_ms) >= Number(b.opened_ms) ? a : b));
}

/* The tally page is dumb on purpose: everything it shows is computed here,
   where the ballots are. A write failure is logged and swallowed, because a
   stale public page must never block a ballot. */
async function writeTally() {
  const round = currentRound();
  if (!round) return;
  try {
    const tmp = `${TALLY_PATH}.tmp`;
    await writeFile(tmp, JSON.stringify(buildTally(round, Date.now()), null, 2));
    await rename(tmp, TALLY_PATH);
  } catch (e) {
    console.error(`tally write failed: ${e.message}`);
  }
}

/* Freeze a round: counts are computed one last time and the ballots are
   DELETED, so who voted for what stops existing the moment it stops being
   needed. The public file never carried identities to begin with. */
function finalizeRound(round, nowMs) {
  round.counts = tallyCounts(round);
  round.status = 'closed';
  round.closed_ms = round.closed_ms || Math.min(nowMs, Number(round.closes_ms) || nowMs);
  delete round.ballots;
}

/* The five-day auto-close, applied lazily: every vote command and every
   recheck sweeps first, so an expired round is frozen by whichever runs
   next rather than by a timer that could silently not exist. */
async function finalizeExpiredRounds() {
  const now = Date.now();
  let changed = false;
  for (const round of Object.values(state.rounds || {})) {
    if (round.status === 'open' && !roundIsOpen(round, now)) {
      finalizeRound(round, now);
      changed = true;
    }
  }
  if (changed) {
    await saveState();
    await writeTally();
  }
}

/* Forget everything about a member: record (with its alert preference and
   cursor), pending challenge, and any ballot in a round that still holds
   ballots. The single definition /unverify and the leave-server cleanup
   share, so the two paths cannot drift apart. */
function forgetMember(userId) {
  const hadVerified = Boolean(state.verified[userId]);
  delete state.verified[userId];
  delete state.pending[userId];
  let ballotDropped = false;
  for (const round of Object.values(state.rounds || {})) {
    if (round.ballots && userId in round.ballots) {
      delete round.ballots[userId];
      ballotDropped = true;
    }
  }
  return { hadVerified, ballotDropped };
}

/* ------------------------------------------------------------- commands */

/* Command options: still used by /holdings, which looks up a PUBLIC address
   nobody is claiming to own. */
const opt = (data, name) => (data.options || []).find((o) => o.name === name)?.value;

async function onVerify(interaction) {
  const userId = interaction.member?.user?.id || interaction.user?.id;
  const address = field(interaction, 'address');
  if (!/^kaspa(test)?:[a-z0-9]{50,}$/i.test(address)) {
    return 'That does not look like a Kaspa address. It starts with `kaspa:` and is about 60 characters.';
  }
  const taken = Object.entries(state.verified)
    .find(([id, v]) => v.address === address && id !== userId);
  if (taken) {
    // Not a leak: it says the address is spoken for, never by whom.
    return 'That address is already verified by another account. Use a different one, or ask them to run `/unverify`.';
  }
  const nonce = randomBytes(8).toString('hex');
  /* Keep the interaction token so the signing page can answer back INTO this
     same ephemeral message. An interaction token lives 15 minutes, exactly as
     long as the challenge, so it is valid for precisely as long as it is
     useful. Without it the page can confirm and Discord stays silent, which
     reads as though nothing happened. */
  state.pending[userId] = {
    user: userId, address, nonce, issued_ms: Date.now(), token: interaction.token,
  };
  await saveState();

  const phrase = challengePhrase(userId, address, nonce);
  /* The phrase goes in the FRAGMENT, not the query string. A fragment never
     reaches the server, so this link keeps a Discord id and a Kaspa address out
     of access logs and out of any Referer the page might send. */
  const link = `${SITE}/verify#${encodeURIComponent(phrase)}`;
  return [
    '**Step 1 of 2.** Open this and press sign:',
    link,
    '',
    'Your wallet pops up, you approve, and it finishes there. Nothing moves and no funds are touched.',
    '',
    '**Prefer to do it by hand?** Sign this phrase in any wallet with a Sign Message feature, then run `/confirm`:',
    `\`\`\`\n${phrase}\n\`\`\``,
    '-# Signing a message is free and cannot authorise a transaction. kascov will never ask for your seed phrase or private key. This expires in 15 minutes.',
  ].join('\n');
}

async function onConfirm(interaction) {
  const userId = interaction.member?.user?.id || interaction.user?.id;
  const signature = field(interaction, 'signature');
  const pending = state.pending[userId];
  if (!challengeIsUsable(pending, userId, Date.now())) {
    return 'No challenge waiting, or it expired. Run `/verify` again to get a fresh one.';
  }
  const phrase = challengePhrase(userId, pending.address, pending.nonce);

  let proof;
  try {
    proof = await proveHolding(pending.address, phrase, signature);
  } catch {
    return 'Could not reach the verifier just now. Try `/confirm` again in a moment; your challenge is still valid.';
  }
  if (!proof?.verified) {
    return `That signature did not check out.\n-# ${proof?.reason || 'signature does not match'}\nMake sure you signed the phrase exactly, with no extra spaces or line breaks.`;
  }

  const balance = balanceFor(proof.holdings, TOKEN_ID);
  if (balance <= 0) {
    delete state.pending[userId];
    await saveState();
    return [
      `Address proven, and it is definitely yours. It just does not hold any $KASCOV right now, so there is no role to give.`,
      '-# Nothing was stored. Run `/verify` again whenever that changes.',
    ].join('\n');
  }

  try {
    await grantRole(userId);
  } catch (e) {
    console.error(e.message);
    return 'Proof accepted, but I could not add the role. My own role probably sits below `verified holder` in the server settings, which Discord refuses. Tell an admin.';
  }
  delete state.pending[userId];
  // alerts default ON for NEW verifications only; anyone verified before the
  // watchtower existed stays silent until they opt in themselves.
  state.verified[userId] = { address: pending.address, verified_ms: Date.now(), balance, alerts: true };
  await saveState();
  return [
    `Verified. **${fmt(balance)} $KASCOV**, proven from chain.`,
    `-# ${pending.address.slice(0, 18)}…${pending.address.slice(-8)} · re-checked periodically, so the role follows what you actually hold. \`/unverify\` removes it. Watchtower DMs are on by default; \`/alerts off\` stops them.`,
  ].join('\n');
}

async function onHoldings(interaction) {
  const address = String(opt(interaction.data, 'address') || '').trim();
  let data;
  try {
    data = await holdingsOf(address);
  } catch {
    return 'Could not read that address. Check it is a Kaspa address on mainnet.';
  }
  const rows = data.token_holdings || [];
  if (!rows.length) return `\`${address.slice(0, 18)}…\` holds no verified tokens.`;
  const lines = rows
    .slice(0, 10)
    .map((h) => `• **${fmt(h.balance)}** ${h.name} \`${h.token_id.slice(0, 8)}…\``);
  return [
    `\`${address.slice(0, 18)}…${address.slice(-8)}\` holds:`,
    ...lines,
    `-# Derived from chain. This is public information and proves nothing about who controls the address.`,
  ].join('\n');
}

async function onUnverify(interaction) {
  const userId = interaction.member?.user?.id || interaction.user?.id;
  const { hadVerified, ballotDropped } = forgetMember(userId);
  await saveState();
  // a forgotten member's ballot leaves the counts too, so the public file
  // never claims a vote from someone who no longer exists here.
  if (ballotDropped) await writeTally();
  try {
    await revokeRole(userId);
  } catch { /* already gone, or never had it */ }
  return hadVerified
    ? 'Done. Role removed and your address forgotten.'
    : 'You were not verified, but anything pending is cleared.';
}

async function onVote(interaction) {
  const userId = interaction.member?.user?.id || interaction.user?.id;
  await finalizeExpiredRounds();
  const now = Date.now();
  const round = currentRound();
  if (!round || !roundIsOpen(round, now)) {
    return `No audit-vote round is open right now. The last tally stays public at ${SITE}/vote.`;
  }
  const gate = voteEligibility(state.verified[userId]);
  if (!gate.ok) return `You cannot vote in this round: ${gate.reason}`;

  const slate = round.options.map((label, i) => `**${i + 1}.** ${label}`).join('\n');
  const choice = opt(interaction.data, 'choice');
  if (choice == null) {
    return [
      `**Audit vote, ${round.id}.** Open until ${new Date(Number(round.closes_ms)).toISOString().slice(0, 16).replace('T', ' ')} UTC:`,
      slate,
      '',
      'Cast with `/vote choice:<number>`. Voting again before close replaces your earlier ballot.',
      `-# ${VOTE_RULES.join(' · ')}.`,
    ].join('\n');
  }
  const replaced = Boolean(round.ballots && userId in round.ballots);
  const cast = castBallot(round, userId, choice, now);
  if (!cast.ok) return `That ballot did not count: ${cast.reason}`;
  round.ballots = cast.ballots;
  await saveState();
  await writeTally();
  return [
    `Ballot ${replaced ? 'replaced' : 'cast'}: **${cast.label}**.`,
    `-# One holder, one vote. The vote accelerates the audit queue; it never decides a verdict, and kascov may audit anything at any time. Public tally, counts only: ${SITE}/vote`,
  ].join('\n');
}

async function onVoteOpen(interaction) {
  const userId = interaction.member?.user?.id || interaction.user?.id;
  await finalizeExpiredRounds();
  const { ownerId } = await guildAndRole();
  if (!isOperator(userId, ownerId, OPERATOR_ID)) return 'Only the operator can open a round.';
  const now = Date.now();
  const open = currentRound();
  if (open && roundIsOpen(open, now)) {
    return `**${open.id}** is still open. Close it with \`/vote-close\` before opening another.`;
  }
  const labels = parseSlate(opt(interaction.data, 'slate'));
  if (!labels) return 'Give 2 to 6 option labels separated by `|`, for example `token-a | token-b | token-c`.';
  state.voteSeq = Number(state.voteSeq || 0) + 1;
  const round = {
    id: `round-${state.voteSeq}`,
    options: labels,
    opened_ms: now,
    closes_ms: now + VOTE_MAX_AGE_MS,
    closed_ms: null,
    status: 'open',
    ballots: {},
  };
  state.rounds[round.id] = round;
  await saveState();
  await writeTally();
  return [
    `**${round.id} is open** with ${labels.length} options:`,
    labels.map((l, i) => `**${i + 1}.** ${l}`).join('\n'),
    '',
    `Verified holders with at least ${fmt(VOTE_DUST_FLOOR)} proven $KASCOV cast with \`/vote choice:<number>\`. Auto-closes in 5 days, or earlier with \`/vote-close\`. Tally: ${SITE}/vote`,
  ].join('\n');
}

async function onVoteClose(interaction) {
  const userId = interaction.member?.user?.id || interaction.user?.id;
  const { ownerId } = await guildAndRole();
  if (!isOperator(userId, ownerId, OPERATOR_ID)) return 'Only the operator can close a round.';
  const round = currentRound();
  if (!round || round.status !== 'open') return 'No round is open.';
  finalizeRound(round, Date.now());
  await saveState();
  await writeTally();
  const lines = round.options.map((l, i) => `**${round.counts[i]}** · ${l}`).join('\n');
  return [
    `**${round.id} closed.** Final counts:`,
    lines,
    `-# Frozen, and the ballots themselves are deleted. Counts stay public at ${SITE}/vote.`,
  ].join('\n');
}

async function onAlerts(interaction) {
  const userId = interaction.member?.user?.id || interaction.user?.id;
  const rec = state.verified[userId];
  if (!rec) return 'Alerts ride on verification. Run `/verify` first.';
  const setting = String(opt(interaction.data, 'setting') || '').toLowerCase();
  if (setting !== 'on' && setting !== 'off') return 'Say `on` or `off`.';
  rec.alerts = setting === 'on';
  // an explicit opt-in is also permission to try DMs again after a bounce
  if (rec.alerts) delete rec.unreachable;
  await saveState();
  return rec.alerts
    ? 'Watchtower DMs are **on**: a market-phase change on a token you hold, or your own proven balance crossing zero or the dust floor. Everything derives from public endpoints, so a DM never tells you anything kascov.io does not already show. `/alerts off` any time.'
    : 'Watchtower DMs are **off**. Nothing will be sent.';
}

/** Find whose challenge a nonce belongs to. Single-use by construction: the
 *  entry is deleted the moment it resolves either way. */
export function pendingByNonce(pending, nonce) {
  if (!nonce || typeof nonce !== 'string') return null;
  for (const [userId, c] of Object.entries(pending || {})) {
    if (c && c.nonce === nonce) return { userId, challenge: c };
  }
  return null;
}

/** The signing page's completion path. Same checks as /confirm, reached
 *  without Discord, so it states its own outcome rather than assuming a
 *  followup message will carry it. */
async function completeFromPage({ nonce, signature }) {
  const found = pendingByNonce(state.pending, String(nonce || '').trim());
  if (!found || !challengeIsUsable(found.challenge, found.userId, Date.now())) {
    return { ok: false, message: 'That challenge has expired or was already used. Run /verify again in Discord.', log: 'no such challenge' };
  }
  const { userId, challenge } = found;
  const sig = String(signature || '').trim();
  const phrase = challengePhrase(userId, challenge.address, challenge.nonce);

  let proof;
  try {
    proof = await proveHolding(challenge.address, phrase, sig);
  } catch {
    return { ok: false, message: 'Could not reach the verifier. Try again in a moment.', log: 'kascov unreachable' };
  }
  if (!proof?.verified) {
    return { ok: false, message: `That signature did not check out. ${proof?.reason || ''}`, log: 'bad signature' };
  }

  const balance = balanceFor(proof.holdings, TOKEN_ID);
  if (balance <= 0) {
    const tok = challenge.token;
    delete state.pending[userId];
    await saveState();
    if (tok) {
      await respondLater(tok, [
        '**Address proven**, and it is definitely yours. It just holds no $KASCOV right now, so there is no role to give.',
        '-# Nothing was stored. Run `/verify` again whenever that changes.',
      ].join('\n'));
    }
    return { ok: false, message: 'Address proven, and it is definitely yours. It just holds no $KASCOV right now, so there is no role to give.', log: 'proven but empty' };
  }
  try {
    await grantRole(userId);
  } catch (e) {
    console.error(e.message);
    return { ok: false, message: 'Proof accepted, but the role could not be added. Tell an admin the bot role may sit too low.', log: 'grant failed' };
  }
  delete state.pending[userId];
  // same default as /confirm: new records opt in, old records never do.
  state.verified[userId] = { address: challenge.address, verified_ms: Date.now(), balance, alerts: true };
  await saveState();

  /* Close the loop in Discord. The member is looking at a browser tab, and the
     message that told them to sign should stop telling them to sign. */
  if (challenge.token) {
    await respondLater(challenge.token, [
      `**Verified.** ${fmt(balance)} $KASCOV, proven from chain.`,
      `-# Signed with your wallet and checked against ${challenge.address.slice(0, 16)}…${challenge.address.slice(-6)}. The role is yours and gets re-checked periodically, so it follows what you actually hold. \`/unverify\` removes it.`,
    ].join('\n'));
  }
  return { ok: true, message: `Verified. ${fmt(balance)} $KASCOV proven from chain. Your role is in Discord now.`, log: `granted ${fmt(balance)}` };
}

/* Slash commands that answer directly. verify/confirm are not here: they open
   a modal first, and their handlers are reached by the form's custom_id. */
const HANDLERS = {
  holdings: onHoldings,
  unverify: onUnverify,
  vote: onVote,
  'vote-open': onVoteOpen,
  'vote-close': onVoteClose,
  alerts: onAlerts,
};

const MODAL_HANDLERS = { kascov_verify: onVerify, kascov_confirm: onConfirm };

/* ------------------------------------------------------------- the server */

/* fetch only REJECTS on a network error, so a 400 from Discord resolves
   happily. Checking res.ok is the difference between knowing the reply landed
   and merely knowing it was sent. */
async function respondLater(token, content) {
  const text = String(content ?? '').trim();
  if (!text) {
    console.error('followup: handler produced nothing, sending a fallback');
  }
  // Discord hard-caps message content at 2000 characters and 400s the whole
  // edit if it is over, which loses the entire answer rather than the tail.
  const body = text ? (text.length > 1990 ? `${text.slice(0, 1987)}...` : text)
    : 'Something went wrong and there is nothing to show. Try again.';
  try {
    const res = await fetch(`${API}/webhooks/${APP_ID}/${token}/messages/@original`, {
      method: 'PATCH',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ content: body }),
    });
    if (!res.ok) {
      console.error(`followup HTTP ${res.status}: ${(await res.text()).slice(0, 300)}`);
      return false;
    }
    return true;
  } catch (e) {
    console.error(`followup network error: ${e.message}`);
    return false;
  }
}

/** Run a handler after the ack and deliver whatever it says. */
async function finish(body, handler, who) {
  try {
    const out = await handler(body);
    const ok = await respondLater(body.token, out);
    console.log(`  ${ok ? 'answered' : 'FAILED TO ANSWER'} ${who}`);
  } catch (e) {
    console.error(`  handler failed for ${who}: ${e.stack || e.message}`);
    await respondLater(body.token, 'Something went wrong. Try again shortly.');
  }
}

/* A one-field form. Discord caps a label at 45 characters and a placeholder at
   100, and silently rejects the whole modal if either is over. */
const oneFieldModal = (customId, title, field) => ({
  type: 9,
  data: {
    custom_id: customId,
    title,
    components: [{
      type: 1,
      components: [{
        type: 4,
        custom_id: field.id,
        label: field.label,
        style: field.long ? 2 : 1,
        min_length: field.min,
        max_length: field.max,
        placeholder: field.placeholder,
        required: true,
      }],
    }],
  },
});

const MODALS = {
  verify: oneFieldModal('kascov_verify', 'Verify a Kaspa address', {
    id: 'address',
    label: 'Your Kaspa address',
    min: 20,
    max: 120,
    placeholder: 'kaspa:q...',
  }),
  confirm: oneFieldModal('kascov_confirm', 'Finish verifying', {
    id: 'signature',
    label: 'The signature from your wallet',
    long: true,
    min: 32,
    max: 400,
    placeholder: 'paste the signature here',
  }),
};

/** Pull a named field out of a modal submission. */
const field = (body, id) => {
  for (const row of body.data?.components || []) {
    for (const c of row.components || []) {
      if (c.custom_id === id) return String(c.value || '').trim();
    }
  }
  return '';
};

function serve() {
  const server = createServer((req, res) => {
    const send = (code, body) => {
      res.writeHead(code, { 'content-type': 'application/json' });
      res.end(typeof body === 'string' ? body : JSON.stringify(body));
    };
    if (req.method !== 'POST') return send(405, { error: 'POST only' });

    /* The signing page finishes here. It carries no Discord signature, and does
       not need one: the only way in is a NONCE this service issued minutes ago,
       and holding it grants nothing on its own. Completing still requires a
       valid signature over the whole phrase by the exact address the challenge
       was bound to, which only that key can produce. */
    if ((req.url || '').startsWith('/discord/complete')) {
      const parts = [];
      let n = 0;
      req.on('data', (c) => { n += c.length; if (n <= 8_000) parts.push(c); });
      req.on('end', async () => {
        let body;
        try { body = JSON.parse(Buffer.concat(parts).toString('utf8')); } catch { body = null; }
        const out = await completeFromPage(body || {});
        console.log(`complete: ${out.ok ? 'granted' : 'refused'} (${out.log || ''})`);
        send(200, out.ok ? { ok: true, message: out.message } : { ok: false, message: out.message });
      });
      return;
    }

    // Collect BYTES, never a growing string: see verifyDiscordSignature.
    const chunks = [];
    let size = 0;
    let tooBig = false;
    req.on('data', (c) => {
      size += c.length;
      if (size > 100_000) { tooBig = true; req.destroy(); return; }
      chunks.push(c);
    });
    req.on('end', async () => {
      if (tooBig) return;
      const raw = Buffer.concat(chunks);
      const sig = req.headers['x-signature-ed25519'];
      const ts = req.headers['x-signature-timestamp'];
      // 401 is required: Discord actively probes with BAD signatures when you
      // save the endpoint URL, and refuses it unless those are rejected.
      if (!sig || !ts || !verifyDiscordSignature(raw, String(sig), String(ts), PUBLIC_KEY)) {
        console.log(`rejected: bad signature (${size} bytes)`);
        return send(401, { error: 'bad signature' });
      }
      let body;
      try { body = JSON.parse(raw.toString('utf8')); } catch { return send(400, { error: 'bad json' }); }

      if (body.type === 1) { console.log('ping'); return send(200, { type: 1 }); }

      const who = body.member?.user?.id || body.user?.id || '?';

      // A slash command opens a MODAL. The address and the signature are typed
      // into a form, so neither ever appears in a command invocation where the
      // channel could see it. The only new fact this bot creates is the link
      // between an account and an address; it should not announce it.
      if (body.type === 2) {
        const name = body.data?.name;
        console.log(`command /${name} from ${who}`);
        if (name === 'verify') return send(200, MODALS.verify);
        if (name === 'confirm') return send(200, MODALS.confirm);
        const handler = HANDLERS[name];
        if (!handler) return send(200, { type: 4, data: { content: 'unknown command', flags: EPHEMERAL } });
        send(200, { type: 5, data: { flags: EPHEMERAL } });
        return finish(body, handler, who);
      }

      if (body.type === 5) { // MODAL_SUBMIT
        const id = body.data?.custom_id;
        console.log(`modal ${id} from ${who}`);
        const handler = MODAL_HANDLERS[id];
        if (!handler) return send(200, { type: 4, data: { content: 'unknown form', flags: EPHEMERAL } });
        send(200, { type: 5, data: { flags: EPHEMERAL } });
        return finish(body, handler, who);
      }

      return send(200, { type: 4, data: { content: 'unsupported', flags: EPHEMERAL } });
    });
  });
  server.listen(PORT, '127.0.0.1', () =>
    console.log(`holder bot listening on 127.0.0.1:${PORT}`));
}

/* --------------------------------------------------------------- modes */

const COMMANDS = [
  // No options on purpose: an option's value is echoed in the command
  // invocation, and the address is the one thing worth not announcing.
  { name: 'verify', description: 'Prove an address is yours and get the verified holder role' },
  { name: 'confirm', description: 'Finish verifying with the signature from your wallet' },
  {
    name: 'holdings', description: 'Show what an address holds, proven from chain',
    options: [{ name: 'address', description: 'Any Kaspa address', type: 3, required: true }],
  },
  { name: 'unverify', description: 'Drop your verified holder role and forget your address' },
  {
    // The choice is a number, not a label: labels change per round, and
    // Discord fixes option choices at registration time.
    name: 'vote', description: 'Audit vote: see the open round, or cast your ballot',
    options: [{
      name: 'choice', description: 'Option number from the slate (leave empty to see the round)',
      type: 4, required: false, min_value: 1, max_value: 6,
    }],
  },
  {
    name: 'vote-open', description: 'Operator: open an audit-vote round',
    options: [{
      name: 'slate', description: 'Two to six option labels, separated by |',
      type: 3, required: true,
    }],
  },
  { name: 'vote-close', description: 'Operator: close the round and freeze the counts' },
  {
    name: 'alerts', description: 'Watchtower DMs on or off',
    options: [{
      name: 'setting', description: 'on or off', type: 3, required: true,
      choices: [{ name: 'on', value: 'on' }, { name: 'off', value: 'off' }],
    }],
  },
];

async function registerCommands() {
  const { guildId } = await guildAndRole();
  // Guild-scoped: appears instantly. Global commands take up to an hour.
  await discord(`/applications/${APP_ID}/guilds/${guildId}/commands`, {
    method: 'PUT', body: JSON.stringify(COMMANDS),
  });
  console.log(`registered ${COMMANDS.length} commands in guild ${guildId}`);
}

/** Re-prove every verified holder and drop the role from anyone who sold out.
 *  No signature needed: we already know the address is theirs, and a balance is
 *  public. Only the LINK between account and address ever needed proving.
 *
 *  The same pass keeps the other two promises: a member who left the server is
 *  forgotten like /unverify, and the watchtower DMs whoever asked for DMs about
 *  changes derived only from public endpoints. */
async function recheck() {
  await finalizeExpiredRounds();
  const ids = Object.keys(state.verified);
  let dropped = 0;
  let departed = 0;
  let guildId = null;
  try {
    ({ guildId } = await guildAndRole());
  } catch (e) {
    // no guild means no membership answers; balances still get rechecked.
    console.error(`recheck: ${e.message}`);
  }

  /* one phase lookup per token per RUN, not per holder: holders share tokens. */
  const phaseCache = new Map();
  const phaseOf = async (tokenId) => {
    if (phaseCache.has(tokenId)) return phaseCache.get(tokenId);
    let phase = null;
    try {
      const res = await fetch(`${KASCOV}/data/${NETWORK}/token/${tokenId}`, {
        signal: AbortSignal.timeout(15_000),
      });
      if (res.ok) phase = (await res.json())?.market?.phase || null;
    } catch { /* unknown stays unknown; never alert off a failed lookup */ }
    phaseCache.set(tokenId, phase);
    return phase;
  };

  let ballotsDropped = false;
  for (const userId of ids) {
    const rec = state.verified[userId];

    /* leave-server cleanup: bot.html promises that leaving deletes the
       record, and this loop is where the promise is kept. Only an explicit
       404 counts; an outage deletes nobody. */
    if (guildId) {
      try {
        if (leftTheServer(await memberStatus(guildId, userId))) {
          ballotsDropped = forgetMember(userId).ballotDropped || ballotsDropped;
          departed += 1;
          continue;
        }
      } catch (e) {
        console.error(`member check ${userId}: ${e.message}`);
      }
    }

    try {
      const data = await holdingsOf(rec.address);
      const holdings = data.token_holdings || [];
      const balance = balanceFor(holdings, TOKEN_ID);

      /* watchtower: snapshot, diff, DM, then move the cursor. The cap bounds
         the extra lookups for an address holding half the galaxy. */
      const phases = {};
      for (const h of holdings.slice(0, 25)) {
        const p = await phaseOf(h.token_id);
        if (p) phases[h.token_id] = p;
        // a failed lookup carries the old phase forward so the change still
        // fires (once) when the endpoint answers again.
        else if (rec.cursor?.phases?.[h.token_id]) phases[h.token_id] = rec.cursor.phases[h.token_id];
      }
      const next = { phases, bucket: balanceBucket(balance) };
      const alerts = deliverableAlerts(rec, next);
      if (alerts.length) {
        const names = Object.fromEntries(holdings.map((h) => [h.token_id, h.name]));
        const outcome = await sendDm(userId, [
          ...alerts.map((a) => alertMessage(a, names)).filter(Boolean),
          '-# Derived from public endpoints, so kascov.io knew first. `/alerts off` in the server stops these.',
        ].join('\n'));
        // a refused DM is a decision: stop trying, silently, until /alerts on.
        if (outcome === 'unreachable') rec.unreachable = true;
      }
      /* the cursor advances even when a send failed, making delivery
         at-most-once: a transient error costs one alert, never a double DM.
         the free site published the fact either way. and it advances for
         opted-out holders too, so opting in later never unleashes a backlog. */
      rec.cursor = next;

      if (balance > 0) {
        state.verified[userId] = { ...rec, balance, checked_ms: Date.now() };
        continue;
      }
      /* the crossing-to-zero DM above was this record's last act; the record
         itself goes, exactly as before the watchtower existed. */
      await revokeRole(userId).catch(() => {});
      ballotsDropped = forgetMember(userId).ballotDropped || ballotsDropped;
      dropped += 1;
    } catch (e) {
      // A failed lookup must never cost someone their role: leave it alone.
      console.error(`recheck ${userId}: ${e.message}`);
    }
  }
  await saveState();
  if (ballotsDropped) await writeTally();
  console.log(`rechecked ${ids.length}, dropped ${dropped}, departed ${departed}`);
}

async function main() {
  for (const [k, v] of Object.entries({ DISCORD_APP_ID: APP_ID, DISCORD_PUBLIC_KEY: PUBLIC_KEY, DISCORD_BOT_TOKEN: BOT_TOKEN })) {
    if (!v) { console.error(`${k} is not set — refusing to run.`); process.exit(2); }
  }
  await loadState();
  if (process.argv.includes('--register')) return registerCommands();
  if (process.argv.includes('--recheck')) return recheck();
  serve();
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main().catch((e) => { console.error(e.stack || e.message); process.exit(1); });
}
