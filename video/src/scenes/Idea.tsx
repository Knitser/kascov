import React from 'react';
import {AbsoluteFill, spring, useCurrentFrame, useVideoConfig} from 'remotion';
import {Avatar} from '../lib/Avatar';
import {fade, glide} from '../lib/anim';
import {mascot} from '../lib/data';
import {avatarParams} from '../lib/identity';
import {T} from '../theme';

const COIN = 280;

export const Idea: React.FC = () => {
  const f = useCurrentFrame();
  const {fps} = useVideoConfig();

  const headOpacity = fade(f, 6, 26);
  const headY = glide(f, [6, 26], [14, 0]);

  /* the plain coin springs in */
  const coinIn = spring({frame: f - 16, fps, config: {damping: 14, stiffness: 120, mass: 0.9}});

  /* at ~f80 it acquires a face: crossfade plain → identicon, shapes pop in */
  const plainOpacity = 1 - fade(f, 80, 98);
  const faceOpacity = fade(f, 80, 98);
  const shapeCount = avatarParams(mascot.c.covenant_id).shapes.length;
  const reveals = Array.from({length: shapeCount}, (_, k) =>
    spring({frame: f - (94 + k * 9), fps, config: {damping: 13, stiffness: 140}})
  );

  /* one soft pulse ring at the transformation moment */
  const pulseP = fade(f, 82, 112);
  const pulseR = COIN / 2 + pulseP * 90;
  const pulseOpacity = pulseP > 0 && pulseP < 1 ? 0.45 * (1 - pulseP) : 0;

  const nameIn = spring({frame: f - 108, fps, config: {damping: 200}});
  const nameOpacity = fade(f, 108, 120);
  const capOpacity = fade(f, 132, 150);

  return (
    <AbsoluteFill style={{alignItems: 'center'}}>
      <div
        style={{
          marginTop: 150,
          fontSize: 46,
          fontWeight: 600,
          color: T.text,
          opacity: headOpacity,
          transform: `translateY(${headY}px)`,
        }}
      >
        coins can now carry <span style={{color: T.accent}}>rules</span> and{' '}
        <span style={{color: T.accent}}>memory</span>.
      </div>

      {/* stage */}
      <div
        style={{
          position: 'absolute',
          top: 330,
          left: 0,
          right: 0,
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'center',
        }}
      >
        <div
          style={{
            position: 'relative',
            width: COIN,
            height: COIN,
            transform: `scale(${coinIn})`,
          }}
        >
          {/* pulse ring */}
          <svg
            viewBox="0 0 480 480"
            width={480}
            height={480}
            style={{position: 'absolute', left: (COIN - 480) / 2, top: (COIN - 480) / 2}}
          >
            <circle
              cx={240}
              cy={240}
              r={pulseR}
              fill="none"
              stroke={T.accent}
              strokeWidth={3}
              opacity={pulseOpacity}
            />
          </svg>
          {/* the identicon face, revealed shape by shape */}
          <div style={{position: 'absolute', inset: 0, opacity: faceOpacity}}>
            <Avatar id={mascot.c.covenant_id} size={COIN} revealShapes={reveals} />
          </div>
          {/* the anonymous coin: flat circle, teal ring */}
          <svg
            viewBox="0 0 64 64"
            width={COIN}
            height={COIN}
            style={{position: 'absolute', inset: 0, opacity: plainOpacity}}
          >
            <circle cx={32} cy={32} r={30} fill={T.card} stroke={T.accent} strokeWidth={2.5} />
          </svg>
        </div>

        <div
          style={{
            marginTop: 44,
            fontFamily: T.mono,
            fontSize: 36,
            color: T.text,
            background: T.card,
            border: `1px solid ${T.borderStrong}`,
            borderRadius: 12,
            padding: '16px 30px',
            opacity: nameOpacity,
            transform: `translateY(${(1 - nameIn) * 26}px)`,
          }}
        >
          {mascot.name}
        </div>

        <div
          style={{
            marginTop: 26,
            fontSize: 32,
            color: T.faint,
            opacity: capOpacity,
          }}
        >
          it has an identity now
        </div>
      </div>
    </AbsoluteFill>
  );
};
