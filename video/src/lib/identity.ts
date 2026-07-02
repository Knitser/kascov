/* Friendly-name + identicon logic ported 1:1 from the kascov website
   (web/app.js) so the video shows the exact same characters as the site.
   Everything is deterministic from the covenant_id bytes. */

export const ADJECTIVES = [
  'brave', 'quick', 'silent', 'gentle', 'bold', 'clever', 'curious', 'dizzy',
  'eager', 'fierce', 'glad', 'happy', 'humble', 'jolly', 'keen', 'lively',
  'lucky', 'mellow', 'nimble', 'noble', 'patient', 'playful', 'proud', 'quiet',
  'rapid', 'restless', 'shy', 'sleepy', 'sly', 'snappy', 'steady', 'stubborn',
  'sunny', 'swift', 'tidy', 'tiny', 'vivid', 'wandering', 'wise', 'zesty',
];

export const COLORS = [
  'teal', 'amber', 'coral', 'indigo', 'jade', 'crimson',
  'cobalt', 'olive', 'violet', 'copper', 'pearl', 'slate',
];

export const ANIMALS = [
  'otter', 'lynx', 'crane', 'fox', 'owl', 'badger', 'heron', 'marmot',
  'falcon', 'tortoise', 'hare', 'raven', 'seal', 'ibis', 'moth', 'newt',
  'panda', 'quail', 'robin', 'stoat', 'tapir', 'urchin', 'vole', 'wren',
  'yak', 'zebra', 'gecko', 'dolphin', 'ferret', 'magpie', 'hedgehog', 'jackal',
  'kiwi', 'lemur', 'mole', 'narwhal', 'osprey', 'puffin', 'squid', 'toad',
];

export function idByte(id: string, i: number): number {
  const v = parseInt(id.slice(i * 2, i * 2 + 2), 16);
  return Number.isNaN(v) ? 0 : v;
}

export function friendlyName(id: string): string {
  const adj = ADJECTIVES[(idByte(id, 0) * 256 + idByte(id, 1)) % ADJECTIVES.length];
  const col = COLORS[(idByte(id, 2) * 256 + idByte(id, 3)) % COLORS.length];
  const ani = ANIMALS[(idByte(id, 4) * 256 + idByte(id, 5)) % ANIMALS.length];
  return `${adj}-${col}-${ani}`;
}

/* ------------------------------------------------ avatar (identicon) */

export type AvatarShape = {
  kind: number;
  cx: number;
  cy: number;
  s: number;
  rot: number;
  col: string;
};

export type AvatarParams = {
  bg: string;
  ring: string;
  shapes: AvatarShape[];
};

/* Same byte-mixing as web/app.js avatarSvg(). */
export function avatarParams(id: string): AvatarParams {
  const b = (i: number) => idByte(id, i);
  const hue = (b(6) * 256 + b(7)) % 360;
  const hue2 = (hue + 60 + (b(8) % 150)) % 360;
  const bg = `hsl(${hue}, 45%, 17%)`;
  const ring = `hsl(${hue}, 50%, 42%)`;
  const count = 2 + (b(9) % 2);
  const shapes: AvatarShape[] = [];
  for (let k = 0; k < count; k++) {
    const o = 10 + k * 5;
    const kind = b(o) % 5;
    const ang = (b(o + 1) / 255) * Math.PI * 2;
    const dist = 3 + (b(o + 2) % 11);
    const cx = +(32 + Math.cos(ang) * dist).toFixed(1);
    const cy = +(32 + Math.sin(ang) * dist).toFixed(1);
    const s = 8 + (b(o + 3) % 8);
    const rot = b(o + 4) % 90;
    const col = `hsl(${(hue2 + k * 47) % 360}, ${60 + (b(o) % 25)}%, ${58 + (b(o + 1) % 16)}%)`;
    shapes.push({kind, cx, cy, s, rot, col});
  }
  return {bg, ring, shapes};
}

/* ------------------------------------------------------ formatting */

export function fmtInt(n: number): string {
  return Number(n).toLocaleString('en-US');
}

/* Same tiering as the site's fmtAmount(); the snapshot is testnet-10. */
export function fmtAmount(sompi: number): string {
  const kas = sompi / 1e8;
  let str: string;
  if (kas >= 1000) str = kas.toLocaleString('en-US', {maximumFractionDigits: 0});
  else if (kas >= 1) str = kas.toLocaleString('en-US', {minimumFractionDigits: 2, maximumFractionDigits: 2});
  else if (kas === 0) str = '0';
  else str = kas.toLocaleString('en-US', {maximumFractionDigits: 4});
  return `${str} TKAS`;
}

export function ordinal(n: number): string {
  const s = ['th', 'st', 'nd', 'rd'];
  const v = n % 100;
  return n + (s[(v - 20) % 10] || s[v] || s[0]);
}

export function shortHex(hex: string, head: number, tail: number): string {
  if (!hex || hex.length <= head + tail + 1) return hex || '';
  return `${hex.slice(0, head)}…${hex.slice(-tail)}`;
}
