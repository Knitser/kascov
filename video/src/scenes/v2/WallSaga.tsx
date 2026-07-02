import React from 'react';
import {
  AbsoluteFill,
  Easing,
  random,
  useCurrentFrame,
  useVideoConfig,
} from 'remotion';
import {Avatar} from '../../lib/Avatar';
import {entries, star, stats} from '../../lib/data';
import {T} from '../../theme';
import {Caption, map, Odometer, pop, seg, Wordmark} from './shared';

/* =====================================================================
   Scenes 4+5 (12s, one component so the wall survives the transition):
   THE WALL — 54 real covenants cascade in, counters spin up. Then the
   TWIST — everything desaturates and crumbles to particles ("Kaspa
   nodes delete all of this after 3 days"), a beat of dark, a teal pulse
   sweeps, the wall reassembles, and the wordmark rises: kascov remembers.
   ===================================================================== */

export const WALL_DUR = 720;

/* ------------------------------------------------------------ layout */

const COLS = 6;
const ROWS = 9;
const CW = 279;
const CH = 72;
const GX = 17;
const GY = 16;
const X0 = 81;
const Y0 = 152;

/* cast: first 54 real covenants (site order), star moved to the center */
const buildCast = () => {
  const list = entries.slice(0, COLS * ROWS);
  const si = list.findIndex((e) => e.c.covenant_id === star.c.covenant_id);
  if (si >= 0) {
    const [s] = list.splice(si, 1);
    list.splice(21, 0, s); /* row 3, col 3 — center-ish */
  }
  return list.slice(0, COLS * ROWS);
};
const CAST = buildCast();

type CardMeta = {
  x: number;
  y: number;
  inAt: number;
  crumbleAt: number;
  backAt: number;
};

const CARDS: CardMeta[] = CAST.map((_, i) => {
  const row = Math.floor(i / COLS);
  const col = i % COLS;
  return {
    x: X0 + col * (CW + GX),
    y: Y0 + row * (CH + GY),
    inAt: 4 + (row + col) * 5 + random(`wall-in-${i}`) * 10,
    crumbleAt: 380 + random(`wall-cr-${i}`) * 52,
    /* reassembly rides the pulse: left columns first, right columns last */
    backAt: 522 + col * 4.5 + random(`wall-bk-${i}`) * 7,
  };
});

/* crumble particles: 6 per card, deterministic trajectories */
type Dust = {
  ci: number;
  ox: number;
  oy: number;
  vx: number;
  vy: number;
  r: number;
  teal: boolean;
};
const DUST: Dust[] = CARDS.flatMap((_, ci) =>
  Array.from({length: 6}, (_, k) => ({
    ci,
    ox: random(`dp-x-${ci}-${k}`) * CW,
    oy: random(`dp-y-${ci}-${k}`) * CH,
    vx: (random(`dp-vx-${ci}-${k}`) - 0.5) * 1.6,
    vy: -0.5 + random(`dp-vy-${ci}-${k}`) * 0.7,
    r: 1.6 + random(`dp-r-${ci}-${k}`) * 2.6,
    teal: random(`dp-c-${ci}-${k}`) < 0.4,
  }))
);

/* gather particles for the reassembly: converge onto each card */
const GATHER: {ci: number; ang: number; dist: number; r: number}[] = CARDS.flatMap(
  (_, ci) =>
    Array.from({length: 4}, (_, k) => ({
      ci,
      ang: random(`gp-a-${ci}-${k}`) * Math.PI * 2,
      dist: 110 + random(`gp-d-${ci}-${k}`) * 170,
      r: 1.8 + random(`gp-r-${ci}-${k}`) * 2.2,
    }))
);

const DIGITS_COV = String(stats.covenants).length;
const DIGITS_EV = String(stats.events).length;

/* --------------------------------------------------------------- card */

