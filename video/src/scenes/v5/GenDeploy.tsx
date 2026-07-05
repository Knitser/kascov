import React from 'react';
import {AbsoluteFill, useCurrentFrame, useVideoConfig} from 'remotion';
import {Avatar} from '../../lib/Avatar';
import {T} from '../../theme';
import {Caption, map, pop, seg} from '../v2/shared';

/* V5 scene 3 (~6s): one command births it — and this exact coin is real:
   rapid-cobalt-heron, deployed on TN10 while testing this very feature. */

export const GENDEPLOY_DUR = 360;

const COIN_ID = '2c00edf6f7366e16e4b771e9f84d17eeb1216b6f83e5e4965d0ca4ff951f52c0';
const CMD = 'kascov-lab deploy --program-hex 6b6c76009c63…  --value 10';
const BIRTH_AT = 150;
const CARD_AT = 200;

export const GenDeploy: React.FC = () => {
  const f = useCurrentFrame();
  const {fps} = useVideoConfig();
  const typed = CMD.slice(0, Math.floor(map(f, [30, 120], [0, CMD.length])));
  const cardIn = pop(f, fps, CARD_AT, 13);

  return (
    <AbsoluteFill style={{alignItems: 'center', justifyContent: 'center'}}>
      <Caption frame={f} fps={fps} at={6} size={56} weight={760} y={96}>
        one command <span style={{color: T.accent}}>births it for real</span>
      </Caption>

      {/* terminal */}
      <div
        style={{
          width: 1150,
          borderRadius: 16,
          background: '#0b0f0e',
          border: `1.5px solid ${T.border}`,
          padding: '20px 26px',
          fontFamily: T.mono,
          fontSize: 25,
          marginTop: 40,
          opacity: seg(f, 20, 36),
        }}
      >
        <div style={{display: 'flex', gap: 8, marginBottom: 14}}>
          {['#e0655f', '#e0b95f', '#6fd9a4'].map((c) => (
            <span key={c} style={{width: 13, height: 13, borderRadius: 99, background: c}} />
          ))}
        </div>
        <div style={{color: T.text}}>
          <span style={{color: T.accent}}>$ </span>
          {typed}
          {f < BIRTH_AT && <span style={{opacity: Math.sin(f / 7) > 0 ? 1 : 0}}>│</span>}
        </div>
        {f >= BIRTH_AT && (
          <div style={{marginTop: 12, opacity: seg(f, BIRTH_AT, BIRTH_AT + 10)}}>
            <span style={{color: T.born, fontWeight: 700}}>BIRTH</span>
            <span style={{color: T.muted}}>  covenant </span>
            <span style={{color: T.accent}}>{COIN_ID.slice(0, 18)}…</span>
          </div>
        )}
      </div>

      {/* the coin appears on kascov — for real */}
      {f >= CARD_AT && (
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 22,
            marginTop: 36,
            padding: '20px 34px',
            borderRadius: 999,
            background: T.card,
            border: `1.5px solid ${T.borderStrong}`,
            opacity: seg(f, CARD_AT, CARD_AT + 10),
            transform: `translateY(${(1 - cardIn) * 26}px)`,
          }}
        >
          <Avatar id={COIN_ID} size={56} />
          <span style={{fontFamily: T.mono, fontSize: 30, fontWeight: 700}}>rapid-cobalt-heron</span>
          <span style={{fontSize: 24, color: T.born, fontWeight: 700, letterSpacing: 1}}>ALIVE</span>
          <span style={{fontSize: 23, color: T.faint}}>· p2sh commitment · watchable a minute later</span>
        </div>
      )}
      {f >= CARD_AT + 60 && (
        <div style={{marginTop: 22, fontSize: 27, color: T.muted, opacity: seg(f, CARD_AT + 60, CARD_AT + 78)}}>
          this exact coin is <span style={{color: T.text, fontWeight: 650}}>real</span> — born on testnet-10 while we tested this feature.
        </div>
      )}
    </AbsoluteFill>
  );
};
