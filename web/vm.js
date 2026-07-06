/* kascov — a symbolic script stepper. Walks a compiled covenant program and
   traces the stack as the contract builds it: concrete for pushes and simple
   stack ops, symbolic for the parts that only resolve against a real spend
   (transaction introspection, signature/ZK checks). It's static analysis, not
   a live execution — but it's the first way to *watch* a Kaspa covenant's
   logic step by step. Pure client-side, no deps beyond the disassembler. */
(() => {
  'use strict';
  const D = () => window.kascovDisasm;
  const short = (h) => (h.length > 16 ? h.slice(0, 10) + '…' + h.slice(-4) : h);

  // human label for a push
  function pushLabel(inst) {
    if (inst.data && inst.data.length) return '0x' + short(D().toHex(inst.data));
    switch (inst.name) {
      case 'OpFalse': return '0';
      case 'OpTrue': return '1';
      case 'Op1Negate': return '-1';
      default: return inst.name.replace(/^Op/, ''); // Op1..Op16 -> "1".."16"
    }
  }

  // symbolic value a KIP-17 introspection opcode reads from the spending tx
  const INTRO = {
    OpTxInputAmount: '‹input.value›', OpTxOutputAmount: '‹output.value›',
    OpTxInputSpk: '‹input.spk›', OpTxOutputSpk: '‹output.spk›',
    OpTxInputCount: '‹tx.inputs.len›', OpTxOutputCount: '‹tx.outputs.len›',
    OpTxInputIndex: '‹this.inputIndex›', OpTxInputSeq: '‹input.seq›',
    OpTxInputDaaScore: '‹input.daa›', OpTxLockTime: '‹tx.lockTime›',
    OpTxVersion: '‹tx.version›', OpTxGas: '‹tx.gas›', OpTxPayloadLen: '‹tx.payload.len›',
    OpTxOutputSpkLen: '‹output.spk.len›', OpTxInputSpkLen: '‹input.spk.len›',
    OpOutpointTxId: '‹outpoint.txid›', OpOutpointIndex: '‹outpoint.idx›',
    OpInputCovenantId: '‹input.covenantId›', OpOutputCovenantId: '‹output.covenantId›',
    OpCovInputCount: '‹cov.inputs›', OpCovOutputCount: '‹cov.outputs›',
    OpCovInputIdx: '‹cov.inputIdx›', OpCovOutputIdx: '‹cov.outputIdx›',
    OpAuthOutputCount: '‹auth.outputs›', OpAuthOutputIdx: '‹auth.outputIdx›',
    OpOutputAuthorizingInput: '‹output.authInput›', OpTxSubnetId: '‹tx.subnet›',
  };

  function symbolicTrace(instructions) {
    const steps = [];
    let d = [];       // data stack (labels)
    let a = [];       // alt stack
    let depth = 0;    // if/else nesting, for indentation
    const pop = (n = 1) => { const o = []; for (let i = 0; i < n; i++) o.unshift(d.length ? d.pop() : '∅'); return o; };
    const bin = (sym) => { const [x, y] = pop(2); d.push(`(${x} ${sym} ${y})`); };

    for (const inst of instructions) {
      const n = inst.name;
      let note = '';
      const indent = depth;

      if (inst.group === 'push') {
        const l = pushLabel(inst);
        d.push(l);
        note = 'push ' + l;
      } else if (INTRO[n]) {
        d.push(INTRO[n]);
        note = 'read ' + INTRO[n] + ' from the spending tx';
      } else {
        switch (n) {
          case 'OpDup': d.push(d.length ? d[d.length - 1] : '∅'); note = 'duplicate top'; break;
          case 'Op2Dup': { const [x, y] = [d[d.length - 2], d[d.length - 1]]; d.push(x, y); note = 'duplicate top 2'; break; }
          case 'OpDrop': pop(1); note = 'drop top'; break;
          case 'Op2Drop': pop(2); note = 'drop top 2'; break;
          case 'OpNip': { const [x, y] = pop(2); d.push(y); note = 'drop 2nd'; break; }
          case 'OpOver': d.push(d[d.length - 2] || '∅'); note = 'copy 2nd to top'; break;
          case 'OpTuck': { const [x, y] = pop(2); d.push(y, x, y); note = 'tuck'; break; }
          case 'OpSwap': { const [x, y] = pop(2); d.push(y, x); note = 'swap top 2'; break; }
          case 'OpRot': { const [x, y, z] = pop(3); d.push(y, z, x); note = 'rotate top 3'; break; }
          case 'OpToAltStack': { const [x] = pop(1); a.push(x); note = '→ alt stack'; break; }
          case 'OpFromAltStack': { d.push(a.length ? a.pop() : '∅'); note = '← alt stack'; break; }
          case 'OpDepth': d.push(String(d.length)); note = 'push stack depth'; break;
          case 'OpPick': { const [k] = pop(1); d.push(`pick(${k})`); note = 'copy nth item'; break; }
          case 'OpRoll': { const [k] = pop(1); d.push(`roll(${k})`); note = 'move nth item up'; break; }
          case 'OpIfDup': note = 'dup if top ≠ 0'; break;
          case 'OpBlake2b': case 'OpBlake3': case 'OpSHA256': { const [x] = pop(1); d.push(`${n.replace('Op', '').toLowerCase()}(${x})`); note = 'hash top'; break; }
          case 'OpCat': { const [x, y] = pop(2); d.push(`${x}‖${y}`); note = 'concatenate'; break; }
          case 'OpSize': d.push(`len(${d[d.length - 1] || '∅'})`); note = 'push byte length'; break;
          case 'OpAdd': bin('+'); note = 'add'; break;
          case 'OpSub': bin('−'); note = 'subtract'; break;
          case 'OpMul': bin('×'); note = 'multiply'; break;
          case 'OpDiv': bin('÷'); note = 'divide'; break;
          case 'OpMod': bin('mod'); note = 'modulo'; break;
          case 'Op1Add': { const [x] = pop(1); d.push(`(${x}+1)`); note = 'add 1'; break; }
          case 'Op1Sub': { const [x] = pop(1); d.push(`(${x}−1)`); note = 'subtract 1'; break; }
          case 'OpNegate': { const [x] = pop(1); d.push(`(−${x})`); note = 'negate'; break; }
          case 'OpAbs': { const [x] = pop(1); d.push(`|${x}|`); note = 'absolute value'; break; }
          case 'OpNumEqual': bin('=='); note = 'numbers equal?'; break;
          case 'OpNumNotEqual': bin('!='); note = 'numbers not equal?'; break;
          case 'OpLessThan': bin('<'); note = 'less than?'; break;
          case 'OpGreaterThan': bin('>'); note = 'greater than?'; break;
          case 'OpLessThanOrEqual': bin('≤'); note = 'less or equal?'; break;
          case 'OpGreaterThanOrEqual': bin('≥'); note = 'greater or equal?'; break;
          case 'OpMin': bin('min'); note = 'minimum'; break;
          case 'OpMax': bin('max'); note = 'maximum'; break;
          case 'OpBoolAnd': bin('&&'); note = 'boolean and'; break;
          case 'OpBoolOr': bin('||'); note = 'boolean or'; break;
          case 'OpEqual': bin('=='); note = 'bytes equal?'; break;
          case 'OpNot': { const [x] = pop(1); d.push(`!${x}`); note = 'logical not'; break; }
          case 'Op0NotEqual': { const [x] = pop(1); d.push(`(${x}≠0)`); note = '≠ 0 ?'; break; }
          case 'OpNumEqualVerify': { pop(2); note = 'require numbers equal'; break; }
          case 'OpEqualVerify': { pop(2); note = 'require bytes equal'; break; }
          case 'OpWithin': { pop(3); d.push('‹within›'); note = 'in range?'; break; }
          case 'OpVerify': { pop(1); note = 'require top is true — else the spend fails'; break; }
          case 'OpReturn': note = 'fail immediately'; break;
          case 'OpCheckSig': { const [k, s] = pop(2); d.push(`checkSig(${s},${k})`); note = 'verify a signature'; break; }
          case 'OpCheckSigVerify': { pop(2); note = 'require a valid signature'; break; }
          case 'OpCheckMultiSig': case 'OpCheckMultiSigECDSA': { d.push('‹multisig›'); note = 'verify m-of-n signatures'; break; }
          case 'OpCheckSigFromStack': case 'OpCheckSigFromStackECDSA': { pop(3); d.push('‹checkSigFromStack›'); note = 'verify a signature over a message'; break; }
          case 'OpCheckLockTimeVerify': note = 'require tx.lockTime ≥ top'; break;
          case 'OpCheckSequenceVerify': note = 'require the coin is old enough (relative timelock)'; break;
          case 'OpZkPrecompile': { const [proof, inputs, vk, tag] = pop(4); d.push('‹zk verified›'); note = 'verify a zero-knowledge proof on-chain (KIP-16)'; break; }
          case 'OpIf': { const [c] = pop(1); depth += 1; note = `if ${c} …`; break; }
          case 'OpNotIf': { const [c] = pop(1); depth += 1; note = `if not ${c} …`; break; }
          case 'OpElse': note = '… else …'; break;
          case 'OpEndIf': depth = Math.max(0, depth - 1); note = 'end if'; break;
          case 'OpNop': note = 'no-op'; break;
          default:
            note = 'runtime opcode';
        }
      }
      steps.push({ offset: inst.offset, name: n, group: inst.group, indent, note, dstack: d.slice(), astack: a.slice() });
    }
    return steps;
  }

  window.kascovVm = { symbolicTrace };
})();
