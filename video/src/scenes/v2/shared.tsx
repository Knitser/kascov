import React from 'react';
import {
  AbsoluteFill,
  Easing,
  interpolate,
  random,
  spring,
} from 'remotion';
import {T} from '../../theme';

/* v2 design constants — same family as the site, slightly deeper black. */
export const V2 = {
  bg: '#0a0f0e',
  glowAccent: 'rgba(112, 199, 186, 0.55)',
} as const;

export const clamp01 = (v: number): number => Math.max(0, Math.min(1, v));

/* 0→1 over [from, to], clamped, cubic-out by default. */
export const seg = (
  frame: number,
  from: number,
  to: number,
  easing: (t: number) => number = Easing.out(Easing.cubic)
): number =>
  interpolate(frame, [from, to], [0, 1], {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
    easing,
  });

/* Arbitrary clamped map with easing. */
export const map = (
  frame: number,
  range: [number, number],
  out: [number, number],
  easing: (t: number) => number = Easing.out(Easing.cubic)
): number =>
  interpolate(frame, range, out, {
    extrapolateLeft: 'clamp',
    extrapolateRight: 'clamp',
    easing,
  });

/* A tasteful entrance spring (damping ~14) starting at `at`. */
export const pop = (frame: number, fps: number, at: number, damping = 14): number =>
  spring({frame: frame - at, fps, config: {damping, stiffness: 130, mass: 0.9}});

/* Scene wrapper with optional edge fades (crossfade-friendly). */
export const Shell: React.FC<{
  duration: number;
  fadeIn?: number;
  fadeOut?: number;
  children: React.ReactNode;
  frame: number;
}> = ({duration, fadeIn = 12, fadeOut = 12, children, frame}) => {
  const opacity =
    (fadeIn > 0 ? seg(frame, 0, fadeIn, Easing.inOut(Easing.quad)) : 1) *
    (fadeOut > 0
      ? 1 - seg(frame, duration - fadeOut, duration, Easing.inOut(Easing.quad))
      : 1);
  return (
    <AbsoluteFill style={{opacity, fontFamily: T.sans, color: T.text}}>
      {children}
    </AbsoluteFill>
  );
};

/* Big caption that springs in at `at` (and optionally leaves at `out`). */
export const Caption: React.FC<{
  frame: number;
  fps: number;
  at: number;
  out?: number;
  size?: number;
  color?: string;
  weight?: number;
  mono?: boolean;
  glow?: boolean;
  y?: number;
  children: React.ReactNode;
}> = ({frame, fps, at, out, size = 56, color = T.text, weight = 600, mono, glow, y = 0, children}) => {
  const s = pop(frame, fps, at);
  const inO = seg(frame, at, at + 8, Easing.linear);
  const outO = out != null ? 1 - seg(frame, out, out + 14, Easing.inOut(Easing.quad)) : 1;
  if (frame < at - 1) return null;
  return (
    <div
      style={{
        position: 'absolute',
        left: 0,
        right: 0,
        top: y,
        display: 'flex',
        justifyContent: 'center',
        opacity: inO * outO,
        transform: `translateY(${(1 - s) * 34}px) scale(${0.94 + s * 0.06})`,
      }}
    >
      <div
        style={{
          fontFamily: mono ? T.mono : T.sans,
          fontSize: size,
          fontWeight: weight,
          color,
          letterSpacing: mono ? 1 : -0.5,
          textAlign: 'center',
          textShadow: glow ? `0 0 34px ${V2.glowAccent}` : undefined,
          maxWidth: 1720,
        }}
      >
        {children}
      </div>
    </div>
  );
};

/* ---------------------------------------------------------- odometer */

const DIGIT_STRIP = [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 0];

/* Slot-machine odometer: each digit column spins its own number of full
   turns and lands EXACTLY on the target digit at progress=1. */
