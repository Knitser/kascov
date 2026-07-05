import React from 'react';
import {AbsoluteFill, useCurrentFrame, useVideoConfig} from 'remotion';
import {T} from '../../theme';
import {Caption, pop, seg} from '../v2/shared';

/* V5 scene 4 (~4.5s): the newcomer polish — tour + honest clocks. */

export const GENEXTRAS_DUR = 270;

export const GenExtras: React.FC = () => {
  const f = useCurrentFrame();
  const {fps} = useVideoConfig();
  const leftIn = pop(f, fps, 40, 13);
  const rightIn = pop(f, fps, 90, 13);

  return (
    <AbsoluteFill style={{alignItems: 'center', justifyContent: 'center'}}>
      <Caption frame={f} fps={fps} at={6} size={54} weight={760} y={110}>
        and for the new folks…
      </Caption>

      <div style={{display: 'flex', gap: 44, marginTop: 60}}>
        {/* tour card */}
        <div
          style={{
            width: 520,
            background: T.card,
            border: `1.5px solid ${T.borderStrong}`,
            borderRadius: 18,
            padding: '26px 30px',
            opacity: seg(f, 40, 54),
            transform: `translateY(${(1 - leftIn) * 26}px)`,
          }}
        >
          <div style={{fontSize: 21, color: T.faint, marginBottom: 10, letterSpacing: 1}}>1/6</div>
          <div style={{fontSize: 29, lineHeight: 1.5, color: T.text}}>
            a <span style={{color: T.accent, fontWeight: 700}}>30-second tour</span> walks the live page —
            a real coin&apos;s birth, its story, its scripts.
          </div>
          <div style={{marginTop: 18, display: 'flex', gap: 12}}>
            <span style={{padding: '8px 20px', borderRadius: 999, border: `1px solid ${T.border}`, fontSize: 22, color: T.muted}}>skip</span>
            <span style={{padding: '8px 20px', borderRadius: 999, background: T.accent, color: '#06211d', fontSize: 22, fontWeight: 700}}>next →</span>
          </div>
        </div>

        {/* honest clocks */}
        <div
          style={{
            width: 520,
            background: T.card,
            border: `1.5px solid ${T.border}`,
            borderRadius: 18,
            padding: '26px 30px',
            opacity: seg(f, 90, 104),
            transform: `translateY(${(1 - rightIn) * 26}px)`,
          }}
        >
          <div style={{fontSize: 29, color: T.text, marginBottom: 18}}>
            times that <span style={{color: T.accent, fontWeight: 700}}>tell the truth</span>, no hovering:
          </div>
          <div style={{fontFamily: T.mono, fontSize: 25, lineHeight: 2}}>
            <div style={{opacity: seg(f, 130, 144)}}>
              <span style={{color: T.text}}>2 hours ago</span>
              <span style={{color: T.faint}}> · Jul 5, 14:32 UTC</span>
            </div>
            <div style={{opacity: seg(f, 150, 164)}}>
              <span style={{color: T.text}}>3 days ago</span>
              <span style={{color: T.faint}}> · Jul 2, 08:15 UTC</span>
            </div>
          </div>
        </div>
      </div>
    </AbsoluteFill>
  );
};
