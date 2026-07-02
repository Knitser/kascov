/* Derives everything the video shows from the real snapshot in src/data.json —
   stats, friendly names (with the site's collision suffix), birth values,
   the cascade cast, and the one covenant that lived a full arc. */

import raw from '../data.json';
import {fmtAmount, friendlyName, ordinal} from './identity';

export type EventKind = 'genesis' | 'transition' | 'burn';

export type CovEvent = {
  accepting_block: string;
  accepting_daa: number;
  kind: EventKind;
  seq: number;
  txid: string;
};

export type Utxo = {
  created_daa: number;
  live: boolean;
  value: number;
};

export type Covenant = {
  covenant_id: string;
  event_count: number;
  events: CovEvent[];
  genesis_daa?: number | null;
  genesis_txid?: string | null;
  last_activity_daa: number;
  live_utxos?: number;
  live_value: number;
  status: 'active' | 'burned';
  utxos?: Utxo[];
};

export type KascovData = {
  covenants: Covenant[];
  generated_at_ms: number;
  network: string;
  stats: {
    active: number;
    burned: number;
    covenants: number;
    events: number;
    last_activity_daa: number;
    live_value: number;
  };
};

const data = raw as unknown as KascovData;

export const stats = data.stats;

export type Entry = {
  c: Covenant;
  name: string;
  moves: number;
  birthValue: number;
};

/* Friendly names can collide; the site suffixes duplicates — do the same. */
const nameCounts = new Map<string, number>();
for (const c of data.covenants) {
  const n = friendlyName(c.covenant_id);
  nameCounts.set(n, (nameCounts.get(n) || 0) + 1);
}

function toEntry(c: Covenant): Entry {
  let name = friendlyName(c.covenant_id);
  if ((nameCounts.get(name) || 0) > 1) name += `-${c.covenant_id.slice(0, 4)}`;
  const moves = c.events.filter((e) => e.kind === 'transition').length;
  let birthValue = 0;
  if (c.genesis_daa != null && Array.isArray(c.utxos)) {
    for (const u of c.utxos) {
      if (u.created_daa === c.genesis_daa) birthValue += u.value;
    }
  }
  return {c, name, moves, birthValue};
}

/* Same ordering as the site: most recent activity first. */
export const entries: Entry[] = data.covenants
  .map(toEntry)
  .sort((a, b) => (b.c.last_activity_daa || 0) - (a.c.last_activity_daa || 0));

/* The star: our own covenant (dizzy-coral-tapir, created by hand on launch
   day) when present, else the first covenant with a complete arc:
   genesis + >=2 transitions + burn. */
export const star: Entry =
  entries.find((e) => e.c.covenant_id.startsWith('05cfc476')) ??
  entries.find((e) => {
    const kinds = e.c.events.map((ev) => ev.kind);
    return (
      kinds.includes('genesis') &&
      kinds.filter((k) => k === 'transition').length >= 2 &&
      kinds.includes('burn')
    );
  }) ??
  entries[0];

export type StarStep = {
  kind: EventKind;
  label: string;
  deltaS: number;
  txid: string;
};

const starGenesisDaa = star.c.genesis_daa ?? star.c.events[0].accepting_daa;

export const starSteps: StarStep[] = star.c.events.map((ev) => {
  let label: string;
  if (ev.kind === 'genesis') {
    label =
      star.birthValue > 0
        ? `born — holding ${fmtAmount(star.birthValue)}`
        : 'born';
  } else if (ev.kind === 'transition') {
    const nth = star.c.events.filter(
      (e) => e.kind === 'transition' && e.seq <= ev.seq
    ).length;
    label = `moved (${ordinal(nth)} time)`;
  } else {
    label =
      star.moves === 0
        ? 'retired without ever moving'
        : star.moves === 1
          ? 'retired after 1 move'
          : `retired after ${star.moves} moves`;
  }
  /* The chain ticks ~10 DAA per second (MS_PER_DAA = 100 on the site). */
  const deltaS = (ev.accepting_daa - starGenesisDaa) / 10;
  return {kind: ev.kind, label, deltaS, txid: ev.txid};
});

export const starLifespanS =
  (star.c.last_activity_daa - starGenesisDaa) / 10;

/* The cascade cast: real covenants with a known genesis and a birth value,
   in site order, excluding the star (saved for its own scene). */
export const cascade: Entry[] = entries
  .filter(
    (e) =>
      e.c.genesis_daa != null &&
      e.birthValue > 0 &&
      e.c.covenant_id !== star.c.covenant_id
  )
  .slice(0, 9);

/* The scene-2 mascot: the covenant that gets a face on screen. */
export const mascot: Entry = cascade[0];

export function cardLine(e: Entry): string {
  if (e.c.status !== 'active') {
    return e.moves === 1 ? 'retired after 1 move' : `retired after ${e.moves} moves`;
  }
  if (e.moves > 0) {
    return `moved ${e.moves}× · holds ${fmtAmount(e.c.live_value)}`;
  }
  return `was born, holding ${fmtAmount(e.birthValue)}`;
}
