import React from 'react';
import {AbsoluteFill, useCurrentFrame, useVideoConfig} from 'remotion';

/* The living BlockDAG, ported from web/dag.js as a deterministic Remotion
   background: columns drift left, each block links to a parent in the
   previous column, one selected-parent chain glows teal, and covenants
   ignite in lifecycle colors. Deterministic (hash of a stable world-column
   index) so every frame is reproducible. */

export const GHOST = {
  bg: '#05100e',
  accent: '#49eacb',
  born: '#5be49b',
  move: '#8ab4ff',
  burn: '#ffb067',
  text: '#eaf6f2',
  muted: '#9fb8b2',
  faint: '#7f9a94',
  display: 'Space Grotesk, Inter, system-ui, sans-serif',
  mono: '"JetBrains Mono", ui-monospace, monospace',
};

const hash = (n: number): number => {
  const x = Math.sin(n * 127.1 + 11.7) * 43758.5453;
  return x - Math.floor(x);
};
const LIFE = [GHOST.born, GHOST.move, GHOST.burn];

const colCount = (wc: number): number => 2 + Math.floor(hash(wc) * 3.99);
const nodeY = (wc: number, r: number, count: number, h: number): number =>
  h * 0.1 + ((r + 0.5) / count) * h * 0.8 + (hash(wc * 13 + r) - 0.5) * ((h * 0.5) / count);
const chainRow = (wc: number): number => Math.floor(hash(wc * 3.7) * colCount(wc));

export const DagBg: React.FC<{dim?: number}> = ({dim = 1}) => {
  const frame = useCurrentFrame();
  const {width: W, height: H, fps} = useVideoConfig();
  const COL_W = 155;
  const drift = (frame / fps) * 24;
  const base = Math.floor(drift / COL_W);
  const off = drift % COL_W;
  const nCols = Math.ceil(W / COL_W) + 3;

  type N = {x: number; y: number; r: number; life: string | null; chain: boolean};
  const cols: {wc: number; x: number; nodes: N[]}[] = [];
  for (let i = -1; i < nCols; i++) {
    const wc = base + i;
    const x = i * COL_W - off;
    const count = colCount(wc);
    const cr = chainRow(wc);
    const nodes: N[] = [];
    for (let r = 0; r < count; r++) {
      const life = hash(wc * 31 + r) < 0.15 ? LIFE[Math.floor(hash(wc * 17 + r) * 3)] : null;
      nodes.push({x, y: nodeY(wc, r, count, H), r: 2.6 + hash(wc * 5 + r) * 2, life, chain: r === cr});
    }
    cols.push({wc, x, nodes});
  }

  const edges: React.ReactNode[] = [];
  const dots: React.ReactNode[] = [];
  for (let c = 1; c < cols.length; c++) {
    const cur = cols[c];
    const prev = cols[c - 1];
    cur.nodes.forEach((n, r) => {
      const pc = colCount(prev.wc);
      const pr = Math.floor(hash(cur.wc * 7 + r) * pc);
      const par = prev.nodes[pr];
      if (par) {
        const isChain = n.chain && par.chain;
        edges.push(
          <line
            key={`e${c}-${r}`}
            x1={par.x}
            y1={par.y}
            x2={n.x}
            y2={n.y}
            stroke={isChain ? GHOST.accent : '#78aaa0'}
            strokeOpacity={isChain ? 0.34 : 0.1}
            strokeWidth={isChain ? 1.6 : 1}
          />
        );
      }
    });
  }
  cols.forEach((col, c) =>
    col.nodes.forEach((n, r) => {
      const col2 = n.life ?? (n.chain ? GHOST.accent : '#b4d6ce');
      const glow = !!n.life || n.chain;
      dots.push(
        <circle
          key={`n${c}-${r}`}
          cx={n.x}
          cy={n.y}
          r={n.r}
          fill={col2}
          fillOpacity={n.life ? 1 : n.chain ? 1 : 0.5}
          style={glow ? {filter: `drop-shadow(0 0 7px ${col2})`} : undefined}
        />
      );
    })
  );

  return (
    <AbsoluteFill style={{opacity: dim}}>
      <svg width={W} height={H} style={{position: 'absolute', inset: 0}}>
        {edges}
        {dots}
      </svg>
    </AbsoluteFill>
  );
};
