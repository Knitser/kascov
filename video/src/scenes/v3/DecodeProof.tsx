import React from 'react';
import {AbsoluteFill, Easing, useCurrentFrame, useVideoConfig} from 'remotion';
import {Avatar} from '../../lib/Avatar';
import {decode, GROUP_COLOR, GROUP_LABEL, GROUP_SOFT} from '../../lib/decode';
import {T} from '../../theme';
import {Caption, map, pop, seg, V2} from '../v2/shared';

/* =====================================================================
   V3 scene 5 (13s): the proof. A real MAINNET covenant that on-chain is
   just `OpBlake2b <hash> OpEqual`. At spend time kascov captured the
   preimage, verified it against the hash, and decoded the 132-op program
   it actually ran — including a zero-knowledge proof verification.
   Every instruction shown here is the real script.
   ===================================================================== */

export const DECODE_DUR = 780;

const INSTS = decode.instructions;
const ZK_INDEX = INSTS.findIndex((i) => i.group === 'zk');

/* pre-render text of one instruction */
const instText = (i: (typeof INSTS)[number]) =>
  i.data ? `${i.name} 0x${i.data.length > 20 ? `${i.data.slice(0, 20)}…` : i.data}` : i.name;

const SCROLL_AT = 250;
const SCROLL_DUR = 210;
const ZK_AT = 470;
const ID_AT = 610;

const ROW_H = 46;
const VIEW_TOP = 240;
const VIEW_H = 700;

