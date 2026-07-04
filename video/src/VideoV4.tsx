import React from 'react';
import {AbsoluteFill, Sequence, useCurrentFrame} from 'remotion';
import {EndCardV2, END_DUR} from './scenes/v2/EndCardV2';
import {Shell, V2} from './scenes/v2/shared';
import {AddressIntel, ADDR_DUR} from './scenes/v4/AddressIntel';
import {LiveChart, LIVECHART_DUR} from './scenes/v4/LiveChart';
import {MobileLive, MOBILE_DUR} from './scenes/v4/MobileLive';
import {Speed, SPEED_DUR} from './scenes/v4/Speed';
import {UpdateHook, UHOOK_DUR} from './scenes/v4/UpdateHook';
import {T} from './theme';

/* =====================================================================
   UpdateV4 — the launch-week update reel @60fps (~39s):
   the 40× week → the speed rebuild → the interactive chart + honest
   badge → address intelligence + what's-running → the phone + digest +
   live push → end card. Every number on screen is a real production
   measurement (lib/updatestats.ts, refreshed at render time).
   ===================================================================== */

const OVERLAP = 12;

const SCENES: {
  name: string;
  dur: number;
  el: React.FC;
  fadeIn: number;
  fadeOut: number;
}[] = [
  {name: 'update-hook', dur: UHOOK_DUR, el: UpdateHook, fadeIn: 0, fadeOut: OVERLAP},
  {name: 'speed', dur: SPEED_DUR, el: Speed, fadeIn: OVERLAP, fadeOut: OVERLAP},
  {name: 'live-chart', dur: LIVECHART_DUR, el: LiveChart, fadeIn: OVERLAP, fadeOut: OVERLAP},
  {name: 'address-intel', dur: ADDR_DUR, el: AddressIntel, fadeIn: OVERLAP, fadeOut: OVERLAP},
  {name: 'mobile-live', dur: MOBILE_DUR, el: MobileLive, fadeIn: OVERLAP, fadeOut: OVERLAP},
  {name: 'end-card', dur: END_DUR, el: EndCardV2, fadeIn: OVERLAP, fadeOut: 0},
];

export const V4_TOTAL = SCENES.reduce((sum, s) => sum + s.dur - OVERLAP, 0) + OVERLAP;

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

export const UpdateVideoV4: React.FC = () => {
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
