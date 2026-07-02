import {Easing, interpolate} from 'remotion';

/* 0→1 over [from, to], clamped, eased (default: cubic out). */
export const fade = (
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

/* Arbitrary clamped interpolation with cubic-out easing. */
export const glide = (
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
