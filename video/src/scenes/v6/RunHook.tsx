import React from 'react';
import {AbsoluteFill, useCurrentFrame, useVideoConfig} from 'remotion';
import {T} from '../../theme';
import {Caption, pop, seg} from '../v2/shared';

/* V6 scene 1 (~4.5s): contracts don't just sit there — they RUN. */

export const RUNHOOK_DUR = 270;

export const RunHook: React.FC = () => {
  const f = useCurrentFrame();
  const {fps} = useVideoConfig();
  const runIn = pop(f, fps, 120, 12);

  return (
    <AbsoluteFill style={{alignItems: 'center', justifyContent: 'center'}}>
      <Caption frame={f} fps={fps} at={8} size={60} weight={780} y={-90}>
        smart contracts on <span style={{color: T.accent}}>$KAS</span>
      </Caption>
      <Caption frame={f} fps={fps} at={46} size={60} weight={780} y={-8}>
        don&rsquo;t just sit there anymore.
      </Caption>

      <div
        style={{
          marginTop: 210,
          fontSize: 110,
          fontWeight: 830,
          letterSpacing: -2,
          color: T.accent,
          opacity: seg(f, 120, 134),
          transform: `scale(${0.9 + runIn * 0.1})`,
          textShadow: '0 0 80px rgba(112, 199, 186, 0.35)',
        }}
      >
        they RUN.
      </div>

      <div style={{marginTop: 40, fontSize: 30, color: T.muted, opacity: seg(f, 190, 208)}}>
        watch a real escrow settle itself on testnet-10 ↓
      </div>
    </AbsoluteFill>
  );
};
