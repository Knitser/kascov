import React from 'react';
import {
  AbsoluteFill,
  Easing,
  random,
  useCurrentFrame,
  useVideoConfig,
} from 'remotion';
import {Avatar} from '../../lib/Avatar';
import {star} from '../../lib/data';
import {T} from '../../theme';
import {Caption, map, pop, seg, V2} from './shared';

/* =====================================================================
   Scene 1+2 (0–420 @60fps): the blockDAG alive, then the turn — the DAG
   recedes, kinetic type lands, and one real coin assembles from
   particles, gets its name and its identity ring.
   ===================================================================== */

export const HOOK_TURN_DUR = 420;

/* ------------------------------------------------- DAG graph (static) */

type DagBlock = {x: number; y: number; spawn: number};
type DagEdge = {a: number; b: number; spawn: number};

const LANES = [225, 372, 519, 666, 813];
const COL_GAP = 112;
const COL_EVERY = 11; /* frames between columns → ~10 blocks/s w/ multi-lane cols */
const N_COLS = 24;
const PRE_COLS = 12; /* columns that already exist on frame 0 */

const buildDag = (): {blocks: DagBlock[]; edges: DagEdge[]} => {
  const blocks: DagBlock[] = [];
  const edges: DagEdge[] = [];
  const colBlocks: number[][] = [];
  for (let i = 0; i < N_COLS; i++) {
    const ids: number[] = [];
    /* 1–3 blocks per column, weighted toward 2 */
    const r = random(`dag-count-${i}`);
    const count = r < 0.16 ? 1 : r < 0.66 ? 2 : 3;
    const laneOrder = [...LANES.keys()].sort(
      (a, b) => random(`dag-lane-${i}-${a}`) - random(`dag-lane-${i}-${b}`)
    );
    for (let k = 0; k < count; k++) {
      const lane = laneOrder[k];
      const x = 140 + i * COL_GAP + (random(`dag-jx-${i}-${k}`) - 0.5) * 26;
      const y = LANES[lane] + (random(`dag-jy-${i}-${k}`) - 0.5) * 52;
      /* the first PRE_COLS columns pre-exist at frame 0; the rest arrive on the beat */
      const spawn = (i - PRE_COLS) * COL_EVERY + Math.floor(random(`dag-js-${i}-${k}`) * 6);
      ids.push(blocks.length);
      blocks.push({x, y, spawn});
    }
    colBlocks.push(ids);
    /* edges: every block links to 1–2 blocks in the previous column */
    if (i > 0) {
      for (const id of ids) {
        const prev = colBlocks[i - 1];
        const prevSorted = [...prev].sort(
          (a, b) =>
            Math.abs(blocks[a].y - blocks[id].y) - Math.abs(blocks[b].y - blocks[id].y)
        );
        const links = prevSorted.slice(
          0,
          1 + (random(`dag-e-${id}`) < 0.55 && prevSorted.length > 1 ? 1 : 0)
        );
        for (const p of links) {
          edges.push({a: p, b: id, spawn: blocks[id].spawn});
        }
        /* occasional long edge two columns back — very DAG */
        if (i > 1 && random(`dag-e2-${id}`) < 0.22) {
          const far = colBlocks[i - 2];
          const pick = far[Math.floor(random(`dag-e3-${id}`) * far.length)];
          edges.push({a: pick, b: id, spawn: blocks[id].spawn});
        }
      }
    }
  }
  return {blocks, edges};
};

const DAG = buildDag();

const BLOCK_W = 56;
const BLOCK_H = 40;

const DagField: React.FC<{frame: number; fps: number}> = ({frame, fps}) => {
  return (
    <svg
      width={1920}
      height={1080}
      viewBox="0 0 1920 1080"
      style={{position: 'absolute', inset: 0}}
    >
      {/* edges below blocks */}
      {DAG.edges.map((e, i) => {
        const t = seg(frame, e.spawn + 2, e.spawn + 16, Easing.out(Easing.quad));
        if (t <= 0) return null;
        const a = DAG.blocks[e.a];
        const b = DAG.blocks[e.b];
        const dx = b.x - a.x;
        const dy = b.y - a.y;
        const len = Math.hypot(dx, dy);
        return (
          <line
            key={i}
            x1={b.x - (dx / len) * (BLOCK_W * 0.42)}
            y1={b.y - (dy / len) * (BLOCK_W * 0.42)}
            x2={a.x + (dx / len) * (BLOCK_W * 0.42)}
            y2={a.y + (dy / len) * (BLOCK_W * 0.42)}
            stroke={T.accent}
            strokeOpacity={0.42}
            strokeWidth={2}
            strokeDasharray={len}
            strokeDashoffset={len * (1 - t)}
          />
        );
      })}
      {/* blocks */}
      {DAG.blocks.map((b, i) => {
        if (frame < b.spawn) return null;
        const s = pop(frame, fps, b.spawn, 13);
        return (
          <g
            key={i}
            opacity={Math.min(1, s * 1.3)}
            transform={`translate(${b.x} ${b.y}) scale(${s}) translate(${-b.x} ${-b.y})`}
          >
            <rect
              x={b.x - BLOCK_W / 2}
              y={b.y - BLOCK_H / 2}
              width={BLOCK_W}
              height={BLOCK_H}
              rx={9}
              fill="rgba(112, 199, 186, 0.13)"
              stroke={T.accent}
              strokeOpacity={0.95}
              strokeWidth={2}
            />
            {/* faint bloom */}
            <rect
              x={b.x - BLOCK_W / 2 - 5}
              y={b.y - BLOCK_H / 2 - 5}
              width={BLOCK_W + 10}
              height={BLOCK_H + 10}
              rx={12}
              fill="none"
              stroke={T.accent}
              strokeOpacity={0.14}
              strokeWidth={5}
            />
            <circle cx={b.x} cy={b.y} r={4.5} fill={T.accent} opacity={0.8} />
          </g>
        );
      })}
    </svg>
  );
};

