import React from 'react';
import {AbsoluteFill, Img, interpolate, staticFile, useCurrentFrame, useVideoConfig} from 'remotion';
import {Caption, pop, seg} from '../v2/shared';
import {DagBg, GHOST} from '../v7/DagBg';

/* =====================================================================
   FeaturesV10 — "the covenant workbench". The update after V9: kascov is
   now a full playground — write a covenant and compile it in the browser,
   verify a real zero-knowledge proof, read any contract in plain English,
   and see multi-contract apps as a live graph. Animated hero scenes for
   the compiler + ZK verify; smooth Ken Burns stills for the rest.
   ===================================================================== */

export const TITLE_DUR = 155;
export const COMPILE_DUR = 360;
export const ZK_DUR = 380;
export const PLAY_DUR = 235;
export const EXPLAIN_DUR = 235;
export const GRAPH_DUR = 250;
export const ENDV10_DUR = 250;

const STEPS = 5;

/* ---- browser chrome wrapper ---- */
const Chrome: React.FC<{url: string; children: React.ReactNode; w?: number; h?: number}> = ({url, children, w = 1520, h = 780}) => (
  <div
    style={{
      width: w,
      borderRadius: 16,
      overflow: 'hidden',
      border: '1px solid rgba(120,220,200,0.16)',
      boxShadow: '0 44px 130px -32px rgba(0,0,0,0.85), 0 0 0 1px rgba(0,0,0,0.4)',
      background: '#05100e',
    }}
  >
    <div style={{height: 42, display: 'flex', alignItems: 'center', gap: 16, padding: '0 18px', background: '#0a1613', borderBottom: '1px solid rgba(120,220,200,0.1)'}}>
      <div style={{display: 'flex', gap: 8}}>
        {['#e0655f', '#e0b95f', '#5be49b'].map((c) => (
          <span key={c} style={{width: 12, height: 12, borderRadius: 99, background: c}} />
        ))}
      </div>
      <div style={{flex: 1, textAlign: 'center', fontFamily: GHOST.mono, fontSize: 16, color: GHOST.faint, background: '#06120f', borderRadius: 8, padding: '5px 0', margin: '0 60px'}}>{url}</div>
    </div>
    <div style={{width: w, height: h, position: 'relative', background: '#060f0d'}}>{children}</div>
  </div>
);

/* ---- smooth Ken Burns still ---- */
const StillShot: React.FC<{name: string; url: string; dur: number; origin?: string}> = ({name, url, dur, origin = 'center 18%'}) => {
  const f = useCurrentFrame();
  const scale = interpolate(f, [0, dur], [1.0, 1.055], {extrapolateLeft: 'clamp', extrapolateRight: 'clamp'});
  const W = 1520;
  const H = Math.round((W * 780) / 1440);
  return (
    <Chrome url={url} w={W} h={H}>
      <div style={{width: W, height: H, overflow: 'hidden'}}>
        <Img src={staticFile(`v10update/still/${name}.png`)} style={{display: 'block', width: W, height: H, objectFit: 'cover', objectPosition: origin, transform: `scale(${scale})`, transformOrigin: origin}} />
      </div>
    </Chrome>
  );
};

/* ---- bottom step chip + caption ---- */
const StepChip: React.FC<{at: number; step: number; children: React.ReactNode}> = ({at, step, children}) => {
  const f = useCurrentFrame();
  const {fps} = useVideoConfig();
  const inn = pop(f, fps, at, 13);
  return (
    <div style={{position: 'absolute', left: 96, bottom: 62, display: 'flex', alignItems: 'center', gap: 18, opacity: seg(f, at, at + 12), transform: `translateY(${(1 - inn) * 18}px)`}}>
      <div style={{fontFamily: GHOST.mono, fontSize: 22, fontWeight: 600, color: GHOST.bg, background: GHOST.accent, borderRadius: 10, padding: '8px 14px', letterSpacing: 1}}>
        {String(step).padStart(2, '0')} / {String(STEPS).padStart(2, '0')}
      </div>
      <div style={{fontFamily: 'Inter, sans-serif', fontSize: 31, color: GHOST.text, background: 'rgba(5,16,14,0.82)', border: '1px solid rgba(73,234,203,0.3)', borderRadius: 12, padding: '13px 24px', backdropFilter: 'blur(8px)', maxWidth: 1240}}>
        {children}
      </div>
    </div>
  );
};

const typed = (s: string, f: number, start: number, cps = 1.15) => s.slice(0, Math.max(0, Math.min(s.length, Math.floor((f - start) * cps))));

