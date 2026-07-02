import React from 'react';
import {Composition} from 'remotion';
import {LaunchVideo} from './Video';

export const Root: React.FC = () => {
  return (
    <Composition
      id="Launch"
      component={LaunchVideo}
      durationInFrames={1200}
      fps={30}
      width={1920}
      height={1080}
    />
  );
};
