import React from 'react';
import {AbsoluteFill, Img, interpolate, staticFile, useCurrentFrame, useVideoConfig} from 'remotion';
import {Caption, pop, seg} from '../v2/shared';
import {DagBg, GHOST} from '../v7/DagBg';

export const TITLE_DUR = 150;
export const SIM_DUR = 320;
export const VERIFIED_DUR = 250;
export const DEBUG_DUR = 380;
export const LANES_DUR = 240;
export const ZK_DUR = 330;
export const ENDV9_DUR = 240;

const pad = (n: number) => String(n).padStart(4, '0');

/* ------- browser window that plays a captured frame sequence ------- */
const BrowserFootage: React.FC<{dir: string; count: number; dur: number; url: string; hold?: number; tail?: number}> = ({
  dir,
  count,
  dur,
  url,
  hold = 16,
  tail = 10,
}) => {
  const f = useCurrentFrame();
  const idx = Math.max(0, Math.min(count - 1, Math.round(interpolate(f, [hold, dur - tail], [0, count - 1], {extrapolateLeft: 'clamp', extrapolateRight: 'clamp'}))));
  const src = staticFile(`v9update/${dir}/f${pad(idx)}.png`);
  const W = 1520;
  const H = Math.round((W * 850) / 1440);
  return (
    <div
      style={{
        width: W,
        borderRadius: 16,
        overflow: 'hidden',
        border: `1px solid rgba(120,220,200,0.16)`,
        boxShadow: '0 44px 130px -32px rgba(0,0,0,0.85), 0 0 0 1px rgba(0,0,0,0.4)',
        background: '#05100e',
      }}
    >
      <div style={{height: 40, display: 'flex', alignItems: 'center', gap: 16, padding: '0 18px', background: '#0a1613', borderBottom: '1px solid rgba(120,220,200,0.1)'}}>
        <div style={{display: 'flex', gap: 8}}>
          {['#e0655f', '#e0b95f', '#5be49b'].map((c) => (
            <span key={c} style={{width: 12, height: 12, borderRadius: 99, background: c}} />
          ))}
        </div>
        <div style={{flex: 1, textAlign: 'center', fontFamily: GHOST.mono, fontSize: 16, color: GHOST.faint, background: '#06120f', borderRadius: 8, padding: '5px 0', margin: '0 60px'}}>
          {url}
        </div>
      </div>
      <Img src={src} style={{display: 'block', width: W, height: H}} />
    </div>
  );
};

/* ------- step badge + caption at the bottom ------- */
const StepChip: React.FC<{at: number; step: number; children: React.ReactNode}> = ({at, step, children}) => {
  const f = useCurrentFrame();
  const {fps} = useVideoConfig();
  const inn = pop(f, fps, at, 13);
  return (
    <div
      style={{
        position: 'absolute',
        left: 96,
        bottom: 66,
        display: 'flex',
        alignItems: 'center',
        gap: 18,
        opacity: seg(f, at, at + 12),
        transform: `translateY(${(1 - inn) * 18}px)`,
      }}
    >
      <div
        style={{
          fontFamily: GHOST.mono,
          fontSize: 22,
          fontWeight: 600,
          color: GHOST.bg,
          background: GHOST.accent,
          borderRadius: 10,
          padding: '8px 14px',
          letterSpacing: 1,
        }}
      >
        {String(step).padStart(2, '0')} / 05
      </div>
      <div
        style={{
          fontFamily: 'Inter, sans-serif',
          fontSize: 32,
          color: GHOST.text,
          background: 'rgba(5,16,14,0.8)',
          border: `1px solid rgba(73,234,203,0.3)`,
          borderRadius: 12,
          padding: '13px 24px',
          backdropFilter: 'blur(8px)',
        }}
      >
        {children}
      </div>
    </div>
  );
};

