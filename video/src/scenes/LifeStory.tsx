import React from 'react';
import {AbsoluteFill, spring, useCurrentFrame, useVideoConfig} from 'remotion';
import {Avatar} from '../lib/Avatar';
import {fade, glide} from '../lib/anim';
import {star, starLifespanS, starSteps} from '../lib/data';
import {shortHex} from '../lib/identity';
import {KindIcon} from '../lib/icons';
import {KIND_COLOR, KIND_SOFT, T} from '../theme';

const COL_W = 1020;
const COL_X = (1920 - COL_W) / 2;
const TL_Y = 288;
const ROW_H = 106;
const NODE = 52;
const NODE_X = COL_X + 10;

export const LifeStory: React.FC = () => {
  const f = useCurrentFrame();
  const {fps} = useVideoConfig();

  const headOpacity = fade(f, 0, 16);
  const headY = glide(f, [0, 16], [12, 0]);

  const endOpacity = fade(f, 158, 176);
  const endY = glide(f, [158, 176], [12, 0]);

  const lived = Math.round(starLifespanS);

  return (
    <AbsoluteFill>
      {/* header: who we're watching */}
      <div
        style={{
          position: 'absolute',
          left: COL_X,
          top: 108,
          display: 'flex',
          alignItems: 'center',
          gap: 30,
          opacity: headOpacity,
          transform: `translateY(${headY}px)`,
        }}
      >
        <Avatar id={star.c.covenant_id} size={104} />
        <div>
          <div style={{display: 'flex', alignItems: 'center', gap: 22}}>
            <span style={{fontFamily: T.mono, fontSize: 44, color: T.text}}>{star.name}</span>
            <span
              style={{
                fontSize: 26,
                fontWeight: 600,
                color: T.burn,
                background: T.burnSoft,
                border: `1px solid ${T.burn}55`,
                borderRadius: 999,
                padding: '6px 18px',
              }}
            >
              retired
            </span>
          </div>
          <div style={{marginTop: 10, fontSize: 30, color: T.faint}}>
            one smart coin's whole life — {lived} seconds on-chain
          </div>
        </div>
      </div>

      {/* vertical timeline of the real events */}
      {starSteps.map((st, k) => {
        const at = 28 + k * 26;
        const nodeIn = spring({frame: f - at, fps, config: {damping: 13, stiffness: 150}});
        const labelOpacity = fade(f, at + 2, at + 14);
        const labelX = glide(f, [at + 2, at + 14], [-14, 0]);
        const color = KIND_COLOR[st.kind];
        const soft = KIND_SOFT[st.kind];
        const y = TL_Y + k * ROW_H;

        /* connector to the next node draws downward */
        const connH = k < starSteps.length - 1 ? fade(f, at + 10, at + 26) : 0;

        return (
          <React.Fragment key={st.txid}>
            {k < starSteps.length - 1 && (
              <div
                style={{
                  position: 'absolute',
                  left: NODE_X + NODE / 2 - 2,
                  top: y + NODE + 6,
                  width: 4,
                  height: (ROW_H - NODE - 12) * connH,
                  borderRadius: 2,
                  background: `${color}44`,
                }}
              />
            )}
            <div
              style={{
                position: 'absolute',
                left: NODE_X,
                top: y,
                width: NODE,
                height: NODE,
                borderRadius: '50%',
                background: soft,
                border: `2px solid ${color}`,
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                transform: `scale(${nodeIn})`,
              }}
            >
              <KindIcon kind={st.kind} size={26} color={color} />
            </div>
            <div
              style={{
                position: 'absolute',
                left: NODE_X + NODE + 34,
                top: y - 4,
                opacity: labelOpacity,
                transform: `translateX(${labelX}px)`,
              }}
            >
              <div style={{fontSize: 34, color: T.text, fontWeight: 550}}>{st.label}</div>
              <div style={{marginTop: 6, fontFamily: T.mono, fontSize: 25, color: T.faint}}>
                tx {shortHex(st.txid, 8, 6)}
              </div>
            </div>
            <div
              style={{
                position: 'absolute',
                left: COL_X,
                width: COL_W,
                top: y + 8,
                textAlign: 'right',
                fontFamily: T.mono,
                fontSize: 30,
                color: k === 0 ? T.faint : color,
                opacity: labelOpacity,
              }}
            >
              {k === 0 ? 't = 0s' : `+${st.deltaS.toFixed(1)}s`}
            </div>
          </React.Fragment>
        );
      })}

      <div
        style={{
          position: 'absolute',
          left: 0,
          right: 0,
          top: TL_Y + starSteps.length * ROW_H + 26,
          textAlign: 'center',
          fontSize: 42,
          fontWeight: 600,
          opacity: endOpacity,
          transform: `translateY(${endY}px)`,
        }}
      >
        <span style={{color: T.text}}>every step, </span>
        <span style={{color: T.accent}}>recorded.</span>
      </div>
    </AbsoluteFill>
  );
};
