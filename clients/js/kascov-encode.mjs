/* kascov-encode.mjs — convert between the two SilverScript constructor-arg
   encodings, in both directions. Zero dependencies; Node 18+ or the browser.

   The problem (kaspanet/silverscript#139, "cost the most time"): the same
   toolchain takes constructor args in two different shapes —

   TAGGED Expr JSON — what `silverc --constructor-args ctor.json` deserializes
   (serde `Vec<Expr>`, tag = "kind", content = "data"):

       [{"kind":"array","data":[{"kind":"byte","data":68}, …]},
        {"kind":"int","data":100000000}]

   BARE positional strings / plain JSON values — what everything else takes:
   the debugger CLI's `--ctor-arg`, the debugger test-file's
   `"constructor_args": [100, 0]`, kascov's `silverc - <arg> …` wrapper, and
   kascov.io's `POST /data/{network}/compile` `"args"` array:

       ["0x4444…44", "100000000"]        (argv strings)
       [100, true, "0xaa"]               (test-file JSON values)

   Both encodings were verified against the real parsers
   (silverscript-lang/src/ast/mod.rs Expr serde; debugger/session/src/args.rs)
   and the round trip was checked by compiling the same contract through both
   CLIs to byte-identical script hex.

   ── README addition (clients/) ────────────────────────────────────────────
   kascov-encode.mjs — arg-encoding converter for SilverScript tooling.

     import { taggedToBare, bareToTagged, isId64 } from './kascov-encode.mjs';

     // tagged silverc ctor JSON  →  bare args for kascov.io /compile
     taggedToBare([{ kind: 'int', data: 86400 }]);        // → ["86400"]

     // bare args  →  tagged JSON for `silverc --constructor-args`
     bareToTagged(['0x' + '44'.repeat(32), '86400']);
     // → [{kind:'array',data:[{kind:'byte',data:68},…]}, {kind:'int',data:86400}]

     isId64('b4ade48e…0410');                             // 64-hex ids: true

   CLI (auto-detects direction):
     node kascov-encode.mjs '[{"kind":"int","data":86400}]'     # tagged → bare
     node kascov-encode.mjs 0x4444…44 86400                     # bare → tagged
     node kascov-encode.mjs --types pubkey,int 0x4444…44 86400  # with hints
     node kascov-encode.mjs --id b4ade48e…                      # validate an id
   ──────────────────────────────────────────────────────────────────────────── */

const HEX_RE = /^[0-9a-fA-F]+$/;
const INT_RE = /^-?[0-9][0-9_]*$/;

/** True when `s` is a 64-hex-character id (covenant id, txid, 32-byte hash).
    A leading 0x is tolerated and does not count toward the 64. The debugger
    test-file's `covenant_id` needs exactly this shape — 64 hex chars for 32
    bytes, not a 32-char string (silverscript#139 point 4).
    @param {string} s
    @returns {boolean} */
export function isId64(s) {
  if (typeof s !== 'string') return false;
  const t = s.trim().replace(/^0x/i, '');
  return t.length === 64 && HEX_RE.test(t);
}

/** Normalize a 64-hex id to lowercase without 0x, or throw.
    @param {string} s
    @returns {string} */
export function normalizeId(s) {
  if (!isId64(s)) throw new Error(`not a 64-hex id: ${JSON.stringify(s)}`);
  return s.trim().replace(/^0x/i, '').toLowerCase();
}

/** Hex string (0x optional) → byte values. Odd-length hex is left-padded with
    one zero nibble, mirroring the debugger's parse_hex_bytes.
    @param {string} hex
    @returns {number[]} */
export function hexToBytes(hex) {
  let h = String(hex).trim().replace(/^0x/i, '');
  if (h.length === 0) return [];
  if (!HEX_RE.test(h)) throw new Error(`not valid hex: ${JSON.stringify(hex)}`);
  if (h.length % 2 !== 0) h = '0' + h;
  const out = new Array(h.length / 2);
  for (let i = 0; i < out.length; i++) out[i] = parseInt(h.slice(2 * i, 2 * i + 2), 16);
  return out;
}

/** Byte values → lowercase hex (no 0x).
    @param {number[]} bytes
    @returns {string} */
export function bytesToHex(bytes) {
  return bytes.map((b) => b.toString(16).padStart(2, '0')).join('');
}

/* ---- building tagged Expr JSON ------------------------------------------ */

/** Tagged Expr for an int. silverc parses i64; JS numbers are only exact to
    2^53−1, so bigger values throw rather than silently corrupt.
    @param {number|bigint|string} n
    @returns {{kind:'int', data:number}} */
export function exprInt(n) {
  const big = typeof n === 'bigint' ? n : BigInt(String(n).replace(/_/g, ''));
  if (big > BigInt(Number.MAX_SAFE_INTEGER) || big < -BigInt(Number.MAX_SAFE_INTEGER)) {
    throw new Error(`int ${big} exceeds JSON-safe integer range (2^53−1); silverc reads i64 but JS numbers cannot carry it exactly`);
  }
  return { kind: 'int', data: Number(big) };
}

