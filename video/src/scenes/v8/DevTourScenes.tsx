import React from 'react';
import {AbsoluteFill, useCurrentFrame, useVideoConfig} from 'remotion';
import {Caption, pop, seg} from '../v2/shared';
import {DagBg, GHOST} from '../v7/DagBg';
import {ScrollFootage} from './ScrollFootage';

export const TITLE_DUR = 160;
export const DASH_DUR = 300;
export const DECODE_DUR = 300;
export const BUILD_DUR = 300;
export const API_DUR = 540;

const Chip: React.FC<{at: number; children: React.ReactNode}> = ({at, children}) => {
  const f = useCurrentFrame();
  const {fps} = useVideoConfig();
  const inn = pop(f, fps, at, 13);
  return (
    <div
      style={{
        position: 'absolute',
        left: 96,
        bottom: 70,
        fontFamily: GHOST.mono,
        fontSize: 30,
        color: GHOST.text,
        background: 'rgba(5,16,14,0.78)',
        border: `1px solid rgba(73,234,203,0.34)`,
        borderRadius: 12,
        padding: '14px 24px',
        opacity: seg(f, at, at + 12),
        transform: `translateY(${(1 - inn) * 16}px)`,
        backdropFilter: 'blur(8px)',
      }}
    >
      <span style={{color: GHOST.accent}}>▸ </span>
      {children}
    </div>
  );
};

export const TitleCard: React.FC = () => {
  const f = useCurrentFrame();
  const {fps} = useVideoConfig();
  return (
    <AbsoluteFill style={{alignItems: 'center', justifyContent: 'center'}}>
      <DagBg dim={seg(f, 0, 40) * 0.8} />
      <Caption frame={f} fps={fps} at={8} size={44} weight={600} y={-70}>
        <span style={{color: GHOST.muted}}>kascov · for developers</span>
      </Caption>
      <div
        style={{
          fontFamily: GHOST.display,
          fontSize: 96,
          fontWeight: 800,
          letterSpacing: -4,
          marginTop: 8,
          opacity: seg(f, 40, 56),
          background: `linear-gradient(96deg, ${GHOST.accent}, ${GHOST.born})`,
          WebkitBackgroundClip: 'text',
          backgroundClip: 'text',
          color: 'transparent',
        }}
      >
        a real, open API.
      </div>
      <div style={{marginTop: 26, fontSize: 28, color: GHOST.faint, opacity: seg(f, 84, 100)}}>
        the live dashboard + every endpoint, documented
      </div>
    </AbsoluteFill>
  );
};

export const DashScene: React.FC = () => {
  const f = useCurrentFrame();
  return (
    <AbsoluteFill style={{alignItems: 'center', justifyContent: 'center'}}>
      <DagBg dim={0.22} />
      <div style={{opacity: seg(f, 0, 14)}}>
        <ScrollFootage dir="dash" count={150} dur={DASH_DUR} url="kascov-explorer.web.app/#/explore" />
      </div>
      <Chip at={20}>explore — the live pulse, records, what&rsquo;s running</Chip>
    </AbsoluteFill>
  );
};

export const DecodeScene: React.FC = () => {
  const f = useCurrentFrame();
  return (
    <AbsoluteFill style={{alignItems: 'center', justifyContent: 'center'}}>
      <DagBg dim={0.22} />
      <div style={{opacity: seg(f, 0, 14)}}>
        <ScrollFootage dir="decode" count={150} dur={DECODE_DUR} url="kascov-explorer.web.app/#/decode" />
      </div>
      <Chip at={20}>decode any contract — then remake it with your own parameters</Chip>
    </AbsoluteFill>
  );
};

export const BuildScene: React.FC = () => {
  const f = useCurrentFrame();
  return (
    <AbsoluteFill style={{alignItems: 'center', justifyContent: 'center'}}>
      <DagBg dim={0.22} />
      <div style={{opacity: seg(f, 0, 14)}}>
        <ScrollFootage dir="build" count={150} dur={BUILD_DUR} url="kascov-explorer.web.app/#/build" />
      </div>
      <Chip at={20}>build — deploy your own smart coin in one command</Chip>
    </AbsoluteFill>
  );
};

export const ApiScene: React.FC = () => {
  const f = useCurrentFrame();
  return (
    <AbsoluteFill style={{alignItems: 'center', justifyContent: 'center'}}>
      <DagBg dim={0.22} />
      <div style={{opacity: seg(f, 0, 14)}}>
        <ScrollFootage dir="api" count={190} dur={API_DUR} url="kascov-explorer.web.app/#/dev" />
      </div>
      {f < 150 && <Chip at={18}>a proper reference — sticky sidebar, scroll-spy</Chip>}
      {f >= 150 && f < 330 && <Chip at={155}>every endpoint: request + response, side by side</Chip>}
      {f >= 330 && <Chip at={335}>typed field tables · no keys · CORS open</Chip>}
    </AbsoluteFill>
  );
};
