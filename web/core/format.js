/* kascov core/format — pure presentation helpers: deterministic naming,
   avatars, HTML escaping, number/time formatting, payload peeks, and the
   plain-words glossary. No state, no network, no DOM — safe to import from
   anywhere (app.js, future view modules). */

/* ------------------------------------------------------- friendly names */

const ADJECTIVES = [
  'brave','quick','silent','gentle','bold','clever','curious','dizzy',
  'eager','fierce','glad','happy','humble','jolly','keen','lively',
  'lucky','mellow','nimble','noble','patient','playful','proud','quiet',
  'rapid','restless','shy','sleepy','sly','snappy','steady','stubborn',
  'sunny','swift','tidy','tiny','vivid','wandering','wise','zesty',
];
const COLORS = [
  'teal','amber','coral','indigo','jade','crimson',
  'cobalt','olive','violet','copper','pearl','slate',
];
const ANIMALS = [
  'otter','lynx','crane','fox','owl','badger','heron','marmot',
  'falcon','tortoise','hare','raven','seal','ibis','moth','newt',
  'panda','quail','robin','stoat','tapir','urchin','vole','wren',
  'yak','zebra','gecko','dolphin','ferret','magpie','hedgehog','jackal',
  'kiwi','lemur','mole','narwhal','osprey','puffin','squid','toad',
];

function idByte(id, i) {
  const v = parseInt(id.slice(i * 2, i * 2 + 2), 16);
  return Number.isNaN(v) ? 0 : v;
}

function friendlyName(id) {
  const adj = ADJECTIVES[(idByte(id, 0) * 256 + idByte(id, 1)) % ADJECTIVES.length];
  const col = COLORS[(idByte(id, 2) * 256 + idByte(id, 3)) % COLORS.length];
  const ani = ANIMALS[(idByte(id, 4) * 256 + idByte(id, 5)) % ANIMALS.length];
  return `${adj}-${col}-${ani}`;
}

/* a template name is "semantic" when it names an actual contract or protocol
   object ("SilverScript · Escrow", "KCC20 token", "genesis0 · list") rather
   than a bare state shape — the ubiquitous p2pk/p2sh shapes stay secondary.
   Returns the name, or null. */
function semanticTemplate(t) {
  return t && !/^p2(pk|sh)/.test(t) ? t : null;
}

/* --------------------------------------------------------------- avatar */

function avatarSvg(id, size) {
  const b = (i) => idByte(id, i);
  const hue = (b(6) * 256 + b(7)) % 360;
  const hue2 = (hue + 60 + (b(8) % 150)) % 360;
  const bg = `hsl(${hue} 45% 17%)`;
  const ring = `hsl(${hue} 50% 42%)`;
  const count = 2 + (b(9) % 2);
  let shapes = '';
  for (let k = 0; k < count; k++) {
    const o = 10 + k * 5;                       // bytes 10.. / 15.. / 20..
    const kind = b(o) % 5;
    const ang = (b(o + 1) / 255) * Math.PI * 2;
    const dist = 3 + (b(o + 2) % 11);           // 3..13 from center
    const cx = +(32 + Math.cos(ang) * dist).toFixed(1);
    const cy = +(32 + Math.sin(ang) * dist).toFixed(1);
    const s = 8 + (b(o + 3) % 8);               // 8..15
    const rot = b(o + 4) % 90;
    const col = `hsl(${(hue2 + k * 47) % 360} ${60 + (b(o) % 25)}% ${58 + (b(o + 1) % 16)}%)`;
    if (kind === 0) {
      shapes += `<circle cx="${cx}" cy="${cy}" r="${(s * 0.72).toFixed(1)}" fill="${col}" opacity="0.92"/>`;
    } else if (kind === 1) {
      shapes += `<rect x="${(cx - s / 2).toFixed(1)}" y="${(cy - s / 2).toFixed(1)}" width="${s}" height="${s}" rx="2" fill="${col}" opacity="0.92" transform="rotate(${rot} ${cx} ${cy})"/>`;
    } else if (kind === 2) {
      const h = s * 0.9;
      const p1 = `${cx},${(cy - h).toFixed(1)}`;
      const p2 = `${(cx - h * 0.87).toFixed(1)},${(cy + h * 0.5).toFixed(1)}`;
      const p3 = `${(cx + h * 0.87).toFixed(1)},${(cy + h * 0.5).toFixed(1)}`;
      shapes += `<polygon points="${p1} ${p2} ${p3}" fill="${col}" opacity="0.92" transform="rotate(${rot} ${cx} ${cy})"/>`;
    } else if (kind === 3) {
      shapes += `<circle cx="${cx}" cy="${cy}" r="${(s * 0.65).toFixed(1)}" fill="none" stroke="${col}" stroke-width="3.5" opacity="0.92"/>`;
    } else {
      shapes += `<rect x="${(cx - s / 2).toFixed(1)}" y="${(cy - s / 2).toFixed(1)}" width="${s}" height="${s}" rx="2" fill="${col}" opacity="0.92" transform="rotate(45 ${cx} ${cy})"/>`;
    }
  }
  return `<svg class="avatar" viewBox="0 0 64 64" width="${size}" height="${size}" aria-hidden="true" focusable="false">` +
    `<circle cx="32" cy="32" r="30" fill="${bg}" stroke="${ring}" stroke-width="2.5"/>${shapes}</svg>`;
}
/* ----------------------------------------------------------------- icons */

