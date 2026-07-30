# Traffic analytics

kascov measures traffic from its own Caddy access logs. There is no browser
analytics SDK, cookie, tracking pixel, or third-party collector.

## Read a report

On the production VPS, from the WSL checkout:

```bash
python3 scripts/traffic_report.py --since 24h
python3 scripts/traffic_report.py --since 7d
python3 scripts/traffic_report.py --since 30d --json
```

The report includes:

- approximate unique browsers, 30-minute sessions, and page views;
- total `/data/*` API calls, split into first-party web UI and external/unknown;
- response bytes, status classes, errors, latency, busiest hour, and top paths.

## Private dashboard

The production dashboard lives at:

```text
https://kascov.io/ops/traffic/
```

Caddy protects the complete `/ops/traffic` tree with HTTP Basic
Authentication, sends `no-store` and `noindex` headers, and omits the
dashboard's own requests from the access log. The browser receives aggregate
counts only. Hashed visitor keys are used in memory to estimate browsers and
are never written to the dashboard snapshot.

`kascov-traffic-snapshot.timer` runs once a minute in WSL and atomically writes
`C:\kascov\web\ops\traffic\traffic.json`. Five-minute and 24-hour windows
refresh every minute; the more expensive 7- and 30-day rolls refresh every 15
minutes.

To install or repair the timer:

```bash
sudo install -m 0644 scripts/kascov-traffic-snapshot.service /etc/systemd/system/
sudo install -m 0644 scripts/kascov-traffic-snapshot.timer /etc/systemd/system/
sudo systemctl daemon-reload
sudo systemctl enable --now kascov-traffic-snapshot.timer
sudo systemctl start kascov-traffic-snapshot.service
```

The Caddy credential is deliberately not stored in git. On the Windows host,
generate a hash with `C:\tools\caddy.exe hash-password`, then create:

```text
C:\caddy\secrets\kascov-traffic-users
```

with one tokenized line:

```text
michiel <the-generated-hash>
```

Create that file before validating or reloading
`scripts/kascov.windows.Caddyfile`.

## Endpoint grouping

Rankings group resource-shaped URLs instead of treating every identifier as a separate endpoint:

- long hexadecimal path segments become `:id`, including ids before `.json`;
- `/addr/<value>` becomes `/addr/:address`;
- long numeric path segments become `:n`.

For example, all per-coin JSON calls roll up under `/data/mainnet/c/:id.json`. This bounds report cardinality and makes the endpoint table describe API shapes rather than individual users or coins.

An SSE connection's logged duration is its healthy open lifetime, not response latency. `/stream` opens count toward API totals but are excluded from the p50/p95 latency histogram.

## Bots, scanners, and health probes

User-agent markers identify ordinary crawlers, social unfurlers, uptime monitors, and probes. Path rules also classify obvious generic scanner traffic—WordPress, `xmlrpc.php`, `.env`, `cgi-bin`, phpMyAdmin, Actuator, admin, and generic PHP probes—as bots even when it presents a browser user agent.

Scanner requests never enter approximate visitor/session counts or top-page rankings. `/health` and `/healthz` are counted separately as health checks.

Unique browsers are approximate. Caddy hashes the Cloudflare/client IP fields
before writing a request to disk, and the report counts those opaque hashes. It
does not print or export identifiers. NAT can combine people, changing networks
can split one person, browser caching can suppress page loads, and the SPA's
`#` route is never sent to the server. API totals are exact for the retained
logs; the visitor metrics are operational estimates.

## Retention and privacy

The production Caddyfile:

- hashes `CF-Connecting-IP`, forwarding, client, and remote IP fields;
- removes values for search/batch/program query parameters;
- redacts credential-bearing headers using Caddy's defaults;
- rolls at midnight or 50 MiB, whichever comes first;
- retains at most 35 rolls and no more than 30 days.

Logging began when this configuration was deployed. Older visitor and API
counts cannot be reconstructed.

## Tests

```bash
python3 -m unittest scripts/test_traffic_report.py
```

The fixtures pin page/API classification, first-party inference, endpoint normalization, scanner exclusion, time filtering, and malformed-row handling.

See [[Operations#First-party traffic measurement]] for how these metrics fit into the production runbook.