/* =============================== TITLE =============================== */
export const TitleCard: React.FC = () => {
  const f = useCurrentFrame();
  const {fps} = useVideoConfig();
  return (
    <AbsoluteFill style={{alignItems: 'center', justifyContent: 'center'}}>
      <DagBg dim={seg(f, 0, 40) * 0.8} />
      <Caption frame={f} fps={fps} at={8} size={40} weight={600} y={-104}>
        <span style={{color: GHOST.muted}}>kascov · the covenant explorer</span>
      </Caption>
      <div
        style={{
          fontFamily: GHOST.display,
          fontSize: 116,
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
        now a workbench.
      </div>
      <div style={{marginTop: 30, fontSize: 30, color: GHOST.faint, opacity: seg(f, 82, 100), fontFamily: 'Inter, sans-serif'}}>
        write a covenant, verify a proof, read any contract — in the browser
      </div>
    </AbsoluteFill>
  );
};

/* ===================== 1 · WRITE & COMPILE (animated) ================ */
const SRC = `contract Escrow(byte[32] arbiter,
                pubkey buyer, pubkey seller) {
  entrypoint function spend(pubkey pk, sig s) {
    require(blake2b(pk) == arbiter);
    require(checkSig(s, pk));
    int fee = 1000;
    int amount = tx.inputs[this
      .activeInputIndex].value - fee;
    require(tx.outputs[0].value == amount);
    ...
  }
}`;

export const CompileScene: React.FC = () => {
  const f = useCurrentFrame();
  const compileAt = 150;
  const resultAt = 176;
  const hex = '78aa2033333333…8769765279ac6900…207c7e01ac7e879b69757551';
  const pressed = f >= compileAt && f < compileAt + 12;
  return (
    <AbsoluteFill style={{alignItems: 'center', justifyContent: 'center'}}>
      <DagBg dim={0.22} />
      <div style={{opacity: seg(f, 0, 14)}}>
        <Chrome url="kascov-explorer.web.app/#/playground" w={1520} h={780}>
          {/* the write | read mode switch */}
          <div style={{position: 'absolute', top: 26, left: 40, display: 'flex', gap: 8, padding: 6, background: '#0a1613', border: '1px solid rgba(120,220,200,0.12)', borderRadius: 999}}>
            <span style={{padding: '8px 18px', borderRadius: 999, fontFamily: 'Inter, sans-serif', fontSize: 20, color: GHOST.faint}}>📄 read</span>
            <span style={{padding: '8px 18px', borderRadius: 999, fontFamily: 'Inter, sans-serif', fontSize: 20, fontWeight: 700, color: GHOST.bg, background: GHOST.accent}}>✎ write a covenant</span>
          </div>
          {/* source editor */}
          <div style={{position: 'absolute', top: 108, left: 40, width: 820}}>
            <div style={{fontFamily: GHOST.mono, fontSize: 18, color: GHOST.faint, marginBottom: 10}}>contract source</div>
            <pre style={{margin: 0, fontFamily: GHOST.mono, fontSize: 23, lineHeight: 1.5, color: GHOST.text, background: '#081512', border: '1px solid rgba(120,220,200,0.12)', borderRadius: 12, padding: 20, height: 470, whiteSpace: 'pre-wrap'}}>
              {typed(SRC, f, 16, 2.4)}
              <span style={{opacity: f % 30 < 15 ? 1 : 0, color: GHOST.accent}}>▋</span>
            </pre>
          </div>
          {/* right column: compile + result */}
          <div style={{position: 'absolute', top: 108, left: 900, width: 580}}>
            <div style={{fontFamily: GHOST.mono, fontSize: 18, color: GHOST.faint, marginBottom: 10}}>constructor args</div>
            <div style={{fontFamily: GHOST.mono, fontSize: 19, color: GHOST.muted, background: '#081512', border: '1px solid rgba(120,220,200,0.12)', borderRadius: 12, padding: 16, height: 120, overflow: 'hidden'}}>
              0x3333…3333<br />0x1111…1111<br />0x2222…2222
            </div>
            <div
              style={{
                marginTop: 22,
                display: 'inline-block',
                fontFamily: 'Inter, sans-serif',
                fontSize: 24,
                fontWeight: 700,
                color: GHOST.bg,
                background: GHOST.accent,
                borderRadius: 12,
                padding: '14px 26px',
                transform: `scale(${pressed ? 0.95 : 1})`,
                boxShadow: pressed ? 'none' : '0 0 30px -6px rgba(73,234,203,0.5)',
              }}
            >
              ⚙ compile
            </div>
            <div style={{marginTop: 26, opacity: seg(f, resultAt, resultAt + 16), transform: `translateY(${(1 - seg(f, resultAt, resultAt + 18)) * 12}px)`}}>
              <div style={{fontFamily: 'Inter, sans-serif', fontSize: 25, fontWeight: 700, color: GHOST.born}}>✓ compiled — 147 bytes of Kaspa script</div>
              <div style={{marginTop: 12, fontFamily: GHOST.mono, fontSize: 18, color: GHOST.faint, background: '#081512', border: '1px solid rgba(120,220,200,0.12)', borderRadius: 10, padding: 14, wordBreak: 'break-all', lineHeight: 1.5}}>
                {typed(hex, f, resultAt + 8, 1.6)}
              </div>
              <div style={{marginTop: 14, fontFamily: 'Inter, sans-serif', fontSize: 22, color: GHOST.accent, opacity: seg(f, resultAt + 40, resultAt + 54)}}>✓ recognized as SilverScript · Escrow</div>
            </div>
          </div>
        </Chrome>
      </div>
      <StepChip at={20} step={1}>
        write a covenant in <b style={{color: GHOST.accent}}>SilverScript</b> and compile it to real Kaspa script — right in the browser
      </StepChip>
    </AbsoluteFill>
  );
};

/* ===================== 2 · VERIFY A ZK PROOF (animated) ============== */
export const ZkVerifyScene: React.FC = () => {
  const f = useCurrentFrame();
  const clickAt = 150;
  const runAt = 158;
  const doneAt = 226;
  const pressed = f >= clickAt && f < clickAt + 12;
  const glow = seg(f, doneAt, doneAt + 20);
  return (
    <AbsoluteFill style={{alignItems: 'center', justifyContent: 'center'}}>
      <DagBg dim={0.22} />
      <div style={{opacity: seg(f, 0, 14)}}>
        <Chrome url="kascov-explorer.web.app/#/playground" w={1360} h={560}>
          <div style={{position: 'absolute', inset: 0, padding: '46px 56px'}}>
            <div style={{display: 'flex', alignItems: 'center', gap: 16}}>
              <span style={{fontFamily: GHOST.mono, fontSize: 21, fontWeight: 700, color: GHOST.move, background: 'rgba(138,180,255,0.12)', border: '1px solid rgba(138,180,255,0.4)', borderRadius: 8, padding: '7px 14px'}}>⬡ ZK proof</span>
              <span style={{fontFamily: 'Inter, sans-serif', fontSize: 30, fontWeight: 700, color: GHOST.text}}>on-chain zero-knowledge verification</span>
              <span style={{fontFamily: GHOST.mono, fontSize: 22, color: GHOST.faint}}>Groth16</span>
            </div>
            <p style={{marginTop: 20, fontFamily: 'Inter, sans-serif', fontSize: 25, lineHeight: 1.5, color: GHOST.muted, maxWidth: 1050}}>
              A self-contained <span style={{fontFamily: GHOST.mono, color: GHOST.text}}>Groth16</span> proof — public inputs + proof + verifying key + <span style={{fontFamily: GHOST.mono, color: GHOST.text}}>OpZkPrecompile</span>. kascov runs the <i>exact</i> verifier Kaspa&rsquo;s L1 uses.
            </p>
            <div style={{marginTop: 30, display: 'flex', alignItems: 'center', gap: 22}}>
              <div
                style={{
                  fontFamily: 'Inter, sans-serif',
                  fontSize: 25,
                  fontWeight: 700,
                  color: GHOST.text,
                  border: '1px solid rgba(138,180,255,0.5)',
                  borderRadius: 12,
                  padding: '14px 26px',
                  transform: `scale(${pressed ? 0.95 : 1})`,
                  background: 'rgba(138,180,255,0.08)',
                }}
              >
                ◆ verify the proof
              </div>
              {/* running… */}
              <span style={{fontFamily: GHOST.mono, fontSize: 23, color: GHOST.faint, opacity: seg(f, runAt, runAt + 8) * (1 - seg(f, doneAt - 6, doneAt))}}>
                running the verifier{'.'.repeat(1 + (Math.floor(f / 10) % 3))}
              </span>
            </div>
            {/* VERIFIED */}
            <div
              style={{
                marginTop: 34,
                opacity: seg(f, doneAt, doneAt + 14),
                transform: `translateY(${(1 - seg(f, doneAt, doneAt + 16)) * 14}px)`,
              }}
            >
              <div
                style={{
                  display: 'inline-flex',
                  alignItems: 'center',
                  gap: 14,
                  fontFamily: 'Inter, sans-serif',
                  fontSize: 30,
                  fontWeight: 800,
                  color: GHOST.born,
                  background: 'rgba(91,228,155,0.10)',
                  border: '1px solid rgba(91,228,155,0.45)',
                  borderRadius: 14,
                  padding: '18px 28px',
                  boxShadow: `0 0 ${glow * 60}px -10px rgba(91,228,155,${glow * 0.7})`,
                }}
              >
                <span style={{fontSize: 34}}>✓</span> the zero-knowledge proof VERIFIED
              </div>
              <div style={{marginTop: 16, fontFamily: 'Inter, sans-serif', fontSize: 23, color: GHOST.muted, opacity: seg(f, doneAt + 20, doneAt + 34)}}>
                the same on-chain check Kaspa&rsquo;s L1 performs — a first for any explorer
              </div>
            </div>
          </div>
        </Chrome>
      </div>
      <StepChip at={20} step={2}>
        verify a real <b style={{color: GHOST.move}}>zero-knowledge proof</b> — the exact <span style={{fontFamily: GHOST.mono}}>ark_groth16</span> check Kaspa runs on-chain
      </StepChip>
    </AbsoluteFill>
  );
};

/* ===================== 3–5 · STILL SCENES =========================== */
const StillScene: React.FC<{name: string; dur: number; url: string; step: number; origin?: string; caption: React.ReactNode}> = ({name, dur, url, step, origin, caption}) => {
  const f = useCurrentFrame();
  return (
    <AbsoluteFill style={{alignItems: 'center', justifyContent: 'center'}}>
      <DagBg dim={0.2} />
      <div style={{opacity: seg(f, 0, 14)}}>
        <StillShot name={name} url={url} dur={dur} origin={origin} />
      </div>
      <StepChip at={16} step={step}>{caption}</StepChip>
    </AbsoluteFill>
  );
};

export const PlaygroundScene: React.FC = () => (
  <StillScene name="playground" dur={PLAY_DUR} url="kascov-explorer.web.app/#/playground" step={3} origin="center 12%" caption={<>one <b style={{color: GHOST.accent}}>playground</b> — read a script or write one, decode · simulate · debug · verify</>} />
);
export const ExplainScene: React.FC = () => (
  <StillScene name="explain" dur={EXPLAIN_DUR} url="kascov-explorer.web.app/#/playground" step={4} origin="center 8%" caption={<>read any covenant in <b style={{color: GHOST.accent}}>plain English</b> — then its opcodes, front and centre</>} />
);
export const GraphScene: React.FC = () => (
  <StillScene name="appgraph" dur={GRAPH_DUR} url="kascov-explorer.web.app/#/explore" step={5} origin="center 40%" caption={<>see multi-contract <b style={{color: GHOST.accent}}>apps</b> — coins that move together — as a live graph</>} />
);

/* ================================ END =============================== */
const FEATURES = [
  'write & compile SilverScript — in the browser',
  'verify a real Groth16 zero-knowledge proof',
  'one playground: decode · simulate · debug · verify',
  'read any covenant in plain English',
  'multi-contract apps as a live graph',
];

export const EndCardV10: React.FC = () => {
  const f = useCurrentFrame();
  return (
    <AbsoluteFill style={{alignItems: 'center', justifyContent: 'center'}}>
      <DagBg dim={0.5} />
      <div style={{fontFamily: GHOST.mono, fontSize: 22, letterSpacing: 4, textTransform: 'uppercase', color: GHOST.accent, opacity: seg(f, 6, 20)}}>what&rsquo;s new in kascov</div>
      <div style={{marginTop: 30, display: 'flex', flexDirection: 'column', gap: 16, width: 900}}>
        {FEATURES.map((t, i) => {
          const at = 22 + i * 12;
          return (
            <div key={i} style={{display: 'flex', alignItems: 'center', gap: 18, fontSize: 29, color: GHOST.text, opacity: seg(f, at, at + 12), transform: `translateY(${(1 - seg(f, at, at + 14)) * 10}px)`, fontFamily: 'Inter, sans-serif'}}>
              <span style={{width: 8, height: 8, borderRadius: 99, background: GHOST.accent, flexShrink: 0}} />
              {t}
            </div>
          );
        })}
      </div>
      <div style={{marginTop: 46, fontSize: 30, fontFamily: GHOST.mono, color: GHOST.faint, opacity: seg(f, 96, 116)}}>
        <span style={{color: GHOST.accent}}>▸ </span>kascov-explorer.web.app
      </div>
    </AbsoluteFill>
  );
};
