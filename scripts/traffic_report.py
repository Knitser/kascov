#!/usr/bin/env python3
"""Privacy-conscious traffic summary for Caddy JSON access logs.

The production Caddyfile hashes client IP fields before they reach disk. This
tool only keeps those opaque values in memory long enough to count approximate
unique browsers and 30-minute sessions; it never prints visitor identifiers.
"""

from __future__ import annotations

import argparse
import collections
import datetime as dt
import glob
import gzip
import json
import math
import re
import sys
from pathlib import Path
from urllib.parse import urlsplit


DEFAULT_LOG_GLOB = "/mnt/c/caddy/logs/kascov-access*"
SESSION_GAP_SECONDS = 30 * 60
API_PREFIX = "/data/"
INTERNAL_PATHS = ("/health", "/healthz")
NON_PAGE_PREFIXES = ("/data/", "/og/", "/badge/", "/img/")
NON_PAGE_EXACT = ("/sitemap.xml", "/feed.xml", "/robots.txt", "/favicon.ico")
ASSET_EXTENSIONS = {
    ".avif",
    ".css",
    ".gif",
    ".ico",
    ".jpeg",
    ".jpg",
    ".js",
    ".json",
    ".map",
    ".mp4",
    ".png",
    ".svg",
    ".webm",
    ".webmanifest",
    ".woff",
    ".woff2",
}
BOT_MARKERS = (
    "bot",
    "crawler",
    "spider",
    "slurp",
    "facebookexternalhit",
    "twitterbot",
    "discordbot",
    "telegrambot",
    "whatsapp",
    "uptime",
    "monitoring",
    "probe",
)
ID_SEGMENT = re.compile(r"(?<=/)[0-9a-fA-F]{32,}(?=/|$|\.json)")
NUMBER_SEGMENT = re.compile(r"(?<=/)\d{4,}(?=/|$)")
ADDRESS_SEGMENT = re.compile(r"(?<=/addr/)[^/]+(?=/|$)")
DURATION_RE = re.compile(r"^(?P<count>\d+)(?P<unit>[mhdw])$")
DURATION_SECONDS = {"m": 60, "h": 3600, "d": 86400, "w": 604800}
LATENCY_BUCKETS = (
    0.001,
    0.005,
    0.010,
    0.025,
    0.050,
    0.100,
    0.250,
    0.500,
    1.0,
    2.5,
    5.0,
    10.0,
    math.inf,
)


def header(headers: dict, name: str) -> str:
    """Return the first header value, case-insensitively."""
    wanted = name.lower()
    for key, value in headers.items():
        if key.lower() != wanted:
            continue
        if isinstance(value, list):
            return str(value[0]) if value else ""
        return str(value)
    return ""


def parse_since(raw: str, now: dt.datetime) -> float | None:
    if raw.lower() == "all":
        return None
    if match := DURATION_RE.fullmatch(raw.lower()):
        seconds = int(match.group("count")) * DURATION_SECONDS[match.group("unit")]
        return now.timestamp() - seconds
    try:
        parsed = dt.datetime.fromisoformat(raw.replace("Z", "+00:00"))
    except ValueError as exc:
        raise argparse.ArgumentTypeError(
            "--since must be all, a duration such as 24h/7d, or an ISO timestamp"
        ) from exc
    if parsed.tzinfo is None:
        parsed = parsed.replace(tzinfo=dt.UTC)
    return parsed.timestamp()


def log_paths(patterns: list[str]) -> list[Path]:
    found: dict[str, Path] = {}
    for pattern in patterns:
        matches = glob.glob(pattern)
        if not matches and Path(pattern).is_file():
            matches = [pattern]
        for match in matches:
            path = Path(match)
            if path.is_file():
                found[str(path.resolve())] = path
    # Caddy's timestamped rolls sort before the current ".json" file, which
    # keeps per-visitor session counting chronological in normal operation.
    return sorted(found.values(), key=lambda p: p.name)


