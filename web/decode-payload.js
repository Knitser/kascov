/* kascov — tiny, dependency-free decoder for KRC-20-style JSON inscriptions
   carried in a transaction payload. Kaspa token protocols (KRC-20 and its
   look-alikes) stamp a small JSON object into the tx payload, e.g.
     {"p":"krc-20","op":"mint","tick":"NACHO","amt":"1000"}
   The payload reaches us as hex (the wire form, starting 7b22… for `{"`), but
   we also accept the raw JSON text for robustness. Everything is best-effort:
   anything that isn't a recognisable inscription object returns null so the
   caller can quietly fall back to its generic payload peek. */
(() => {
  'use strict';

  /* hex → latin1 string (payloads are ASCII JSON; matches app.js payloadPeek) */
  function hexToStr(hex) {
    let s = '';
    for (let i = 0; i + 1 < hex.length; i += 2) {
      s += String.fromCharCode(parseInt(hex.slice(i, i + 2), 16));
    }
    return s;
  }

  /* pull the JSON text out of a payload that may be hex-encoded or already raw */
  function jsonText(payload) {
    if (typeof payload !== 'string') return null;
    const p = payload.trim();
    if (!p) return null;
    /* pure hex, even length → decode the bytes (real inscriptions start 7b22) */
    if (/^[0-9a-fA-F]+$/.test(p) && p.length % 2 === 0) {
      const t = hexToStr(p).trim();
      return t.startsWith('{') ? t : null;
    }
    /* otherwise it's only an inscription if it already looks like a JSON object */
    return p.startsWith('{') ? p : null;
  }

  /* decode a payload into a normalised inscription record, or null.
     recognised fields (all optional except a protocol + op): p, op, tick,
     amt, max, lim, dec, to. Unknown extra keys are preserved on .raw. */
  function decodeInscription(payload) {
    const text = jsonText(payload);
    if (!text) return null;
    let obj;
    try { obj = JSON.parse(text); } catch (e) { return null; }
    if (!obj || typeof obj !== 'object' || Array.isArray(obj)) return null;
    /* an inscription has a protocol tag and an operation — be lenient on case */
    const proto = obj.p != null ? String(obj.p) : '';
    const op = obj.op != null ? String(obj.op) : '';
    if (!proto || !op) return null;
    const str = (v) => (v == null ? '' : String(v));
    return {
      proto,                       // e.g. "krc-20"
      op,                          // e.g. "mint" / "deploy" / "transfer"
      tick: str(obj.tick),
      amt: str(obj.amt),
      max: str(obj.max),
      lim: str(obj.lim),
      dec: str(obj.dec),
      to: str(obj.to),
      raw: obj,
    };
  }

  /* short chip label: "KRC-20 · mint · NACHO" (drops empty parts) */
  function chipLabel(insc) {
    if (!insc) return '';
    const parts = [insc.proto.toUpperCase(), insc.op.toLowerCase()];
    if (insc.tick) parts.push(insc.tick.toUpperCase());
    return parts.join(' · ');
  }

  /* fuller one-line tooltip listing the interesting numeric fields */
  function chipTitle(insc) {
    if (!insc) return '';
    const bits = [chipLabel(insc)];
    if (insc.amt) bits.push(`amount ${insc.amt}`);
    if (insc.max) bits.push(`max supply ${insc.max}`);
    if (insc.lim) bits.push(`mint limit ${insc.lim}`);
    if (insc.dec) bits.push(`decimals ${insc.dec}`);
    if (insc.to) bits.push(`to ${insc.to}`);
    return bits.join(' · ');
  }

  window.kascovPayload = { decodeInscription, chipLabel, chipTitle };
})();
