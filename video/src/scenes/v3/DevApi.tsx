import React from 'react';
import {AbsoluteFill, Easing, useCurrentFrame, useVideoConfig} from 'remotion';
import {decode} from '../../lib/decode';
import {fmtInt} from '../../lib/identity';
import {T} from '../../theme';
import {Caption, map, pop, seg} from '../v2/shared';

/* =====================================================================
   V3 scene 6 (6s): the API. A terminal card types a real curl and the
   real stats print back. Chips: CORS *, no keys, open source.
   ===================================================================== */

export const DEVAPI_DUR = 360;

const CMD = 'curl kascov-explorer.web.app/data/mainnet-live.json | jq .stats';
const OUT_LINES = [
  '{',
  `  "covenants": ${decode.stats?.mainnet.covenants ?? 5},`,
  `  "events": ${decode.stats?.mainnet.events ?? 857},`,
  '  "live_value": …,',
  '}',
];
const CHIPS = ['CORS *', 'no keys', 'open source · github.com/Knitser/kascov'];

export const DevApi: React.FC = () => {
  const f = useCurrentFrame();
  const {fps} = useVideoConfig();

  const cardIn = pop(f, fps, 20, 15);
  const cardO = seg(f, 18, 32);
  const chars = Math.floor(map(f, [44, 128], [0, CMD.length], Easing.linear));

  return (
    <AbsoluteFill>
      <Caption frame={f} fps={fps} at={4} size={56} weight={740} y={110}>
        everything kascov sees is <span style={{color: T.accent}}>yours</span>
      </Caption>

      <AbsoluteFill style={{alignItems: 'center', justifyContent: 'center'}}>
        <div
          style={{
            width: 1240,
            borderRadius: 18,
            background: '#0a0f0e',
            border: `1.5px solid ${T.border}`,
            boxShadow: '0 30px 80px rgba(0,0,0,0.5)',
            opacity: cardO,
            transform: `translateY(${(1 - cardIn) * 30}px)`,
            overflow: 'hidden',
          }}
        >
          {/* title bar */}
          <div style={{display: 'flex', gap: 10, padding: '16px 20px', background: T.bgSoft, borderBottom: `1px solid ${T.border}`}}>
            {['#ff5f57', '#febc2e', '#28c840'].map((c) => (
              <div key={c} style={{width: 15, height: 15, borderRadius: 8, background: c, opacity: 0.85}} />
            ))}
          </div>
          <div style={{padding: '30px 38px', fontFamily: T.mono, fontSize: 29, lineHeight: 1.65}}>
            <div>
              <span style={{color: T.born}}>$ </span>
              <span style={{color: T.text}}>{CMD.slice(0, chars)}</span>
              <span
                style={{
                  display: 'inline-block',
                  width: 15,
                  height: 28,
                  marginLeft: 4,
                  transform: 'translateY(4px)',
                  background: T.accent,
                  opacity: f < 150 && f % 14 < 9 ? 0.9 : 0,
                }}
              />
            </div>
            {OUT_LINES.map((l, i) => {
              const at = 150 + i * 12;
              return (
                <div key={i} style={{color: T.accent, opacity: seg(f, at, at + 8)}}>
                  {l}
                </div>
              );
            })}
          </div>
        </div>

        {/* chips */}
        <div style={{display: 'flex', gap: 20, marginTop: 44}}>
          {CHIPS.map((c, i) => {
            const at = 224 + i * 14;
            const s = pop(f, fps, at, 14);
            const o = seg(f, at, at + 8);
            return (
              <div
                key={c}
                style={{
                  padding: '12px 26px',
                  borderRadius: 999,
                  background: T.accentSoft,
                  border: `1px solid ${T.borderStrong}`,
                  color: T.accent,
                  fontFamily: T.mono,
                  fontSize: 26,
                  fontWeight: 650,
                  opacity: o,
                  transform: `translateY(${(1 - s) * 18}px)`,
                }}
              >
                {c}
              </div>
            );
          })}
        </div>
      </AbsoluteFill>
    </AbsoluteFill>
  );
};