const ICONS = {
  born: '<svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false">' +
    '<path d="M12 21v-8"/><path d="M12 13C12 9.2 9.3 6.6 5 6.2c.4 4.4 3 7 7 6.8z"/><path d="M12 11c0-3 2-5.2 6-5.6-.3 3.4-2.4 5.4-6 5.6z"/></svg>',
  move: '<svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false">' +
    '<path d="M20.5 3.5v5h-5"/><path d="M3.7 10a8.5 8.5 0 0 1 14.2-4l2.6 2.5"/><path d="M3.5 20.5v-5h5"/><path d="M20.3 14a8.5 8.5 0 0 1-14.2 4l-2.6-2.5"/></svg>',
  burn: '<svg viewBox="0 0 24 24" width="20" height="20" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false">' +
    '<path d="M12 2.5c.6 3 2.2 4.7 4 6.5 1.8 1.8 3 3.6 3 6a7 7 0 0 1-14 0c0-1.8.7-3.4 1.8-4.6.4 1.1 1 1.9 2 2.4C8.2 9.4 9.6 5.5 12 2.5z"/><path d="M12 21.5a3.2 3.2 0 0 1-3.2-3.2c0-1.5 1.2-2.6 2-3.6.5-.6.9-1.3 1.2-2 1.3 1.6 3.2 3.4 3.2 5.6a3.2 3.2 0 0 1-3.2 3.2z"/></svg>',
};

const KIND_META = {
  genesis:    { icon: 'born', cls: 'kind-born' },
  transition: { icon: 'move', cls: 'kind-move' },
  burn:       { icon: 'burn', cls: 'kind-burn' },
};

/* plain-words explanations, surfaced as hover/long-press titles wherever
   jargon appears — the site should never assume the reader knows KIPs */
const GLOSSARY = {
  alive: 'this smart coin still has live (unspent) state on the network — its story can continue',
  retired: 'every piece of this coin\u2019s state has been spent without continuing the covenant — its story ended (recorded here forever)',
  genesis: 'born: a transaction created this coin\u2019s permanent identity (its KIP-20 covenant id)',
  transition: 'moved: the coin\u2019s state was spent and continued under the same identity — same coin, new state',
  burn: 'retired: the state was spent without continuing the covenant — the value left, the identity ended',
  'p2pk state': 'the simplest state shape: a public key + OpCheckSig — whoever holds the key controls this piece of state',
  'p2sh commitment': 'a 35-byte hash commitment (OpBlake2b <hash> OpEqual) — the actual program stays hidden until the coin is spent, then kascov captures and verifies it',
  live_states: 'state pieces (UTXOs) with this shape that are unspent right now',
  ever_seen: 'every state piece with this shape kascov has ever indexed, spent or not',
  ran_at_spend: 'revealed programs: hidden P2SH programs this shape, seen and hash-verified when their coins were spent',
  lineage: 'whether kascov saw this coin\u2019s whole life — \u201cno\u201d means it was born before we started watching, so earlier history is honestly missing',
  digest_born: 'new smart coins created in the last 24 hours',
  digest_moved: 'state transitions in the last 24 hours — coins changing hands or updating state',
  digest_retired: 'coins whose story ended in the last 24 hours',
};
const esc = (s) => String(s).replace(/[&<>"']/g, (c) => (
  { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]
));
function ordinal(n) {
  const s = ['th', 'st', 'nd', 'rd'];
  const v = n % 100;
  return n + (s[(v - 20) % 10] || s[v] || s[0]);
}
function fmtInt(n) { return Number(n).toLocaleString('en-US'); }
function relTime(ms) {
  const diff = Date.now() - ms;
  if (diff < 0) return 'just now';
  const sec = diff / 1000;
  if (sec < 45) return 'just now';
  if (sec < 90) return 'about a minute ago';
  const min = sec / 60;
  if (min < 45) return `about ${Math.round(min)} minutes ago`;
  if (min < 90) return 'about an hour ago';
  const hr = min / 60;
  if (hr < 22) return `about ${Math.round(hr)} hours ago`;
  const day = hr / 24;
  if (day < 1.5) return 'yesterday';
  if (day < 26) return `${Math.round(day)} days ago`;
  const mon = day / 30.4;
  if (mon < 11.5) return `${Math.round(mon)} months ago`;
  return `${Math.round(day / 365)} years ago`;
}

