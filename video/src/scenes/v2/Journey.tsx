import React from 'react';
import {
  AbsoluteFill,
  Easing,
  interpolateColors,
  random,
  useCurrentFrame,
  useVideoConfig,
} from 'remotion';
import {Avatar} from '../../lib/Avatar';
import {star} from '../../lib/data';
import {KindIcon} from '../../lib/icons';
import {KIND_COLOR, KIND_SOFT, T} from '../../theme';
import {Caption, HashTicker, map, pop, seg} from './shared';

/* =====================================================================
   Scene 3 (8s): the journey. The star coin (a real covenant that lived
   a full arc) travels a bezier path — BORN, MOVED ×3, RETIRED — leaving
   a glowing lineage trail. The camera follows, then pulls back to show
   the whole life as one timeline.
   ===================================================================== */

export const JOURNEY_DUR = 480;

const WORLD_W = 2340;

/* Five stops for the star's five real events (genesis, 3 moves, burn). */
const STOPS: {x: number; y: number}[] = [
  {x: 600, y: 640},
  {x: 1050, y: 400},
  {x: 1480, y: 660},
  {x: 1880, y: 405},
  {x: 2120, y: 620},
];

const TAG_AT = [10, 120, 210, 300, 390];
const LEGS: [number, number][] = [
  [56, 118],
  [148, 208],
  [238, 298],
  [328, 386],
];

const LABELS = ['BORN', 'MOVED', 'MOVED ×2', 'MOVED ×3', 'RETIRED'];

/* cubic bezier with horizontal tangents at each stop → smooth S-curves */
const ctrl = (k: number) => {
  const a = STOPS[k];
  const b = STOPS[k + 1];
  const dx = (b.x - a.x) * 0.46;
  return {c1: {x: a.x + dx, y: a.y}, c2: {x: b.x - dx, y: b.y}};
};

const bezPoint = (k: number, t: number): {x: number; y: number} => {
  const a = STOPS[k];
  const b = STOPS[k + 1];
  const {c1, c2} = ctrl(k);
  const u = 1 - t;
  const x =
    u * u * u * a.x + 3 * u * u * t * c1.x + 3 * u * t * t * c2.x + t * t * t * b.x;
  const y =
    u * u * u * a.y + 3 * u * u * t * c1.y + 3 * u * t * t * c2.y + t * t * t * b.y;
  return {x, y};
};

const legPath = (k: number): string => {
  const a = STOPS[k];
  const b = STOPS[k + 1];
  const {c1, c2} = ctrl(k);
  return `M ${a.x} ${a.y} C ${c1.x} ${c1.y}, ${c2.x} ${c2.y}, ${b.x} ${b.y}`;
};

/* approximate leg lengths once, at module scope */
const LEG_LEN = [0, 1, 2, 3].map((k) => {
  let len = 0;
  let prev = bezPoint(k, 0);
  for (let i = 1; i <= 48; i++) {
    const p = bezPoint(k, i / 48);
    len += Math.hypot(p.x - prev.x, p.y - prev.y);
    prev = p;
  }
  return len;
});

/* colors per stop (real event kinds) */
const KINDS = star.c.events.map((e) => e.kind);
const TXIDS = star.c.events.map((e) => e.txid);

/* faint parallax dust in the world background */
const DUST = Array.from({length: 42}, (_, i) => ({
  x: random(`dust-x-${i}`) * WORLD_W,
  y: 120 + random(`dust-y-${i}`) * 880,
  r: 1.5 + random(`dust-r-${i}`) * 2.5,
  o: 0.08 + random(`dust-o-${i}`) * 0.16,
}));

const Tag: React.FC<{
  frame: number;
  fps: number;
  stop: number;
  small: boolean;
}> = ({frame, fps, stop, small}) => {
  const at = TAG_AT[stop];
  if (frame < at) return null;
  const kind = KINDS[stop] ?? 'transition';
  const color = KIND_COLOR[kind];
  const softBg = KIND_SOFT[kind];
  const s = pop(frame, fps, at, 12);
  const o = seg(frame, at, at + 6, Easing.linear);
  const above = STOPS[stop].y > 540;
  const scale = small ? 0.94 : 1;
  return (
    <div
      style={{
        position: 'absolute',
        left: STOPS[stop].x,
        top: STOPS[stop].y + (above ? -128 : 128),
        transform: `translate(-50%, ${above ? '-100%' : '0%'}) scale(${s * scale})`,
        opacity: o,
        display: 'flex',
        flexDirection: above ? 'column' : 'column-reverse',
        alignItems: 'center',
        gap: 10,
      }}
    >
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 12,
          padding: '12px 24px',
          borderRadius: 14,
          background: `linear-gradient(180deg, ${softBg}, rgba(10,15,14,0.75))`,
          border: `2px solid ${color}`,
          boxShadow: `0 0 26px ${softBg}, inset 0 0 18px rgba(0,0,0,0.25)`,
        }}
      >
        <KindIcon kind={kind} size={30} color={color} />
        <span
          style={{
            fontFamily: T.mono,
            fontSize: 32,
            fontWeight: 700,
            letterSpacing: 3,
            color,
          }}
        >
          {LABELS[stop]}
        </span>
      </div>
      <HashTicker frame={frame} at={at + 4} txid={TXIDS[stop] ?? ''} size={21} />
    </div>
  );
};

