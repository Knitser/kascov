import React from 'react';
import {Img, interpolate, staticFile, useCurrentFrame} from 'remotion';
import {GHOST} from '../v7/DagBg';

/* Plays a captured smooth-scroll frame sequence inside a browser window,
   mapping the scene-local frame to the captured frame index. The footage is
   real: sticky sidebar + scroll-spy preserved. */
export const ScrollFootage: React.FC<{
  dir: string;
  count: number;
  dur: number;
  url: string;
  hold?: number; // frames to hold at the start before scrolling
}> = ({dir, count, dur, url, hold = 18}) => {
  const f = useCurrentFrame();
  const idx = Math.max(
    0,
    Math.min(count - 1, Math.round(interpolate(f, [hold, dur - 6], [0, count - 1], {extrapolateLeft: 'clamp', extrapolateRight: 'clamp'})))
  );
  const src = staticFile(`devtour/${dir}/f${String(idx).padStart(4, '0')}.png`);
  const W = 1580;
  const H = Math.round((W * 850) / 1440); // capture was 1440x850 CSS
  return (
    <div
      style={{
        width: W,
        borderRadius: 16,
        overflow: 'hidden',
        border: `1px solid rgba(120,220,200,0.16)`,
        boxShadow: '0 40px 120px -30px rgba(0,0,0,0.8), 0 0 0 1px rgba(0,0,0,0.4)',
        background: '#05100e',
      }}
    >
      <div style={{height: 40, display: 'flex', alignItems: 'center', gap: 16, padding: '0 18px', background: '#0a1613', borderBottom: '1px solid rgba(120,220,200,0.1)'}}>
        <div style={{display: 'flex', gap: 8}}>
          {['#e0655f', '#e0b95f', '#5be49b'].map((c) => (
            <span key={c} style={{width: 12, height: 12, borderRadius: 99, background: c}} />
          ))}
        </div>
        <div style={{flex: 1, textAlign: 'center', fontFamily: GHOST.mono, fontSize: 16, color: GHOST.faint, background: '#06120f', borderRadius: 8, padding: '5px 0', margin: '0 60px'}}>
          {url}
        </div>
      </div>
      <Img src={src} style={{display: 'block', width: W, height: H}} />
    </div>
  );
};
