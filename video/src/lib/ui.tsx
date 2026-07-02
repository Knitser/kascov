import React from 'react';
import {AbsoluteFill, useCurrentFrame} from 'remotion';
import {fade} from './anim';
import {T} from '../theme';

/* Wraps a scene: fades itself in/out at the sequence edges. */
export const FadeScene: React.FC<{
  duration: number;
  fadeInFrames?: number;
  fadeOutFrames?: number;
  children: React.ReactNode;
}> = ({duration, fadeInFrames = 10, fadeOutFrames = 10, children}) => {
  const f = useCurrentFrame();
  const opacity =
    fade(f, 0, fadeInFrames) *
    (fadeOutFrames > 0 ? 1 - fade(f, duration - fadeOutFrames, duration) : 1);
  return (
    <AbsoluteFill style={{opacity, fontFamily: T.sans, color: T.text}}>
      {children}
    </AbsoluteFill>
  );
};