/** Tagged Expr for a byte array (what pubkey / sig / datasig / byte[N] args
    are — there is no "bytes" variant, a byte[32] is an array of 32 byte Exprs).
    @param {string|number[]} hexOrBytes
    @returns {{kind:'array', data:{kind:'byte', data:number}[]}} */
export function exprBytes(hexOrBytes) {
  const bytes = Array.isArray(hexOrBytes) ? hexOrBytes : hexToBytes(hexOrBytes);
  for (const b of bytes) {
    if (!Number.isInteger(b) || b < 0 || b > 255) throw new Error(`byte out of range: ${b}`);
  }
  return { kind: 'array', data: bytes.map((b) => ({ kind: 'byte', data: b })) };
}

/** @param {boolean} b @returns {{kind:'bool', data:boolean}} */
export function exprBool(b) {
  return { kind: 'bool', data: !!b };
}

/** @param {string} s @returns {{kind:'string', data:string}} */
export function exprString(s) {
  return { kind: 'string', data: String(s) };
}

/* ---- tagged → bare ------------------------------------------------------- */

/** One tagged Expr → a bare JSON value (number / boolean / string / array /
    object). Byte arrays become "0x…" hex strings; state_objects become plain
    objects — the shapes the debugger test-file and `--ctor-arg` JSON literals
    accept.
    @param {object} expr - a tagged Expr ({kind, data})
    @returns {number|boolean|string|Array|object} */
export function taggedToValue(expr) {
  if (expr === null || typeof expr !== 'object' || typeof expr.kind !== 'string') {
    throw new Error(`not a tagged Expr: ${JSON.stringify(expr)}`);
  }
  const { kind, data } = expr;
  switch (kind) {
    case 'int':
    case 'date_literal':
      if (typeof data !== 'number') throw new Error(`${kind} data must be a number`);
      return data;
    case 'bool':
      return !!data;
    case 'string':
      return String(data);
    case 'byte':
      return '0x' + bytesToHex([data]);
    case 'array': {
      if (!Array.isArray(data)) throw new Error('array data must be an array');
      if (data.length > 0 && data.every((e) => e && e.kind === 'byte')) {
        return '0x' + bytesToHex(data.map((e) => e.data));
      }
      return data.map(taggedToValue);
    }
    case 'state_object': {
      if (!Array.isArray(data)) throw new Error('state_object data must be an array');
      const out = {};
      for (const field of data) out[field.name] = taggedToValue(field.expr);
      return out;
    }
    default:
      throw new Error(`unsupported Expr kind for constructor args: ${kind} (supported: int, bool, byte, string, date_literal, array, state_object)`);
  }
}

/** Tagged silverc ctor JSON → bare positional argv strings — ready for the
    debugger's `--ctor-arg`, kascov's silverc wrapper, or kascov.io /compile's
    `"args"`. Scalars print bare ("86400", "true"); byte arrays print as
    "0x…"; nested arrays / state objects print as JSON literals.
    @param {object[]} exprs - array of tagged Exprs
    @returns {string[]} */
export function taggedToBare(exprs) {
  if (!Array.isArray(exprs)) throw new Error('expected an array of tagged Exprs');
  return exprs.map((e) => {
    const v = taggedToValue(e);
    return typeof v === 'string' ? v : typeof v === 'object' ? JSON.stringify(v) : String(v);
  });
}

/* ---- bare → tagged ------------------------------------------------------- */

/** A bare JSON value → tagged Expr. `type` (a SilverScript type name like
    "int", "bool", "pubkey", "byte[32]", "byte", "string") disambiguates;
    without it: numbers → int, booleans → bool, "0x…"/64-hex strings → byte
    arrays, other strings → string, arrays → element-wise, objects →
    state_object.
    @param {number|boolean|string|Array|object} value
    @param {string} [type]
    @returns {object} tagged Expr */
