import React from 'react';
import {AbsoluteFill, Sequence, useCurrentFrame} from 'remotion';
import {EndCardV2, END_DUR} from './scenes/v2/EndCardV2';
import {HookTurn, HOOK_TURN_DUR} from './scenes/v2/HookTurn';
import {Journey, JOURNEY_DUR} from './scenes/v2/Journey';
import {Shell, V2} from './scenes/v2/shared';
import {WallSaga, WALL_DUR} from './scenes/v2/WallSaga';
import {T} from './theme';

/* =====================================================================
   LaunchV2 — 2004 frames @ 60fps = 33.4s. Scenes crossfade (12-frame
   overlaps); the first frame is already in motion, the last is a clean
   end card.
   ===================================================================== */

const OVERLAP = 12;

const SCENES: {
  name: string;
  dur: number;
  el: React.FC;
  fadeIn: number;
  fadeOut: number;
}[] = [
  {name: 'hook-turn', dur: HOOK_TURN_DUR, el: HookTurn, fadeIn: 0, fadeOut: OVERLAP},
  {name: 'journey', dur: JOURNEY_DUR, el: Journey, fadeIn: OVERLAP, fadeOut: OVERLAP},
  {name: 'wall-saga', dur: WALL_DUR, el: WallSaga, fadeIn: OVERLAP, fadeOut: OVERLAP},
  {name: 'end-card', dur: END_DUR, el: EndCardV2, fadeIn: OVERLAP, fadeOut: 0},
];

export const V2_TOTAL = SCENES.reduce((sum, s) => sum + s.dur - OVERLAP, 0) + OVERLAP;

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

export const LaunchVideoV2: React.FC = () => {
  let at = 0;
  return (
    <AbsoluteFill style={{backgroundColor: V2.bg, fontFamily: T.sans, color: T.text}}>
      {/* barely-there teal atmosphere, like the site */}
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
