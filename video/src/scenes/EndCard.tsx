import React from 'react';
import {AbsoluteFill, spring, useCurrentFrame, useVideoConfig} from 'remotion';
import {Avatar} from '../lib/Avatar';
import {fade, glide} from '../lib/anim';
import {cascade} from '../lib/data';
import {EyeMark} from '../lib/icons';
import {T} from '../theme';

export const EndCard: React.FC = () => {
  const f = useCurrentFrame();
  const {fps} = useVideoConfig();

  const markIn = spring({frame: f - 6, fps, config: {damping: 200}});
  const markOpacity = fade(f, 6, 22);
  const tagOpacity = fade(f, 28, 44);
  const urlIn = spring({frame: f - 48, fps, config: {damping: 15, stiffness: 140}});
  const urlOpacity = fade(f, 48, 60);
  const ghOpacity = fade(f, 76, 92);
  const rowOpacity = fade(f, 100, 124);

  return (
    <AbsoluteFill style={{alignItems: 'center', justifyContent: 'center'}}>
      <div
        style={{
          display: 'flex',
          flexDirection: 'column',
          alignItems: 'center',
          transform: 'translateY(-30px)',
        }}
      >
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 30,
            opacity: markOpacity,
            transform: `translateY(${(1 - markIn) * 20}px)`,
          }}
        >
          <EyeMark size={78} color={T.accent} />
          <span
            style={{
              fontFamily: T.mono,
              fontSize: 112,
              fontWeight: 600,
              color: T.text,
              letterSpacing: 2,
            }}
          >
            kascov
          </span>
        </div>

        <div style={{marginTop: 30, fontSize: 38, color: T.muted, opacity: tagOpacity}}>
          watch Kaspa's smart coins live their lives
        </div>

        <div
          style={{
            marginTop: 76,
            fontFamily: T.mono,
            fontSize: 66,
            color: T.accent,
            opacity: urlOpacity,
            transform: `translateY(${(1 - urlIn) * 24}px)`,
          }}
        >
          kascov-explorer.web.app
        </div>

        <div
          style={{
            marginTop: 30,
            fontFamily: T.mono,
            fontSize: 30,
            color: T.faint,
            opacity: ghOpacity,
          }}
        >
          open-source · github.com/Knitser/kascov
        </div>

        {/* the cast, watching back */}
        <div style={{marginTop: 78, display: 'flex', gap: 22, opacity: rowOpacity * 0.55}}>
          {cascade.map((e) => (
            <Avatar key={e.c.covenant_id} id={e.c.covenant_id} size={44} />
          ))}
        </div>
      </div>
    </AbsoluteFill>
  );
};
