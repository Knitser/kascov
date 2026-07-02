import React from 'react';
import {AbsoluteFill, Sequence} from 'remotion';
import {FadeScene} from './lib/ui';
import {ColdOpen} from './scenes/ColdOpen';
import {EndCard} from './scenes/EndCard';
import {Idea} from './scenes/Idea';
import {LifeStory} from './scenes/LifeStory';
import {Testnet} from './scenes/Testnet';
import {Twist} from './scenes/Twist';
import {T} from './theme';

/* 1200 frames @ 30fps = 40s */
const SCENES: {name: string; dur: number; el: React.FC; fadeOut?: number}[] = [
  {name: 'cold-open', dur: 120, el: ColdOpen},
  {name: 'idea', dur: 180, el: Idea},
  {name: 'testnet', dur: 300, el: Testnet},
  {name: 'life-story', dur: 210, el: LifeStory},
  {name: 'twist', dur: 180, el: Twist},
  {name: 'end-card', dur: 210, el: EndCard, fadeOut: 0},
];

export const LaunchVideo: React.FC = () => {
  let at = 0;
  return (
    <AbsoluteFill style={{backgroundColor: T.bg}}>
      {/* barely-there teal glow, like the site background */}
      <AbsoluteFill
        style={{
          background:
            'radial-gradient(1100px 700px at 50% -8%, rgba(112, 199, 186, 0.055), transparent 70%)',
        }}
      />
      {SCENES.map((s) => {
        const from = at;
        at += s.dur;
        const El = s.el;
        return (
          <Sequence key={s.name} name={s.name} from={from} durationInFrames={s.dur}>
            <FadeScene duration={s.dur} fadeOutFrames={s.fadeOut ?? 10}>
              <El />
            </FadeScene>
          </Sequence>
        );
      })}
    </AbsoluteFill>
  );
};
