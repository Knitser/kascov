import React from 'react';
import {AbsoluteFill, Easing, useCurrentFrame, useVideoConfig} from 'remotion';
import {Avatar} from '../../lib/Avatar';
import {entries} from '../../lib/data';
import {T} from '../../theme';
import {map, pop, seg, Wordmark} from './shared';

/* =====================================================================
   Scene 6 (7s): the end card. Wordmark settles (carried over from the
   "kascov remembers" moment), tagline, then the URL types out huge with
   a soft glow pulse. Holds clean for 3.5s+.
   ===================================================================== */

export const END_DUR = 420;

const URL = 'kascov-explorer.web.app';
const TYPE_AT = 56;
const TYPE_END = 128;

const ROW = entries.slice(0, 9);

export const EndCardV2: React.FC = () => {
  const f = useCurrentFrame();
  const {fps} = useVideoConfig();

  /* wordmark springs in fresh after "kascov remembers." bows out */
  const wmIn = pop(f, fps, 4);
  const wmO = seg(f, 2, 16);

  const tagO = seg(f, 26, 44);
  const tagY = map(f, [26, 44], [16, 0]);

  const chars = Math.floor(map(f, [TYPE_AT, TYPE_END], [0, URL.length], Easing.linear));
  const urlStarted = f >= TYPE_AT - 6;
  const caretOn = urlStarted && f < TYPE_END + 24 && f % 14 < 9;
  const typed = f >= TYPE_END;
  /* soft glow pulse once the URL is complete */
  const pulse = typed ? 0.55 + 0.45 * Math.sin((f - TYPE_END) / 26) : 0;
  const urlPop = pop(f, fps, TYPE_END, 13);

  const ghO = seg(f, 148, 166);
  const rowO = seg(f, 172, 196);

  return (
    <AbsoluteFill style={{alignItems: 'center'}}>
      {/* wordmark */}
      <div
        style={{
          position: 'absolute',
          top: 250,
          opacity: wmO,
          transform: `translateY(${(1 - wmIn) * 30}px)`,
        }}
      >
        <Wordmark size={118} glow={0.4} />
      </div>

      {/* tagline */}
      <div
        style={{
          position: 'absolute',
          top: 418,
          fontSize: 42,
          fontWeight: 500,
          color: T.muted,
          opacity: tagO,
          transform: `translateY(${tagY}px)`,
        }}
      >
        watch Kaspa's smart coins live their lives
      </div>

      {/* the URL, huge, typing out */}
      {urlStarted && (
        <div
          style={{
            position: 'absolute',
            top: 528,
            height: 130,
            display: 'flex',
            alignItems: 'center',
            transform: `scale(${1 + (1 - urlPop) * 0.03})`,
          }}
        >
          <div
            style={{
              position: 'relative',
              fontFamily: T.mono,
              fontSize: 88,
              fontWeight: 700,
              letterSpacing: 1,
              color: T.accent,
              textShadow: `0 0 14px rgba(112,199,186,${0.3 + pulse * 0.2}), 0 0 ${48 + pulse * 26}px rgba(112,199,186,${0.16 + pulse * 0.14})`,
            }}
          >
            {/* ghost keeps center width stable while typing */}
            <span style={{opacity: 0}}>{URL}</span>
            <span style={{position: 'absolute', left: 0, top: 0, whiteSpace: 'pre'}}>
              {URL.slice(0, chars)}
              <span
                style={{
                  display: 'inline-block',
                  width: 30,
                  height: 72,
                  marginLeft: 10,
                  transform: 'translateY(6px)',
                  background: T.accent,
                  opacity: caretOn ? 0.9 : 0,
                }}
              />
            </span>
          </div>
        </div>
      )}

      {/* open-source line */}
      <div
        style={{
          position: 'absolute',
          top: 706,
          fontFamily: T.mono,
          fontSize: 31,
          color: T.faint,
          opacity: ghO,
        }}
      >
        open-source · github.com/Knitser/kascov
      </div>

      {/* the cast, watching back */}
      <div
        style={{
          position: 'absolute',
          top: 806,
          display: 'flex',
          gap: 24,
          opacity: rowO * 0.55,
        }}
      >
        {ROW.map((e) => (
          <Avatar key={e.c.covenant_id} id={e.c.covenant_id} size={46} />
        ))}
      </div>
    </AbsoluteFill>
  );
};
