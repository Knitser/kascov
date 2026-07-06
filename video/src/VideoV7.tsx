import React from 'react';
import {AbsoluteFill, Sequence, useCurrentFrame} from 'remotion';
import {EndCardV2, END_DUR} from './scenes/v2/EndCardV2';
import {Shell} from './scenes/v2/shared';
import {ApiScene, API_DUR} from './scenes/v7/ApiScene';
import {BuildScene, BUILD_DUR} from './scenes/v7/BuildScene';
import {GHOST} from './scenes/v7/DagBg';
import {RebuiltHook, REBUILT_DUR} from './scenes/v7/RebuiltHook';
import {WatchScene, WATCH_DUR} from './scenes/v7/WatchScene';
import {T} from './theme';

/* =====================================================================
   GhostDagV7 — the redesign reel (~28s @60fps): the living BlockDAG →
   watch coins live → make your own (a real escrow settles itself) →
   the whole thing as a real JSON API → end card. Ported to kascov's
   GHOSTDAG look: deeper abyss, electric teal, the lattice as signature.
   ===================================================================== */

const OVERLAP = 12;

const SCENES = [
  {name: 'hook', dur: REBUILT_DUR, el: RebuiltHook, fadeIn: 0, fadeOut: OVERLAP},
  {name: 'watch', dur: WATCH_DUR, el: WatchScene, fadeIn: OVERLAP, fadeOut: OVERLAP},
  {name: 'build', dur: BUILD_DUR, el: BuildScene, fadeIn: OVERLAP, fadeOut: OVERLAP},
  {name: 'api', dur: API_DUR, el: ApiScene, fadeIn: OVERLAP, fadeOut: OVERLAP},
  {name: 'end', dur: END_DUR, el: EndCardV2, fadeIn: OVERLAP, fadeOut: 0},
];

export const V7_TOTAL = SCENES.reduce((s, x) => s + x.dur - OVERLAP, 0) + OVERLAP;

const ShellAt: React.FC<{dur: number; fadeIn: number; fadeOut: number; children: React.ReactNode}> = ({
  dur,
  fadeIn,
  fadeOut,
  children,
}) => {
  const f = useCurrentFrame();
  return (
    <Shell frame={f} duration={dur} fadeIn={fadeIn} fadeOut={fadeOut}>
      {children}
    </Shell>
  );
};

export const GhostDagVideoV7: React.FC = () => {
  let at = 0;
  return (
    <AbsoluteFill style={{backgroundColor: GHOST.bg, fontFamily: T.sans, color: GHOST.text}}>
      <AbsoluteFill
        style={{
          background:
            'radial-gradient(1300px 720px at 50% -12%, rgba(73,234,203,0.10), transparent 66%)',
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
