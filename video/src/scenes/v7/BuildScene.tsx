import React from 'react';
import {AbsoluteFill, useCurrentFrame, useVideoConfig} from 'remotion';
import {Caption, map, pop, seg} from '../v2/shared';
import {DagBg, GHOST} from './DagBg';

export const BUILD_DUR = 400;

const CMD = 'cargo run -p kascov-lab -- escrow-demo';
const BIRTH = 130;
const SETTLE = 200;
const CARD = 262;

export const BuildScene: React.FC = () => {
  const f = useCurrentFrame();
  const {fps} = useVideoConfig();
  const typed = CMD.slice(0, Math.floor(map(f, [24, 96], [0, CMD.length])));
  const cardIn = pop(f, fps, CARD, 13);
  return (
    <AbsoluteFill style={{alignItems: 'center', justifyContent: 'center'}}>
      <DagBg dim={0.32} />
      <Caption frame={f} fps={fps} at={6} size={56} weight={760} y={-250}>
        make your <span style={{color: GHOST.accent}}>own</span>
      </Caption>
      <div
        style={{
          width: 1120,
          borderRadius: 16,
          background: '#06120f',
          border: `1px solid rgba(120,220,200,0.14)`,
          padding: '22px 28px',
          fontFamily: GHOST.mono,
          fontSize: 25,
          marginTop: 20,
          opacity: seg(f, 16, 32),
        }}
      >
        <div style={{display: 'flex', gap: 8, marginBottom: 14}}>
          {['#e0655f', '#e0b95f', '#5be49b'].map((c) => (
            <span key={c} style={{width: 13, height: 13, borderRadius: 99, background: c}} />
          ))}
        </div>
        <div style={{color: GHOST.text}}>
          <span style={{color: GHOST.accent}}>$ </span>
          {typed}
          {f < BIRTH && <span style={{opacity: Math.sin(f / 7) > 0 ? 1 : 0}}>│</span>}
        </div>
        {f >= BIRTH && (
          <div style={{marginTop: 12, opacity: seg(f, BIRTH, BIRTH + 10)}}>
            <span style={{color: GHOST.born, fontWeight: 700}}>BIRTH</span>
            <span style={{color: GHOST.muted}}>    covenant </span>
            <span style={{color: GHOST.accent}}>da2fe11796…</span>
            <span style={{color: GHOST.muted}}> holds 5 TKAS</span>
          </div>
        )}
        {f >= SETTLE && (
          <div style={{marginTop: 10, opacity: seg(f, SETTLE, SETTLE + 10)}}>
            <span style={{color: GHOST.burn, fontWeight: 700}}>SETTLED</span>
            <span style={{color: GHOST.muted}}>  Escrow → buyer </span>
            <span style={{color: GHOST.text}}>(4.99999 TKAS)</span>
          </div>
        )}
      </div>
      {f >= CARD && (
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 18,
            marginTop: 30,
            padding: '16px 30px',
            borderRadius: 999,
            background: 'rgba(11,26,23,0.85)',
            border: `1px solid rgba(73,234,203,0.34)`,
            opacity: seg(f, CARD, CARD + 10),
            transform: `translateY(${(1 - cardIn) * 24}px)`,
          }}
        >
          <span style={{fontFamily: GHOST.mono, fontSize: 26, fontWeight: 700, color: GHOST.text}}>lively-slate-urchin</span>
          <span style={{fontSize: 22, color: GHOST.accent, fontWeight: 700}}>revealed at spend</span>
          <span style={{fontSize: 22, color: GHOST.muted}}>· SilverScript · Escrow</span>
        </div>
      )}
      {f >= CARD + 44 && (
        <div style={{marginTop: 22, fontSize: 27, color: GHOST.muted, opacity: seg(f, CARD + 44, CARD + 60)}}>
          the contract ran itself — on-chain, named forever.
        </div>
      )}
    </AbsoluteFill>
  );
};
