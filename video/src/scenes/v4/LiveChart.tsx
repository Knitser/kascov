import React from 'react';
import {AbsoluteFill, Easing, random, useCurrentFrame, useVideoConfig} from 'remotion';
import {T} from '../../theme';
import {Caption, map, pop, seg} from '../v2/shared';

/* =====================================================================
   V4 scene 3 (~7.5s): the new activity chart. Range pills, stacked
   born/moved/retired bars regrowing per range, a tooltip beat, and the
   honest badge flipping amber when the indexer is replaying history.
   ===================================================================== */

export const LIVECHART_DUR = 450;

const RANGES = ['1h', '6h', '24h', '2d', 'all'];
const SWITCH_AT = 150; // "24h" -> "all" click
const TIP_AT = 250;
const BADGE_AT = 330;

const N = 26;

/* deterministic per-range bar data */
const bucket = (i: number, wide: boolean) => {
  const s = wide ? 7 : 3;
  const born = 8 + Math.floor(random(`b${s}-${i}`) * 46);
  const moved = 6 + Math.floor(random(`m${s}-${i}`) * 40);
  const burned = 4 + Math.floor(random(`r${s}-${i}`) * 30);
  return {born, moved, burned, total: born + moved + burned};
};

export const LiveChart: React.FC = () => {
  const f = useCurrentFrame();
  const {fps} = useVideoConfig();

  const wide = f >= SWITCH_AT + 12;
  const activeRange = wide ? 'all' : '24h';
  /* bars re-grow from zero on the range switch */
  const growSince = wide ? SWITCH_AT + 12 : 40;
  const buckets = Array.from({length: N}, (_, i) => bucket(i, wide));
  const maxTotal = Math.max(...buckets.map((b) => b.total));

  const tipIn = pop(f, fps, TIP_AT, 13);
  const tipOn = f >= TIP_AT && f < BADGE_AT + 60;
  const tipBucket = buckets[17];

  const badgeAmber = f >= BADGE_AT;

  const plotW = 1240;
  const plotH = 420;
  const colW = plotW / N;

  return (
    <AbsoluteFill style={{alignItems: 'center', justifyContent: 'center'}}>
      <Caption frame={f} fps={fps} at={6} size={56} weight={760} y={92}>
        the network&apos;s pulse, <span style={{color: T.accent}}>any window you want</span>
      </Caption>

      {/* chart card */}
      <div
        style={{
          width: plotW + 80,
          background: T.card,
          border: `1.5px solid ${T.border}`,
          borderRadius: 20,
          padding: '30px 40px 26px',
          marginTop: 40,
          opacity: seg(f, 24, 46),
        }}
      >
        {/* head: caption + range pills */}
        <div style={{display: 'flex', alignItems: 'center', justifyContent: 'space-between', marginBottom: 24}}>
          <span style={{fontSize: 26, color: T.muted}}>
            life on the testnet · born / moved / retired
          </span>
          <div style={{display: 'flex', gap: 10}}>
            {RANGES.map((r) => {
              const active = r === activeRange;
              const clickPulse = r === 'all' && f >= SWITCH_AT && f < SWITCH_AT + 14 ? 1.12 : 1;
              return (
                <span
                  key={r}
                  style={{
                    fontFamily: T.mono,
                    fontSize: 24,
                    fontWeight: 650,
                    padding: '8px 20px',
                    borderRadius: 999,
                    border: `1.5px solid ${active ? T.borderStrong : T.border}`,
                    background: active ? T.accentSoft : 'transparent',
                    color: active ? T.accent : T.muted,
                    transform: `scale(${clickPulse})`,
                  }}
                >
                  {r}
                </span>
              );
            })}
          </div>
        </div>

        {/* stacked bars */}
        <div style={{position: 'relative', display: 'flex', alignItems: 'flex-end', gap: 6, height: plotH}}>
          {buckets.map((b, i) => {
            const grow = seg(f, growSince + i * 3, growSince + 22 + i * 3, Easing.out(Easing.cubic));
            const h = (n: number) => Math.max((n / maxTotal) * plotH * 0.92 * grow, n ? 3 : 0);
            return (
              <div key={i} style={{width: colW - 6, display: 'flex', flexDirection: 'column', justifyContent: 'flex-end', gap: 2, height: plotH}}>
                <div style={{height: h(b.burned), background: T.burn, borderRadius: '4px 4px 0 0'}} />
                <div style={{height: h(b.moved), background: T.move}} />
                <div style={{height: h(b.born), background: T.born, borderRadius: '0 0 3px 3px'}} />
              </div>
            );
          })}

          {/* tooltip beat */}
          {tipOn && (
            <div
              style={{
                position: 'absolute',
                left: 17 * colW - 130,
                bottom: plotH * 0.7,
                background: T.bgSoft,
                border: `1.5px solid ${T.borderStrong}`,
                borderRadius: 14,
                padding: '14px 22px',
                fontFamily: T.mono,
                fontSize: 24,
                opacity: seg(f, TIP_AT, TIP_AT + 8),
                transform: `translateY(${(1 - tipIn) * 14}px)`,
                boxShadow: `0 10px 44px rgba(0,0,0,0.5)`,
                whiteSpace: 'nowrap',
              }}
            >
              <span style={{color: T.born}}>{tipBucket.born} born</span>
              <span style={{color: T.faint}}> · </span>
              <span style={{color: T.move}}>{tipBucket.moved} moved</span>
              <span style={{color: T.faint}}> · </span>
              <span style={{color: T.burn}}>{tipBucket.burned} retired</span>
            </div>
          )}
        </div>
      </div>

      {/* honest badge beat */}
      <div style={{display: 'flex', alignItems: 'center', gap: 22, marginTop: 34, opacity: seg(f, BADGE_AT - 40, BADGE_AT - 20)}}>
        <span
          style={{
            display: 'inline-flex',
            alignItems: 'center',
            gap: 12,
            fontFamily: T.mono,
            fontSize: 27,
            fontWeight: 650,
            padding: '10px 24px',
            borderRadius: 999,
            border: `1.5px solid ${badgeAmber ? 'rgba(242,165,102,0.5)' : 'rgba(111,217,164,0.5)'}`,
            color: badgeAmber ? T.burn : T.born,
            background: badgeAmber ? T.burnSoft : T.bornSoft,
          }}
        >
          <span
            style={{
              width: 14,
              height: 14,
              borderRadius: 999,
              background: badgeAmber ? T.burn : T.born,
              boxShadow: `0 0 ${12 + 6 * Math.sin(f / 9)}px ${badgeAmber ? T.burn : T.born}`,
            }}
          />
          {badgeAmber ? 'catching up · 53m behind' : 'watching live'}
        </span>
        <span style={{fontSize: 28, color: T.muted, opacity: seg(f, BADGE_AT + 14, BADGE_AT + 34)}}>
          and when it&apos;s replaying history, <span style={{color: T.text, fontWeight: 650}}>it tells you the truth.</span>
        </span>
      </div>
    </AbsoluteFill>
  );
};
