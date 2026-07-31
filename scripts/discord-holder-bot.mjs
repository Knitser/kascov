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
const ROLE_NAME = process.env.KASCOV_ROLE_NAME || 'verified holder';
/* The coin the role is about. Empty means "any verified token". */
const TOKEN_ID = process.env.KASCOV_ROLE_TOKEN
  || 'c58c826d0aa9cee62f93208718c674883f5c89a8aca4933dc41fb0391539abe2';

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

/* ---------------------------------------------------------------- state */

let state = { pending: {}, verified: {} };

async function loadState() {
  try {
    state = JSON.parse(await readFile(STATE_PATH, 'utf8'));
    state.pending ||= {};
    state.verified ||= {};
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
let cache = { guildId: null, roleId: null, at: 0 };
async function guildAndRole() {
  if (cache.guildId && Date.now() - cache.at < 600_000) return cache;
  const guilds = await discord('/users/@me/guilds');
  if (!guilds.length) throw new Error('the bot is not in any server yet');
  const guildId = guilds[0].id;
  const roles = await discord(`/guilds/${guildId}/roles`);
  const want = ROLE_NAME.toLowerCase();
  const role = roles.find((r) => r.name.toLowerCase() === want);
  if (!role) throw new Error(`no role named "${ROLE_NAME}" in ${guilds[0].name}`);
  cache = { guildId, roleId: role.id, at: Date.now() };
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
  state.pending[userId] = { user: userId, address, nonce, issued_ms: Date.now() };
  await saveState();

  const phrase = challengePhrase(userId, address, nonce);
  return [
    '**Step 1 of 2.** Copy this phrase and sign it in your wallet:',
    `\`\`\`\n${phrase}\n\`\`\``,
    '**Where to sign it** (nothing pops up on its own, you do this in your wallet app):',
    '• **Kastle** ・ open the extension, account menu, Sign Message',
    '• **KasWare** ・ open the extension, Settings, Sign Message',
    '• **kaspa CLI** ・ `message sign <your-address>` then paste the phrase',
    '',
    'It gives you back a long hex string. Run `/confirm` and paste it into the form.',
    '-# Copy the whole phrase, including the numbers at both ends. Signing a message is free, moves no funds, and cannot authorise a transaction. kascov will never ask for your seed phrase or private key. This expires in 15 minutes.',
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
  state.verified[userId] = { address: pending.address, verified_ms: Date.now(), balance };
  await saveState();
  return [
    `Verified. **${fmt(balance)} $KASCOV**, proven from chain.`,
    `-# ${pending.address.slice(0, 18)}…${pending.address.slice(-8)} · re-checked periodically, so the role follows what you actually hold. \`/unverify\` removes it.`,
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
  const had = state.verified[userId];
  delete state.verified[userId];
  delete state.pending[userId];
  await saveState();
  try {
    await revokeRole(userId);
  } catch { /* already gone, or never had it */ }
  return had
    ? 'Done. Role removed and your address forgotten.'
    : 'You were not verified, but anything pending is cleared.';
}

/* Slash commands that answer directly. verify/confirm are not here: they open
   a modal first, and their handlers are reached by the form's custom_id. */
const HANDLERS = { holdings: onHoldings, unverify: onUnverify };

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
 *  public. Only the LINK between account and address ever needed proving. */
async function recheck() {
  const ids = Object.keys(state.verified);
  let dropped = 0;
  for (const userId of ids) {
    const rec = state.verified[userId];
    try {
      const data = await holdingsOf(rec.address);
      const balance = balanceFor(data.token_holdings, TOKEN_ID);
      if (balance > 0) {
        state.verified[userId] = { ...rec, balance, checked_ms: Date.now() };
        continue;
      }
      await revokeRole(userId).catch(() => {});
      delete state.verified[userId];
      dropped += 1;
    } catch (e) {
      // A failed lookup must never cost someone their role: leave it alone.
      console.error(`recheck ${userId}: ${e.message}`);
    }
  }
  await saveState();
  console.log(`rechecked ${ids.length}, dropped ${dropped}`);
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
