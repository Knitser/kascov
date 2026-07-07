import React from 'react';
import {AbsoluteFill, Sequence, useCurrentFrame} from 'remotion';
import {Shell} from './scenes/v2/shared';
import {GHOST} from './scenes/v7/DagBg';
import {
  CompileScene,
  COMPILE_DUR,
  EndCardV10,
  ENDV10_DUR,
  ExplainScene,
  EXPLAIN_DUR,
  GraphScene,
  GRAPH_DUR,
  PlaygroundScene,
  PLAY_DUR,
  TitleCard,
  TITLE_DUR,
  ZkVerifyScene,
  ZK_DUR,
} from './scenes/v10/Scenes10';
import {T} from './theme';

/* =====================================================================
   FeaturesV10 — "now a workbench". The update reel after V9: write & compile
   a covenant in the browser, verify a real ZK proof, one unified playground,
   plain-English contracts, and a live app graph. ~32s @60fps.
   ===================================================================== */

const OVERLAP = 12;

const SCENES = [
  {name: 'title', dur: TITLE_DUR, el: TitleCard, fadeIn: 0, fadeOut: OVERLAP},
  {name: 'compile', dur: COMPILE_DUR, el: CompileScene, fadeIn: OVERLAP, fadeOut: OVERLAP},
  {name: 'zk', dur: ZK_DUR, el: ZkVerifyScene, fadeIn: OVERLAP, fadeOut: OVERLAP},
  {name: 'playground', dur: PLAY_DUR, el: PlaygroundScene, fadeIn: OVERLAP, fadeOut: OVERLAP},
  {name: 'explain', dur: EXPLAIN_DUR, el: ExplainScene, fadeIn: OVERLAP, fadeOut: OVERLAP},
  {name: 'graph', dur: GRAPH_DUR, el: GraphScene, fadeIn: OVERLAP, fadeOut: OVERLAP},
  {name: 'end', dur: ENDV10_DUR, el: EndCardV10, fadeIn: OVERLAP, fadeOut: 0},
];

export const V10_TOTAL = SCENES.reduce((s, x) => s + x.dur - OVERLAP, 0) + OVERLAP;

const ShellAt: React.FC<{dur: number; fadeIn: number; fadeOut: number; children: React.ReactNode}> = ({dur, fadeIn, fadeOut, children}) => {
  const f = useCurrentFrame();
  return (
    <Shell frame={f} duration={dur} fadeIn={fadeIn} fadeOut={fadeOut}>
      {children}
    </Shell>
  );
};

export const FeaturesVideoV10: React.FC = () => {
  let at = 0;
  return (
    <AbsoluteFill style={{backgroundColor: GHOST.bg, fontFamily: T.sans, color: GHOST.text}}>
      <AbsoluteFill style={{background: 'radial-gradient(1300px 720px at 50% -12%, rgba(73,234,203,0.09), transparent 66%)'}} />
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
