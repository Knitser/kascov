/* kascov · contract generator ("make this yours")
   The three canonical SilverScript sources (verbatim from
   kaspanet/silverscript tests/examples) plus the pure helpers the /decode
   panel uses to turn edited constructor args into source text, compiled
   hex (via kascovDisasm.emitFromSkeleton), and a deploy command. */
(() => {
  'use strict';

  const SOURCES = {
    'SilverScript · Mecenas': `pragma silverscript ^0.1.0;

contract Mecenas(pubkey recipient, byte[32] funder, int pledge, int period) {
    entrypoint function receive() {
        require(this.age >= period);

        // Check that the first output sends to the recipient
        byte[34] recipientScriptPubKey = new ScriptPubKeyP2PK(recipient);
        require(tx.outputs[0].scriptPubKey == byte[](recipientScriptPubKey));

        // Calculate the value that's left
        int minerFee = 1000;
        int currentValue = tx.inputs[this.activeInputIndex].value;
        int changeValue = currentValue - pledge - minerFee;

        // If there is not enough left for *another* pledge after this one,
        // we send the remainder to the recipient. Otherwise we send the
        // pledge to the recipient and the change back to the contract
        if (changeValue <= pledge + minerFee) {
            require(tx.outputs[0].value == currentValue - minerFee);
        } else {
            require(tx.outputs[0].value == pledge);
            byte[] changeScriptPubKey = tx.inputs[this.activeInputIndex].scriptPubKey;
            require(tx.outputs[1].scriptPubKey == changeScriptPubKey);
            require(tx.outputs[1].value == changeValue);
        }
    }

    entrypoint function reclaim(pubkey pk, sig s) {
        require(blake2b(pk) == funder);
        require(checkSig(s, pk));
    }
}`,
    'SilverScript · Escrow': `pragma silverscript ^0.1.0;

contract Escrow(byte[32] arbiter, pubkey buyer, pubkey seller) {
    entrypoint function spend(pubkey pk, sig s) {
        require(blake2b(pk) == arbiter);
        require(checkSig(s, pk));

        // Check that the correct amount is sent
        int minerFee = 1000; // hardcoded fee
        int amount = tx.inputs[this.activeInputIndex].value - minerFee;
        require(tx.outputs[0].value == amount);

        // Check that the transaction sends to either the buyer or the seller
        byte[34] buyerLock = new ScriptPubKeyP2PK(buyer);
        byte[34] sellerLock = new ScriptPubKeyP2PK(seller);
        bool sendsToBuyer = tx.outputs[0].scriptPubKey == byte[](buyerLock);
        bool sendsToSeller = tx.outputs[0].scriptPubKey == byte[](sellerLock);
        require(sendsToBuyer || sendsToSeller);
    }
}`,
    'SilverScript · LastWill': `pragma silverscript ^0.1.0;

contract LastWill(byte[32] inheritor, byte[32] cold, byte[32] hot) {
    entrypoint function inherit(pubkey pk, sig s) {
        require(this.age >= 180);
        require(blake2b(pk) == inheritor);
        require(checkSig(s, pk));
    }

    entrypoint function cold(pubkey pk, sig s) {
        require(blake2b(pk) == cold);
        require(checkSig(s, pk));
    }

    entrypoint function refresh(pubkey pk, sig s) {
        require(blake2b(pk) == hot);
        require(checkSig(s, pk));

        // Check that the correct amount is sent
        int minerFee = 1000; // hardcoded fee
        int amount = tx.inputs[this.activeInputIndex].value - minerFee;
        require(tx.outputs[0].value == amount);

        // Check that the funds are sent back to the contract
        byte[] selfLock = tx.inputs[this.activeInputIndex].scriptPubKey;
        require(tx.outputs[0].scriptPubKey == selfLock);
    }
}`,
  };

  const D = () => window.kascovDisasm;

  /* ---- field validation & conversion (by declared kind, never by length) */

  function validateField(kind, text) {
    const t = (text || '').trim();
    if (kind === 'pubkey' || kind === 'hash32') {
      const clean = t.replace(/^0x/i, '').replace(/\s+/g, '').toLowerCase();
      if (!/^[0-9a-f]{64}$/.test(clean)) {
        return { ok: false, err: 'needs exactly 32 bytes (64 hex characters)' };
      }
      return { ok: true, value: Array.from(D().parseHex(clean)), display: clean };
    }
    if (kind === 'amount') {
      if (!/^\d+(\.\d{1,8})?$/.test(t)) return { ok: false, err: 'amount in TKAS, up to 8 decimals' };
      const [whole, frac = ''] = t.split('.');
      const sompi = BigInt(whole) * 100000000n + BigInt((frac + '00000000').slice(0, 8));
      if (sompi < 1n) return { ok: false, err: 'must be at least 1 sompi' };
      if (sompi > 9223372036854775807n) return { ok: false, err: 'too large' };
      return { ok: true, value: D().snumEncode(sompi), sompi, display: t };
    }
    if (kind === 'daa') {
      if (!/^\d+$/.test(t)) return { ok: false, err: 'whole number of DAA ticks' };
      const n = BigInt(t);
      if (n < 1n) return { ok: false, err: 'must be ≥ 1' };
      return { ok: true, value: D().snumEncode(n), display: t };
    }
    return { ok: false, err: 'unknown field kind' };
  }

  /* decoded hex → what the input field should show for editing */
  function prefillFor(kind, hexValue) {
    if (kind === 'amount') {
      const sompi = D().snumDecode(Array.from(D().parseHex(hexValue) || []));
      return sompiToTkas(sompi);
    }
    if (kind === 'daa') {
      return String(D().snumDecode(Array.from(D().parseHex(hexValue) || [])));
    }
    return hexValue;
  }

  function sompiToTkas(sompi) {
    const s = typeof sompi === 'bigint' ? sompi : BigInt(sompi);
    const whole = s / 100000000n;
    const frac = (s % 100000000n).toString().padStart(8, '0').replace(/0+$/, '');
    return frac ? `${whole}.${frac}` : String(whole);
  }

  /* ---- output builders ---- */

  function displayLine(p, v) {
    if (p.kind === 'amount') return `${p.source} = ${v.sompi} sompi (${v.display} TKAS)`;
    if (p.kind === 'daa') return `${p.source} = ${v.display} DAA ticks (≈ ${Math.round(Number(v.display) / 600)} min)`;
    return `${p.source} = 0x${v.display}  (${p.kind === 'pubkey' ? 'x-only pubkey' : 'byte[32]'})`;
  }

  function buildSource(templateName, params, values, meta) {
    const src = SOURCES[templateName];
    if (!src) return null;
    const date = meta && meta.date ? meta.date : '';
    const header = [
      `// ${templateName} — instance generated on kascov /decode${date ? ' (' + date + ')' : ''}`,
      '// constructor args baked into the compiled hex below:',
      ...params.map((p) => `//   ${displayLine(p, values[p.name])}`),
      '',
    ].join('\n');
    return header + src;
  }

  function buildDeployCommand(programHex, valueSompi) {
    return [
      '# 1. make a key, fund the printed address at https://faucet-testnet.kaspanet.io',
      'cargo run -p kascov-lab -- keygen',
      '',
      '# 2. birth your contract (its state is a hidden p2sh commitment):',
      `cargo run -p kascov-lab -- deploy --program-hex ${programHex} --value ${valueSompi}`,
      '',
      '# 3. REVEAL it — spend it so kascov names it your contract, on-chain, forever:',
      'cargo run -p kascov-lab -- spend --program-hex <same hex> --entrypoint reclaim',
      '#    (reclaim needs funder = your keygen blake2b. or do it all in one command:)',
      'cargo run -p kascov-lab -- contract-demo',
    ].join('\n');
  }

  /* per-template reveal step — which entrypoint spends (and reveals) the coin */
  const REVEAL_CMDS = {
    'SilverScript · Mecenas': 'spend --program-hex "$PROGRAM_HEX" --entrypoint reclaim',
    'SilverScript · Escrow': 'settle-escrow --program-hex "$PROGRAM_HEX" --release-to buyer',
    'SilverScript · LastWill': 'spend --program-hex "$PROGRAM_HEX" --entrypoint cold',
  };

  /* the whole loop wrapped as a self-contained, runnable bash file — the
     guided builder offers this as a downloadable deploy.sh (opts: template,
     date). The compiled hex and coin value are baked in as shell variables. */
  function buildDeployScript(programHex, valueSompi, opts) {
    opts = opts || {};
    const short = (opts.template || '').replace('SilverScript · ', '') || 'contract';
    const reveal = REVEAL_CMDS[opts.template] || 'spend --program-hex "$PROGRAM_HEX" --entrypoint <entrypoint>';
    const tkas = sompiToTkas(valueSompi);
    return [
      '#!/usr/bin/env bash',
      `# kascov — deploy your ${short} covenant on Kaspa testnet-10`,
      opts.date ? `# generated on kascov, ${opts.date}` : '# generated on kascov',
      '#',
      '# Prereqs: the kascov repo (https://github.com/Knitser/kascov) and a',
      '# faucet-funded testnet key. Run this from the repo root.',
      'set -euo pipefail',
      '',
      `PROGRAM_HEX="${programHex}"`,
      `VALUE_SOMPI="${valueSompi}"   # ${tkas} TKAS`,
      '',
      'echo "==> 1/3  make a throwaway testnet key"',
      'echo "         fund the printed address at https://faucet-testnet.kaspanet.io"',
      'cargo run -p kascov-lab -- keygen',
      '',
      'read -r -p "Funded the address? Press Enter to deploy… " _',
      '',
      'echo "==> 2/3  birth the contract (a hidden p2sh commitment)"',
      'cargo run -p kascov-lab -- deploy --program-hex "$PROGRAM_HEX" --value "$VALUE_SOMPI"',
      '',
      'echo "==> 3/3  reveal it — spend under the contract’s own rules"',
      `cargo run -p kascov-lab -- ${reveal}`,
      '',
      'echo "Done. Watch it appear on https://kascov.io"',
      '',
    ].join('\n');
  }

  window.kascovGen = { SOURCES, validateField, prefillFor, sompiToTkas, buildSource, buildDeployCommand, buildDeployScript };
})();