export const Journey: React.FC = () => {
  const f = useCurrentFrame();
  const {fps} = useVideoConfig();

  /* ---- where is the coin? */
  let coin = STOPS[0];
  let hop = 0;
  for (let k = 0; k < LEGS.length; k++) {
    const [a, b] = LEGS[k];
    if (f >= b) {
      coin = STOPS[k + 1];
    } else if (f >= a) {
      const t = seg(f, a, b, Easing.inOut(Easing.cubic));
      coin = bezPoint(k, t);
      hop = Math.sin(Math.PI * t);
      break;
    } else {
      break;
    }
  }

  /* ---- camera: follow, then pull back to reveal the full timeline */
  const followX = Math.max(0, Math.min(WORLD_W - 1920, coin.x - 860));
  const followY = (coin.y - 540) * 0.3;
  const pull = seg(f, 424, 468, Easing.inOut(Easing.cubic));
  const s = 1 - pull * 0.2; /* 1 → 0.8 (2340*0.8 = 1872 < 1920) */
  const tx = -followX + (24 - -followX) * pull;
  const ty = -followY + (128 - -followY) * pull;

  /* ---- ring color follows the life events */
  const ringColor = interpolateColors(
    f,
    [TAG_AT[0], TAG_AT[0] + 14, TAG_AT[1], TAG_AT[1] + 14, TAG_AT[4], TAG_AT[4] + 16],
    [T.accent, T.born, T.born, T.move, T.move, T.burn]
  );

  const retired = seg(f, TAG_AT[4] + 4, TAG_AT[4] + 30);
  const coinScale = (1 + hop * 0.15) * (1 - retired * 0.06);

  return (
    <AbsoluteFill>
      {/* world container under camera transform */}
      <div
        style={{
          position: 'absolute',
          width: WORLD_W,
          height: 1080,
          transform: `translate(${tx}px, ${ty}px) scale(${s})`,
          transformOrigin: '0 0',
        }}
      >
        {/* parallax dust */}
        <svg
          width={WORLD_W}
          height={1080}
          style={{position: 'absolute', inset: 0}}
        >
          {DUST.map((d, i) => (
            <circle key={i} cx={d.x} cy={d.y} r={d.r} fill={T.accent} opacity={d.o} />
          ))}

          {/* the trail: glow layer + core, revealed with the coin */}
          {LEGS.map((leg, k) => {
            const r = seg(f, leg[0], leg[1], Easing.inOut(Easing.cubic));
            if (r <= 0) return null;
            const len = LEG_LEN[k];
            const color = KIND_COLOR[KINDS[k + 1] ?? 'transition'];
            return (
              <g key={k}>
                <path
                  d={legPath(k)}
                  fill="none"
                  stroke={color}
                  strokeOpacity={0.16}
                  strokeWidth={16}
                  strokeLinecap="round"
                  strokeDasharray={len}
                  strokeDashoffset={len * (1 - r)}
                />
                <path
                  d={legPath(k)}
                  fill="none"
                  stroke={color}
                  strokeOpacity={0.85}
                  strokeWidth={4.5}
                  strokeLinecap="round"
                  strokeDasharray={len}
                  strokeDashoffset={len * (1 - r)}
                />
              </g>
            );
          })}

          {/* stop markers + arrival ripples */}
          {STOPS.map((p, k) => {
            const at = TAG_AT[k];
            if (f < at - 2) return null;
            const kind = KINDS[k] ?? 'transition';
            const color = KIND_COLOR[kind];
            const rip = seg(f, at, at + 34, Easing.out(Easing.cubic));
            return (
              <g key={k}>
                {rip > 0 && rip < 1 && (
                  <circle
                    cx={p.x}
                    cy={p.y}
                    r={20 + rip * 96}
                    fill="none"
                    stroke={color}
                    strokeWidth={3}
                    opacity={(1 - rip) * 0.6}
                  />
                )}
                <circle cx={p.x} cy={p.y} r={9} fill={color} opacity={0.95} />
              </g>
            );
          })}
        </svg>

        {/* event tags at each stop */}
        {STOPS.map((_, k) => (
          <Tag key={k} frame={f} fps={fps} stop={k} small={pull > 0.5} />
        ))}

        {/* the traveling coin: avatar + identity ring */}
        <div
          style={{
            position: 'absolute',
            left: coin.x,
            top: coin.y,
            transform: `translate(-50%, -50%) scale(${coinScale})`,
          }}
        >
          <svg
            width={220}
            height={220}
            viewBox="-110 -110 220 220"
            style={{position: 'absolute', left: -110, top: -110, overflow: 'visible'}}
          >
            <circle
              cx={0}
              cy={0}
              r={88}
              fill="none"
              stroke={ringColor}
              strokeWidth={4}
              opacity={0.95}
              style={{filter: `drop-shadow(0 0 12px ${ringColor})`}}
            />
          </svg>
          <div
            style={{
              position: 'absolute',
              left: -65,
              top: -65,
              filter: `drop-shadow(0 0 18px rgba(112,199,186,0.3))`,
            }}
          >
            <Avatar id={star.c.covenant_id} size={130} />
          </div>
          {/* nameplate riding along */}
          <div
            style={{
              position: 'absolute',
              left: -200,
              width: 400,
              top: 96,
              textAlign: 'center',
              fontFamily: T.mono,
              fontSize: 24,
              color: T.muted,
              opacity: 0.9 - hop * 0.5,
            }}
          >
            {star.name}
          </div>
        </div>
      </div>

      {/* screen-fixed caption at the pull-back */}
      <Caption frame={f} fps={fps} at={434} size={60} weight={700} y={928}>
        every step is <span style={{color: T.accent}}>its life story</span>.
      </Caption>
    </AbsoluteFill>
  );
};