/* ------------------------------------------- coin assembly particles */

type Particle = {
  ang: number;
  r0: number;
  delay: number;
  dur: number;
  size: number;
  white: boolean;
};

const N_PART = 60;
const PARTICLES: Particle[] = Array.from({length: N_PART}, (_, i) => ({
  ang: random(`asm-a-${i}`) * Math.PI * 2,
  r0: 340 + random(`asm-r-${i}`) * 320,
  delay: random(`asm-d-${i}`) * 26,
  dur: 30 + random(`asm-t-${i}`) * 18,
  size: 3 + random(`asm-s-${i}`) * 4.5,
  white: random(`asm-w-${i}`) < 0.3,
}));

/* -------------------------------------------------------- the scene */

const WORDS = ['and', 'now,', 'its', 'coins', 'carry'];
const ASM_AT = 252; /* particles start */
const NAME_AT = 318;
const RING_AT = 330;

export const HookTurn: React.FC = () => {
  const f = useCurrentFrame();
  const {fps} = useVideoConfig();

  /* ---- DAG camera: gentle drift during the hook, recede at the turn */
  const driftX = map(f, [0, 160], [40, -90], Easing.linear);
  const driftS = map(f, [0, 160], [1.06, 1.0], Easing.linear);
  const recede = seg(f, 150, 205, Easing.inOut(Easing.cubic));
  const dagScale = driftS * (1 - recede * 0.2);
  const dagOpacity = 1 - recede * 0.88;
  const dagY = recede * -40;

  /* ---- kinetic line timing */
  const lineOut = seg(f, 242, 264, Easing.inOut(Easing.quad));

  /* ---- coin assembly */
  const coinIn = pop(f, fps, ASM_AT + 22, 15);
  const coinOpacity = seg(f, ASM_AT + 18, ASM_AT + 42);
  const nameChars = Math.floor(map(f, [NAME_AT, NAME_AT + 44], [0, star.name.length], Easing.linear));
  const caretOn = f >= NAME_AT - 4 && f < NAME_AT + 56 && f % 14 < 9;
  const ringT = seg(f, RING_AT, RING_AT + 62, Easing.inOut(Easing.cubic));
  const RING_R = 148;
  const RING_C = 2 * Math.PI * RING_R;

  /* ---- exit: the coin shrinks + glides toward where the journey begins */
  const exit = seg(f, 392, 420, Easing.inOut(Easing.cubic));
  const coinX = 960 + (600 - 960) * exit;
  const coinY = 500 + (640 - 500) * exit;
  const coinScale = 1 - exit * 0.41; /* 220 → ~130 avatar */

  return (
    <AbsoluteFill style={{backgroundColor: 'transparent'}}>
      {/* the blockDAG, alive from frame 0 */}
      <AbsoluteFill
        style={{
          opacity: dagOpacity,
          transform: `translate(${driftX}px, ${dagY}px) scale(${dagScale})`,
          transformOrigin: '50% 46%',
        }}
      >
        <DagField frame={f} fps={fps} />
      </AbsoluteFill>

      {/* hook caption — slams in at 0.8s */}
      {f < 175 && (
        <>
          {/* scrim so the caption pops over the busy DAG */}
          <AbsoluteFill
            style={{
              opacity: seg(f, 46, 58) * (1 - seg(f, 150, 168)),
              background:
                'radial-gradient(900px 300px at 50% 88%, rgba(6, 10, 9, 0.9), transparent 75%)',
            }}
          />
          <Caption frame={f} fps={fps} at={48} out={150} size={74} weight={750} y={860}>
            Kaspa mines <span style={{color: T.accent, fontFamily: T.mono, fontWeight: 700}}>10 blocks</span> per second.
          </Caption>
        </>
      )}

      {/* the turn: kinetic type, word by word */}
      {f >= 164 && f < 280 && (
        <AbsoluteFill
          style={{
            alignItems: 'center',
            justifyContent: 'center',
            opacity: 1 - lineOut,
            transform: `translateY(${-lineOut * 70}px)`,
          }}
        >
          <div style={{display: 'flex', gap: 26, alignItems: 'baseline'}}>
            {WORDS.map((w, i) => {
              const at = 168 + i * 8;
              const s = pop(f, fps, at, 13);
              const o = seg(f, at, at + 6, Easing.linear);
              return (
                <span
                  key={i}
                  style={{
                    fontSize: 92,
                    fontWeight: 700,
                    letterSpacing: -1,
                    opacity: o,
                    display: 'inline-block',
                    transform: `translateY(${(1 - s) * 46}px) scale(${0.9 + s * 0.1})`,
                  }}
                >
                  {w}
                </span>
              );
            })}
            {(() => {
              const at = 168 + WORDS.length * 8 + 6;
              const s = pop(f, fps, at, 11);
              const o = seg(f, at, at + 5, Easing.linear);
              return (
                <span
                  style={{
                    fontSize: 104,
                    fontWeight: 800,
                    color: T.accent,
                    fontFamily: T.mono,
                    letterSpacing: 0,
                    opacity: o,
                    display: 'inline-block',
                    transform: `translateY(${(1 - s) * 54}px) scale(${0.82 + s * 0.18})`,
                    textShadow: `0 0 44px ${V2.glowAccent}`,
                  }}
                >
                  rules.
                </span>
              );
            })()}
          </div>
        </AbsoluteFill>
      )}

      {/* one coin assembles from particles */}
      {f >= ASM_AT && (
        <AbsoluteFill>
          <div
            style={{
              position: 'absolute',
              left: coinX,
              top: coinY,
              transform: `translate(-50%, -50%) scale(${coinScale})`,
            }}
          >
            {/* converging particles */}
            <svg
              width={1400}
              height={1400}
              viewBox="-700 -700 1400 1400"
              style={{position: 'absolute', left: -700, top: -700, overflow: 'visible'}}
            >
              {PARTICLES.map((p, i) => {
                const t = seg(f, ASM_AT + p.delay, ASM_AT + p.delay + p.dur, Easing.inOut(Easing.cubic));
                if (t <= 0 || t >= 1) return null;
                const r = p.r0 * (1 - t) + 88 * t;
                const ang = p.ang + t * 0.9; /* slight swirl on the way in */
                const o = Math.sin(Math.PI * t) * 0.95;
                return (
                  <circle
                    key={i}
                    cx={Math.cos(ang) * r}
                    cy={Math.sin(ang) * r}
                    r={p.size * (1 - t * 0.4)}
                    fill={p.white ? '#e9f1ef' : T.accent}
                    opacity={o}
                  />
                );
              })}
              {/* identity ring drawing 0→360° */}
              {ringT > 0 && (
                <circle
                  cx={0}
                  cy={0}
                  r={RING_R}
                  fill="none"
                  stroke={T.accent}
                  strokeWidth={5}
                  strokeLinecap="round"
                  strokeDasharray={RING_C}
                  strokeDashoffset={RING_C * (1 - ringT)}
                  transform="rotate(-90)"
                  style={{filter: `drop-shadow(0 0 14px ${V2.glowAccent})`}}
                />
              )}
            </svg>

            {/* the avatar itself */}
            <div
              style={{
                position: 'absolute',
                left: -110,
                top: -110,
                opacity: coinOpacity,
                transform: `scale(${0.6 + coinIn * 0.4})`,
                filter: `drop-shadow(0 0 ${26 * coinOpacity}px rgba(112,199,186,0.35))`,
              }}
            >
              <Avatar id={star.c.covenant_id} size={220} />
            </div>

            {/* friendly name types out */}
            {f >= NAME_AT - 6 && (
              <div
                style={{
                  position: 'absolute',
                  left: -400,
                  width: 800,
                  top: 186,
                  textAlign: 'center',
                  fontFamily: T.mono,
                  fontSize: 46,
                  fontWeight: 600,
                  letterSpacing: 1,
                  color: T.text,
                  whiteSpace: 'pre',
                }}
              >
                {star.name.slice(0, nameChars)}
                <span
                  style={{
                    display: 'inline-block',
                    width: 18,
                    height: 36,
                    marginLeft: 6,
                    transform: 'translateY(3px)',
                    background: T.accent,
                    opacity: caretOn ? 0.9 : 0.1,
                  }}
                />
              </div>
            )}

            {/* tiny caption under the name */}
            {f >= NAME_AT + 50 && (
              <div
                style={{
                  position: 'absolute',
                  left: -400,
                  width: 800,
                  top: 252,
                  textAlign: 'center',
                  fontSize: 27,
                  color: T.muted,
                  opacity: seg(f, NAME_AT + 52, NAME_AT + 68) * (1 - exit),
                }}
              >
                a smart coin, born on Kaspa testnet-10
              </div>
            )}
          </div>
        </AbsoluteFill>
      )}
    </AbsoluteFill>
  );
};
