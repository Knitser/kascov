# vendored

`lightweight-charts.js` — TradingView Lightweight Charts™ v4.2.0, Apache-2.0.
Vendored rather than pulled from a CDN so the page works offline and the bytes
being served are the bytes in the repository. Loaded lazily by app.js only
when a market page actually draws candles, so it never taxes first paint.
