import React from 'react';
import {AbsoluteFill, Easing, useCurrentFrame, useVideoConfig} from 'remotion';
import {Avatar} from '../../lib/Avatar';
import {cascade, cardLine, star, starSteps} from '../../lib/data';
import {decode} from '../../lib/decode';
import {fmtInt} from '../../lib/identity';
import {KIND_COLOR, KIND_SOFT, T} from '../../theme';
import {Caption, map, Odometer, pop, seg} from '../v2/shared';

/* =====================================================================
   V3 scene 3 (9s): the explorer itself. Live badge + stat odometers,
   real story cards sliding in, then one coin's life story timeline.
   ===================================================================== */

export const EXPLORER3_DUR = 540;

const STORIES = cascade.slice(0, 3);
const STEPS = starSteps.slice(0, 4);

const StoryCard: React.FC<{
  frame: number;
  fps: number;
  at: number;
  entry: (typeof STORIES)[number];
  y: number;
}> = ({frame, fps, at, entry, y}) => {
  const s = pop(frame, fps, at, 15);
  const o = seg(frame, at, at + 8);
  if (frame < at - 1) return null;
  const kind = entry.c.events[entry.c.events.length - 1]?.kind ?? 'transition';
  return (
    <div
      style={{
        position: 'absolute',
        left: 1020,
        top: y,
        width: 700,
        display: 'flex',
        alignItems: 'center',
        gap: 18,
        padding: '16px 22px',
        borderRadius: 14,
        background: T.card,
        border: `1px solid ${T.border}`,
        opacity: o,
        transform: `translateX(${(1 - s) * 90}px)`,
      }}
    >
      <Avatar id={entry.c.covenant_id} size={52} />
      <div style={{minWidth: 0}}>
        <div style={{fontFamily: T.mono, fontSize: 26, fontWeight: 650, color: T.text}}>{entry.name}</div>
        <div style={{fontSize: 22, color: T.muted, marginTop: 2}}>{cardLine(entry)}</div>
      </div>
      <div
        style={{
          marginLeft: 'auto',
          width: 14,
          height: 14,
          borderRadius: 7,
          background: KIND_COLOR[kind],
          boxShadow: `0 0 14px ${KIND_COLOR[kind]}`,
          flex: 'none',
        }}
      />
    </div>
  );
};

export const Explorer3: React.FC = () => {
  const f = useCurrentFrame();
  const {fps} = useVideoConfig();

  const statP = seg(f, 60, 150, Easing.inOut(Easing.cubic));
  const liveO = seg(f, 26, 44);
  const livePulse = 0.5 + 0.5 * Math.sin(f / 16);
  const tlIn = seg(f, 290, 320);

  return (
    <AbsoluteFill>
      <Caption frame={f} fps={fps} at={8} size={64} weight={780} y={96}>
        the explorer: <span style={{color: T.accent}}>kascov-explorer.web.app</span>
      </Caption>

      {/* live badge */}
      <div
        style={{
          position: 'absolute',
          top: 208,
          left: 0,
          right: 0,
          display: 'flex',
          justifyContent: 'center',
          gap: 12,
          alignItems: 'center',
          opacity: liveO,
        }}
      >
        <div
          style={{
            width: 14,
            height: 14,
            borderRadius: 7,
            background: T.born,
            boxShadow: `0 0 ${10 + livePulse * 14}px ${T.born}`,
          }}
        />
        <span style={{fontSize: 30, fontWeight: 650, color: T.born}}>watching live</span>
        <span style={{fontSize: 26, color: T.faint}}>— seconds behind the chain tip, exact timestamps</span>
      </div>

      {/* stat odometers (real, frozen at render time) */}
      <div style={{position: 'absolute', top: 300, left: 130, display: 'flex', gap: 90, opacity: seg(f, 56, 72)}}>
        {[
          {n: decode.stats?.tn10.covenants ?? 7900, label: 'smart coins on testnet-10', digits: 4},
          {n: decode.stats?.tn10.events ?? 32414, label: 'life events recorded', digits: 5},
          {n: decode.stats?.mainnet.covenants ?? 5, label: 'mainnet coins — indexed from day one', digits: 1},
        ].map((s, i) => (
          <div key={i}>
            <Odometer target={s.n} progress={statP} digits={s.digits} size={78} color={T.accent} />
            <div style={{fontSize: 24, color: T.muted, marginTop: 10, maxWidth: 330}}>{s.label}</div>
          </div>
        ))}
      </div>

      {/* real story cards */}
      <Caption frame={f} fps={fps} at={160} size={34} weight={600} color={T.muted} y={508}>
        every coin gets a name, a face, and a life story — updating as it happens
      </Caption>
      {STORIES.map((e, i) => (
        <StoryCard key={e.c.covenant_id} frame={f} fps={fps} at={196 + i * 26} entry={e} y={600 + i * 118} />
      ))}

      {/* one coin's timeline (the star) */}
      {tlIn > 0 && (
        <div style={{position: 'absolute', left: 150, top: 588, width: 760, opacity: tlIn}}>
          <div style={{display: 'flex', alignItems: 'center', gap: 20, marginBottom: 26}}>
            <Avatar id={star.c.covenant_id} size={72} />
            <div>
              <div style={{fontFamily: T.mono, fontSize: 32, fontWeight: 700}}>{star.name}</div>
              <div style={{fontSize: 22, color: T.faint}}>one coin&apos;s life, minute by minute</div>
            </div>
          </div>
          {STEPS.map((st, i) => {
            const at = 330 + i * 34;
            const s = pop(f, fps, at, 16);
            const o = seg(f, at, at + 8);
            return (
              <div
                key={i}
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  gap: 16,
                  marginBottom: 14,
                  opacity: o,
                  transform: `translateY(${(1 - s) * 20}px)`,
                }}
              >
                <div
                  style={{
                    width: 34,
                    height: 34,
                    borderRadius: 17,
                    background: KIND_SOFT[st.kind],
                    border: `2px solid ${KIND_COLOR[st.kind]}`,
                    flex: 'none',
                  }}
                />
                <span style={{fontSize: 26, color: T.text, fontWeight: 550}}>{st.label}</span>
                <span style={{fontFamily: T.mono, fontSize: 20, color: T.faint, marginLeft: 'auto'}}>
                  +{Math.round(st.deltaS)}s
                </span>
              </div>
            );
          })}
        </div>
      )}
    </AbsoluteFill>
  );
};
