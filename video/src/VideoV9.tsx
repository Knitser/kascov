import React from 'react';
import {AbsoluteFill, Sequence, useCurrentFrame} from 'remotion';
import {Shell} from './scenes/v2/shared';
import {GHOST} from './scenes/v7/DagBg';
import {
  DebuggerScene,
  DEBUG_DUR,
  EndCardV9,
  ENDV9_DUR,
  LanesScene,
  LANES_DUR,
  SimScene,
  SIM_DUR,
  TitleCard,
  TITLE_DUR,
  VerifiedScene,
  VERIFIED_DUR,
  ZkScene,
  ZK_DUR,
} from './scenes/v9/UpdateScenes';
import {T} from './theme';

/* =====================================================================
   FeaturesV9 — the feature-update reel. Five new powers, step by step:
   1) covenant simulation (animated terminal), then real captured footage
   of 2) verified contracts, 3) the visual debugger stepping, 4) based-app
   lanes, 5) the KIP-16 ZK panel on a real mainnet coin. ~28s @60fps.
   ===================================================================== */

const OVERLAP = 12;

const SCENES = [
  {name: 'title', dur: TITLE_DUR, el: TitleCard, fadeIn: 0, fadeOut: OVERLAP},
  {name: 'sim', dur: SIM_DUR, el: SimScene, fadeIn: OVERLAP, fadeOut: OVERLAP},
  {name: 'verified', dur: VERIFIED_DUR, el: VerifiedScene, fadeIn: OVERLAP, fadeOut: OVERLAP},
  {name: 'debugger', dur: DEBUG_DUR, el: DebuggerScene, fadeIn: OVERLAP, fadeOut: OVERLAP},
  {name: 'lanes', dur: LANES_DUR, el: LanesScene, fadeIn: OVERLAP, fadeOut: OVERLAP},
  {name: 'zk', dur: ZK_DUR, el: ZkScene, fadeIn: OVERLAP, fadeOut: OVERLAP},
  {name: 'end', dur: ENDV9_DUR, el: EndCardV9, fadeIn: OVERLAP, fadeOut: 0},
];

export const V9_TOTAL = SCENES.reduce((s, x) => s + x.dur - OVERLAP, 0) + OVERLAP;

const ShellAt: React.FC<{dur: number; fadeIn: number; fadeOut: number; children: React.ReactNode}> = ({dur, fadeIn, fadeOut, children}) => {
  const f = useCurrentFrame();
  return (
    <Shell frame={f} duration={dur} fadeIn={fadeIn} fadeOut={fadeOut}>
      {children}
    </Shell>
  );
};

export const FeaturesVideoV9: React.FC = () => {
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