const Card: React.FC<{i: number; frame: number; fps: number}> = ({i, frame, fps}) => {
  const e = CAST[i];
  const m = CARDS[i];

  /* phase A: cascade in */
  const inS = pop(frame, fps, m.inAt, 14);
  const inO = seg(frame, m.inAt, m.inAt + 6, Easing.linear);

  /* phase B: crumble */
  const cr = seg(frame, m.crumbleAt, m.crumbleAt + 22, Easing.in(Easing.quad));

  /* phase D: reassemble */
  const back = frame >= m.backAt - 2 ? pop(frame, fps, m.backAt, 13) : 0;
  const backO = seg(frame, m.backAt, m.backAt + 5, Easing.linear);

  let opacity: number;
  let ty: number;
  let sc: number;
  let rot = 0;
  if (frame < m.crumbleAt) {
    opacity = inO;
    ty = (1 - inS) * 44;
    sc = 0.92 + inS * 0.08;
  } else if (cr < 1) {
    opacity = 1 - cr;
    ty = cr * cr * 34;
    sc = 1 - cr * 0.1;
    rot = (random(`rot-${i}`) - 0.5) * 7 * cr;
  } else if (frame < m.backAt - 2) {
    return null;
  } else {
    opacity = backO;
    ty = (1 - back) * 30;
    sc = 0.9 + back * 0.1;
  }
  if (opacity <= 0) return null;

  const isStar = e.c.covenant_id === star.c.covenant_id;
  const burned = e.c.status !== 'active';

  /* brief teal flash as a card is pulled back together by the pulse */
  const flash =
    frame >= m.backAt
      ? Math.max(0, 1 - (frame - m.backAt) / 16)
      : 0;

  return (
    <div
      style={{
        position: 'absolute',
        left: m.x,
        top: m.y,
        width: CW,
        height: CH,
        opacity,
        transform: `translateY(${ty}px) scale(${sc}) rotate(${rot}deg)`,
        display: 'flex',
        alignItems: 'center',
        gap: 13,
        padding: '0 16px',
        boxSizing: 'border-box',
        borderRadius: 13,
        background: T.card,
        border: `1.5px solid ${
          isStar
            ? 'rgba(112,199,186,0.65)'
            : flash > 0
              ? `rgba(112,199,186,${0.09 + flash * 0.55})`
              : T.border
        }`,
        boxShadow: isStar
          ? '0 0 22px rgba(112,199,186,0.22)'
          : flash > 0
            ? `0 0 ${24 * flash}px rgba(112,199,186,${0.3 * flash})`
            : undefined,
      }}
    >
      <Avatar id={e.c.covenant_id} size={40} />
      <span
        style={{
          fontFamily: T.mono,
          fontSize: 19,
          color: T.text,
          whiteSpace: 'nowrap',
          overflow: 'hidden',
          textOverflow: 'ellipsis',
          flex: 1,
        }}
      >
        {e.name}
      </span>
      <span
        style={{
          width: 9,
          height: 9,
          borderRadius: 5,
          background: burned ? T.burn : T.born,
          opacity: 0.85,
          flexShrink: 0,
        }}
      />
    </div>
  );
};

/* -------------------------------------------------------------- scene */

