import datetime as dt
import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import traffic_report


def row(
    ts,
    path,
    *,
    ip="visitor-a",
    ua="Mozilla/5.0 Test Browser",
    status=200,
    referer="",
    size=100,
    duration=0.01,
):
    headers = {
        "User-Agent": [ua],
        "Cf-Connecting-Ip": [ip],
    }
    if referer:
        headers["Referer"] = [referer]
    return {
        "ts": ts,
        "request": {
            "method": "GET",
            "uri": path,
            "headers": headers,
            "remote_ip": "edge",
        },
        "status": status,
        "size": size,
        "duration": duration,
    }


class TrafficReportTests(unittest.TestCase):
    def test_counts_visitors_sessions_and_api_sources(self):
        base = dt.datetime(2026, 7, 25, tzinfo=dt.UTC).timestamp()
        report = traffic_report.TrafficReport()
        report.add(row(base, "/"))
        report.add(row(base + 60, "/guide", ip="visitor-a"))
        report.add(row(base + 1900, "/", ip="visitor-a"))
        report.add(row(base + 120, "/", ip="visitor-b"))
        report.add(
            row(
                base + 130,
                "/data/testnet-10-live.json",
                ip="visitor-b",
                referer="https://kascov.io/",
            )
        )
        report.add(
            row(
                base + 140,
                "/data/testnet-10/coin/" + "a" * 64,
                ip="api-client",
                ua="curl/8.0",
            )
        )
        report.add(row(base + 150, "/style.css", ip="visitor-b"))
        report.add(row(base + 160, "/health", ua="monitoring-probe/1.0"))
        report.add(row(base + 170, "/wp-login.php", ip="scanner"))
        data = report.as_dict(10)

        self.assertEqual(data["visitors"]["unique_browsers_approx"], 2)
        self.assertEqual(data["visitors"]["active_browsers_approx"], 2)
        self.assertEqual(data["visitors"]["sessions_30m"], 3)
        self.assertEqual(data["visitors"]["page_views"], 4)
        self.assertEqual(data["requests"]["api_calls"], 2)
        self.assertEqual(data["requests"]["first_party_api_calls"], 1)
        self.assertEqual(data["requests"]["external_api_calls_approx"], 1)
        self.assertEqual(data["requests"]["health_checks"], 1)
        self.assertEqual(data["requests"]["bot_requests"], 2)
        self.assertNotIn("/wp-login.php", data["top_pages"])
        self.assertIn("/data/testnet-10/coin/:id", data["top_api_endpoints"])

    def test_series_buckets_requests_without_exporting_visitor_keys(self):
        base = dt.datetime(2026, 7, 25, tzinfo=dt.UTC).timestamp()
        report = traffic_report.TrafficReport(bucket_seconds=60)
        report.add(row(base + 1, "/", ip="private-hash"))
        report.add(
            row(
                base + 61,
                "/data/testnet-10-live.json",
                ip="private-hash",
                referer="https://kascov.io/",
            )
        )
        report.add(row(base + 62, "/data/mainnet-live.json", ip="api-client", ua="curl/8"))
        data = report.as_dict(10)

        self.assertEqual(len(data["series"]), 2)
        self.assertEqual(data["series"][0]["page_views"], 1)
        self.assertEqual(data["series"][1]["api_calls"], 2)
        self.assertEqual(data["series"][1]["first_party_api_calls"], 1)
        self.assertEqual(data["series"][1]["external_api_calls"], 1)
        self.assertNotIn("private-hash", json.dumps(data))
        self.assertNotIn("api-client", json.dumps(data))

    def test_spa_fallback_does_not_turn_scanner_paths_into_page_views(self):
        base = dt.datetime(2026, 7, 25, tzinfo=dt.UTC).timestamp()
        report = traffic_report.TrafficReport()
        report.add(row(base, "/.git/config"))
        report.add(row(base + 1, "/phpinfo"))
        report.add(row(base + 2, "/decode"))
        data = report.as_dict(10)

        self.assertEqual(data["visitors"]["page_views"], 1)
        self.assertEqual(data["top_pages"], {"/decode": 1})

    def test_private_dashboard_requests_do_not_feed_back_into_traffic(self):
        base = dt.datetime(2026, 7, 25, tzinfo=dt.UTC).timestamp()
        report = traffic_report.TrafficReport()
        report.add(row(base, "/ops/traffic/"))
        report.add(row(base + 1, "/ops/traffic/traffic.json"))
        report.add(row(base + 2, "/"))

        self.assertEqual(report.as_dict(10)["requests"]["total"], 1)

    def test_analyze_filters_old_and_malformed_rows(self):
        cutoff = dt.datetime(2026, 7, 25, tzinfo=dt.UTC).timestamp()
        with tempfile.TemporaryDirectory() as temp:
            path = Path(temp) / "access.json"
            path.write_text(
                json.dumps(row(cutoff - 1, "/"))
                + "\nnot-json\n"
                + json.dumps(row(cutoff + 1, "/"))
                + "\n",
                encoding="utf-8",
            )
            report = traffic_report.analyze([path], cutoff)

        self.assertEqual(report.total_requests, 1)
        self.assertEqual(report.filtered_old_lines, 1)
        self.assertEqual(report.malformed_lines, 1)

    def test_since_duration_and_iso(self):
        now = dt.datetime(2026, 7, 25, tzinfo=dt.UTC)
        self.assertEqual(traffic_report.parse_since("24h", now), now.timestamp() - 86400)
        self.assertEqual(
            traffic_report.parse_since("2026-07-24T00:00:00Z", now),
            dt.datetime(2026, 7, 24, tzinfo=dt.UTC).timestamp(),
        )
        self.assertIsNone(traffic_report.parse_since("all", now))

    def test_endpoint_normalization_bounds_public_identifiers(self):
        self.assertEqual(
            traffic_report.normalize_path("/data/mainnet/c/" + "a" * 64 + ".json"),
            "/data/mainnet/c/:id.json",
        )
        self.assertEqual(
            traffic_report.normalize_path("/data/mainnet/addr/kaspa%3Along-address.json"),
            "/data/mainnet/addr/:address",
        )


if __name__ == "__main__":
    unittest.main()
