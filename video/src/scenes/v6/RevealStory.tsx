import React from 'react';
import {AbsoluteFill, useCurrentFrame, useVideoConfig} from 'remotion';
import {Avatar} from '../../lib/Avatar';
import {T} from '../../theme';
import {Caption, pop, seg} from '../v2/shared';

/* V6 scene 3 (~6.5s): the spend revealed the program — kascov names the
   coin forever, every party labeled. lively-slate-urchin is real. */

export const REVEALSTORY_DUR = 390;

const COIN_ID = 'da2fe117968d68d825d3fc28aa9045222401243c269d168a74db2b82ef55d6d7';
const PARTIES = [
  ['arbiter_hash', 'f4445661ce47…'],
  ['buyer', '4b3402bea711…'],
  ['seller', '5e5e5e5e5e5e…'],
];

export const RevealStory: React.FC = () => {
  const f = useCurrentFrame();
  const {fps} = useVideoConfig();
  const cardIn = pop(f, fps, 60, 13);

  return (
    <AbsoluteFill style={{alignItems: 'center', justifyContent: 'center'}}>
      <Caption frame={f} fps={fps} at={6} size={54} weight={760} y={64}>
        spending it <span style={{color: T.accent}}>revealed the contract</span> on-chain
      </Caption>

      <div
        style={{
          width: 1100,
          marginTop: 42,
          background: T.card,
          border: `1.5px solid ${T.border}`,
          borderRadius: 18,
          padding: '28px 40px',
          opacity: seg(f, 60, 74),
          transform: `translateY(${(1 - cardIn) * 26}px)`,
        }}
      >
        <div style={{display: 'flex', alignItems: 'center', gap: 20, marginBottom: 20}}>
          <Avatar id={COIN_ID} size={56} />
          <span style={{fontFamily: T.mono, fontSize: 29, fontWeight: 700}}>lively-slate-urchin</span>
          <span style={{fontSize: 22, color: T.burn, fontWeight: 700, letterSpacing: 1}}>RETIRED</span>
          <span style={{fontSize: 22, color: T.faint}}>· its story ends, recorded forever</span>
        </div>

        <div
          style={{
            borderTop: `1px solid ${T.border}`,
            paddingTop: 20,
            opacity: seg(f, 120, 136),
          }}
        >
          <div style={{fontSize: 24, color: T.muted, marginBottom: 14}}>
            revealed at spend — the program this coin actually ran:
          </div>
          <span
            style={{
              fontFamily: T.mono,
              fontSize: 27,
              fontWeight: 700,
              color: T.accent,
              background: T.accentSoft,
              border: `1px solid ${T.borderStrong}`,
              borderRadius: 999,
              padding: '8px 22px',
            }}
          >
            SilverScript · Escrow
          </span>
          <div style={{display: 'flex', gap: 40, marginTop: 22}}>
            {PARTIES.map(([k, v], i) => (
              <div key={k} style={{fontFamily: T.mono, fontSize: 24, opacity: seg(f, 170 + i * 14, 184 + i * 14)}}>
                <span style={{color: T.faint}}>{k} = </span>
                <span style={{color: T.text}}>{v}</span>
              </div>
            ))}
          </div>
        </div>
      </div>

      <div style={{marginTop: 32, fontSize: 29, color: T.muted, opacity: seg(f, 280, 298)}}>
        named for <span style={{color: T.text, fontWeight: 650}}>everyone, permanently</span> — hash-verified, not guessed.
      </div>
    </AbsoluteFill>
  );
};
