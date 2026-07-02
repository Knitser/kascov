import React from 'react';
import {AbsoluteFill, Easing, random, useCurrentFrame, useVideoConfig} from 'remotion';
import {T} from '../../theme';
import {Caption, map, seg, V2, Wordmark} from '../v2/shared';

/* =====================================================================
   V3 scene 2 (5.5s): the problem. A long ribbon of history blocks; a
   pruning sweep erases everything older than "~3 days" while the copy
   lands. Then the answer: kascov remembers.
   ===================================================================== */

export const FORGET_DUR = 330;

type Cell = {x: number; y: number; j: number};
const COLS = 30;
const ROWS = 3;
const CELLS: Cell[] = [];
for (let i = 0; i < COLS; i++) {
  for (let r = 0; r < ROWS; r++) {
    if (random(`fg-skip-${i}-${r}`) < 0.25) continue;
    CELLS.push({
      x: 90 + i * 60 + (random(`fg-x-${i}-${r}`) - 0.5) * 14,
      y: 430 + r * 78 + (random(`fg-y-${i}-${r}`) - 0.5) * 22,
      j: random(`fg-j-${i}-${r}`),
    });
  }
}

const SWEEP_AT = 92;
const SWEEP_DUR = 130;

export const Forget: React.FC = () => {
  const f = useCurrentFrame();
  const {fps} = useVideoConfig();

  /* the pruning front moves left→right across the ribbon */
  const front = map(f, [SWEEP_AT, SWEEP_AT + SWEEP_DUR], [40, 1500], Easing.inOut(Easing.quad));
  const answer = seg(f, 236, 258, Easing.inOut(Easing.quad));

  return (
    <AbsoluteFill>
      {/* history ribbon */}
      <svg width={1920} height={1080} viewBox="0 0 1920 1080" style={{position: 'absolute', inset: 0, opacity: 1 - answer * 0.75}}>
        {CELLS.map((c, i) => {
          const gone = seg(f, SWEEP_AT + ((c.x - 40) / 1460) * SWEEP_DUR, SWEEP_AT + ((c.x - 40) / 1460) * SWEEP_DUR + 16, Easing.in(Easing.quad));
          const o = (0.5 + c.j * 0.5) * (1 - gone);
          if (o <= 0.01) return null;
          return (
            <g key={i} opacity={o} transform={`translate(${c.x} ${c.y + gone * 26}) scale(${1 - gone * 0.4})`}>
              <rect x={-24} y={-17} width={48} height={34} rx={8} fill="rgba(112, 199, 186, 0.10)" stroke={T.accent} strokeOpacity={0.6} strokeWidth={1.6} />
              <circle r={3.4} fill={T.accent} opacity={0.7} />
            </g>
          );
        })}
        {/* the pruning front */}
        {f >= SWEEP_AT && f <= SWEEP_AT + SWEEP_DUR + 20 && (
          <g opacity={0.9 - seg(f, SWEEP_AT + SWEEP_DUR, SWEEP_AT + SWEEP_DUR + 18)}>
            <line x1={front} y1={360} x2={front} y2={700} stroke={T.burn} strokeWidth={3} strokeDasharray="10 8" />
            <text x={front + 14} y={388} fill={T.burn} fontFamily={T.mono} fontSize={24} fontWeight={600}>
              pruning
            </text>
          </g>
        )}
      </svg>

      <Caption frame={f} fps={fps} at={16} out={100} size={62} weight={700} y={180}>
        nodes validate all of it —
      </Caption>
      <Caption frame={f} fps={fps} at={104} out={224} size={62} weight={700} y={180}>
        then <span style={{color: T.burn}}>forget it</span>. after ~3 days, history is gone.
      </Caption>
      <Caption frame={f} fps={fps} at={150} out={224} size={38} weight={500} color={T.muted} y={280}>
        no “get covenant” RPC · no explorer decodes them
      </Caption>

      {/* the answer */}
      {answer > 0 && (
        <AbsoluteFill style={{alignItems: 'center', justifyContent: 'center', opacity: answer}}>
          <div style={{display: 'flex', alignItems: 'center', gap: 34, transform: `translateY(${(1 - answer) * 30}px)`}}>
            <Wordmark size={96} glow={0.5} />
            <span style={{fontSize: 78, fontWeight: 800, color: T.text, letterSpacing: -1}}>
              remembers<span style={{color: T.accent}}>.</span>
            </span>
          </div>
          <div
            style={{
              marginTop: 30,
              fontSize: 36,
              color: T.muted,
              opacity: seg(f, 262, 282),
            }}
          >
            watching every smart coin, live, since day one — on{' '}
            <span style={{color: T.text, fontWeight: 650}}>mainnet</span> and{' '}
            <span style={{color: T.text, fontWeight: 650}}>testnet-10</span>
          </div>
        </AbsoluteFill>
      )}
      {/* soft glow floor for the answer beat */}
      <AbsoluteFill
        style={{
          opacity: answer * 0.5,
          background: `radial-gradient(900px 420px at 50% 55%, ${V2.glowAccent.replace('0.55', '0.10')}, transparent 70%)`,
        }}
      />
    </AbsoluteFill>
  );
};
