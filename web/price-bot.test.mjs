/* The price bot's formatting is pure, so the honesty rules are testable:
   a fall must render as plainly as a rise, and an unpriceable token must not
   acquire a number on the way to Discord. */
import test from 'node:test';
import assert from 'node:assert/strict';

import {
  buildEmbed, fmtChange, priceKas, fmtKas,
} from '../scripts/discord-price-bot.mjs';

const TOKEN = 'c58c826d0aa9cee62f93208718c674883f5c89a8aca4933dc41fb0391539abe2';

test('a price is derived from the integer pair, never pre-divided', () => {
  // 108,318.56 KAS over 548,394 tokens
  assert.equal(priceKas(10831856000000, 548394), '0.197520');
  assert.equal(priceKas(null, 100), null);
  assert.equal(priceKas(100, 0), null);
  assert.equal(priceKas(0, 100), null);
});

test('a fall is shown exactly as plainly as a rise', () => {
  assert.equal(fmtChange(-6779), '▼ -67.79%');
  assert.equal(fmtChange(1250), '▲ +12.50%');
  assert.equal(fmtChange(0), '= +0.00%');
  assert.equal(fmtChange(null), null);
});

test('sompi renders as KAS without inventing precision', () => {
  assert.equal(fmtKas(4581856000000, 0), '45,819');
  assert.equal(fmtKas(100000000, 2), '1.00');
});

test('an unpriceable token gets no price, and says why', () => {
  const embed = buildEmbed({
    token: { name: 'humble-crimson-tortoise', supply: 1000000, holders: 62, status: 'verified' },
    market: { unpriced_reason: 'the market covenant’s program is not yet verified' },
  }, { tokenId: TOKEN });
  const price = embed.fields.find((f) => f.name === 'price');
  assert.match(price.value, /not published/);
  assert.match(price.value, /not yet verified/);
  // and nothing anywhere pretends otherwise
  assert.ok(!/\d+\.\d+ KAS/.test(price.value));
  assert.match(embed.footer.text, /no verified market program/);
});

test('a bonding token shows its progress and its real drawdown', () => {
  const embed = buildEmbed({
    token: { name: 'humble-crimson-tortoise', supply: 1000000, holders: 62, status: 'verified' },
    market: {
      phase: 'bonding',
      grad_progress_bps: 1832,
      spot_num_sompi: 10831856000000,
      spot_den: 548394,
      change_24h_bps: -6779,
      reserve_sompi: 4581856000000,
      volume_24h_sompi: 15454300000000,
      trades_24h: 59,
      program: { skeleton: 'KRON curve v1', exercised_trades: 333 },
    },
  }, { tokenId: TOKEN });

  const by = (n) => embed.fields.find((f) => f.name === n)?.value;
  assert.match(by('price'), /0\.197520 KAS/);
  assert.match(by('24h'), /-67\.79%/);          // the drop is not hidden
  assert.match(by('bonding'), /18\.3%/);
  assert.equal(by('holders'), '62');
  assert.match(embed.footer.text, /333 trades replayed/);
  assert.match(embed.url, new RegExp(TOKEN));
});

test('the embed always points at the page that proves it', () => {
  const embed = buildEmbed({ token: { name: 'x' }, market: {} }, { tokenId: TOKEN });
  assert.match(embed.description, /kascov\.io\/#\/mainnet\/token\//);
  assert.match(embed.description, /nothing comes from a launchpad API/);
});
