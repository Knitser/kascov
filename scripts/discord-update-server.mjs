#!/usr/bin/env node
/* =====================================================================
   The pre-public server pass, fitted to the guild as it actually is:

   - THE WORKBENCH (already there, empty) gets the dev rooms, and the
     existing #dev moves in with a real topic
   - #changelog becomes an Announcement channel so other servers follow
   - #general's topic says out loud that everything is welcome here,
     other tokens and projects included
   - a markets room so chart talk has a home of its own
   - #show-your-build under BUILT HERE, feeding the community page
   - #beta under $KASCov, visible to verified holders only
   - empty topics filled; a permanent invite minted on #welcome

   Idempotent: existing things are patched only where stated, created
   things are skipped when a channel of that name exists. Dry-run by
   default; --apply makes the calls.
   ===================================================================== */

const TOKEN = process.env.DISCORD_BOT_TOKEN || '';
const APPLY = process.argv.includes('--apply');
const API = 'https://discord.com/api/v10';
if (!TOKEN) { console.error('DISCORD_BOT_TOKEN is not set'); process.exit(1); }

const VIEW = 0x400n;
const HOLDER_ROLE = 'verified holder';

async function call(method, path, body) {
  const res = await fetch(`${API}${path}`, {
    method,
    headers: {
      authorization: `Bot ${TOKEN}`,
      'content-type': 'application/json',
      'x-audit-log-reason': 'kascov pre-public server pass',
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

/* channel names carry emoji prefixes; match on the bare suffix */
const bare = (name) => name.toLowerCase().replace(/^[^a-z0-9]*┃/, '').trim();

const main = async () => {
  const g = (await call('GET', '/users/@me/guilds'))[0];
  const chans = await call('GET', `/guilds/${g.id}/channels`);
  const roles = await call('GET', `/guilds/${g.id}/roles`);
  const holderRole = roles.find((r) => r.name.toLowerCase() === HOLDER_ROLE);
  if (!holderRole) throw new Error('verified holder role not found');

  const cat = (frag) => chans.find((c) => c.type === 4 && c.name.toUpperCase().includes(frag));
  const chan = (frag) => chans.find((c) => c.type !== 4 && bare(c.name) === frag);
  const workbench = cat('WORKBENCH');
  const explorer = cat('EXPLORER');
  const builtHere = cat('BUILT HERE');
  const kascovCat = cat('$KASCOV');
  if (!workbench || !explorer || !builtHere || !kascovCat) throw new Error('expected category missing');

  const acts = [];
  const act = (label, fn) => acts.push({ label, fn });

  /* ---- 1. #changelog becomes followable (guild has COMMUNITY) */
  const changelog = chan('changelog');
  if (changelog && changelog.type === 0) {
    act('#changelog -> Announcement type (other servers can follow it)', () =>
      call('PATCH', `/channels/${changelog.id}`, { type: 5 }));
  }

  /* ---- 2. the open-culture topics */
  const general = chan('general');
  const GENERAL_TOPIC = 'Anything goes: Kaspa, covenants, other tokens, other projects, markets. kascov verifies, you talk.';
  if (general && general.topic !== GENERAL_TOPIC) {
    act('#general topic -> everything welcome, other tokens included', () =>
      call('PATCH', `/channels/${general.id}`, { topic: GENERAL_TOPIC }));
  }
  const audit = chan('what-to-audit-next');
  const AUDIT_TOPIC = 'Post a covenant or program you want verified. Verified holders steer the queue here; the chain still decides every verdict.';
  if (audit && audit.topic !== AUDIT_TOPIC) {
    act('#what-to-audit-next topic -> mentions the holder vote', () =>
      call('PATCH', `/channels/${audit.id}`, { topic: AUDIT_TOPIC }));
  }

  /* ---- 3. the dev wing: move #dev into THE WORKBENCH, add the rooms */
  const dev = chan('dev');
  if (dev && (dev.parent_id !== workbench.id || !dev.topic)) {
    act('#dev -> THE WORKBENCH with a real topic', () =>
      call('PATCH', `/channels/${dev.id}`, {
        parent_id: workbench.id,
        topic: 'Building on kascov or covenants? Start here. No question too small.',
      }));
  }
  const NEW_DEV = [
    ['🔌┃api-and-clients', 'The public API, SSE streams, webhooks, the js/py thin clients, the holder lane.'],
    ['🧬┃covenant-forensics', 'Bytecode, skeletons, the audit bench, replay findings. Bring hex.'],
    ['📐┃kcc-specs', 'The covenant standards: KCC-1, KCC-0020, KCC-0021. PR links welcome.'],
  ];
  for (const [name, topic] of NEW_DEV) {
    if (!chan(bare(name))) {
      act(`create #${name} in THE WORKBENCH`, () =>
        call('POST', `/guilds/${g.id}/channels`, { name, type: 0, topic, parent_id: workbench.id }));
    }
  }

  /* ---- 4. a home for chart talk, so it does not have to live anywhere else */
  if (!chan('tokens-and-markets')) {
    act('create #📈┃tokens-and-markets in THE EXPLORER', () =>
      call('POST', `/guilds/${g.id}/channels`, {
        name: '📈┃tokens-and-markets', type: 0, parent_id: explorer.id,
        topic: 'Any Kaspa token, any market, charts and calls welcome. Not financial advice, and lobbying for a verdict still costs the door.',
      }));
  }

  /* ---- 5. show-your-build under BUILT HERE + topics for its empty rooms */
  if (!chan('show-your-build')) {
    act('create #🚀┃show-your-build in BUILT HERE', () =>
      call('POST', `/guilds/${g.id}/channels`, {
        name: '🚀┃show-your-build', type: 0, parent_id: builtHere.id,
        topic: 'What you are building on Kaspa L1. The community page on kascov.io feeds from here.',
      }));
  }
  const roadmap = chan('roadmap');
  if (roadmap && !roadmap.topic) {
    act('#roadmap topic', () => call('PATCH', `/channels/${roadmap.id}`, {
      topic: 'Where kascov is going, in the order it is going there. Holders vote on the discretionary slots.',
    }));
  }
  const otherTools = chan('other-tools');
  if (otherTools && !otherTools.topic) {
    act('#other-tools topic', () => call('PATCH', `/channels/${otherTools.id}`, {
      topic: 'Ecosystem tools worth knowing: wallets, indexers, explorers, SDKs. Not ours, still good.',
    }));
  }

  /* ---- 6. #beta under $KASCOV, verified holders only */
  if (!chan('beta')) {
    act('create #🧪┃beta in $KASCOV (verified holders only)', () =>
      call('POST', `/guilds/${g.id}/channels`, {
        name: '🧪┃beta', type: 0, parent_id: kascovCat.id,
        topic: 'New tools land here a short window before everyone else. Tools only, never findings.',
        permission_overwrites: [
          { id: g.id, type: 0, deny: String(VIEW), allow: '0' },
          { id: holderRole.id, type: 0, allow: String(VIEW), deny: '0' },
        ],
      }));
  }

  /* ---- 7. the front door */
  const welcome = chan('welcome');
  if (!welcome) throw new Error('#welcome not found');
  act('mint permanent invite on #welcome', async () => {
    const inv = await call('POST', `/channels/${welcome.id}/invites`, { max_age: 0, max_uses: 0, unique: false });
    console.log(`  INVITE: https://discord.gg/${inv.code}`);
  });

  /* ---- run */
  if (!acts.length) { console.log('nothing to do; the server already matches'); return; }
  for (const a of acts) {
    console.log((APPLY ? 'doing: ' : 'would: ') + a.label);
    if (APPLY) await a.fn();
  }
  if (!APPLY) console.log('\nrun again with --apply to make it so.');
};

main().catch((e) => { console.error(String(e.message || e)); process.exit(1); });
