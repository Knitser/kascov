/* BLAKE2b-256 for the browser — verifies pasted programs against p2sh
   commitments client-side. Pure JS (BigInt), no deps; self-tests at load
   against a known vector and refuses to expose itself if that fails. */
(() => {
  'use strict';
  const IV = [
    0x6a09e667f3bcc908n, 0xbb67ae8584caa73bn, 0x3c6ef372fe94f82bn, 0xa54ff53a5f1d36f1n,
    0x510e527fade682d1n, 0x9b05688c2b3e6c1fn, 0x1f83d9abfb41bd6bn, 0x5be0cd19137e2179n,
  ];
  const SIGMA = [
    [0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15],
    [14,10,4,8,9,15,13,6,1,12,0,2,11,7,5,3],
    [11,8,12,0,5,2,15,13,10,14,3,6,7,1,9,4],
    [7,9,3,1,13,12,11,14,2,6,5,10,4,0,15,8],
    [9,0,5,7,2,4,10,15,14,1,11,12,6,8,3,13],
    [2,12,6,10,0,11,8,3,4,13,7,5,15,14,1,9],
    [12,5,1,15,14,13,4,10,0,7,6,3,9,2,8,11],
    [13,11,7,14,12,1,3,9,5,0,15,4,8,6,2,10],
    [6,15,14,9,11,3,0,8,12,2,13,7,1,4,10,5],
    [10,2,8,4,7,6,1,5,15,11,9,14,3,12,13,0],
    [0,1,2,3,4,5,6,7,8,9,10,11,12,13,14,15],
    [14,10,4,8,9,15,13,6,1,12,0,2,11,7,5,3],
  ];
  const M64 = (1n << 64n) - 1n;
  const rotr = (x, n) => ((x >> n) | (x << (64n - n))) & M64;

  function compress(h, block, t, last) {
    const m = new Array(16);
    for (let i = 0; i < 16; i++) {
      let w = 0n;
      for (let j = 7; j >= 0; j--) w = (w << 8n) | BigInt(block[i * 8 + j]);
      m[i] = w;
    }
    const v = h.concat(IV.slice());
    v[12] ^= BigInt(t) & M64;
    if (last) v[14] ^= M64;
    const G = (a, b, c, d, x, y) => {
      v[a] = (v[a] + v[b] + x) & M64; v[d] = rotr(v[d] ^ v[a], 32n);
      v[c] = (v[c] + v[d]) & M64;     v[b] = rotr(v[b] ^ v[c], 24n);
      v[a] = (v[a] + v[b] + y) & M64; v[d] = rotr(v[d] ^ v[a], 16n);
      v[c] = (v[c] + v[d]) & M64;     v[b] = rotr(v[b] ^ v[c], 63n);
    };
    for (let r = 0; r < 12; r++) {
      const s = SIGMA[r];
      G(0, 4, 8, 12, m[s[0]], m[s[1]]);  G(1, 5, 9, 13, m[s[2]], m[s[3]]);
      G(2, 6, 10, 14, m[s[4]], m[s[5]]); G(3, 7, 11, 15, m[s[6]], m[s[7]]);
      G(0, 5, 10, 15, m[s[8]], m[s[9]]); G(1, 6, 11, 12, m[s[10]], m[s[11]]);
      G(2, 7, 8, 13, m[s[12]], m[s[13]]); G(3, 4, 9, 14, m[s[14]], m[s[15]]);
    }
    for (let i = 0; i < 8; i++) h[i] = (h[i] ^ v[i] ^ v[i + 8]) & M64;
  }

  /* blake2b-256(bytes) -> Uint8Array(32); unkeyed, no salt/personalization */
  function blake2b256(input) {
    const h = IV.slice();
    h[0] ^= 0x01010000n ^ 32n; // digest_length=32, fanout=1, depth=1
    let t = 0;
    let i = 0;
    // full blocks except the last (the final block is always compressed with last=true)
    while (input.length - i > 128) {
      t += 128;
      compress(h, input.subarray(i, i + 128), t, false);
      i += 128;
    }
    const block = new Uint8Array(128);
    block.set(input.subarray(i));
    t += input.length - i;
    compress(h, block, t, true);
    const out = new Uint8Array(32);
    for (let k = 0; k < 4; k++) {
      let w = h[k];
      for (let j = 0; j < 8; j++) { out[k * 8 + j] = Number(w & 0xffn); w >>= 8n; }
    }
    return out;
  }

  /* known-answer self-test: blake2b-256("abc") */
  const abc = blake2b256(new Uint8Array([0x61, 0x62, 0x63]));
  const want = 'bddd813c634239723171ef3fee98579b94964e3bb1cb3e427262c8c068d52319';
  const got = Array.from(abc).map((b) => b.toString(16).padStart(2, '0')).join('');
  if (got === want) {
    window.kascovBlake2b256 = blake2b256;
  } else {
    /* refuse to verify anything with a broken hash */
    console.error('kascov: blake2b self-test failed — reveal preview disabled');
  }
})();