def open_log(path: Path):
    if path.suffix == ".gz":
        return gzip.open(path, "rt", encoding="utf-8", errors="replace")
    return path.open("rt", encoding="utf-8", errors="replace")


def visitor_key(request: dict) -> str:
    headers = request.get("headers") or {}
    # Cloudflare is the public edge. Its connecting-IP field has already been
    # hashed by Caddy; fall back to Caddy's likewise-hashed client/remote IP.
    for name in ("Cf-Connecting-Ip", "True-Client-Ip", "X-Forwarded-For"):
        value = header(headers, name)
        if value:
            return value.split(",", 1)[0].strip()
    return str(request.get("client_ip") or request.get("remote_ip") or "")


def is_browser(user_agent: str) -> bool:
    ua = user_agent.lower()
    return "mozilla/" in ua and not any(marker in ua for marker in BOT_MARKERS)


def is_bot(user_agent: str) -> bool:
    ua = user_agent.lower()
    return any(marker in ua for marker in BOT_MARKERS)


def is_asset(path: str) -> bool:
    return Path(path).suffix.lower() in ASSET_EXTENSIONS


def is_page_view(method: str, status: int, path: str, user_agent: str) -> bool:
    if method not in ("GET", "HEAD") or not (200 <= status < 400):
        return False
    if not is_browser(user_agent) or is_asset(path):
        return False
    if path in NON_PAGE_EXACT or path in INTERNAL_PATHS:
        return False
    return not path.startswith(NON_PAGE_PREFIXES)


def normalize_path(path: str) -> str:
    path = ID_SEGMENT.sub(":id", path)
    path = ADDRESS_SEGMENT.sub(":address", path)
    return NUMBER_SEGMENT.sub(":n", path)


def is_first_party(request: dict) -> bool:
    headers = request.get("headers") or {}
    fetch_site = header(headers, "Sec-Fetch-Site").lower()
    if fetch_site in ("same-origin", "same-site"):
        return True
    referer = header(headers, "Referer")
    if not referer:
        return False
    try:
        host = (urlsplit(referer).hostname or "").lower()
    except ValueError:
        return False
    return host in ("kascov.io", "www.kascov.io")


class LatencyHistogram:
    def __init__(self) -> None:
        self.counts = [0] * len(LATENCY_BUCKETS)
        self.total = 0

    def add(self, seconds: float) -> None:
        self.total += 1
        for index, upper in enumerate(LATENCY_BUCKETS):
            if seconds <= upper:
                self.counts[index] += 1
                return

    def percentile(self, fraction: float) -> float:
        if not self.total:
            return 0.0
        target = math.ceil(self.total * fraction)
        seen = 0
        for upper, count in zip(LATENCY_BUCKETS, self.counts):
            seen += count
            if seen >= target:
                return upper
        return LATENCY_BUCKETS[-1]


