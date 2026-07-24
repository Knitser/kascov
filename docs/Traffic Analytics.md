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
