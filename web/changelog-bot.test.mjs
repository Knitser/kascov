/* The changelog bot's two dangerous moments are both about WHEN it posts, not
   what it says: standing it up must not flood a fresh channel, and a second run
   over an unchanged feed must not repeat itself. Both are pure functions, so
   both are pinned here. */
import test from 'node:test';
import assert from 'node:assert/strict';

import {
  buildEntryEmbed, pendingEntries, stampOf,
} from '../scripts/discord-changelog-bot.mjs';

/* newest first, exactly as changelog.json is served */
const FEED = [
  { date: '2026-07-30', title: 'every holder, trade and address leads to one page', body: 'c' },
  { date: '2026-07-29', title: 'a live map of who holds what', body: 'b' },
  { date: '2026-07-29', title: 'every verification pass is now on the record', body: 'a' },
];

test('an entry is identified by date AND title, because a date repeats', () => {
  // two things shipped on 2026-07-29; date alone would collide
  assert.notEqual(stampOf(FEED[1]), stampOf(FEED[2]));
  assert.equal(stampOf(FEED[0]), '2026-07-30|every holder, trade and address leads to one page');
});

test('the first run posts nothing at all', () => {
  // the whole point: standing this up must not dump the backlog into a new channel
  assert.deepEqual(pendingEntries(FEED, new Set(), { firstRun: true }), []);
});

test('a new entry is picked up once the bot has a baseline', () => {
  const seen = new Set([stampOf(FEED[1]), stampOf(FEED[2])]);
  const out = pendingEntries(FEED, seen);
  assert.equal(out.length, 1);
  assert.equal(out[0].title, FEED[0].title);
});

test('an unchanged feed posts nothing the second time', () => {
  const seen = new Set(FEED.map(stampOf));
  assert.deepEqual(pendingEntries(FEED, seen), []);
});

test('several new entries go out oldest first, not newest first', () => {
  const out = pendingEntries(FEED, new Set());
  assert.deepEqual(out.map((e) => e.body), ['a', 'b', 'c']);
});

test('an embed carries the date and links back to the changelog', () => {
  const e = buildEntryEmbed(FEED[0]);
  assert.match(e.footer.text, /2026-07-30/);
  assert.equal(e.title, FEED[0].title);
  assert.match(e.url, /kascov\.io\/#\/changelog/);
});

test('a body too long for Discord is truncated rather than rejected', () => {
  const e = buildEntryEmbed({ date: '2026-07-30', title: 't', body: 'x'.repeat(5000) });
  assert.ok(e.description.length <= 4000);
  assert.match(e.description, /\.\.\.$/);
});

test('a malformed feed is survived, never thrown on', () => {
  assert.deepEqual(pendingEntries(null, new Set()), []);
  assert.deepEqual(pendingEntries(undefined, new Set()), []);
  assert.equal(stampOf(undefined), '|');
});
