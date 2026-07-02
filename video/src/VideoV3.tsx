import React from 'react';
import {AbsoluteFill, Sequence, useCurrentFrame} from 'remotion';
import {EndCardV2, END_DUR} from './scenes/v2/EndCardV2';
import {Shell, V2} from './scenes/v2/shared';
import {DecodeProof, DECODE_DUR} from './scenes/v3/DecodeProof';
import {DevApi, DEVAPI_DUR} from './scenes/v3/DevApi';
import {Explorer3, EXPLORER3_DUR} from './scenes/v3/Explorer3';
import {Forget, FORGET_DUR} from './scenes/v3/Forget';
import {Hook3, HOOK3_DUR} from './scenes/v3/Hook3';
import {Tools3, TOOLS3_DUR} from './scenes/v3/Tools3';
import {T} from './theme';

/* =====================================================================
   LaunchV3 — the full product tour @60fps (~52s):
   hook → the pruning problem → the live explorer → the tools →
   the mainnet decode proof (real 132-op ZK program) → the API → end card.
   ===================================================================== */

const OVERLAP = 12;

const SCENES: {
  name: string;
  dur: number;
  el: React.FC;
  fadeIn: number;
  fadeOut: number;
}[] = [
  {name: 'hook', dur: HOOK3_DUR, el: Hook3, fadeIn: 0, fadeOut: OVERLAP},
  {name: 'forget', dur: FORGET_DUR, el: Forget, fadeIn: OVERLAP, fadeOut: OVERLAP},
  {name: 'explorer', dur: EXPLORER3_DUR, el: Explorer3, fadeIn: OVERLAP, fadeOut: OVERLAP},
  {name: 'tools', dur: TOOLS3_DUR, el: Tools3, fadeIn: OVERLAP, fadeOut: OVERLAP},
  {name: 'decode-proof', dur: DECODE_DUR, el: DecodeProof, fadeIn: OVERLAP, fadeOut: OVERLAP},
  {name: 'dev-api', dur: DEVAPI_DUR, el: DevApi, fadeIn: OVERLAP, fadeOut: OVERLAP},
  {name: 'end-card', dur: END_DUR, el: EndCardV2, fadeIn: OVERLAP, fadeOut: 0},
];

export const V3_TOTAL = SCENES.reduce((sum, s) => sum + s.dur - OVERLAP, 0) + OVERLAP;

const ShellAt: React.FC<{
  dur: number;
  fadeIn: number;
  fadeOut: number;
  children: React.ReactNode;
}> = ({dur, fadeIn, fadeOut, children}) => {
  const f = useCurrentFrame();
  return (
    <Shell frame={f} duration={dur} fadeIn={fadeIn} fadeOut={fadeOut}>
      {children}
    </Shell>
  );
};

export const LaunchVideoV3: React.FC = () => {
  let at = 0;
  return (
    <AbsoluteFill style={{backgroundColor: V2.bg, fontFamily: T.sans, color: T.text}}>
      <AbsoluteFill
        style={{
          background:
            'radial-gradient(1200px 750px at 50% -10%, rgba(112, 199, 186, 0.06), transparent 70%)',
        }}
      />
      {SCENES.map((s) => {
        const from = at;
        at += s.dur - OVERLAP;
        const El = s.el;
        return (
          <Sequence key={s.name} name={s.name} from={from} durationInFrames={s.dur}>
            <ShellAt dur={s.dur} fadeIn={s.fadeIn} fadeOut={s.fadeOut}>
              <El />
            </ShellAt>
          </Sequence>
        );
      })}
    </AbsoluteFill>
  );
};
