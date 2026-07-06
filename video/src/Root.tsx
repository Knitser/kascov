import React from 'react';
import {Composition} from 'remotion';
import {LaunchVideo} from './Video';
import {LaunchVideoV2, V2_TOTAL} from './VideoV2';
import {UpdateVideoV4, V4_TOTAL} from './VideoV4';
import {GeneratorVideoV5, V5_TOTAL} from './VideoV5';
import {RunVideoV6, V6_TOTAL} from './VideoV6';
import {GhostDagVideoV7, V7_TOTAL} from './VideoV7';
import {LaunchVideoV3, V3_TOTAL} from './VideoV3';

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
      <Composition
        id="GeneratorV5"
        component={GeneratorVideoV5}
        durationInFrames={V5_TOTAL}
        fps={60}
        width={1920}
        height={1080}
      />
      <Composition
        id="RunV6"
        component={RunVideoV6}
        durationInFrames={V6_TOTAL}
        fps={60}
        width={1920}
        height={1080}
      />
      <Composition
        id="GhostDagV7"
        component={GhostDagVideoV7}
        durationInFrames={V7_TOTAL}
        fps={60}
        width={1920}
        height={1080}
      />
      <Composition
        id="UpdateV4"
        component={UpdateVideoV4}
        durationInFrames={V4_TOTAL}
        fps={60}
        width={1920}
        height={1080}
      />
      <Composition
        id="LaunchV3"
        component={LaunchVideoV3}
        durationInFrames={V3_TOTAL}
        fps={60}
        width={1920}
        height={1080}
      />
    </>
  );
};
