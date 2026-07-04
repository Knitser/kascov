import React from 'react';
import {AbsoluteFill, Easing, useCurrentFrame, useVideoConfig} from 'remotion';
import {T} from '../../theme';
import {U} from '../../lib/updatestats';
import {Caption, map, pop, seg} from '../v2/shared';

/* =====================================================================
   V4 scene 1 (~6.5s): the sequel hook. Launch week recap — the testnet
   exploded 40× and the explorer grew up with it.
   ===================================================================== */

export const UHOOK_DUR = 390;

const fmt = (n: number) => n.toLocaleString('en-US');

const Stat: React.FC<{
  frame: number;
  fps: number;
  at: number;
  n: number;
  label: string;
  color: string;
}> = ({frame, fps, at, n, label, color}) => {
  const inP = pop(frame, fps, at, 13);
  const count = Math.round(map(frame, [at, at + 90], [0, n], Easing.out(Easing.cubic)));
  return (
    <div
      style={{
        background: T.card,
        border: `1.5px solid ${T.border}`,
        borderRadius: 18,
        padding: '30px 54px',
        textAlign: 'center',
        opacity: seg(frame, at, at + 10),
        transform: `translateY(${(1 - inP) * 30}px)`,
      }}
    >
      <div style={{fontFamily: T.mono, fontSize: 64, fontWeight: 800, color}}>{fmt(count)}</div>
      <div style={{fontSize: 27, color: T.muted, marginTop: 6}}>{label}</div>
    </div>
  );
};

export const UpdateHook: React.FC = () => {
  const f = useCurrentFrame();
  const {fps} = useVideoConfig();

  return (
    <AbsoluteFill style={{alignItems: 'center', justifyContent: 'center'}}>
      <Caption frame={f} fps={fps} at={8} size={62} weight={780} y={150}>
        one week ago, <span style={{color: T.accent}}>kascov</span> started watching
      </Caption>

      <div style={{display: 'flex', gap: 34, marginTop: 40}}>
        <Stat frame={f} fps={fps} at={70} n={U.coins} label="smart coins tracked" color={T.accent} />
        <Stat frame={f} fps={fps} at={92} n={U.events} label="life events recorded" color={T.move} />
      </div>

      <div
        style={{
          marginTop: 56,
          fontSize: 44,
          fontWeight: 720,
          color: T.text,
          opacity: seg(f, 210, 232),
        }}
      >
        the testnet exploded <span style={{color: T.burn, fontFamily: T.mono}}>{U.growth}</span> in a week.
      </div>
      <div style={{marginTop: 16, fontSize: 34, color: T.muted, opacity: seg(f, 275, 297)}}>
        so the explorer grew up. here&apos;s what&apos;s new. 🔭
      </div>
    </AbsoluteFill>
  );
};
