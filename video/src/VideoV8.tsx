import React from 'react';
import {AbsoluteFill, Sequence, useCurrentFrame} from 'remotion';
import {EndCardV2, END_DUR} from './scenes/v2/EndCardV2';
import {Shell} from './scenes/v2/shared';
import {GHOST} from './scenes/v7/DagBg';
import {ApiScene, API_DUR, DashScene, DASH_DUR, TitleCard, TITLE_DUR} from './scenes/v8/DevTourScenes';
import {T} from './theme';

/* =====================================================================
   DevTourV8 — real footage of the live site scrolling: the dashboard
   overview, then the developer / API reference (the star), captured
   from prod with the sticky sidebar + scroll-spy intact, framed in a
   browser window with captions. ~24s @60fps.
   ===================================================================== */

const OVERLAP = 12;

const SCENES = [
  {name: 'title', dur: TITLE_DUR, el: TitleCard, fadeIn: 0, fadeOut: OVERLAP},
  {name: 'dash', dur: DASH_DUR, el: DashScene, fadeIn: OVERLAP, fadeOut: OVERLAP},
  {name: 'api', dur: API_DUR, el: ApiScene, fadeIn: OVERLAP, fadeOut: OVERLAP},
  {name: 'end', dur: END_DUR, el: EndCardV2, fadeIn: OVERLAP, fadeOut: 0},
];

export const V8_TOTAL = SCENES.reduce((s, x) => s + x.dur - OVERLAP, 0) + OVERLAP;

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

export const DevTourVideoV8: React.FC = () => {
  let at = 0;
  return (
    <AbsoluteFill style={{backgroundColor: GHOST.bg, fontFamily: T.sans, color: GHOST.text}}>
      <AbsoluteFill
        style={{background: 'radial-gradient(1300px 720px at 50% -12%, rgba(73,234,203,0.09), transparent 66%)'}}
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