export const WallSaga: React.FC = () => {
  const f = useCurrentFrame();
  const {fps} = useVideoConfig();

  /* counters */
  const count = seg(f, 58, 150, Easing.out(Easing.cubic));
  const counterO = seg(f, 52, 66) * (1 - seg(f, 344, 364));
  const counterPop = 1 + (1 - pop(f, fps, 150, 12)) * 0.05;

  /* desaturation before the crumble */
  const gray = seg(f, 348, 372) * (1 - seg(f, 512, 530));
  const crumbleDim = 1 - seg(f, 348, 380) * 0.45 + seg(f, 512, 534) * 0.45;
  /* pull the wall down while big type owns the screen */
  const wmDim = seg(f, 566, 592);
  const dim = crumbleDim * (1 - counterO * 0.42) * (1 - wmDim * 0.5);

  /* the pulse */
  const pulseX = map(f, [510, 540], [-560, 2480], Easing.inOut(Easing.cubic));
  const pulseO = seg(f, 510, 516) * (1 - seg(f, 534, 544));
  const flashO = seg(f, 512, 518) * (1 - seg(f, 518, 546));

  /* wordmark moment */
  const wmS = pop(f, fps, 574, 14);
  /* hold, then leave cleanly before the crossfade so no double image */
  const wmO = seg(f, 574, 584) * (1 - seg(f, 700, 716));
  const wmScrim = seg(f, 566, 592) * 0.92;
  const wmGlow = 0.5 + 0.5 * Math.sin((f - 590) / 22);

  return (
    <AbsoluteFill>
      {/* THE WALL */}
      <AbsoluteFill
        style={{
          filter:
            gray > 0.01 || dim < 0.99
              ? `grayscale(${gray}) brightness(${dim})`
              : undefined,
        }}
      >
        {CAST.map((_, i) => (
          <Card key={i} i={i} frame={f} fps={fps} />
        ))}
      </AbsoluteFill>

      {/* crumble dust */}
      {f > 380 && f < 560 && (
        <svg width={1920} height={1080} style={{position: 'absolute', inset: 0}}>
          {DUST.map((d, i) => {
            const start = CARDS[d.ci].crumbleAt + 6;
            const t = f - start;
            if (t < 0 || t > 95) return null;
            const x = CARDS[d.ci].x + d.ox + d.vx * t;
            const y = CARDS[d.ci].y + d.oy + d.vy * t + 0.028 * t * t;
            const o =
              seg(t, 0, 8, Easing.linear) * (1 - seg(t, 55, 95, Easing.linear)) * 0.7;
            if (o <= 0) return null;
            return (
              <circle
                key={i}
                cx={x}
                cy={y}
                r={d.r}
                fill={d.teal ? T.accent : '#7d8d89'}
                opacity={o}
              />
            );
          })}
          {/* gather dust flying back in */}
          {GATHER.map((g, i) => {
            const end = CARDS[g.ci].backAt;
            const t = seg(f, end - 20, end, Easing.in(Easing.quad));
            if (t <= 0 || t >= 1) return null;
            const cx = CARDS[g.ci].x + CW / 2;
            const cy = CARDS[g.ci].y + CH / 2;
            const r = g.dist * (1 - t);
            return (
              <circle
                key={i}
                cx={cx + Math.cos(g.ang + t * 0.7) * r}
                cy={cy + Math.sin(g.ang + t * 0.7) * r}
                r={g.r}
                fill={T.accent}
                opacity={Math.sin(Math.PI * t) * 0.85}
              />
            );
          })}
        </svg>
      )}

      {/* center scrim + counters */}
      {f < 380 && (
        <>
          <AbsoluteFill
            style={{
              opacity: counterO,
              background:
                'linear-gradient(180deg, transparent 26%, rgba(4, 8, 7, 0.94) 40%, rgba(4, 8, 7, 0.94) 66%, transparent 80%)',
            }}
          />
          <div
            style={{
              position: 'absolute',
              left: 0,
              right: 0,
              top: 405,
              display: 'flex',
              justifyContent: 'center',
              alignItems: 'center',
              gap: 38,
              opacity: counterO,
              transform: `scale(${counterPop})`,
            }}
          >
            <div style={{display: 'flex', alignItems: 'center', gap: 24}}>
              <Odometer target={stats.covenants} progress={count} digits={DIGITS_COV} size={132} color={T.accent} />
              <span style={{fontSize: 46, fontWeight: 600, color: T.text}}>smart coins</span>
            </div>
            <span style={{fontSize: 46, color: T.faint}}>·</span>
            <div style={{display: 'flex', alignItems: 'center', gap: 24}}>
              <Odometer target={stats.events} progress={count} digits={DIGITS_EV} size={132} color={T.accent} />
              <span style={{fontSize: 46, fontWeight: 600, color: T.text}}>life events</span>
            </div>
          </div>
          <Caption frame={f} fps={fps} at={172} out={344} size={54} weight={600} color={T.muted} y={634}>
            this is the Kaspa testnet, <span style={{color: T.text}}>right now</span>.
          </Caption>
        </>
      )}

      {/* the delete caption over the crumble */}
      {f >= 366 && f < 512 && (
        <>
          <AbsoluteFill
            style={{
              opacity: seg(f, 370, 386) * (1 - seg(f, 474, 494)),
              background:
                'radial-gradient(1050px 380px at 50% 50%, rgba(5, 9, 8, 0.9), transparent 74%)',
            }}
          />
          <Caption frame={f} fps={fps} at={374} out={474} size={66} weight={700} y={478}>
            Kaspa nodes <span style={{color: T.burn}}>delete</span> all of this after{' '}
            <span style={{color: T.burn, fontFamily: T.mono}}>3 days</span>.
          </Caption>
        </>
      )}

      {/* teal pulse sweep + flash */}
      {pulseO > 0 && (
        <div
          style={{
            position: 'absolute',
            top: -60,
            bottom: -60,
            left: pulseX,
            width: 440,
            opacity: pulseO,
            transform: 'skewX(-11deg)',
            background:
              'linear-gradient(90deg, transparent, rgba(112,199,186,0.34) 45%, rgba(233,241,239,0.2) 50%, rgba(112,199,186,0.34) 55%, transparent)',
          }}
        />
      )}
      {flashO > 0 && (
        <AbsoluteFill
          style={{
            opacity: flashO * 0.35,
            background:
              'radial-gradient(1300px 800px at 50% 50%, rgba(112,199,186,0.65), transparent 70%)',
          }}
        />
      )}

      {/* the money moment: kascov remembers. */}
      {f >= 560 && (
        <>
          <AbsoluteFill
            style={{
              opacity: wmScrim,
              background:
                'linear-gradient(180deg, transparent 22%, rgba(4, 8, 7, 0.96) 38%, rgba(4, 8, 7, 0.96) 64%, transparent 80%)',
            }}
          />
          <div
            style={{
              position: 'absolute',
              left: 0,
              right: 0,
              top: 448,
              display: 'flex',
              justifyContent: 'center',
              opacity: wmO,
              transform: `translateY(${(1 - wmS) * 90}px)`,
            }}
          >
            <div style={{display: 'flex', alignItems: 'center', gap: 34}}>
              <Wordmark size={118} glow={0.35 + wmGlow * 0.3} />
              <span
                style={{
                  fontFamily: T.mono,
                  fontSize: 118,
                  fontWeight: 600,
                  color: T.accent,
                  letterSpacing: 1,
                  opacity: seg(f, 592, 606),
                  textShadow: `0 0 ${34 + wmGlow * 26}px rgba(112,199,186,0.5)`,
                }}
              >
                remembers<span style={{marginLeft: -30}}>.</span>
              </span>
            </div>
          </div>
        </>
      )}
    </AbsoluteFill>
  );
};
