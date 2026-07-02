import React from 'react';
import {Composition} from 'remotion';
import {LaunchVideo} from './Video';
import {LaunchVideoV2, V2_TOTAL} from './VideoV2';

export const Root: React.FC = () => {
  return (
    <>
      <Composition
        id="Launch"
        component={LaunchVideo}
        durationInFrames={1200}
        fps={30}
        width={1920}
        height={1080}
      />
      <Composition
        id="LaunchV2"
        component={LaunchVideoV2}
        durationInFrames={V2_TOTAL}
        fps={60}
        width={1920}
        height={1080}
      />
    </>
  );
};