export function valueToTagged(value, type) {
  const t = (type || '').trim();
  if (t === 'int') {
    return exprInt(typeof value === 'string' && /^0x/i.test(value.trim()) ? BigInt(value.trim()) : value);
  }
  if (t === 'bool') {
    if (typeof value === 'boolean') return exprBool(value);
    if (value === 'true' || value === 'false') return exprBool(value === 'true');
    throw new Error(`invalid bool: ${JSON.stringify(value)}`);
  }
  if (t === 'string') return exprString(value);
  if (t === 'byte') {
    const bytes = typeof value === 'number' ? [value] : hexToBytes(value);
    if (bytes.length !== 1) throw new Error(`byte expects 1 byte, got ${bytes.length}`);
    return { kind: 'byte', data: bytes[0] };
  }
  if (t === 'pubkey' || t === 'sig' || t === 'datasig' || t === 'hash32' || /^byte\s*\[/.test(t)) {
    const expr = exprBytes(value);
    const fixed = t.match(/^byte\s*\[\s*(\d+)\s*\]$/);
    if (fixed && expr.data.length !== Number(fixed[1])) {
      throw new Error(`${t} expects ${fixed[1]} bytes, got ${expr.data.length}`);
    }
    if (t === 'pubkey' && expr.data.length !== 32) throw new Error(`pubkey expects 32 bytes, got ${expr.data.length}`);
    return expr;
  }
  if (t) throw new Error(`unknown type hint: ${t}`);

  // No hint — infer.
  if (typeof value === 'number') return exprInt(value);
  if (typeof value === 'bigint') return exprInt(value);
  if (typeof value === 'boolean') return exprBool(value);
  if (Array.isArray(value)) return { kind: 'array', data: value.map((v) => valueToTagged(v)) };
  if (value !== null && typeof value === 'object') {
    return { kind: 'state_object', data: Object.entries(value).map(([name, v]) => ({ name, expr: valueToTagged(v) })) };
  }
  if (typeof value === 'string') {
    const s = value.trim();
    if (s === 'true' || s === 'false') return exprBool(s === 'true');
    if (INT_RE.test(s)) return exprInt(s);
    if (s.startsWith('[') || s.startsWith('{')) return valueToTagged(JSON.parse(s));
    if (/^0x/i.test(s) || (s.length >= 2 && s.length % 2 === 0 && HEX_RE.test(s) && !/^\d+$/.test(s))) {
      return exprBytes(s);
    }
    return exprString(s);
  }
  throw new Error(`cannot encode value: ${JSON.stringify(value)}`);
}

/** Bare positional args (strings or plain JSON values) → tagged silverc ctor
    JSON, ready to write to the file `silverc --constructor-args` reads.
    `types` (optional, same length) pins each arg's SilverScript type — pass
    the contract's constructor signature to remove all inference ambiguity,
    e.g. ['pubkey', 'byte[32]', 'int', 'int'] for a Mecenas.
    @param {(string|number|boolean|Array|object)[]} args
    @param {string[]} [types]
    @returns {object[]} tagged Exprs */
export function bareToTagged(args, types = []) {
  if (!Array.isArray(args)) throw new Error('expected an array of bare args');
  if (types.length && types.length !== args.length) {
    throw new Error(`got ${args.length} args but ${types.length} type hints`);
  }
  return args.map((a, i) => valueToTagged(a, types[i]));
}

/* ---- CLI ------------------------------------------------------------------
   Direction is auto-detected: a single JSON-array argument whose elements all
   carry a "kind" tag is tagged input (→ bare); anything else is bare input
   (→ tagged). Output is JSON on stdout. */
function cliMain(argv) {
  const args = [...argv];
  let types = [];
  const ti = args.indexOf('--types');
  if (ti !== -1) {
    types = (args[ti + 1] || '').split(',').map((s) => s.trim()).filter(Boolean);
    args.splice(ti, 2);
  }
  const ii = args.indexOf('--id');
  if (ii !== -1) {
    const id = args[ii + 1] || '';
    if (!isId64(id)) {
      process.stderr.write(`not a valid 64-hex id (need exactly 64 hex chars = 32 bytes): ${id}\n`);
      process.exit(1);
    }
    process.stdout.write(normalizeId(id) + '\n');
    return;
  }
  if (args.length === 0) {
    process.stderr.write(
      'usage: node kascov-encode.mjs <tagged-json | bare-arg …>\n' +
      '       node kascov-encode.mjs --types pubkey,byte[32],int,int <bare-arg …>\n' +
      '       node kascov-encode.mjs --id <64-hex covenant id / txid>\n'
    );
    process.exit(1);
  }
  const first = args[0].trim();
  if (args.length === 1 && first.startsWith('[')) {
    const parsed = JSON.parse(first);
    if (Array.isArray(parsed) && parsed.length > 0 && parsed.every((e) => e && typeof e === 'object' && typeof e.kind === 'string')) {
      process.stdout.write(JSON.stringify(taggedToBare(parsed)) + '\n');
      return;
    }
    process.stdout.write(JSON.stringify(bareToTagged(parsed, types)) + '\n');
    return;
  }
  process.stdout.write(JSON.stringify(bareToTagged(args, types)) + '\n');
}

/* Run the CLI only when executed directly (never on import, never in a browser). */
if (typeof process !== 'undefined' && process.argv && process.argv[1] && import.meta.url) {
  const { pathToFileURL } = await import('node:url');
  if (import.meta.url === pathToFileURL(process.argv[1]).href) {
    try {
      cliMain(process.argv.slice(2));
    } catch (e) {
      process.stderr.write(`kascov-encode: ${e.message}\n`);
      process.exit(1);
    }
  }
}
