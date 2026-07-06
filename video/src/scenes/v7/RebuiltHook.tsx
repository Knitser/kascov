import React from 'react';
import {AbsoluteFill, useCurrentFrame, useVideoConfig} from 'remotion';
import {Caption, seg, pop} from '../v2/shared';
import {DagBg, GHOST} from './DagBg';

export const REBUILT_DUR = 330;

export const RebuiltHook: React.FC = () => {
  const f = useCurrentFrame();
  const {fps} = useVideoConfig();
  const aliveIn = pop(f, fps, 108, 12);
  return (
    <AbsoluteFill style={{alignItems: 'center', justifyContent: 'center'}}>
      <DagBg dim={seg(f, 0, 44) * 0.9} />
      <Caption frame={f} fps={fps} at={10} size={44} weight={600} y={-120}>
        <span style={{color: GHOST.muted}}>every smart coin on Kaspa&nbsp;L1</span>
      </Caption>
      <div
        style={{
          fontFamily: GHOST.display,
          fontSize: 150,
          fontWeight: 800,
          letterSpacing: -6,
          lineHeight: 1,
          marginTop: 10,
          opacity: seg(f, 108, 124),
          transform: `scale(${0.9 + aliveIn * 0.1})`,
          background: `linear-gradient(96deg, ${GHOST.accent}, ${GHOST.born})`,
          WebkitBackgroundClip: 'text',
          backgroundClip: 'text',
          color: 'transparent',
          textShadow: '0 0 90px rgba(73,234,203,0.3)',
        }}
      >
        alive.
      </div>
      <div style={{marginTop: 40, fontSize: 30, color: GHOST.faint, opacity: seg(f, 200, 220)}}>
        born · moving · retiring — recorded live, remembered forever
      </div>
    </AbsoluteFill>
  );
};
