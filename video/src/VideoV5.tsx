import React from 'react';
import {AbsoluteFill, Sequence, useCurrentFrame} from 'remotion';
import {EndCardV2, END_DUR} from './scenes/v2/EndCardV2';
import {Shell, V2} from './scenes/v2/shared';
import {GenDeploy, GENDEPLOY_DUR} from './scenes/v5/GenDeploy';
import {GenEdit, GENEDIT_DUR} from './scenes/v5/GenEdit';
import {GenExtras, GENEXTRAS_DUR} from './scenes/v5/GenExtras';
import {GenHook, GENHOOK_DUR} from './scenes/v5/GenHook';
import {T} from './theme';

/* =====================================================================
   GeneratorV5 — the short "make this yours" reel (~27s @60fps):
   found a contract → edit it in the browser (verified live) → one
   command births it (starring rapid-cobalt-heron, a real TN10 coin
   deployed while testing the feature) → tour + honest clocks → end card.
   ===================================================================== */

const OVERLAP = 12;

const SCENES: {
  name: string;
  dur: number;
  el: React.FC;
  fadeIn: number;
  fadeOut: number;
}[] = [
  {name: 'hook', dur: GENHOOK_DUR, el: GenHook, fadeIn: 0, fadeOut: OVERLAP},
  {name: 'edit', dur: GENEDIT_DUR, el: GenEdit, fadeIn: OVERLAP, fadeOut: OVERLAP},
  {name: 'deploy', dur: GENDEPLOY_DUR, el: GenDeploy, fadeIn: OVERLAP, fadeOut: OVERLAP},
  {name: 'extras', dur: GENEXTRAS_DUR, el: GenExtras, fadeIn: OVERLAP, fadeOut: OVERLAP},
  {name: 'end-card', dur: END_DUR, el: EndCardV2, fadeIn: OVERLAP, fadeOut: 0},
];

export const V5_TOTAL = SCENES.reduce((sum, s) => sum + s.dur - OVERLAP, 0) + OVERLAP;

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

export const GeneratorVideoV5: React.FC = () => {
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