/* =============================== TITLE =============================== */
export const TitleCard: React.FC = () => {
  const f = useCurrentFrame();
  const {fps} = useVideoConfig();
  return (
    <AbsoluteFill style={{alignItems: 'center', justifyContent: 'center'}}>
      <DagBg dim={seg(f, 0, 40) * 0.8} />
      <Caption frame={f} fps={fps} at={8} size={40} weight={600} y={-96}>
        <span style={{color: GHOST.muted}}>kascov · the covenant explorer</span>
      </Caption>
      <div
        style={{
          fontFamily: GHOST.display,
          fontSize: 108,
          fontWeight: 800,
          letterSpacing: -5,
          marginTop: 4,
          opacity: seg(f, 36, 54),
          background: `linear-gradient(96deg, ${GHOST.accent}, ${GHOST.born})`,
          WebkitBackgroundClip: 'text',
          backgroundClip: 'text',
          color: 'transparent',
        }}
      >
        five new powers.
      </div>
      <div style={{marginTop: 30, fontSize: 30, color: GHOST.faint, opacity: seg(f, 80, 98), fontFamily: 'Inter, sans-serif'}}>
        things no other explorer — on any chain — can do
      </div>
    </AbsoluteFill>
  );
};

/* ========================= 1 · SIMULATION =========================== */
const typed = (s: string, f: number, start: number, cps = 1.1) => {
  const n = Math.max(0, Math.min(s.length, Math.floor((f - start) * cps)));
  return s.slice(0, n);
};

const TermLine: React.FC<{y: number; children: React.ReactNode; show: number}> = ({y, children, show}) => {
  const f = useCurrentFrame();
  return (
    <div style={{position: 'absolute', top: y, left: 40, right: 40, fontFamily: GHOST.mono, fontSize: 27, opacity: seg(f, show, show + 8)}}>{children}</div>
  );
};

export const SimScene: React.FC = () => {
  const f = useCurrentFrame();
  const cmd1 = 'kascov-lab spend --entrypoint reclaim --dry-run';
  const cmd2 = 'kascov-lab spend --entrypoint claim   --dry-run';
  return (
    <AbsoluteFill style={{alignItems: 'center', justifyContent: 'center'}}>
      <DagBg dim={0.24} />
      <div style={{width: 1180, opacity: seg(f, 0, 14)}}>
        <div
          style={{
            borderRadius: 16,
            overflow: 'hidden',
            border: '1px solid rgba(120,220,200,0.18)',
            boxShadow: '0 44px 130px -32px rgba(0,0,0,0.85)',
            background: '#06110f',
            height: 460,
            position: 'relative',
          }}
        >
          <div style={{height: 40, display: 'flex', alignItems: 'center', gap: 8, padding: '0 18px', background: '#0a1613', borderBottom: '1px solid rgba(120,220,200,0.1)'}}>
            {['#e0655f', '#e0b95f', '#5be49b'].map((c) => (
              <span key={c} style={{width: 12, height: 12, borderRadius: 99, background: c}} />
            ))}
            <span style={{marginLeft: 12, fontFamily: GHOST.mono, fontSize: 15, color: GHOST.faint}}>kascov-lab — simulate a spend</span>
          </div>
          <TermLine y={70} show={8}>
            <span style={{color: GHOST.accent}}>$ </span>
            <span style={{color: GHOST.text}}>{typed(cmd1, f, 14)}</span>
          </TermLine>
          <TermLine y={116} show={72}>
            <span style={{color: GHOST.faint}}>SIMULATE   Mecenas · reclaim   (not broadcast)</span>
          </TermLine>
          <TermLine y={158} show={96}>
            <span style={{color: GHOST.born, fontWeight: 600}}>✓ PASS</span>
            <span style={{color: GHOST.muted}}> — a node would accept this spend</span>
          </TermLine>
          <TermLine y={252} show={170}>
            <span style={{color: GHOST.accent}}>$ </span>
            <span style={{color: GHOST.text}}>{typed(cmd2, f, 176)}</span>
          </TermLine>
          <TermLine y={298} show={236}>
            <span style={{color: GHOST.faint}}>SIMULATE   Mecenas · claim   (not broadcast)</span>
          </TermLine>
          <TermLine y={340} show={260}>
            <span style={{color: GHOST.burn, fontWeight: 600}}>✗ FAIL</span>
            <span style={{color: GHOST.muted}}> — the contract rejects this spend</span>
          </TermLine>
        </div>
      </div>
      <StepChip at={20} step={1}>simulate any spend through Kaspa&rsquo;s real script engine — before you broadcast</StepChip>
    </AbsoluteFill>
  );
};

