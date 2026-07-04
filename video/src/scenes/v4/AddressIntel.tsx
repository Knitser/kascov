import React from 'react';
import {AbsoluteFill, useCurrentFrame, useVideoConfig} from 'remotion';
import {Avatar} from '../../lib/Avatar';
import {T} from '../../theme';
import {Caption, map, pop, seg} from '../v2/shared';

/* =====================================================================
   V4 scene 4 (~7s): address intelligence + what's-running analytics.
   A pasted address resolves to every smart coin it ever touched; then
   the template bars answer "what actually runs on this network".
   ===================================================================== */

export const ADDR_DUR = 420;

const ADDR = 'kaspatest:qq7f3k…x2m9';
const TYPED = 'kaspatest:qq7f3k';
const COINS = [
  {id: 'a17e02c4d1a17e02', name: 'brave-slate-jackal', role: 'controls now', live: true},
  {id: '4477aa02bb4477aa', name: 'gentle-coral-lynx', role: 'controls now', live: true},
  {id: '99d1e3f50299d1e3', name: 'tiny-cobalt-zebra', role: 'past owner', live: false},
  {id: 'c3a94b7d02c3a94b', name: 'swift-copper-falcon', role: 'past owner', live: false},
];
const TPL_AT = 240;
const TEMPLATES = [
  {name: 'p2pk state', n: 742, w: 1.0, color: T.accent},
  {name: 'p2sh commitment', n: 92, w: 0.16, color: T.move},
  {name: 'SilverScript · Mecenas', n: 14, w: 0.05, color: T.born},
];

export const AddressIntel: React.FC = () => {
  const f = useCurrentFrame();
  const {fps} = useVideoConfig();

  const typed = TYPED.slice(0, Math.floor(map(f, [20, 80], [0, TYPED.length])));
  const searched = f >= 96;

  return (
    <AbsoluteFill style={{alignItems: 'center', justifyContent: 'center'}}>
      <Caption frame={f} fps={fps} at={6} size={56} weight={760} y={86}>
        paste an <span style={{color: T.accent}}>address</span> — meet every coin it touched
      </Caption>

      {/* search bar */}
      <div
        style={{
          width: 980,
          marginTop: 44,
          display: 'flex',
          alignItems: 'center',
          gap: 18,
          background: T.card,
          border: `1.5px solid ${searched ? T.borderStrong : T.border}`,
          borderRadius: 999,
          padding: '20px 34px',
          fontFamily: T.mono,
          fontSize: 30,
          opacity: seg(f, 10, 30),
        }}
      >
        <span style={{color: T.faint, fontSize: 26}}>⌕</span>
        <span style={{color: T.text}}>
          {searched ? ADDR : typed}
          {!searched && <span style={{opacity: Math.sin(f / 8) > 0 ? 1 : 0}}>│</span>}
        </span>
      </div>

      {/* result cards fan out */}
      <div style={{display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 20, width: 1060, marginTop: 34}}>
        {COINS.map((c, i) => {
          const at = 110 + i * 12;
          const inP = pop(f, fps, at, 13);
          return (
            <div
              key={c.id}
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 18,
                background: T.card,
                border: `1px solid ${T.border}`,
                borderRadius: 16,
                padding: '16px 22px',
                opacity: seg(f, at, at + 8),
                transform: `translateY(${(1 - inP) * 24}px)`,
              }}
            >
              <Avatar id={c.id.repeat(4)} size={46} />
              <span style={{fontFamily: T.mono, fontSize: 26, fontWeight: 700, color: T.text}}>{c.name}</span>
              <span
                style={{
                  marginLeft: 'auto',
                  fontSize: 21,
                  fontWeight: 650,
                  padding: '5px 14px',
                  borderRadius: 999,
                  background: c.live ? T.bornSoft : 'rgba(255,255,255,0.06)',
                  color: c.live ? T.born : T.faint,
                }}
              >
                {c.role}
              </span>
            </div>
          );
        })}
      </div>

      {/* what's running here */}
      <div style={{width: 1060, marginTop: 40, opacity: seg(f, TPL_AT, TPL_AT + 18)}}>
        <div style={{fontSize: 30, fontWeight: 720, color: T.text, marginBottom: 18}}>
          …and see <span style={{color: T.accent}}>what&apos;s running</span> on the whole network
        </div>
        {TEMPLATES.map((t, i) => {
          const at = TPL_AT + 16 + i * 14;
          const grow = seg(f, at, at + 30);
          return (
            <div key={t.name} style={{display: 'flex', alignItems: 'center', gap: 20, marginBottom: 14, opacity: seg(f, at, at + 8)}}>
              <span style={{fontFamily: T.mono, fontSize: 24, color: T.text, width: 330, flex: 'none'}}>{t.name}</span>
              <div style={{height: 20, borderRadius: 10, width: 560 * t.w * grow, background: t.color, boxShadow: `0 0 18px ${t.color}44`}} />
              <span style={{fontFamily: T.mono, fontSize: 24, fontWeight: 700, color: t.color}}>
                {Math.round(t.n * grow)}
              </span>
            </div>
          );
        })}
      </div>
    </AbsoluteFill>
  );
};
