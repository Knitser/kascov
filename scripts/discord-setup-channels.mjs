#!/usr/bin/env node
/* =====================================================================
   One-shot server furniture: the dev wing, the holder #beta channel,
   and the permanent front-door invite.

   Idempotent by name: anything that already exists is left exactly as
   it is, so running this twice changes nothing. Dry-run by default;
   --apply makes the calls. Zero dependencies, credential from env only
   (DISCORD_BOT_TOKEN via the same env file the holder bot uses).

   Needs the bot to have: Manage Channels, Manage Roles (it already
   grants the holder role), Create Instant Invite.

   Usage:  node scripts/discord-setup-channels.mjs           # dry-run
           node scripts/discord-setup-channels.mjs --apply   # do it
   ===================================================================== */

const TOKEN = process.env.DISCORD_BOT_TOKEN || '';
const APPLY = process.argv.includes('--apply');
const API = 'https://discord.com/api/v10';

if (!TOKEN) {
  console.error('DISCORD_BOT_TOKEN is not set; nothing to do.');
  process.exit(1);
}

const HOLDER_ROLE_NAME = 'verified holder';

/* the dev wing: a category and five rooms, topics in site voice */
const DEV_CATEGORY = 'THE WORKBENCH';
const DEV_CHANNELS = [
  ['🛠┃dev-general', 'building on kascov or covenants? start here. no question too small.'],
  ['🔌┃api-and-clients', 'the public API, SSE streams, webhooks, the js/py thin clients, the holder lane.'],
  ['🔬┃covenant-forensics', 'bytecode, skeletons, the audit bench, replay findings. bring hex.'],
  ['📐┃kcc-specs', 'the covenant standards: KCC-1, KCC-0020, KCC-0021. PR links welcome.'],
  ['🚀┃show-your-build', 'what you are building on Kaspa L1. the community page on kascov.io feeds from here.'],
];

/* the holder wing: #beta is visible to verified holders only */
const HOLDER_CATEGORY = 'HOLDERS';
const BETA_CHANNEL = ['🧪┃beta', 'new tools land here a short window before everyone else. tools only, never findings.'];

/* the front door: a permanent invite on whichever of these exists first */
const INVITE_CHANNEL_PREFERENCE = ['welcome', 'general', 'start-here'];

const VIEW_CHANNEL = 0x400n;

async function call(method, path, body) {
  const res = await fetch(`${API}${path}`, {
    method,
    headers: {
      authorization: `Bot ${TOKEN}`,
      'content-type': 'application/json',
      'x-audit-log-reason': 'kascov setup: dev wing + beta + invite',
    },
    body: body == null ? undefined : JSON.stringify(body),
  });
  if (res.status === 429) {
    const wait = (await res.json()).retry_after || 1;
    await new Promise((r) => setTimeout(r, wait * 1000 + 250));
    return call(method, path, body);
  }
  if (!res.ok) throw new Error(`${method} ${path} -> ${res.status}: ${await res.text()}`);
  return res.status === 204 ? null : res.json();
}

const main = async () => {
  const guilds = await call('GET', '/users/@me/guilds');
  if (!guilds.length) throw new Error('the bot is in no guild');
  const guild = guilds[0];
  console.log(`guild: ${guild.name} (${guild.id})${guilds.length > 1 ? ' — NOTE: bot is in several guilds, using the first' : ''}`);

  const channels = await call('GET', `/guilds/${guild.id}/channels`);
  const roles = await call('GET', `/guilds/${guild.id}/roles`);
  const byName = new Map(channels.map((c) => [c.name.toLowerCase(), c]));
  const holderRole = roles.find((r) => r.name.toLowerCase() === HOLDER_ROLE_NAME);
  if (!holderRole) throw new Error(`role "${HOLDER_ROLE_NAME}" not found — run the holder bot setup first`);

  const plan = [];

  async function ensureCategory(name) {
    const existing = channels.find((c) => c.type === 4 && c.name.toLowerCase() === name.toLowerCase());
    if (existing) return existing.id;
    plan.push(`create category ${name}`);
    if (!APPLY) return null;
    const made = await call('POST', `/guilds/${guild.id}/channels`, { name, type: 4 });
    console.log(`  created category ${name}`);
    return made.id;
  }

  async function ensureText(name, topic, parentId, overwrites) {
    if (byName.has(name)) { console.log(`  exists, untouched: #${name}`); return byName.get(name); }
    plan.push(`create #${name}`);
    if (!APPLY) return null;
    const made = await call('POST', `/guilds/${guild.id}/channels`, {
      name, type: 0, topic,
      parent_id: parentId || undefined,
      permission_overwrites: overwrites || undefined,
    });
    console.log(`  created #${name}`);
    return made;
  }

  console.log('\n-- the dev wing');
  const devCat = await ensureCategory(DEV_CATEGORY);
  for (const [name, topic] of DEV_CHANNELS) await ensureText(name, topic, devCat);

  console.log('\n-- the holder wing');
  const holderCat = await ensureCategory(HOLDER_CATEGORY);
  /* @everyone loses sight of #beta; the proven role gets it back */
  const betaOverwrites = [
    { id: guild.id, type: 0, deny: String(VIEW_CHANNEL), allow: '0' },
    { id: holderRole.id, type: 0, allow: String(VIEW_CHANNEL), deny: '0' },
  ];
  await ensureText(BETA_CHANNEL[0], BETA_CHANNEL[1], holderCat, betaOverwrites);

  console.log('\n-- the front door');
  const door = INVITE_CHANNEL_PREFERENCE.map((n) => byName.get(n)).find(Boolean)
    || channels.find((c) => c.type === 0);
  if (!door) throw new Error('no text channel to put an invite on');
  console.log(`  invite channel: #${door.name}`);
  if (APPLY) {
    /* permanent and reusable: this is the link the website carries */
    const inv = await call('POST', `/channels/${door.id}/invites`, { max_age: 0, max_uses: 0, unique: false });
    console.log(`  INVITE: https://discord.gg/${inv.code}`);
  } else {
    plan.push(`create permanent invite on #${door.name}`);
  }

  if (!APPLY) {
    console.log('\nDRY RUN — would do:');
    for (const p of plan) console.log('  * ' + p);
    if (!plan.length) console.log('  nothing; everything already exists');
    console.log('run again with --apply to make it so.');
  }
};

main().catch((e) => { console.error(String(e.message || e)); process.exit(1); });
