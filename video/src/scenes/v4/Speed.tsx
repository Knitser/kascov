import React from 'react';
import {AbsoluteFill, Easing, useCurrentFrame, useVideoConfig} from 'remotion';
import {T} from '../../theme';
import {U} from '../../lib/updatestats';
import {Caption, map, pop, seg} from '../v2/shared';

/* =====================================================================
   V4 scene 2 (~6s): the speed story. 42k coins melted the old data
   layer; the rebuild made first paint ~instant. Bars shrink on screen.
   ===================================================================== */

export const SPEED_DUR = 360;

const Bar: React.FC<{
  frame: number;
  at: number;
  label: string;
  from: string;
  to: string;
  fullW: number;
  toFrac: number;
  color: string;
}> = ({frame, at, label, from, to, fullW, toFrac, color}) => {
  const grow = seg(frame, at, at + 26);
  /* the bar first draws at its old size, then collapses to the new one */
  const shrink = map(frame, [at + 70, at + 130], [1, toFrac], Easing.inOut(Easing.cubic));
  const w = fullW * grow * shrink;
  const swapped = frame >= at + 96;
  return (
    <div style={{marginBottom: 44, opacity: seg(frame, at, at + 12)}}>
      <div style={{display: 'flex', justifyContent: 'space-between', alignItems: 'baseline', width: fullW}}>
        <span style={{fontSize: 30, color: T.text, fontWeight: 650}}>{label}</span>
        <span
          style={{
            fontFamily: T.mono,
            fontSize: 34,
            fontWeight: 800,
            color: swapped ? color : T.burn,
          }}
        >
          {swapped ? to : from}
        </span>
      </div>
      <div
        style={{
          marginTop: 12,
          height: 26,
          width: w,
          borderRadius: 13,
          background: swapped ? color : T.burn,
          transition: 'background 0.2s',
          boxShadow: swapped ? `0 0 26px ${color}55` : 'none',
        }}
      />
    </div>
  );
};

export const Speed: React.FC = () => {
  const f = useCurrentFrame();
  const {fps} = useVideoConfig();
  const W = 1080;

  return (
    <AbsoluteFill style={{alignItems: 'center', justifyContent: 'center'}}>
      <Caption frame={f} fps={fps} at={6} size={56} weight={760} y={130}>
        50k coins melted the old site. <span style={{color: T.accent}}>we rebuilt the data layer.</span>
      </Caption>

      <div style={{marginTop: 60}}>
        <Bar frame={f} at={70} label="full picture, first load" from={`${U.oldLoadS}s`} to={`~${U.newLoadS}s`} fullW={W} toFrac={0.06} color={T.born} />
        <Bar frame={f} at={120} label="data over the wire" from={`${U.oldMB} MB`} to={`${U.newMB} MB`} fullW={W} toFrac={0.09} color={T.accent} />
        <Bar frame={f} at={170} label="a coin's page" from="everything" to="just its story" fullW={W} toFrac={0.04} color={T.move} />
      </div>

      <div style={{marginTop: 30, fontSize: 32, color: T.muted, opacity: seg(f, 300, 322)}}>
        every coin still one click away — it just stops making you wait.
      </div>
    </AbsoluteFill>
  );
};
