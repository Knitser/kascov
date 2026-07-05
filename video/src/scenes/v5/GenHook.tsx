import React from 'react';
import {AbsoluteFill, useCurrentFrame, useVideoConfig} from 'remotion';
import {T} from '../../theme';
import {Caption, pop, seg} from '../v2/shared';

/* V5 scene 1 (~4.5s): you found a contract — the decoder names it. */

export const GENHOOK_DUR = 270;

const FIELDS = [
  ['recipient', '1111…1111'],
  ['funder_hash', '3333…3333'],
  ['pledge', '1 TKAS'],
  ['period', '1000 DAA'],
];

export const GenHook: React.FC = () => {
  const f = useCurrentFrame();
  const {fps} = useVideoConfig();
  const cardIn = pop(f, fps, 46, 14);

  return (
    <AbsoluteFill style={{alignItems: 'center', justifyContent: 'center'}}>
      <Caption frame={f} fps={fps} at={8} size={62} weight={780} y={150}>
        found a smart contract on <span style={{color: T.accent}}>$KAS</span>?
      </Caption>

      <div
        style={{
          marginTop: 40,
          background: T.card,
          border: `1.5px solid ${T.border}`,
          borderRadius: 18,
          padding: '30px 44px',
          opacity: seg(f, 46, 60),
          transform: `translateY(${(1 - cardIn) * 28}px)`,
        }}
      >
        <div style={{display: 'flex', alignItems: 'center', gap: 18, marginBottom: 22}}>
          <span
            style={{
              fontFamily: T.mono,
              fontSize: 27,
              fontWeight: 700,
              color: T.accent,
              background: T.accentSoft,
              border: `1px solid ${T.borderStrong}`,
              borderRadius: 999,
              padding: '8px 22px',
            }}
          >
            SilverScript · Mecenas
          </span>
          <span style={{fontSize: 26, color: T.muted}}>the decoder names it, args labeled</span>
        </div>
        <div style={{display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '12px 40px'}}>
          {FIELDS.map(([k, v], i) => (
            <div key={k} style={{fontFamily: T.mono, fontSize: 25, opacity: seg(f, 80 + i * 10, 94 + i * 10)}}>
              <span style={{color: T.faint}}>{k} = </span>
              <span style={{color: T.text}}>{v}</span>
            </div>
          ))}
        </div>
      </div>

      <div style={{marginTop: 44, fontSize: 46, fontWeight: 760, color: T.text, opacity: seg(f, 180, 200)}}>
        new: <span style={{color: T.accent}}>make it yours.</span>
      </div>
    </AbsoluteFill>
  );
};
