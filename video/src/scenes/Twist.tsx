import React from 'react';
import {AbsoluteFill, spring, useCurrentFrame, useVideoConfig} from 'remotion';
import {fade, glide} from '../lib/anim';
import {starSteps} from '../lib/data';
import {KindIcon} from '../lib/icons';
import {KIND_COLOR, KIND_SOFT, T} from '../theme';

const CHIP = 196;
const CHIP_GAP = 44;
const ROW_W = starSteps.length * CHIP + (starSteps.length - 1) * CHIP_GAP;
const ROW_X = (1920 - ROW_W) / 2;
const ROW_Y = 400;

const WORD: Record<string, string> = {
  genesis: 'born',
  transition: 'moved',
  burn: 'retired',
};

export const Twist: React.FC = () => {
  const f = useCurrentFrame();
  const {fps} = useVideoConfig();

  const headIn = fade(f, 0, 18);
  const headY = glide(f, [0, 18], [12, 0]);
  /* the claim dims once the history starts to rot */
  const headDim = 1 - 0.7 * fade(f, 66, 96);

  const rememberIn = spring({frame: f - 108, fps, config: {damping: 15, stiffness: 160}});
  const rememberOpacity = fade(f, 108, 114);
  const capOpacity = fade(f, 148, 164);

  return (
    <AbsoluteFill style={{alignItems: 'center'}}>
      <div
        style={{
          marginTop: 178,
          fontSize: 56,
          fontWeight: 650,
          color: T.text,
          opacity: headIn * headDim,
          transform: `translateY(${headY}px)`,
        }}
      >
        Kaspa nodes delete history after ~3 days.
      </div>

      {/* the life story from the last scene — rotting away, then rescued */}
      {starSteps.map((st, k) => {
        const dk = 34 + k * 7; /* dissolve start */
        const sk = 116 + k * 3; /* snap-back start */
        const dis = fade(f, dk, dk + 28);
        const back = Math.min(1, spring({frame: f - sk, fps, config: {damping: 14, stiffness: 190}}));
        const ghost = dis * (1 - back);

        const color = KIND_COLOR[st.kind];
        const opacity = 1 - 0.95 * ghost;
        const blur = 4 * ghost;
        const drift = 22 * ghost;
        const scale = 1 - 0.08 * ghost;
        const glow = back > 0 && f >= sk ? 0.3 * back : 0;

        return (
          <div
            key={st.txid}
            style={{
              position: 'absolute',
              left: ROW_X + k * (CHIP + CHIP_GAP),
              top: ROW_Y,
              width: CHIP,
              height: CHIP,
              borderRadius: 18,
              background: T.card,
              border: `1px solid ${glow > 0 ? T.borderStrong : T.border}`,
              boxShadow: glow > 0 ? `0 0 46px rgba(112, 199, 186, ${glow})` : 'none',
              display: 'flex',
              flexDirection: 'column',
              alignItems: 'center',
              justifyContent: 'center',
              gap: 18,
              opacity,
              filter: `grayscale(${ghost}) blur(${blur}px)`,
              transform: `translateY(${drift}px) scale(${scale})`,
            }}
          >
            <div
              style={{
                width: 72,
                height: 72,
                borderRadius: '50%',
                background: KIND_SOFT[st.kind],
                border: `2px solid ${color}`,
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
              }}
            >
              <KindIcon kind={st.kind} size={36} color={color} />
            </div>
            <div style={{fontSize: 29, color: T.muted, fontWeight: 550}}>{WORD[st.kind]}</div>
          </div>
        );
      })}

      {/* the rescue */}
      <div
        style={{
          position: 'absolute',
          top: 726,
          left: 0,
          right: 0,
          textAlign: 'center',
          fontSize: 84,
          fontWeight: 700,
          opacity: rememberOpacity,
          transform: `scale(${0.92 + 0.08 * rememberIn})`,
        }}
      >
        <span style={{fontFamily: T.mono, color: T.accent}}>kascov</span>
        <span style={{color: T.text}}> remembers.</span>
      </div>

      <div
        style={{
          position: 'absolute',
          top: 856,
          left: 0,
          right: 0,
          textAlign: 'center',
          fontSize: 30,
          color: T.faint,
          opacity: capOpacity,
        }}
      >
        every life event, archived for good
      </div>
    </AbsoluteFill>
  );
};
