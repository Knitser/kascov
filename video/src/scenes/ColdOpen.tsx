import React from 'react';
import {AbsoluteFill, Easing, interpolate, useCurrentFrame} from 'remotion';
import {fade, glide} from '../lib/anim';
import {T} from '../theme';

const LINE = 'Kaspa just became programmable.';

export const ColdOpen: React.FC = () => {
  const f = useCurrentFrame();

  const capOpacity = fade(f, 12, 32);
  const capY = glide(f, [12, 32], [10, 0]);

  const chars = Math.floor(
    interpolate(f, [44, 94], [0, LINE.length], {
      extrapolateLeft: 'clamp',
      extrapolateRight: 'clamp',
      easing: Easing.linear,
    })
  );
  const caretOn = f > 38 && f < 106 && f % 16 < 10;

  return (
    <AbsoluteFill
      style={{
        justifyContent: 'center',
        alignItems: 'center',
        flexDirection: 'column',
        gap: 42,
      }}
    >
      <div
        style={{
          fontFamily: T.mono,
          fontSize: 30,
          letterSpacing: 3,
          color: T.accent,
          opacity: capOpacity,
          transform: `translateY(${capY}px)`,
        }}
      >
        June 30, 2026 — the Toccata hardfork
      </div>
      <div
        style={{
          position: 'relative',
          fontSize: 68,
          fontWeight: 650,
          letterSpacing: -0.5,
          color: T.text,
        }}
      >
        {/* ghost keeps the centered width stable while typing */}
        <span style={{opacity: 0}}>{LINE}</span>
        <span
          style={{
            position: 'absolute',
            left: 0,
            top: 0,
            whiteSpace: 'pre',
          }}
        >
          {LINE.slice(0, chars)}
          <span
            style={{
              display: 'inline-block',
              width: 24,
              height: 52,
              marginLeft: 8,
              transform: 'translateY(4px)',
              background: T.accent,
              opacity: caretOn ? 0.9 : 0.12,
            }}
          />
        </span>
      </div>
    </AbsoluteFill>
  );
};
