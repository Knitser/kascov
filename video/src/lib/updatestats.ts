/* Frozen production numbers for the V4 update video — refreshed right
   before render so every figure on screen is real. */
export const U = {
  coins: 50409, // TN10 smart coins tracked (live feed, refreshed at render)
  events: 230260, // life events recorded
  growth: '40×', // 1.3k → 50k+ coins in the launch week
  oldLoadS: 24, // full-picture load before the rework (measured)
  newLoadS: 1, // grid paint after (CDN-warm, measured 0.76s)
  oldMB: 27, // old all-in-one snapshot, compressed
  newMB: 2.2, // grid feed, compressed
  digest: {born: 104, moved: 105, retired: 19}, // a real last-24h window
} as const;
