import React from 'react';
import {
  AbsoluteFill,
  Easing,
  interpolate,
  spring,
  useCurrentFrame,
  useVideoConfig,
} from 'remotion';
import {Avatar} from '../lib/Avatar';
import {fade, glide} from '../lib/anim';
import {cardLine, cascade, stats} from '../lib/data';
import {fmtAmount, fmtInt} from '../lib/identity';
import {T} from '../theme';

const CARD_W = 540;
const CARD_H = 124;
const GAP = 24;
const COLS = 3;
const GRID_X = (1920 - (CARD_W * COLS + GAP * (COLS - 1))) / 2;
const GRID_Y = 252;

const tick = (f: number, from: number, to: number, target: number) =>
  Math.round(
    interpolate(f, [from, to], [0, target], {
      extrapolateLeft: 'clamp',
      extrapolateRight: 'clamp',
      easing: Easing.out(Easing.cubic),
    })
  );

export const Testnet: React.FC = () => {
  const f = useCurrentFrame();
  const {fps} = useVideoConfig();

  const headOpacity = fade(f, 0, 18);
  const headY = glide(f, [0, 18], [12, 0]);

  const coins = tick(f, 128, 190, stats.covenants);
  const events = tick(f, 128, 196, stats.events);
  const statsOpacity = fade(f, 124, 140);
  const subOpacity = fade(f, 200, 220);

  return (
    <AbsoluteFill style={{alignItems: 'center'}}>
      <div
        style={{
          marginTop: 116,
          fontSize: 44,
          fontWeight: 600,
          color: T.text,
          opacity: headOpacity,
          transform: `translateY(${headY}px)`,
        }}
      >
        right now, on the Kaspa testnet…
      </div>

      {cascade.map((e, i) => {
        const col = i % COLS;
        const row = Math.floor(i / COLS);
        const delay = 20 + i * 6;
        const spr = spring({frame: f - delay, fps, config: {damping: 15, stiffness: 130}});
        const opacity = fade(f, delay, delay + 8);
        return (
          <div
            key={e.c.covenant_id}
            style={{
              position: 'absolute',
              left: GRID_X + col * (CARD_W + GAP),
              top: GRID_Y + row * (CARD_H + GAP),
              width: CARD_W,
              height: CARD_H,
              background: T.card,
              border: `1px solid ${T.border}`,
              borderRadius: 14,
              display: 'flex',
              alignItems: 'center',
              gap: 20,
              padding: '0 24px',
              boxSizing: 'border-box',
              opacity,
              transform: `translateY(${(1 - spr) * -70}px)`,
            }}
          >
            <Avatar id={e.c.covenant_id} size={58} style={{flex: 'none'}} />
            <div style={{minWidth: 0}}>
              <div
                style={{
                  fontFamily: T.mono,
                  fontSize: 29,
                  color: T.text,
                  marginBottom: 8,
                  whiteSpace: 'nowrap',
                  overflow: 'hidden',
                  textOverflow: 'ellipsis',
                }}
              >
                {e.name}
              </div>
              <div
                style={{
                  fontSize: 27,
                  color: T.faint,
                  whiteSpace: 'nowrap',
                  overflow: 'hidden',
                  textOverflow: 'ellipsis',
                }}
              >
                {cardLine(e)}
              </div>
            </div>
          </div>
        );
      })}

      {/* real totals, ticking up */}
      <div
        style={{
          position: 'absolute',
          top: 760,
          left: 0,
          right: 0,
          display: 'flex',
          justifyContent: 'center',
          alignItems: 'baseline',
          gap: 26,
          opacity: statsOpacity,
        }}
      >
        <span style={{fontSize: 72, fontWeight: 700, color: T.accent, fontVariantNumeric: 'tabular-nums'}}>
          {fmtInt(coins)}
        </span>
        <span style={{fontSize: 36, color: T.muted}}>smart coins</span>
        <span style={{fontSize: 36, color: T.faint, margin: '0 8px'}}>·</span>
        <span style={{fontSize: 72, fontWeight: 700, color: T.accent, fontVariantNumeric: 'tabular-nums'}}>
          {fmtInt(events)}
        </span>
        <span style={{fontSize: 36, color: T.muted}}>life events</span>
      </div>

      <div
        style={{
          position: 'absolute',
          top: 880,
          left: 0,
          right: 0,
          textAlign: 'center',
          fontSize: 30,
          color: T.faint,
          opacity: subOpacity,
        }}
      >
        together holding {fmtAmount(stats.live_value)} on testnet-10
      </div>
    </AbsoluteFill>
  );
};
