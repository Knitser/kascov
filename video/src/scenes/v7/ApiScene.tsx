import React from 'react';
import {AbsoluteFill, useCurrentFrame, useVideoConfig} from 'remotion';
import {Caption, pop, seg} from '../v2/shared';
import {DagBg, GHOST} from './DagBg';

export const API_DUR = 300;

const EPS = [
  '/data/testnet-10-live.json',
  '/data/testnet-10/c/{id}.json',
  '/data/testnet-10/families.json',
  '/data/testnet-10/stream',
];

export const ApiScene: React.FC = () => {
  const f = useCurrentFrame();
  const {fps} = useVideoConfig();
  return (
    <AbsoluteFill style={{alignItems: 'center', justifyContent: 'center'}}>
      <DagBg dim={0.3} />
      <Caption frame={f} fps={fps} at={6} size={56} weight={760} y={-190}>
        all of it, as <span style={{color: GHOST.accent}}>JSON</span>
      </Caption>
      <div style={{fontSize: 26, color: GHOST.faint, marginTop: -110, opacity: seg(f, 30, 46)}}>
        no keys · CORS open · a real API
      </div>
      <div style={{width: 960, marginTop: 60, display: 'flex', flexDirection: 'column', gap: 12}}>
        {EPS.map((ep, i) => {
          const at = 56 + i * 18;
          const inn = pop(f, fps, at, 13);
          return (
            <div
              key={ep}
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 16,
                background: 'rgba(11,26,23,0.72)',
                border: `1px solid rgba(120,220,200,0.1)`,
                borderRadius: 12,
                padding: '15px 22px',
                opacity: seg(f, at, at + 12),
                transform: `translateY(${(1 - inn) * 18}px)`,
              }}
            >
              <span style={{fontFamily: GHOST.mono, fontSize: 18, fontWeight: 600, color: GHOST.born, background: 'rgba(91,228,155,0.14)', border: '1px solid rgba(91,228,155,0.35)', borderRadius: 6, padding: '3px 9px'}}>GET</span>
              <span style={{fontFamily: GHOST.mono, fontSize: 24, color: GHOST.text}}>{ep}</span>
            </div>
          );
        })}
      </div>
    </AbsoluteFill>
  );
};