/* ===================== footage-backed feature scenes ================= */
const FootageScene: React.FC<{dir: string; count: number; dur: number; url: string; step: number; caption: React.ReactNode; hold?: number}> = ({
  dir,
  count,
  dur,
  url,
  step,
  caption,
  hold,
}) => {
  const f = useCurrentFrame();
  return (
    <AbsoluteFill style={{alignItems: 'center', justifyContent: 'center'}}>
      <DagBg dim={0.2} />
      <div style={{opacity: seg(f, 0, 14), transform: `scale(${interpolate(seg(f, 0, dur), [0, 1], [1.015, 1])})`}}>
        <BrowserFootage dir={dir} count={count} dur={dur} url={url} hold={hold} />
      </div>
      <StepChip at={18} step={step}>
        {caption}
      </StepChip>
    </AbsoluteFill>
  );
};

export const VerifiedScene: React.FC = () => (
  <FootageScene dir="verified" count={64} dur={VERIFIED_DUR} url="kascov-explorer.web.app/#/decode" step={2} caption={<>verified contracts — proven to recompile <b style={{color: GHOST.accent}}>byte-identical</b> to their source</>} />
);
export const DebuggerScene: React.FC = () => (
  <FootageScene dir="debugger" count={46} dur={DEBUG_DUR} url="kascov-explorer.web.app/#/decode" step={3} hold={10} caption={<>a visual debugger — <b style={{color: GHOST.accent}}>watch a covenant run</b>, opcode by opcode</>} />
);
export const LanesScene: React.FC = () => (
  <FootageScene dir="lanes" count={60} dur={LANES_DUR} url="kascov-explorer.web.app/#/explore" step={4} caption={<>based-app activity — Kaspa&rsquo;s apps, <b style={{color: GHOST.accent}}>by namespace</b></>} />
);
export const ZkScene: React.FC = () => (
  <FootageScene dir="zk" count={60} dur={ZK_DUR} url="kascov-explorer.web.app/#/decode" step={5} caption={<>on-chain <b style={{color: GHOST.move}}>zero-knowledge</b> verification, decoded — a real mainnet coin</>} />
);

/* ================================ END =============================== */
const FEATURES = [
  'simulate a spend — pass / fail, no broadcast',
  'verified contracts — byte-identical proof',
  'a visual covenant debugger',
  'based-app activity by namespace',
  'on-chain ZK verification, decoded',
];

export const EndCardV9: React.FC = () => {
  const f = useCurrentFrame();
  return (
    <AbsoluteFill style={{alignItems: 'center', justifyContent: 'center'}}>
      <DagBg dim={0.5} />
      <div
        style={{
          fontFamily: GHOST.display,
          fontSize: 84,
          fontWeight: 800,
          letterSpacing: -3,
          opacity: seg(f, 6, 24),
          background: `linear-gradient(96deg, ${GHOST.accent}, ${GHOST.born})`,
          WebkitBackgroundClip: 'text',
          backgroundClip: 'text',
          color: 'transparent',
        }}
      >
        five, all live.
      </div>
      <div style={{marginTop: 34, display: 'flex', flexDirection: 'column', gap: 14}}>
        {FEATURES.map((t, i) => {
          const at = 30 + i * 14;
          return (
            <div key={i} style={{display: 'flex', alignItems: 'center', gap: 16, fontSize: 30, color: GHOST.text, opacity: seg(f, at, at + 12), transform: `translateX(${(1 - seg(f, at, at + 14)) * -18}px)`, fontFamily: 'Inter, sans-serif'}}>
              <span style={{color: GHOST.accent, fontFamily: GHOST.mono, fontSize: 22}}>{String(i + 1).padStart(2, '0')}</span>
              {t}
            </div>
          );
        })}
      </div>
      <div style={{marginTop: 46, fontSize: 36, fontFamily: GHOST.mono, color: GHOST.accent, opacity: seg(f, 120, 140)}}>kascov-explorer.web.app</div>
    </AbsoluteFill>
  );
};
