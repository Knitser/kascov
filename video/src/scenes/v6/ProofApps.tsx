import React from 'react';
import {AbsoluteFill, useCurrentFrame, useVideoConfig} from 'remotion';
import {Avatar} from '../../lib/Avatar';
import {T} from '../../theme';
import {Caption, pop, seg} from '../v2/shared';

/* V6 scene 4 (~6s): two more firsts — prove a hidden contract in your
   browser (no spend), and "apps": coins that move together. */

export const PROOFAPPS_DUR = 360;

const ZK_A = '09ef275e1671c76086764b6030ea5229dbd9af0ba818db6e0aae64eb8a3f63cb';
const ZK_B = '901be291efb290173572f8e4a8d0d55e0d4cfd8f0e0a973a2158f1efd0d2a318';

const Panel: React.FC<{at: number; children: React.ReactNode}> = ({at, children}) => {
  const f = useCurrentFrame();
  const {fps} = useVideoConfig();
  const inn = pop(f, fps, at, 13);
  return (
    <div
      style={{
        flex: 1,
        background: T.card,
        border: `1.5px solid ${T.border}`,
        borderRadius: 18,
        padding: '30px 36px',
        opacity: seg(f, at, at + 12),
        transform: `translateY(${(1 - inn) * 24}px)`,
      }}
    >
      {children}
    </div>
  );
};

export const ProofApps: React.FC = () => {
  const f = useCurrentFrame();
  const {fps} = useVideoConfig();

  return (
    <AbsoluteFill style={{alignItems: 'center', justifyContent: 'center'}}>
      <Caption frame={f} fps={fps} at={6} size={54} weight={760} y={64}>
        and two more <span style={{color: T.accent}}>firsts</span>
      </Caption>

      <div style={{display: 'flex', gap: 30, width: 1500, marginTop: 46}}>
        <Panel at={50}>
          <div style={{fontSize: 30, fontWeight: 740, marginBottom: 16}}>
            prove a hidden contract <span style={{color: T.accent}}>without spending</span>
          </div>
          <div style={{fontFamily: T.mono, fontSize: 22, color: T.muted, marginBottom: 18}}>
            …/c/da2fe117…<span style={{color: T.accent}}>?program=6b6c76…</span>
          </div>
          <div style={{fontSize: 25, opacity: seg(f, 120, 136)}}>
            <span style={{color: T.born, fontWeight: 700}}>✓ hash-verified</span>
            <span style={{color: T.muted}}> — your browser checks blake2b itself.</span>
          </div>
          <div style={{fontSize: 23, color: T.faint, marginTop: 12, opacity: seg(f, 150, 166)}}>
            nothing leaves your machine.
          </div>
        </Panel>

        <Panel at={90}>
          <div style={{fontSize: 30, fontWeight: 740, marginBottom: 16}}>
            <span style={{color: T.accent}}>apps</span> · coins that move together
          </div>
          <div style={{display: 'flex', alignItems: 'center', gap: 4, marginBottom: 16}}>
            <Avatar id={ZK_A} size={52} />
            <span style={{marginLeft: -14}}>
              <Avatar id={ZK_B} size={52} />
            </span>
            <span style={{fontSize: 24, color: T.muted, marginLeft: 16}}>
              one tx, several contracts = a covenant app
            </span>
          </div>
          <div style={{fontSize: 25, opacity: seg(f, 160, 176)}}>
            kascov found the <span style={{color: T.text, fontWeight: 650}}>first multi-contract app on mainnet</span>
            <span style={{color: T.muted}}> — the ZK pair, 8 shared transactions.</span>
          </div>
        </Panel>
      </div>

      <div style={{marginTop: 40, fontSize: 28, color: T.muted, opacity: seg(f, 250, 268)}}>
        decode → edit → deploy → <span style={{color: T.accent, fontWeight: 700}}>run</span>. the whole loop is live.
      </div>
    </AbsoluteFill>
  );
};