export const Odometer: React.FC<{
  target: number;
  progress: number; /* 0..1, pre-eased by the caller */
  digits: number;
  size: number;
  color?: string;
}> = ({target, progress, digits, size, color = T.text}) => {
  const cellH = Math.round(size * 1.16);
  const cols: React.ReactNode[] = [];
  for (let k = digits - 1; k >= 0; k--) {
    const targetDigit = Math.floor(target / 10 ** k) % 10;
    const spins = (digits - k) * 10; /* low digits spin faster */
    const dv = ((targetDigit + spins) * progress) % 10;
    cols.push(
      <div
        key={k}
        style={{
          height: cellH,
          width: size * 0.66,
          overflow: 'hidden',
          position: 'relative',
        }}
      >
        <div style={{transform: `translateY(${-dv * cellH}px)`}}>
          {DIGIT_STRIP.map((d, i) => (
            <div
              key={i}
              style={{
                height: cellH,
                display: 'flex',
                alignItems: 'center',
                justifyContent: 'center',
                fontFamily: T.mono,
                fontSize: size,
                fontWeight: 700,
                color,
                fontVariantNumeric: 'tabular-nums',
              }}
            >
              {d}
            </div>
          ))}
        </div>
      </div>
    );
  }
  return (
    <div
      style={{
        display: 'flex',
        maskImage:
          'linear-gradient(to bottom, transparent 0%, black 14%, black 86%, transparent 100%)',
        WebkitMaskImage:
          'linear-gradient(to bottom, transparent 0%, black 14%, black 86%, transparent 100%)',
      }}
    >
      {cols}
    </div>
  );
};

/* ------------------------------------------------------ hash ticker */

const HEX = '0123456789abcdef';

/* Shows a short tx hash that "settles" out of slot-machine hex noise.
   Fully deterministic: noise chars come from random(seed) keyed on
   frame + position. */
export const HashTicker: React.FC<{
  frame: number;
  at: number;
  txid: string;
  size?: number;
  color?: string;
}> = ({frame, at, txid, size = 22, color = T.faint}) => {
  const short = `${txid.slice(0, 8)}…${txid.slice(-6)}`;
  const chars = short.split('');
  const t = frame - at;
  if (t < 0) return null;
  const settled = Math.floor(map(t, [6, 30], [0, chars.length], Easing.linear));
  return (
    <span
      style={{
        fontFamily: T.mono,
        fontSize: size,
        color,
        letterSpacing: 1.5,
        fontVariantNumeric: 'tabular-nums',
      }}
    >
      {chars.map((c, i) => {
        if (i < settled || c === '…') return c;
        return HEX[Math.floor(random(`tick-${i}-${frame}`) * 16)];
      })}
    </span>
  );
};

/* ------------------------------------------------------- wordmark */

export const Wordmark: React.FC<{
  size: number;
  markScale?: number;
  suffix?: React.ReactNode;
  glow?: number; /* 0..1 glow strength */
}> = ({size, suffix, glow = 0}) => {
  return (
    <div style={{display: 'flex', alignItems: 'center', gap: size * 0.26}}>
      <svg
        viewBox="0 0 64 64"
        width={size * 0.72}
        height={size * 0.72}
        style={{
          display: 'block',
          color: T.accent,
          filter: glow > 0 ? `drop-shadow(0 0 ${18 * glow}px ${V2.glowAccent})` : undefined,
        }}
      >
        <circle cx={32} cy={32} r={24} fill="none" stroke="currentColor" strokeWidth={7} />
        <circle cx={32} cy={32} r={9} fill="currentColor" />
      </svg>
      <span
        style={{
          fontFamily: T.mono,
          fontSize: size,
          fontWeight: 600,
          color: T.text,
          letterSpacing: 2,
          textShadow: glow > 0 ? `0 0 ${40 * glow}px rgba(112,199,186,${0.35 * glow})` : undefined,
        }}
      >
        kascov
      </span>
      {suffix}
    </div>
  );
};
