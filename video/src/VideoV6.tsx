import React from 'react';
import {AbsoluteFill, Sequence, useCurrentFrame} from 'remotion';
import {EndCardV2, END_DUR} from './scenes/v2/EndCardV2';
import {Shell, V2} from './scenes/v2/shared';
import {EscrowSettle, ESCROWSETTLE_DUR} from './scenes/v6/EscrowSettle';
import {ProofApps, PROOFAPPS_DUR} from './scenes/v6/ProofApps';
import {RevealStory, REVEALSTORY_DUR} from './scenes/v6/RevealStory';
import {RunHook, RUNHOOK_DUR} from './scenes/v6/RunHook';
import {T} from './theme';

/* =====================================================================
   RunV6 — "they RUN now" (~28s @60fps): contracts don't sit there →
   a real escrow settles itself in one command (lively-slate-urchin,
   settled on TN10 while building the feature) → the spend reveals the
   program, named forever → reveal-preview + apps → end card.
   ===================================================================== */

const OVERLAP = 12;

const SCENES: {
  name: string;
  dur: number;
  el: React.FC;
  fadeIn: number;
  fadeOut: number;
}[] = [
  {name: 'hook', dur: RUNHOOK_DUR, el: RunHook, fadeIn: 0, fadeOut: OVERLAP},
  {name: 'settle', dur: ESCROWSETTLE_DUR, el: EscrowSettle, fadeIn: OVERLAP, fadeOut: OVERLAP},
  {name: 'reveal', dur: REVEALSTORY_DUR, el: RevealStory, fadeIn: OVERLAP, fadeOut: OVERLAP},
  {name: 'proof-apps', dur: PROOFAPPS_DUR, el: ProofApps, fadeIn: OVERLAP, fadeOut: OVERLAP},
  {name: 'end-card', dur: END_DUR, el: EndCardV2, fadeIn: OVERLAP, fadeOut: 0},
];

export const V6_TOTAL = SCENES.reduce((sum, s) => sum + s.dur - OVERLAP, 0) + OVERLAP;

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

export const RunVideoV6: React.FC = () => {
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
