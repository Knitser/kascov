import React from 'react';
import {AbsoluteFill, Easing, useCurrentFrame, useVideoConfig} from 'remotion';
import {Avatar} from '../../lib/Avatar';
import {cascade, star} from '../../lib/data';
import {T} from '../../theme';
import {Caption, HashTicker, map, pop, seg} from '../v2/shared';

/* =====================================================================
   V3 scene 4 (8s): the tools, three fast beats —
   paste-a-txid search → watchlist star → record holders.
   ===================================================================== */

export const TOOLS3_DUR = 480;

const TXID = star.c.events[0]?.txid ?? star.c.covenant_id;
const REC = cascade.slice(3, 6);
const REC_LABELS = ['oldest alive', 'most traveled', 'richest'];

const Panel: React.FC<{
  frame: number;
  fps: number;
  at: number;
  out: number;
  children: React.ReactNode;
}> = ({frame, fps, at, out, children}) => {
  const s = pop(frame, fps, at, 15);
  const o = seg(frame, at, at + 8) * (1 - seg(frame, out, out + 14, Easing.inOut(Easing.quad)));
  if (frame < at - 1 || o <= 0) return null;
  return (
    <AbsoluteFill
      style={{
        alignItems: 'center',
        justifyContent: 'center',
        opacity: o,
        transform: `translateY(${(1 - s) * 36}px)`,
      }}
    >
      {children}
    </AbsoluteFill>
  );
};

export const Tools3: React.FC = () => {
  const f = useCurrentFrame();
  const {fps} = useVideoConfig();

  /* beat 1: search — a txid types into the pill, the coin pops out */
  const typedChars = Math.floor(map(f, [36, 96], [0, 30], Easing.linear));
  const found = pop(f, fps, 112, 12);
  const flash = seg(f, 112, 152, Easing.out(Easing.cubic));

  /* beat 2: the star toggles gold */
  const starOn = f >= 236;
  const starPop = pop(f, fps, 236, 10);

  /* beat 3 record cards */
  const short = `${TXID.slice(0, 18)}…`;

  return (
    <AbsoluteFill>
      <Caption frame={f} fps={fps} at={4} size={54} weight={720} y={92}>
        built for testers<span style={{color: T.accent}}>:</span> answers in one paste
      </Caption>

      {/* ---- beat 1: txid search */}
      <Panel frame={f} fps={fps} at={24} out={168}>
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 16,
            background: T.bgSoft,
            border: `1.5px solid ${T.borderStrong}`,
            borderRadius: 999,
            padding: '20px 34px',
            width: 1000,
          }}
        >
          <svg viewBox="0 0 20 20" width={30} height={30}>
            <circle cx={8.5} cy={8.5} r={5.5} fill="none" stroke={T.faint} strokeWidth={2} />
            <line x1={12.8} y1={12.8} x2={17} y2={17} stroke={T.faint} strokeWidth={2} strokeLinecap="round" />
          </svg>
          <span style={{fontFamily: T.mono, fontSize: 30, color: T.text, whiteSpace: 'pre'}}>
            {short.slice(0, typedChars)}
            <span
              style={{
                display: 'inline-block',
                width: 14,
                height: 30,
                marginLeft: 4,
                background: T.accent,
                opacity: f % 14 < 9 ? 0.9 : 0.1,
                transform: 'translateY(4px)',
              }}
            />
          </span>
        </div>
        {f >= 108 && (
          <div
            style={{
              marginTop: 44,
              display: 'flex',
              alignItems: 'center',
              gap: 22,
              padding: '20px 30px',
              borderRadius: 16,
              background: T.card,
              border: `2px solid ${T.accent}`,
              boxShadow: `0 0 ${40 * (1 - flash) + 16}px rgba(112,199,186,${0.5 - flash * 0.28})`,
              transform: `scale(${0.9 + found * 0.1})`,
            }}
          >
            <Avatar id={star.c.covenant_id} size={64} />
            <div>
              <div style={{fontFamily: T.mono, fontSize: 30, fontWeight: 700}}>{star.name}</div>
              <div style={{fontSize: 22, color: T.muted}}>found it — the exact event, highlighted</div>
            </div>
          </div>
        )}
        <div style={{marginTop: 34, fontSize: 30, color: T.muted}}>
          paste any <span style={{color: T.text, fontWeight: 650}}>transaction id</span> — land on the coin it touched
        </div>
      </Panel>

      {/* ---- beat 2: watchlist */}
      <Panel frame={f} fps={fps} at={186} out={300}>
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 24,
            padding: '24px 36px',
            borderRadius: 18,
            background: T.card,
            border: `1px solid ${T.border}`,
          }}
        >
          <Avatar id={star.c.covenant_id} size={72} />
          <div style={{fontFamily: T.mono, fontSize: 34, fontWeight: 700}}>{star.name}</div>
          <div
            style={{
              fontSize: 52,
              color: starOn ? '#ffd479' : T.faint,
              transform: `scale(${starOn ? 0.9 + starPop * 0.35 : 1})`,
              textShadow: starOn ? '0 0 26px rgba(255,212,121,0.65)' : undefined,
              marginLeft: 12,
            }}
          >
            ★
          </div>
        </div>
        <div style={{marginTop: 34, fontSize: 30, color: T.muted}}>
          star your coins — a <span style={{color: '#ffd479', fontWeight: 650}}>watchlist</span> that survives reloads
        </div>
      </Panel>

      {/* ---- beat 3: record holders */}
      <Panel frame={f} fps={fps} at={318} out={TOOLS3_DUR - 6}>
        <div style={{display: 'flex', gap: 26}}>
          {REC.map((e, i) => {
            const at = 326 + i * 14;
            const s = pop(f, fps, at, 15);
            const o = seg(f, at, at + 8);
            return (
              <div
                key={e.c.covenant_id}
                style={{
                  width: 420,
                  padding: '22px 26px',
                  borderRadius: 16,
                  background: T.card,
                  border: `1px solid ${T.border}`,
                  opacity: o,
                  transform: `translateY(${(1 - s) * 26}px)`,
                }}
              >
                <div style={{fontSize: 20, fontWeight: 700, letterSpacing: 1.5, color: T.accent, textTransform: 'uppercase'}}>
                  {REC_LABELS[i]}
                </div>
                <div style={{display: 'flex', alignItems: 'center', gap: 16, marginTop: 14}}>
                  <Avatar id={e.c.covenant_id} size={54} />
                  <div style={{fontFamily: T.mono, fontSize: 26, fontWeight: 650}}>{e.name}</div>
                </div>
              </div>
            );
          })}
        </div>
        <div style={{marginTop: 40, fontSize: 30, color: T.muted}}>
          record holders, sorting, live suggestions — and a{' '}
          <span style={{color: T.text, fontWeight: 650}}>JSON API</span> under it all
        </div>
        <div style={{marginTop: 16, opacity: seg(f, 392, 410)}}>
          <HashTicker frame={f} at={392} txid={TXID} size={22} />
        </div>
      </Panel>
    </AbsoluteFill>
  );
};
