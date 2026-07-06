import React from 'react';
import {AbsoluteFill, useCurrentFrame, useVideoConfig} from 'remotion';
import {T} from '../../theme';
import {Caption, map, seg} from '../v2/shared';

/* V6 scene 2 (~7.5s): one command — deploy an escrow, the arbiter settles
   it, the contract itself forces the payout. Real run, real output. */

export const ESCROWSETTLE_DUR = 450;

const CMD = 'kascov-lab escrow-demo';
const BIRTH_AT = 100;
const WAIT_AT = 160;
const SETTLE_AT = 250;

const Line: React.FC<{at: number; children: React.ReactNode}> = ({at, children}) => {
  const f = useCurrentFrame();
  if (f < at) return null;
  return <div style={{marginTop: 10, opacity: seg(f, at, at + 10)}}>{children}</div>;
};

export const EscrowSettle: React.FC = () => {
  const f = useCurrentFrame();
  const {fps} = useVideoConfig();
  const typed = CMD.slice(0, Math.floor(map(f, [26, 80], [0, CMD.length])));

  return (
    <AbsoluteFill style={{alignItems: 'center', justifyContent: 'center'}}>
      <Caption frame={f} fps={fps} at={6} size={54} weight={760} y={70}>
        a real escrow, <span style={{color: T.accent}}>one command</span>
      </Caption>

      <div
        style={{
          width: 1220,
          borderRadius: 16,
          background: '#0b0f0e',
          border: `1.5px solid ${T.border}`,
          padding: '22px 28px',
          fontFamily: T.mono,
          fontSize: 25,
          marginTop: 40,
          opacity: seg(f, 16, 32),
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
        <Line at={BIRTH_AT}>
          <span style={{color: T.born, fontWeight: 700}}>BIRTH</span>
          <span style={{color: T.muted}}>    covenant </span>
          <span style={{color: T.accent}}>da2fe11796…</span>
          <span style={{color: T.muted}}> holds 5 TKAS in escrow</span>
        </Line>
        <Line at={WAIT_AT}>
          <span style={{color: T.faint}}>arbiter signs · the contract checks the payout itself…</span>
        </Line>
        <Line at={SETTLE_AT}>
          <span style={{color: T.burn, fontWeight: 700}}>SETTLED</span>
          <span style={{color: T.muted}}>  Escrow → buyer </span>
          <span style={{color: T.text, fontWeight: 650}}>(4.99999000 TKAS released)</span>
        </Line>
      </div>

      {f >= SETTLE_AT + 70 && (
        <div style={{marginTop: 34, fontSize: 30, color: T.muted, opacity: seg(f, SETTLE_AT + 70, SETTLE_AT + 88)}}>
          no middleman. the <span style={{color: T.text, fontWeight: 650}}>script itself</span> forced where the money could go.
        </div>
      )}
    </AbsoluteFill>
  );
};
