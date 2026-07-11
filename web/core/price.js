/* kascov core/price — KAS→USD tails on real-money amounts (mainnet only).
   The rate rides a feature-detected /data/price.json: one probe per 5 minutes
   while a mainnet view is up, and any miss (404/503/network error) silences
   the feature for a cooldown before reprobing — an older worker without the
   route must leave zero trace in the UI. Testnet never fetches and never
   shows a dollar sign (play money). */

import { fmtAmount, state } from './state.js';

const PRICE_TTL_MS = 5 * 60_000;
const PRICE_MISS_COOLDOWN_MS = 10 * 60_000; /* a miss goes quiet, then reprobes */
const price = { data: null, at: 0, deadUntil: 0, inflight: null };
let usdOn = true;
try { usdOn = localStorage.getItem('kascov-usd') !== '0'; } catch (e) { /* private mode */ }

/* returns a promise ONLY when this call started a fetch — callers hang their
   one repaint off it without stacking repaints on a shared in-flight probe */
function loadPrice() {
  if (Date.now() < price.deadUntil || state.network !== 'mainnet') return null;
  if (price.data && Date.now() - price.at < PRICE_TTL_MS) return null;
  if (price.inflight) return null;
  const miss = () => { price.deadUntil = Date.now() + PRICE_MISS_COOLDOWN_MS; return null; };
  price.inflight = fetch('data/price.json', { cache: 'no-cache' })
    .then(async (res) => {
      if (!res.ok) return miss();
      const d = await res.json();
      if (!d || typeof d.kas_usd !== 'number' || !(d.kas_usd > 0)) return miss();
      price.data = d;
      price.at = Date.now();
      return d;
    })
    .catch(miss)
    .finally(() => { price.inflight = null; });
  return price.inflight;
}

/* dollar formatting: 2 significant decimals under $1, cents above */
function fmtUsd(usd) {
  const str = usd >= 1
    ? usd.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 })
    : usd.toLocaleString('en-US', { maximumSignificantDigits: 2 });
  return `$${str}`;
}

/* fmtAmount plus an "≈ $X" tail — plain text, so the esc()'d call sites on
   coin pages, address pages and the watch strip keep working unchanged */
function amountWithUsd(sompi, network) {
  const base = fmtAmount(sompi, network);
  if (network !== 'mainnet' || !usdOn || !price.data || !(sompi > 0)) return base;
  return `${base} ≈ ${fmtUsd((sompi / 1e8) * price.data.kas_usd)}`;
}

/* the toggle chip — only rendered where dollar tails can actually appear */
function usdToggleHtml() {
  if (state.network !== 'mainnet' || !price.data) return '';
  return `<button type="button" class="chip chip-usd" data-action="usd" aria-pressed="${usdOn}"` +
    ` title="show approximate US-dollar values next to KAS amounts">≈ USD</button>`;
}

/* the ≈USD chip was clicked — flip, persist, report the new setting */
function toggleUsd() {
  usdOn = !usdOn;
  try { localStorage.setItem('kascov-usd', usdOn ? '1' : '0'); } catch (e) { /* private mode */ }
  return usdOn;
}

export { loadPrice, fmtUsd, amountWithUsd, usdToggleHtml, toggleUsd };
