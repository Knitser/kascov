import datetime as dt
import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))
import traffic_snapshot
from test_traffic_report import row


class TrafficSnapshotTests(unittest.TestCase):
    def test_builds_all_windows_and_exports_aggregate_data_only(self):
        now = dt.datetime(2026, 7, 25, 12, tzinfo=dt.UTC)
        recent = now.timestamp() - 60
        old = now.timestamp() - 8 * 24 * 60 * 60
        with tempfile.TemporaryDirectory() as temp:
            log = Path(temp) / "kascov-access.json"
            log.write_text(
                json.dumps(row(recent, "/", ip="secret-browser"))
                + "\n"
                + json.dumps(
                    row(
                        recent + 1,
                        "/data/testnet-10-live.json",
                        ip="secret-browser",
                        referer="https://kascov.io/",
                    )
                )
                + "\n"
                + json.dumps(row(old, "/", ip="old-browser"))
                + "\n",
                encoding="utf-8",
            )
            snapshot = traffic_snapshot.build_snapshot([log], now)

        self.assertEqual(list(snapshot["windows"]), ["5m", "24h", "7d", "30d"])
        self.assertEqual(
            snapshot["windows"]["5m"]["visitors"]["active_browsers_approx"], 1
        )
        self.assertEqual(snapshot["windows"]["5m"]["requests"]["api_calls"], 1)
        self.assertEqual(snapshot["windows"]["30d"]["visitors"]["page_views"], 2)
        serialized = json.dumps(snapshot)
        self.assertNotIn("secret-browser", serialized)
        self.assertNotIn("old-browser", serialized)
        self.assertFalse(snapshot["privacy"]["visitor_identifiers_exported"])

    def test_reuses_long_windows_until_the_slow_refresh_is_due(self):
        now = dt.datetime(2026, 7, 25, 12, tzinfo=dt.UTC)
        with tempfile.TemporaryDirectory() as temp:
            log = Path(temp) / "kascov-access.json"
            log.write_text(
                json.dumps(row(now.timestamp() - 60, "/")) + "\n",
                encoding="utf-8",
            )
            first = traffic_snapshot.build_snapshot([log], now)
            later = now + dt.timedelta(minutes=2)
            log.write_text(
                log.read_text(encoding="utf-8")
                + json.dumps(row(later.timestamp() - 1, "/", ip="visitor-b"))
                + "\n",
                encoding="utf-8",
            )
            second = traffic_snapshot.build_snapshot(
                [log],
                later,
                existing=first,
                long_refresh_seconds=900,
            )

        self.assertEqual(
            second["long_windows_generated_at"], first["long_windows_generated_at"]
        )
        self.assertEqual(second["windows"]["30d"], first["windows"]["30d"])
        self.assertEqual(second["windows"]["5m"]["visitors"]["page_views"], 2)

    def test_atomic_writer_replaces_the_snapshot(self):
        with tempfile.TemporaryDirectory() as temp:
            output = Path(temp) / "nested" / "traffic.json"
            traffic_snapshot.write_atomic(output, {"schema": 1})
            self.assertEqual(json.loads(output.read_text(encoding="utf-8")), {"schema": 1})
            self.assertEqual(list(output.parent.glob("*.tmp")), [])


if __name__ == "__main__":
    unittest.main()
