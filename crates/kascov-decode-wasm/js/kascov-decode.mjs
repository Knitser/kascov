// Loads the kascov-decode WebAssembly module and gives JavaScript the four
// answers of the kascov-decode command. Each call takes hex text (a 0x prefix
// and whitespace are fine) and returns what the command prints: an object, or
// the opcode listing as a string for disasm. Input the decoder cannot read
// throws an Error that says why.
//
//   import { load } from './kascov-decode.mjs';
//   // in a browser, fetch it:
//   const kd = await load(new URL('./kascov_decode_wasm.wasm', import.meta.url));
//   // in Node, hand it the bytes:
//   const kd = await load(await readFile('kascov_decode_wasm.wasm'));
//
//   kd.name(programHex).template;
//   kd.check(signatureScriptHex, spentOutputScriptHex).spk_matches;
//
// The module imports nothing. load() takes a URL to fetch, a fetch Response,
// the module's bytes, or an already compiled WebAssembly.Module.

const encoder = new TextEncoder();
const decoder = new TextDecoder();

async function compile(source) {
  if (source instanceof WebAssembly.Module) return source;
  if (source instanceof ArrayBuffer || ArrayBuffer.isView(source)) return WebAssembly.compile(source);
  const response = typeof Response !== 'undefined' && source instanceof Response ? source : await fetch(source);
  if (!response.ok) throw new Error(`kascov-decode: could not fetch the module (HTTP ${response.status})`);
  return WebAssembly.compile(await response.arrayBuffer());
}

export async function load(source) {
  const instance = await WebAssembly.instantiate(await compile(source), {});
  const { memory, kd_alloc, kd_free, kd_name, kd_hash, kd_disasm, kd_check, kd_version } = instance.exports;

  // Copy text into a module buffer. An empty or missing input is (0, 0),
  // which the module reads as empty without touching memory.
  const put = (text) => {
    const bytes = encoder.encode(text ?? '');
    if (bytes.length === 0) return [0, 0];
    const ptr = kd_alloc(bytes.length);
    new Uint8Array(memory.buffer, ptr, bytes.length).set(bytes);
    return [ptr, bytes.length];
  };

  // Read an answer (4 bytes of little-endian length, then JSON) and free it.
  const take = (ptr) => {
    const len = new DataView(memory.buffer).getUint32(ptr, true);
    const text = decoder.decode(new Uint8Array(memory.buffer, ptr + 4, len));
    kd_free(ptr, 4 + len);
    const answer = JSON.parse(text);
    if ('error' in answer) throw new Error(`kascov-decode: ${answer.error}`);
    return answer.ok;
  };

  const call = (fn, ...texts) => {
    const args = texts.map(put);
    try {
      return take(fn(...args.flat()));
    } finally {
      for (const [ptr, len] of args) if (len) kd_free(ptr, len);
    }
  };

  return {
    version: take(kd_version()),
    /** What the program is: template, fields, compiler generation, argent reading. */
    name: (programHex) => call(kd_name, programHex),
    /** The program's BLAKE2b-256 and the P2SH script that commits to it. */
    hash: (programHex) => call(kd_hash, programHex),
    /** The opcode listing, one instruction per line after a summary line. */
    disasm: (programHex) => call(kd_disasm, programHex),
    /**
     * The program a spending input revealed, named, and checked against the
     * spent output's script public key when one is given (spk_matches).
     */
    check: (signatureScriptHex, spentOutputScriptHex) => call(kd_check, signatureScriptHex, spentOutputScriptHex),
  };
}
