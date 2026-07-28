#!/usr/bin/env python3
"""Build the aggregate JSON consumed by kascov's private traffic dashboard.

The snapshot deliberately contains counts only. Hashed visitor keys stay
inside ``TrafficReport`` long enough to count browsers and are discarded
before this file is written.
"""

from __future__ import annotations

import argparse
import datetime as dt
import json
import os
import sys
import tempfile
from pathlib import Path

import traffic_report


DEFAULT_OUTPUT = "/mnt/c/kascov/web/ops/traffic/traffic.json"
DEFAULT_LONG_REFRESH_SECONDS = 15 * 60
WINDOWS = {
    "5m": {"seconds": 5 * 60, "bucket_seconds": 60},
    "24h": {"seconds": 24 * 60 * 60, "bucket_seconds": 15 * 60},
    "7d": {"seconds": 7 * 24 * 60 * 60, "bucket_seconds": 2 * 60 * 60},
    "30d": {"seconds": 30 * 24 * 60 * 60, "bucket_seconds": 6 * 60 * 60},
}
LONG_WINDOWS = ("7d", "30d")


def parse_iso(raw: str | None) -> dt.datetime | None:
    if not raw:
        return None
    try:
        parsed = dt.datetime.fromisoformat(raw.replace("Z", "+00:00"))
    except (TypeError, ValueError):
        return None
    return parsed if parsed.tzinfo else parsed.replace(tzinfo=dt.UTC)


def load_existing(path: Path) -> dict:
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
    except (FileNotFoundError, json.JSONDecodeError, OSError):
        return {}
    return data if isinstance(data, dict) else {}


def refresh_long_windows(existing: dict, now: dt.datetime, interval: int) -> bool:
    windows = existing.get("windows")
    if not isinstance(windows, dict) or any(name not in windows for name in LONG_WINDOWS):
        return True
    generated = parse_iso(existing.get("long_windows_generated_at"))
    if generated is None:
        return True
    return (now - generated).total_seconds() >= max(60, interval)


def candidate_paths(paths: list[Path], cutoff: float) -> list[Path]:
    """Skip rolls that cannot contain rows in the requested time window.

    Keep a one-day margin around file mtimes because Caddy rolls at midnight
    as well as by size. The current .json file is always retained.
    """
    margin = 24 * 60 * 60
    selected: list[Path] = []
    for path in paths:
        if path.name.endswith(".json"):
            selected.append(path)
            continue
        try:
            if path.stat().st_mtime >= cutoff - margin:
                selected.append(path)
        except OSError:
            continue
    return selected


def analyze_windows(
    paths: list[Path],
    now: dt.datetime,
    window_names: tuple[str, ...],
    top: int,
) -> dict[str, dict]:
    reports = {
        name: traffic_report.TrafficReport(WINDOWS[name]["bucket_seconds"])
        for name in window_names
    }
    cutoffs = {
        name: now.timestamp() - WINDOWS[name]["seconds"]
        for name in window_names
    }
    earliest = min(cutoffs.values())

    for path in candidate_paths(paths, earliest):
        with traffic_report.open_log(path) as handle:
            for line in handle:
                try:
                    row = json.loads(line)
                    timestamp = float(row.get("ts") or 0)
                except (json.JSONDecodeError, TypeError, ValueError):
                    for report in reports.values():
                        report.malformed_lines += 1
                    continue
                if not row.get("request"):
                    continue
                for name, report in reports.items():
                    if timestamp >= cutoffs[name]:
                        report.add(row)

    return {name: report.as_dict(top) for name, report in reports.items()}


def build_snapshot(
    paths: list[Path],
    now: dt.datetime,
    *,
    top: int = 12,
    existing: dict | None = None,
    long_refresh_seconds: int = DEFAULT_LONG_REFRESH_SECONDS,
) -> dict:
    existing = existing or {}
    refresh_long = refresh_long_windows(existing, now, long_refresh_seconds)
    names = tuple(WINDOWS) if refresh_long else ("5m", "24h")
    windows = analyze_windows(paths, now, names, max(1, top))

    long_generated = now
    if not refresh_long:
        old_windows = existing.get("windows") or {}
        for name in LONG_WINDOWS:
            windows[name] = old_windows[name]
        long_generated = parse_iso(existing.get("long_windows_generated_at")) or now

    return {
        "schema": 1,
        "generated_at": traffic_report.iso_time(now.timestamp()),
        "long_windows_generated_at": traffic_report.iso_time(long_generated.timestamp()),
        "refresh_seconds": 60,
        "privacy": {
            "aggregate_only": True,
            "visitor_identifiers_exported": False,
            "visitor_counting": "approximate; Caddy-hashed address plus browser requests",
        },
        "windows": {name: windows[name] for name in WINDOWS},
    }


def write_atomic(path: Path, data: dict) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = ""
    try:
        with tempfile.NamedTemporaryFile(
            "w",
            encoding="utf-8",
            dir=path.parent,
            prefix=f".{path.name}.",
            suffix=".tmp",
            delete=False,
        ) as handle:
            temporary = handle.name
            json.dump(data, handle, separators=(",", ":"), sort_keys=True)
            handle.write("\n")
        os.replace(temporary, path)
    finally:
        if temporary:
            try:
                Path(temporary).unlink()
            except FileNotFoundError:
                pass


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Generate aggregate JSON for the private kascov traffic dashboard."
    )
    parser.add_argument(
        "logs",
        nargs="*",
        help=f"log paths/globs (default: {traffic_report.DEFAULT_LOG_GLOB})",
    )
    parser.add_argument("--output", default=DEFAULT_OUTPUT, help="snapshot output path")
    parser.add_argument("--top", type=int, default=12, help="top paths per table")
    parser.add_argument(
        "--long-refresh",
        type=int,
        default=DEFAULT_LONG_REFRESH_SECONDS,
        help="seconds between 7d/30d rescans (default: 900)",
    )
    return parser


def main(argv: list[str] | None = None) -> int:
    args = build_parser().parse_args(argv)
    paths = traffic_report.log_paths(args.logs or [traffic_report.DEFAULT_LOG_GLOB])
    if not paths:
        print("traffic_snapshot.py: no Caddy access logs found", file=sys.stderr)
        return 1
    output = Path(args.output)
    now = dt.datetime.now(dt.UTC)
    snapshot = build_snapshot(
        paths,
        now,
        top=args.top,
        existing=load_existing(output),
        long_refresh_seconds=args.long_refresh,
    )
    write_atomic(output, snapshot)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