class TrafficReport:
    def __init__(self) -> None:
        self.total_requests = 0
        self.api_calls = 0
        self.first_party_api_calls = 0
        self.health_checks = 0
        self.page_views = 0
        self.sessions = 0
        self.bot_requests = 0
        self.errors = 0
        self.bytes_out = 0
        self.malformed_lines = 0
        self.filtered_old_lines = 0
        self.first_ts: float | None = None
        self.last_ts: float | None = None
        self.visitor_last_seen: dict[str, float] = {}
        self.pages: collections.Counter[str] = collections.Counter()
        self.api_endpoints: collections.Counter[str] = collections.Counter()
        self.statuses: collections.Counter[str] = collections.Counter()
        self.hours: collections.Counter[str] = collections.Counter()
        self.latency = LatencyHistogram()

    def add(self, row: dict) -> None:
        request = row.get("request") or {}
        ts = float(row.get("ts") or 0)
        method = str(request.get("method") or "")
        uri = str(request.get("uri") or "/")
        path = urlsplit(uri).path or "/"
        headers = request.get("headers") or {}
        user_agent = header(headers, "User-Agent")
        status = int(row.get("status") or 0)
        size = int(row.get("size") or 0)
        duration = float(row.get("duration") or 0)

        self.total_requests += 1
        self.bytes_out += max(0, size)
        self.latency.add(max(0.0, duration))
        self.statuses[f"{status // 100}xx" if status else "unknown"] += 1
        if status >= 400:
            self.errors += 1
        if is_bot(user_agent):
            self.bot_requests += 1
        if path in INTERNAL_PATHS:
            self.health_checks += 1
        if path.startswith(API_PREFIX):
            self.api_calls += 1
            if is_first_party(request):
                self.first_party_api_calls += 1
            self.api_endpoints[normalize_path(path)] += 1
        if is_page_view(method, status, path, user_agent):
            self.page_views += 1
            self.pages[normalize_path(path)] += 1
            key = visitor_key(request)
            if key:
                previous = self.visitor_last_seen.get(key)
                if previous is None or ts - previous > SESSION_GAP_SECONDS:
                    self.sessions += 1
                if previous is None or ts > previous:
                    self.visitor_last_seen[key] = ts

        when = dt.datetime.fromtimestamp(ts, dt.UTC)
        self.hours[when.strftime("%Y-%m-%d %H:00 UTC")] += 1
        self.first_ts = ts if self.first_ts is None else min(self.first_ts, ts)
        self.last_ts = ts if self.last_ts is None else max(self.last_ts, ts)

    def as_dict(self, top: int) -> dict:
        external = max(0, self.api_calls - self.first_party_api_calls)
        busiest = self.hours.most_common(1)
        return {
            "window": {
                "first": iso_time(self.first_ts),
                "last": iso_time(self.last_ts),
            },
            "visitors": {
                "unique_browsers_approx": len(self.visitor_last_seen),
                "sessions_30m": self.sessions,
                "page_views": self.page_views,
            },
            "requests": {
                "total": self.total_requests,
                "api_calls": self.api_calls,
                "first_party_api_calls": self.first_party_api_calls,
                "external_api_calls_approx": external,
                "health_checks": self.health_checks,
                "bot_requests": self.bot_requests,
                "errors": self.errors,
                "bytes_out": self.bytes_out,
            },
            "latency_seconds": {
                "p50_upper_bound": finite_or_label(self.latency.percentile(0.50)),
                "p95_upper_bound": finite_or_label(self.latency.percentile(0.95)),
            },
            "busiest_hour": (
                {"hour": busiest[0][0], "requests": busiest[0][1]} if busiest else None
            ),
            "status_classes": dict(sorted(self.statuses.items())),
            "top_pages": dict(self.pages.most_common(top)),
            "top_api_endpoints": dict(self.api_endpoints.most_common(top)),
            "input": {
                "malformed_lines": self.malformed_lines,
                "filtered_old_lines": self.filtered_old_lines,
            },
        }


def finite_or_label(value: float) -> float | str:
    return value if math.isfinite(value) else ">10"


def iso_time(timestamp: float | None) -> str | None:
    if timestamp is None:
        return None
    return dt.datetime.fromtimestamp(timestamp, dt.UTC).isoformat().replace("+00:00", "Z")


def analyze(paths: list[Path], cutoff: float | None) -> TrafficReport:
    report = TrafficReport()
    for path in paths:
        with open_log(path) as handle:
            for line in handle:
                try:
                    row = json.loads(line)
                    ts = float(row.get("ts") or 0)
                except (json.JSONDecodeError, TypeError, ValueError):
                    report.malformed_lines += 1
                    continue
                if cutoff is not None and ts < cutoff:
                    report.filtered_old_lines += 1
                    continue
                if row.get("request"):
                    report.add(row)
    return report


