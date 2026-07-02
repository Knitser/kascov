/* The hero decode — a real mainnet covenant (quiet-pearl-zebra) whose actual
   program kascov revealed at spend time and disassembled. Generated from live
   data into src/decode.json; see the session that built the video. */

import raw from '../decode.json';
import {T} from '../theme';

export type OpGroup =
  | 'push'
  | 'standard'
  | 'introspection'
  | 'covenant'
  | 'zk'
  | 'unknown';

export type Inst = {
  off: number;
  name: string;
  group: OpGroup;
  data: string | null;
};

export type Decode = {
  covenant_id: string;
  name: string;
  network: string;
  status: string;
  events: number;
  commit_asm: string[];
  commit_hex: string;
  revealed_hex: string;
  instructions: Inst[];
  groups: Partial<Record<OpGroup, number>>;
  spent_txid: string;
  /* network stats frozen at generation time, for the odometers */
  stats?: {
    tn10: {covenants: number; events: number};
    mainnet: {covenants: number; events: number};
  };
};

export const decode = raw as unknown as Decode;

/* Group → color, matching the site's #/decode op classes. */
export const GROUP_COLOR: Record<OpGroup, string> = {
  push: T.accent,
  standard: T.text,
  introspection: T.move,
  covenant: T.burn,
  zk: '#c398ff',
  unknown: '#ff8b8b',
};

export const GROUP_SOFT: Record<OpGroup, string> = {
  push: 'rgba(112, 199, 186, 0.14)',
  standard: 'rgba(255, 255, 255, 0.06)',
  introspection: 'rgba(152, 168, 255, 0.16)',
  covenant: 'rgba(242, 165, 102, 0.16)',
  zk: 'rgba(195, 152, 255, 0.18)',
  unknown: 'rgba(255, 139, 139, 0.16)',
};

export const GROUP_LABEL: Record<OpGroup, string> = {
  push: 'data push',
  standard: 'standard',
  introspection: 'KIP-17 introspection',
  covenant: 'KIP-20 covenant',
  zk: 'KIP-16 zero-knowledge',
  unknown: 'unknown',
};
