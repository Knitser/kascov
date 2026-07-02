import React from 'react';
import {avatarParams} from './identity';

/* React port of the site's avatarSvg() — identical geometry and colors,
   with an optional per-shape reveal (0..1) used by the "it gets a face"
   moment in scene 2. */

export const Avatar: React.FC<{
  id: string;
  size: number;
  revealShapes?: number[];
  style?: React.CSSProperties;
}> = ({id, size, revealShapes, style}) => {
  const p = avatarParams(id);
  return (
    <svg
      viewBox="0 0 64 64"
      width={size}
      height={size}
      style={{display: 'block', ...style}}
    >
      <circle cx={32} cy={32} r={30} fill={p.bg} stroke={p.ring} strokeWidth={2.5} />
      {p.shapes.map((sh, k) => {
        const reveal = revealShapes ? Math.max(0, Math.min(1.15, revealShapes[k] ?? 1)) : 1;
        if (reveal <= 0) return null;
        const opacity = 0.92 * Math.min(1, reveal);
        let el: React.ReactNode = null;
        const {kind, cx, cy, s, rot, col} = sh;
        if (kind === 0) {
          el = <circle cx={cx} cy={cy} r={+(s * 0.72).toFixed(1)} fill={col} />;
        } else if (kind === 1) {
          el = (
            <rect
              x={+(cx - s / 2).toFixed(1)}
              y={+(cy - s / 2).toFixed(1)}
              width={s}
              height={s}
              rx={2}
              fill={col}
              transform={`rotate(${rot} ${cx} ${cy})`}
            />
          );
        } else if (kind === 2) {
          const h = s * 0.9;
          const p1 = `${cx},${+(cy - h).toFixed(1)}`;
          const p2 = `${+(cx - h * 0.87).toFixed(1)},${+(cy + h * 0.5).toFixed(1)}`;
          const p3 = `${+(cx + h * 0.87).toFixed(1)},${+(cy + h * 0.5).toFixed(1)}`;
          el = (
            <polygon
              points={`${p1} ${p2} ${p3}`}
              fill={col}
              transform={`rotate(${rot} ${cx} ${cy})`}
            />
          );
        } else if (kind === 3) {
          el = (
            <circle
              cx={cx}
              cy={cy}
              r={+(s * 0.65).toFixed(1)}
              fill="none"
              stroke={col}
              strokeWidth={3.5}
            />
          );
        } else {
          el = (
            <rect
              x={+(cx - s / 2).toFixed(1)}
              y={+(cy - s / 2).toFixed(1)}
              width={s}
              height={s}
              rx={2}
              fill={col}
              transform={`rotate(45 ${cx} ${cy})`}
            />
          );
        }
        return (
          <g
            key={k}
            opacity={opacity}
            transform={`translate(${cx} ${cy}) scale(${reveal}) translate(${-cx} ${-cy})`}
          >
            {el}
          </g>
        );
      })}
    </svg>
  );
};