def human_bytes(count: int) -> str:
    value = float(count)
    for suffix in ("B", "KiB", "MiB", "GiB", "TiB"):
        if value < 1024 or suffix == "TiB":
            return f"{value:.1f} {suffix}"
        value /= 1024
    return f"{value:.1f} TiB"


def human_latency(value: float | str) -> str:
    if isinstance(value, str):
        return f"{value}s"
    if value < 1:
        return f"≤{value * 1000:.0f}ms"
    return f"≤{value:.1f}s"


def print_counter(title: str, values: dict) -> None:
    print(f"\n{title}")
    if not values:
        print("  (none)")
        return
    width = max(len(str(value)) for value in values.values())
    for name, value in values.items():
        print(f"  {value:>{width}}  {name}")


def print_human(data: dict, label: str, files: int) -> None:
    visitors = data["visitors"]
    requests = data["requests"]
    latency = data["latency_seconds"]
    print(f"kascov traffic — {label}")
    if data["window"]["first"]:
        print(f"{data['window']['first']} → {data['window']['last']} · {files} log file(s)")
    else:
        print(f"no matching requests · {files} log file(s)")
    print("\nVisitors (approximate, hashed IP; browser page loads only)")
    print(f"  {visitors['unique_browsers_approx']:>8}  unique browsers")
    print(f"  {visitors['sessions_30m']:>8}  30-minute sessions")
    print(f"  {visitors['page_views']:>8}  page views")
    print("\nRequests")
    print(f"  {requests['total']:>8}  total")
    print(f"  {requests['api_calls']:>8}  API calls (/data/*)")
    print(f"  {requests['first_party_api_calls']:>8}  from kascov web UI")
    print(f"  {requests['external_api_calls_approx']:>8}  external/unknown API clients")
    print(f"  {requests['health_checks']:>8}  health checks")
    print(f"  {requests['bot_requests']:>8}  crawler/monitor requests")
    print(f"  {requests['errors']:>8}  4xx/5xx")
    print(f"  {human_bytes(requests['bytes_out']):>8}  response bytes")
    print(
        "  "
        f"{human_latency(latency['p50_upper_bound'])} p50 · "
        f"{human_latency(latency['p95_upper_bound'])} p95"
    )
    busiest = data["busiest_hour"]
    if busiest:
        print(f"  busiest: {busiest['hour']} ({busiest['requests']} requests)")
    print_counter("Top pages", data["top_pages"])
    print_counter("Top API endpoints", data["top_api_endpoints"])
    print_counter("Status classes", data["status_classes"])
    malformed = data["input"]["malformed_lines"]
    if malformed:
        print(f"\nwarning: skipped {malformed} malformed log line(s)", file=sys.stderr)


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Summarize visitors and API traffic from Caddy JSON access logs."
    )
    parser.add_argument(
        "logs",
        nargs="*",
        help=f"log paths/globs (default: {DEFAULT_LOG_GLOB})",
    )
    parser.add_argument(
        "--since",
        default="24h",
        help="window: 30m, 24h, 7d, all, or an ISO timestamp (default: 24h)",
    )
    parser.add_argument("--top", type=int, default=10, help="top rows per table")
    parser.add_argument("--json", action="store_true", help="emit machine-readable JSON")
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    now = dt.datetime.now(dt.UTC)
    try:
        cutoff = parse_since(args.since, now)
    except argparse.ArgumentTypeError as exc:
        print(f"traffic_report.py: error: {exc}", file=sys.stderr)
        return 2
    paths = log_paths(args.logs or [DEFAULT_LOG_GLOB])
    if not paths:
        print(
            "traffic_report.py: no access logs found; pass a path/glob or enable Caddy logging",
            file=sys.stderr,
        )
        return 1
    report = analyze(paths, cutoff)
    data = report.as_dict(max(1, args.top))
    if args.json:
        print(json.dumps(data, indent=2, sort_keys=True))
    else:
        print_human(data, args.since, len(paths))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
