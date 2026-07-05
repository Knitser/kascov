import React from 'react';
import {AbsoluteFill, useCurrentFrame, useVideoConfig} from 'remotion';
import {T} from '../../theme';
import {Caption, map, pop, seg} from '../v2/shared';

/* V5 scene 2 (~6s): edit the params in the browser; every keystroke
   re-compiles and must round-trip through the decoder. */

export const GENEDIT_DUR = 360;

const TYPE_AT = 70; // pledge retype
const KEY_AT = 150; // recipient becomes "yours"
const VERIFY_AT = 240;

export const GenEdit: React.FC = () => {
  const f = useCurrentFrame();
  const {fps} = useVideoConfig();

  const pledgeTyped = '2.5'.slice(0, Math.floor(map(f, [TYPE_AT, TYPE_AT + 34], [0, 3])));
  const keyTyped = 'ab7f…your key…9c04'.slice(0, Math.floor(map(f, [KEY_AT, KEY_AT + 50], [0, 18])));
  const verifyIn = pop(f, fps, VERIFY_AT, 12);

  const field = (label: string, value: string, active: boolean, at: number, hint: string) => (
    <div style={{opacity: seg(f, at, at + 12)}}>
      <div style={{fontSize: 22, fontWeight: 650, color: T.text, marginBottom: 8}}>
        {label} <span style={{color: T.faint, fontWeight: 400}}>{hint}</span>
      </div>
      <div
        style={{
          fontFamily: T.mono,
          fontSize: 26,
          padding: '14px 18px',
          borderRadius: 12,
          background: T.bgSoft,
          border: `1.5px solid ${active ? T.accent : T.border}`,
          color: T.text,
          minWidth: 380,
        }}
      >
        {value}
        {active && <span style={{opacity: Math.sin(f / 7) > 0 ? 1 : 0, color: T.accent}}>│</span>}
      </div>
    </div>
  );

  return (
    <AbsoluteFill style={{alignItems: 'center', justifyContent: 'center'}}>
      <Caption frame={f} fps={fps} at={6} size={56} weight={760} y={100}>
        edit the parameters <span style={{color: T.accent}}>right in the browser</span>
      </Caption>

      <div style={{display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '30px 50px', marginTop: 60}}>
        {field('recipient', f < KEY_AT ? '1111…1111' : keyTyped, f >= KEY_AT && f < KEY_AT + 55, 30, '(your key)')}
        {field('pledge', f < TYPE_AT ? '1' : pledgeTyped, f >= TYPE_AT && f < TYPE_AT + 40, 40, '(TKAS per claim)')}
        {field('funder_hash', '3333…3333', false, 50, '(blake2b of the funder)')}
        {field('period', '1000', false, 60, '(≈ 2 minutes)')}
      </div>

      {/* live verification line */}
      <div
        style={{
          marginTop: 54,
          fontSize: 32,
          fontWeight: 700,
          color: T.born,
          opacity: seg(f, VERIFY_AT, VERIFY_AT + 10),
          transform: `scale(${0.9 + verifyIn * 0.1})`,
          display: 'flex',
          alignItems: 'center',
          gap: 14,
        }}
      >
        <span
          style={{
            width: 30, height: 30, borderRadius: 999, background: T.bornSoft,
            border: `2px solid ${T.born}`, display: 'grid', placeItems: 'center', fontSize: 20,
          }}
        >
          ✓
        </span>
        re-compiles &amp; re-decodes with your args — verified on every keystroke
      </div>
    </AbsoluteFill>
  );
};
