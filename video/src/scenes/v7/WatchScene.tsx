import React from 'react';
import {AbsoluteFill, useCurrentFrame, useVideoConfig} from 'remotion';
import {Caption, seg, pop} from '../v2/shared';
import {DagBg, GHOST} from './DagBg';

export const WATCH_DUR = 330;

const ROWS = [
  {name: 'silent-cobalt-hedgehog', verb: 'was born', color: GHOST.born},
  {name: 'tiny-violet-toad', verb: 'moved', color: GHOST.move},
  {name: 'glad-olive-raven', verb: 'retired', color: GHOST.burn},
  {name: 'quiet-amber-zebra', verb: 'was born', color: GHOST.born},
  {name: 'rapid-cobalt-heron', verb: 'moved', color: GHOST.move},
];

export const WatchScene: React.FC = () => {
  const f = useCurrentFrame();
  const {fps} = useVideoConfig();
  return (
    <AbsoluteFill style={{alignItems: 'center', justifyContent: 'center'}}>
      <DagBg dim={0.4} />
      <Caption frame={f} fps={fps} at={8} size={58} weight={760} y={-230}>
        watch them <span style={{color: GHOST.accent}}>live</span>
      </Caption>
      <div style={{width: 1080, marginTop: 60, display: 'flex', flexDirection: 'column', gap: 14}}>
        {ROWS.map((r, i) => {
          const at = 40 + i * 20;
          const inn = pop(f, fps, at, 13);
          return (
            <div
              key={r.name}
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 20,
                background: 'rgba(11,26,23,0.72)',
                border: `1px solid rgba(120,220,200,0.1)`,
                borderRadius: 14,
                padding: '18px 26px',
                opacity: seg(f, at, at + 12),
                transform: `translateX(${(1 - inn) * -30}px)`,
              }}
            >
              <span style={{width: 12, height: 12, borderRadius: 99, background: r.color, boxShadow: `0 0 12px ${r.color}`}} />
              <span style={{fontFamily: GHOST.mono, fontSize: 27, fontWeight: 600, color: GHOST.text}}>{r.name}</span>
              <span style={{fontSize: 25, color: GHOST.muted}}>{r.verb}</span>
              <span style={{marginLeft: 'auto', fontFamily: GHOST.mono, fontSize: 20, color: GHOST.faint}}>just now</span>
            </div>
          );
        })}
      </div>
    </AbsoluteFill>
  );
};
