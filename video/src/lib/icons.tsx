import React from 'react';

/* Event icons ported from the site (web/app.js ICONS): born (sprout),
   move (cycle arrows), burn (flame). Stroke inherits `color`. */

type IconProps = {size?: number; color?: string};

const base = (size: number, color: string | undefined, children: React.ReactNode) => (
  <svg
    viewBox="0 0 24 24"
    width={size}
    height={size}
    fill="none"
    stroke="currentColor"
    strokeWidth={2}
    strokeLinecap="round"
    strokeLinejoin="round"
    style={{display: 'block', color}}
  >
    {children}
  </svg>
);

export const BornIcon: React.FC<IconProps> = ({size = 20, color}) =>
  base(
    size,
    color,
    <>
      <path d="M12 21v-8" />
      <path d="M12 13C12 9.2 9.3 6.6 5 6.2c.4 4.4 3 7 7 6.8z" />
      <path d="M12 11c0-3 2-5.2 6-5.6-.3 3.4-2.4 5.4-6 5.6z" />
    </>
  );

export const MoveIcon: React.FC<IconProps> = ({size = 20, color}) =>
  base(
    size,
    color,
    <>
      <path d="M20.5 3.5v5h-5" />
      <path d="M3.7 10a8.5 8.5 0 0 1 14.2-4l2.6 2.5" />
      <path d="M3.5 20.5v-5h5" />
      <path d="M20.3 14a8.5 8.5 0 0 1-14.2 4l-2.6-2.5" />
    </>
  );

export const BurnIcon: React.FC<IconProps> = ({size = 20, color}) =>
  base(
    size,
    color,
    <>
      <path d="M12 2.5c.6 3 2.2 4.7 4 6.5 1.8 1.8 3 3.6 3 6a7 7 0 0 1-14 0c0-1.8.7-3.4 1.8-4.6.4 1.1 1 1.9 2 2.4C8.2 9.4 9.6 5.5 12 2.5z" />
      <path d="M12 21.5a3.2 3.2 0 0 1-3.2-3.2c0-1.5 1.2-2.6 2-3.6.5-.6.9-1.3 1.2-2 1.3 1.6 3.2 3.4 3.2 5.6a3.2 3.2 0 0 1-3.2 3.2z" />
    </>
  );

export const KindIcon: React.FC<{kind: string; size?: number; color?: string}> = ({
  kind,
  size,
  color,
}) => {
  if (kind === 'genesis') return <BornIcon size={size} color={color} />;
  if (kind === 'burn') return <BurnIcon size={size} color={color} />;
  return <MoveIcon size={size} color={color} />;
};

/* The site wordmark's "camera eye" mark. */
export const EyeMark: React.FC<{size?: number; color?: string}> = ({size = 22, color}) => (
  <svg viewBox="0 0 64 64" width={size} height={size} style={{display: 'block', color}}>
    <circle cx={32} cy={32} r={24} fill="none" stroke="currentColor" strokeWidth={7} />
    <circle cx={32} cy={32} r={9} fill="currentColor" />
  </svg>
);
