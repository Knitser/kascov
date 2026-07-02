import React from 'react';
import {AbsoluteFill, Easing, random, useCurrentFrame, useVideoConfig} from 'remotion';
import {T} from '../../theme';
import {Caption, map, pop, seg, V2} from '../v2/shared';

/* =====================================================================
   V3 scene 1 (5s): the blockDAG alive, two beats of kinetic copy.
   Same DAG language as V2's hook, tightened — the coin story comes later.
   ===================================================================== */

export const HOOK3_DUR = 300;

type DagBlock = {x: number; y: number; spawn: number};
type DagEdge = {a: number; b: number; spawn: number};

const LANES = [235, 380, 525, 670, 815];
const COL_GAP = 112;
const COL_EVERY = 10;
const N_COLS = 24;
const PRE_COLS = 14;

const buildDag = (): {blocks: DagBlock[]; edges: DagEdge[]} => {
  const blocks: DagBlock[] = [];
  const edges: DagEdge[] = [];
  const colBlocks: number[][] = [];
  for (let i = 0; i < N_COLS; i++) {
    const ids: number[] = [];
    const r = random(`d3-count-${i}`);
    const count = r < 0.16 ? 1 : r < 0.66 ? 2 : 3;
    const laneOrder = [...LANES.keys()].sort(
      (a, b) => random(`d3-lane-${i}-${a}`) - random(`d3-lane-${i}-${b}`)
    );
    for (let k = 0; k < count; k++) {
      const lane = laneOrder[k];
      const x = 140 + i * COL_GAP + (random(`d3-jx-${i}-${k}`) - 0.5) * 26;
      const y = LANES[lane] + (random(`d3-jy-${i}-${k}`) - 0.5) * 52;
      const spawn = (i - PRE_COLS) * COL_EVERY + Math.floor(random(`d3-js-${i}-${k}`) * 6);
      ids.push(blocks.length);
      blocks.push({x, y, spawn});
    }
    colBlocks.push(ids);
    if (i > 0) {
      for (const id of ids) {
        const prev = colBlocks[i - 1];
        const prevSorted = [...prev].sort(
          (a, b) => Math.abs(blocks[a].y - blocks[id].y) - Math.abs(blocks[b].y - blocks[id].y)
        );
        const links = prevSorted.slice(
          0,
          1 + (random(`d3-e-${id}`) < 0.55 && prevSorted.length > 1 ? 1 : 0)
        );
        for (const p of links) edges.push({a: p, b: id, spawn: blocks[id].spawn});
        if (i > 1 && random(`d3-e2-${id}`) < 0.22) {
          const far = colBlocks[i - 2];
          const pick = far[Math.floor(random(`d3-e3-${id}`) * far.length)];
          edges.push({a: pick, b: id, spawn: blocks[id].spawn});
        }
      }
    }
  }
  return {blocks, edges};
};

const DAG = buildDag();
const BW = 56;
const BH = 40;

export const Hook3: React.FC = () => {
  const f = useCurrentFrame();
  const {fps} = useVideoConfig();

  const driftX = map(f, [0, HOOK3_DUR], [30, -80], Easing.linear);
  const dim = seg(f, 150, 190, Easing.inOut(Easing.quad));

  return (
    <AbsoluteFill>
      <AbsoluteFill
        style={{
          opacity: 1 - dim * 0.55,
          transform: `translateX(${driftX}px)`,
          transformOrigin: '50% 46%',
        }}
      >
        <svg width={1920} height={1080} viewBox="0 0 1920 1080" style={{position: 'absolute', inset: 0}}>
          {DAG.edges.map((e, i) => {
            const t = seg(f, e.spawn + 2, e.spawn + 16, Easing.out(Easing.quad));
            if (t <= 0) return null;
            const a = DAG.blocks[e.a];
            const b = DAG.blocks[e.b];
            const dx = b.x - a.x;
            const dy = b.y - a.y;
            const len = Math.hypot(dx, dy);
            return (
              <line
                key={i}
                x1={b.x - (dx / len) * (BW * 0.42)}
                y1={b.y - (dy / len) * (BW * 0.42)}
                x2={a.x + (dx / len) * (BW * 0.42)}
                y2={a.y + (dy / len) * (BW * 0.42)}
                stroke={T.accent}
                strokeOpacity={0.42}
                strokeWidth={2}
                strokeDasharray={len}
                strokeDashoffset={len * (1 - t)}
              />
            );
          })}
          {DAG.blocks.map((b, i) => {
            if (f < b.spawn) return null;
            const s = pop(f, fps, b.spawn, 13);
            return (
              <g
                key={i}
                opacity={Math.min(1, s * 1.3)}
                transform={`translate(${b.x} ${b.y}) scale(${s}) translate(${-b.x} ${-b.y})`}
              >
                <rect
                  x={b.x - BW / 2}
                  y={b.y - BH / 2}
                  width={BW}
                  height={BH}
                  rx={9}
                  fill="rgba(112, 199, 186, 0.13)"
                  stroke={T.accent}
                  strokeOpacity={0.95}
                  strokeWidth={2}
                />
                <circle cx={b.x} cy={b.y} r={4.5} fill={T.accent} opacity={0.8} />
              </g>
            );
          })}
        </svg>
      </AbsoluteFill>

      {/* scrim + captions */}
      <AbsoluteFill
        style={{
          opacity: seg(f, 40, 54) * 0.9,
          background: 'radial-gradient(1000px 340px at 50% 84%, rgba(6, 10, 9, 0.92), transparent 75%)',
        }}
      />
      <Caption frame={f} fps={fps} at={44} out={146} size={72} weight={750} y={840}>
        Kaspa mines <span style={{color: T.accent, fontFamily: T.mono, fontWeight: 700}}>10 blocks</span> per second.
      </Caption>
      <Caption frame={f} fps={fps} at={162} size={84} weight={800} y={470}>
        since Toccata, its coins{' '}
        <span
          style={{
            color: T.accent,
            fontFamily: T.mono,
            textShadow: `0 0 40px ${V2.glowAccent}`,
          }}
        >
          carry rules.
        </span>
      </Caption>
      <Caption frame={f} fps={fps} at={214} size={40} weight={500} color={T.muted} y={600}>
        smart coins — identity, state and programs, on L1
      </Caption>
    </AbsoluteFill>
  );
};