export const DecodeProof: React.FC = () => {
  const f = useCurrentFrame();
  const {fps} = useVideoConfig();

  /* beat 1: the on-chain shape (a hash commitment) */
  const commitIn = pop(f, fps, 30, 14);
  const commitO = seg(f, 28, 40) * (1 - seg(f, 196, 226, Easing.inOut(Easing.quad)));

  /* beat 2: program scroll — the column of real instructions sweeps through */
  const listIn = seg(f, 216, 244);
  /* scroll progress eased so it decelerates right onto the ZK line */
  const zkRow = ZK_INDEX * ROW_H;
  const endScroll = Math.max(0, zkRow - VIEW_H * 0.52);
  const scroll = map(f, [SCROLL_AT, SCROLL_AT + SCROLL_DUR], [0, endScroll], Easing.inOut(Easing.cubic));

  /* beat 3: zk flash */
  const zkOn = f >= ZK_AT;
  const zkPop = pop(f, fps, ZK_AT, 10);
  const zkGlow = zkOn ? 0.5 + 0.5 * Math.sin((f - ZK_AT) / 18) : 0;

  /* beat 4: identity card */
  const idIn = pop(f, fps, ID_AT, 14);
  const idO = seg(f, ID_AT, ID_AT + 12);

  /* legend counts tick up during the scroll */
  const legendP = seg(f, SCROLL_AT, SCROLL_AT + SCROLL_DUR, Easing.linear);
  const legend = (['push', 'introspection', 'covenant', 'zk'] as const).map((g) => ({
    g,
    n: Math.round((decode.groups[g] ?? 0) * legendP),
    total: decode.groups[g] ?? 0,
  }));

  return (
    <AbsoluteFill>
      <Caption frame={f} fps={fps} at={6} out={198} size={58} weight={750} y={86}>
        on-chain, a covenant is <span style={{color: T.burn}}>just a hash</span>
      </Caption>

      {/* beat 1: the commitment card */}
      {commitO > 0 && (
        <AbsoluteFill style={{alignItems: 'center', justifyContent: 'center', opacity: commitO}}>
          <div
            style={{
              padding: '38px 52px',
              borderRadius: 18,
              background: T.card,
              border: `1.5px solid ${T.border}`,
              fontFamily: T.mono,
              fontSize: 34,
              transform: `scale(${0.92 + commitIn * 0.08})`,
            }}
          >
            <span style={{color: T.text}}>OpBlake2b </span>
            <span style={{color: T.accent}}>0x{decode.commit_hex.slice(4, 24)}…{decode.commit_hex.slice(-6, -2)} </span>
            <span style={{color: T.text}}>OpEqual</span>
          </div>
          <div style={{marginTop: 30, fontSize: 32, color: T.muted, opacity: seg(f, 78, 96)}}>
            35 bytes. that&apos;s everything the network shows you.
          </div>
          <div style={{marginTop: 14, fontSize: 32, color: T.text, fontWeight: 650, opacity: seg(f, 140, 158)}}>
            until it spends — and kascov catches <span style={{color: T.accent}}>the program it ran.</span>
          </div>
        </AbsoluteFill>
      )}

      {/* beat 2+3: the real instruction column */}
      {listIn > 0 && (
        <>
          <Caption frame={f} fps={fps} at={220} size={50} weight={740} y={78}>
            the same coin, <span style={{color: T.accent}}>revealed &amp; verified</span> — all 132 instructions, real
          </Caption>

          <div
            style={{
              position: 'absolute',
              left: 330,
              top: VIEW_TOP,
              width: 900,
              height: VIEW_H,
              opacity: listIn,
              overflow: 'hidden',
              maskImage: 'linear-gradient(to bottom, transparent 0%, black 7%, black 93%, transparent 100%)',
              WebkitMaskImage: 'linear-gradient(to bottom, transparent 0%, black 7%, black 93%, transparent 100%)',
            }}
          >
            <div style={{transform: `translateY(${-scroll}px)`}}>
              {INSTS.map((inst, i) => {
                const isZk = i === ZK_INDEX;
                const zkScale = isZk && zkOn ? 1 + zkPop * 0.06 : 1;
                return (
                  <div
                    key={i}
                    style={{
                      height: ROW_H,
                      display: 'flex',
                      alignItems: 'center',
                      gap: 26,
                      fontFamily: T.mono,
                      fontSize: 27,
                      paddingLeft: 22,
                      borderRadius: 10,
                      background: isZk && zkOn ? GROUP_SOFT.zk : 'transparent',
                      boxShadow: isZk && zkOn ? `0 0 ${26 + zkGlow * 22}px rgba(195,152,255,${0.35 + zkGlow * 0.2})` : undefined,
                      transform: `scale(${zkScale})`,
                      transformOrigin: 'left center',
                    }}
                  >
                    <span style={{color: T.faint, fontSize: 21, width: 66, flex: 'none'}}>
                      {inst.off.toString(16).padStart(4, '0')}
                    </span>
                    <span
                      style={{
                        color: GROUP_COLOR[inst.group],
                        fontWeight: inst.group === 'standard' ? 450 : 650,
                        whiteSpace: 'nowrap',
                      }}
                    >
                      {instText(inst)}
                    </span>
                  </div>
                );
              })}
            </div>
          </div>

          {/* legend: live-counting group chips */}
          <div style={{position: 'absolute', right: 150, top: 320, width: 420, opacity: seg(f, 252, 274)}}>
            {legend.map(({g, n, total}) => (
              <div
                key={g}
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  gap: 16,
                  marginBottom: 22,
                  padding: '16px 22px',
                  borderRadius: 14,
                  background: GROUP_SOFT[g],
                  border: `1px solid ${GROUP_COLOR[g]}44`,
                }}
              >
                <span style={{fontFamily: T.mono, fontSize: 40, fontWeight: 800, color: GROUP_COLOR[g], width: 74}}>
                  {g === 'zk' ? (zkOn ? 1 : 0) : n}
                </span>
                <span style={{fontSize: 25, color: T.text, fontWeight: 550}}>
                  {GROUP_LABEL[g]}
                  {g !== 'zk' && total > 1 ? ' ops' : ' op'}
                </span>
              </div>
            ))}
            {/* zk callout */}
            {zkOn && (
              <div
                style={{
                  marginTop: 8,
                  fontSize: 30,
                  lineHeight: 1.45,
                  color: T.text,
                  opacity: seg(f, ZK_AT + 12, ZK_AT + 30),
                }}
              >
                that purple line is a{' '}
                <span style={{color: '#c398ff', fontWeight: 750}}>zero-knowledge proof</span> being verified on Kaspa L1.
              </div>
            )}
          </div>
        </>
      )}

      {/* beat 4: identity strip */}
      {idO > 0 && (
        <div
          style={{
            position: 'absolute',
            left: 0,
            right: 0,
            bottom: 64,
            display: 'flex',
            justifyContent: 'center',
            opacity: idO,
            transform: `translateY(${(1 - idIn) * 26}px)`,
          }}
        >
          <div
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 22,
              padding: '18px 34px',
              borderRadius: 999,
              background: T.card,
              border: `1.5px solid ${T.borderStrong}`,
              boxShadow: `0 0 44px ${V2.glowAccent.replace('0.55', '0.18')}`,
            }}
          >
            <Avatar id={decode.covenant_id} size={56} />
            <span style={{fontFamily: T.mono, fontSize: 30, fontWeight: 700}}>{decode.name}</span>
            <span style={{fontSize: 25, color: T.born, fontWeight: 700, letterSpacing: 1}}>LIVE ON MAINNET</span>
            <span style={{fontSize: 24, color: T.faint}}>· hash-verified · decoded in your browser</span>
          </div>
        </div>
      )}
    </AbsoluteFill>
  );
};
