import React from 'react';
import {AbsoluteFill, useCurrentFrame, useVideoConfig} from 'remotion';
import {T} from '../../theme';
import {U} from '../../lib/updatestats';
import {map, pop, seg} from '../v2/shared';

/* =====================================================================
   V4 scene 5 (~7s): the daily digest + the phone. A last-24h strip
   counts up inside an iPhone frame; SSE events flash in live. The
   whole site is mobile-perfect now — the frame IS the pitch.
   ===================================================================== */

export const MOBILE_DUR = 420;

const EVENTS_AT = 210;
const FEED = [
  {kind: 'born', name: 'lucky-indigo-ibis', color: T.born},
  {kind: 'moved', name: 'quiet-teal-robin', color: T.move},
  {kind: 'born', name: 'mellow-coral-toad', color: T.born},
];

export const MobileLive: React.FC = () => {
  const f = useCurrentFrame();
  const {fps} = useVideoConfig();
  const phoneIn = pop(f, fps, 20, 15);

  const count = (n: number, at: number) =>
    Math.round(map(f, [at, at + 60], [0, n]));

  return (
    <AbsoluteFill style={{flexDirection: 'row', alignItems: 'center', justifyContent: 'center', gap: 90}}>
      {/* the phone */}
      <div
        style={{
          width: 380,
          height: 780,
          borderRadius: 54,
          border: `3px solid rgba(255,255,255,0.16)`,
          background: T.bg,
          boxShadow: '0 30px 90px rgba(0,0,0,0.6), inset 0 0 0 10px #000',
          padding: '26px 16px',
          transform: `translateY(${(1 - phoneIn) * 60}px) rotate(${(1 - phoneIn) * -3}deg)`,
          opacity: seg(f, 20, 34),
          overflow: 'hidden',
          flex: 'none',
        }}
      >
        {/* notch */}
        <div style={{width: 120, height: 24, borderRadius: 14, background: '#000', margin: '0 auto 18px'}} />
        {/* header */}
        <div style={{display: 'flex', alignItems: 'center', gap: 8, padding: '0 10px', marginBottom: 14}}>
          <div style={{width: 16, height: 16, borderRadius: 999, border: `3px solid ${T.accent}`}} />
          <span style={{fontWeight: 800, fontSize: 19, color: T.text}}>kascov</span>
          <span
            style={{
              marginLeft: 'auto',
              fontFamily: T.mono,
              fontSize: 12,
              fontWeight: 650,
              color: T.born,
              border: '1px solid rgba(111,217,164,0.5)',
              background: T.bornSoft,
              borderRadius: 999,
              padding: '3px 10px',
            }}
          >
            ● watching live
          </span>
        </div>
        {/* digest strip */}
        <div
          style={{
            margin: '0 10px',
            background: T.card,
            border: `1px solid ${T.border}`,
            borderRadius: 14,
            padding: '12px 8px 10px',
            opacity: seg(f, 70, 90),
          }}
        >
          <div style={{textAlign: 'center', fontSize: 13, color: T.muted, marginBottom: 8}}>the last 24 hours</div>
          <div style={{display: 'flex', justifyContent: 'space-evenly'}}>
            {[
              {n: count(U.digest.born, 90), label: 'born', c: T.born},
              {n: count(U.digest.moved, 100), label: 'moved', c: T.move},
              {n: count(U.digest.retired, 110), label: 'retired', c: T.burn},
            ].map((s) => (
              <div key={s.label} style={{textAlign: 'center'}}>
                <div style={{fontFamily: T.mono, fontSize: 26, fontWeight: 800, color: s.c}}>{s.n}</div>
                <div style={{fontSize: 12, color: T.muted}}>{s.label}</div>
              </div>
            ))}
          </div>
        </div>
        {/* mini chart */}
        <div style={{display: 'flex', alignItems: 'flex-end', gap: 3, height: 90, margin: '16px 10px 14px'}}>
          {Array.from({length: 18}, (_, i) => {
            const h = 18 + ((i * 37) % 61);
            const grow = seg(f, 120 + i * 3, 140 + i * 3);
            return (
              <div key={i} style={{flex: 1, display: 'flex', flexDirection: 'column', justifyContent: 'flex-end', gap: 1.5, height: 90}}>
                <div style={{height: h * 0.32 * grow, background: T.burn, borderRadius: 2}} />
                <div style={{height: h * 0.3 * grow, background: T.move}} />
                <div style={{height: h * 0.38 * grow, background: T.born, borderRadius: '0 0 2px 2px'}} />
              </div>
            );
          })}
        </div>
        {/* live stories pushing in */}
        <div style={{margin: '0 10px'}}>
          {FEED.map((e, i) => {
            const at = EVENTS_AT + i * 34;
            const inP = pop(f, fps, at, 12);
            return (
              <div
                key={i}
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  gap: 10,
                  background: T.card,
                  border: `1px solid ${T.border}`,
                  borderLeft: `3px solid ${e.color}`,
                  borderRadius: 12,
                  padding: '10px 12px',
                  marginBottom: 8,
                  fontSize: 14,
                  opacity: seg(f, at, at + 6),
                  transform: `translateX(${(1 - inP) * 40}px)`,
                }}
              >
                <span style={{color: T.text, fontWeight: 700, fontFamily: T.mono, fontSize: 13}}>{e.name}</span>
                <span style={{color: e.color, marginLeft: 'auto', fontWeight: 650}}>{e.kind}</span>
              </div>
            );
          })}
        </div>
      </div>

      {/* copy column */}
      <div style={{width: 620}}>
        <div
          style={{
            fontSize: 58,
            fontWeight: 780,
            color: T.text,
            letterSpacing: -0.5,
            marginBottom: 10,
            opacity: seg(f, 40, 58),
            transform: `translateY(${(1 - pop(f, fps, 40)) * 30}px)`,
          }}
        >
          your <span style={{color: T.accent}}>pocket telescope</span>
        </div>
        {[
          {at: 120, text: 'the whole site rebuilt for phones — nothing cut'},
          {at: 180, text: 'a daily digest: the last 24 hours at a glance'},
          {at: EVENTS_AT + 20, text: 'events push to open pages the second they happen'},
        ].map((l) => (
          <div
            key={l.at}
            style={{
              fontSize: 34,
              color: T.text,
              margin: '26px 0',
              display: 'flex',
              gap: 16,
              alignItems: 'baseline',
              opacity: seg(f, l.at, l.at + 18),
            }}
          >
            <span style={{color: T.accent, fontWeight: 800}}>·</span>
            {l.text}
          </div>
        ))}
      </div>
    </AbsoluteFill>
  );
};