function relTimeShort(ms) {
  return relTime(ms)
    .replace('about ', '')
    .replace('a minute ago', '1m ago')
    .replace(/(\d+) minutes ago/, '$1m ago')
    .replace('an hour ago', '1h ago')
    .replace(/(\d+) hours ago/, '$1h ago')
    .replace('yesterday', '1d ago')
    .replace(/(\d+) days ago/, '$1d ago');
}

function fmtClock(ms, withSeconds, withDate) {
  const d = new Date(ms);
  const opt = { hour: '2-digit', minute: '2-digit', hour12: false };
  if (withSeconds) opt.second = '2-digit';
  const time = d.toLocaleTimeString([], opt);
  if (!withDate) return time;
  return `${d.toLocaleDateString([], { month: 'short', day: 'numeric' })} ${time}`;
}

function fmtSpan(ms) {
  const min = ms / 60000;
  if (min < 2) return 'about a minute';
  if (min < 90) return `${Math.round(min)} minutes`;
  const hr = min / 60;
  if (hr < 36) return `${Math.round(hr)} hour${Math.round(hr) === 1 ? '' : 's'}`;
  const day = Math.round(hr / 24);
  return `${day} day${day === 1 ? '' : 's'}`;
}
function shortHex(hex, head, tail) {
  if (!hex || hex.length <= head + tail + 1) return hex || '';
  return `${hex.slice(0, head)}…${hex.slice(-tail)}`;
}
/* decode a little-endian hex byte string into a human integer ("71,753") —
   the client-side twin of the worker's KCC20 state-amount decode (8 LE
   bytes, same bytes that feed the derived supply). BigInt keeps 8-byte
   values exact. Returns null when the input isn't plain even-length hex,
   so callers can fall back to showing the raw value instead of lying. */
function leAmount(hex) {
  if (typeof hex !== 'string' || hex.length === 0 || hex.length % 2 !== 0 ||
      !/^[0-9a-fA-F]+$/.test(hex)) return null;
  let v = 0n;
  for (let i = hex.length - 2; i >= 0; i -= 2) {
    v = (v << 8n) | BigInt(parseInt(hex.slice(i, i + 2), 16));
  }
  return v.toLocaleString('en-US');
}
/* provenance chip: does the index hold this coin's every state back to
   genesis? optional field — render nothing when the export didn't ship it. */
function lineageBadge(c) {
  if (!c || typeof c.lineage_complete !== 'boolean') return '';
  return c.lineage_complete
    ? `<span class="flag flag-yes" title="every state back to genesis is indexed — this coin’s origin is provable">provably genesis ✓</span>`
    : `<span class="flag flag-off" title="indexing began after this coin was born — its earliest history is missing">adopted mid-life</span>`;
}
/* decode a tx payload for humans: printable bytes read as text, everything
   else falls back to a truncated hex peek. returns null when there's nothing
   to show. `label` is the compact display, `title` the full value tooltip. */
function payloadPeek(hex, len) {
  if (typeof hex === 'string' && hex.length >= 2) {
    const bytes = [];
    for (let i = 0; i + 1 < hex.length; i += 2) bytes.push(parseInt(hex.slice(i, i + 2), 16));
    const printable = bytes.length &&
      bytes.every((b) => b === 9 || b === 10 || b === 13 || (b >= 32 && b <= 126));
    if (printable) {
      const full = bytes.map((b) => String.fromCharCode(b)).join('');
      const flat = full.replace(/\s+/g, ' ').trim();
      const clipped = flat.length > 48 ? flat.slice(0, 48) + '…' : flat;
      return { label: `“${clipped}”`, title: full, bytes: bytes.length, mono: false };
    }
    return { label: shortHex(hex, 12, 8), title: hex, bytes: bytes.length, mono: true };
  }
  if (len != null) return { label: `${fmtInt(len)} bytes`, title: '', bytes: null, mono: false };
  return null;
}
/* exact UTC stamp for tooltips on relative times */
function utcTitle(ms) {
  return new Date(ms).toISOString().replace('T', ' ').replace(/\.\d+Z$/, ' UTC');
}
/* compact absolute time, always UTC: "Jul 5, 14:32 UTC" (year only when
   it isn't this year) — shown inline so nobody has to hover, mobile included */
const MONTHS = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
function absShort(ms) {
  const d = new Date(ms);
  const year = d.getUTCFullYear() === new Date().getUTCFullYear() ? '' : ` ${d.getUTCFullYear()}`;
  const hh = String(d.getUTCHours()).padStart(2, '0');
  const mm = String(d.getUTCMinutes()).padStart(2, '0');
  return `${MONTHS[d.getUTCMonth()]} ${d.getUTCDate()}${year}, ${hh}:${mm} UTC`;
}

export {
  idByte, friendlyName, semanticTemplate, avatarSvg,
  ICONS, KIND_META, GLOSSARY,
  esc, ordinal, fmtInt,
  relTime, relTimeShort, fmtClock, fmtSpan, shortHex, leAmount,
  lineageBadge, payloadPeek, utcTitle, absShort,
};
