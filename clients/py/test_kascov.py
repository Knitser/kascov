"""The thin client's one real contract is the URLs it constructs: every method
must hit a route the worker actually registers (main.rs), and the SSE URL in
particular has been wrong before — so its exact shape is pinned here, with and
without the per-covenant filter. The optional lane token must ride on EVERY
request or on none.

Run:  python3 clients/py/test_kascov.py
"""
import json
import sys
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).parent))
from kascov import Kascov

COVENANT = "a" * 64


class FakeResponse:
    """Just enough of urlopen's return: context manager + read (json.load)
    + line iteration (the SSE stream)."""

    def __init__(self, body=b"{}", lines=()):
        self.body = body
        self.lines = list(lines)

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        return False

    def read(self, *a):
        return self.body

    def __iter__(self):
        return iter(self.lines)


def capture(client_call, response=None):
    """Run one client call against a mocked urlopen; return the Request."""
    seen = {}

    def fake_urlopen(req, timeout=None):
        seen["req"] = req
        return response or FakeResponse()

    with mock.patch("urllib.request.urlopen", fake_urlopen):
        result = client_call()
        # a stream() call is lazy — drain it so the request actually happens
        if not isinstance(result, dict):
            list(result)
    return seen["req"]


class StreamUrlShape(unittest.TestCase):
    def test_matches_the_live_route(self):
        # main.rs registers .route("/data/{network}/stream", get(stream_handler))
        self.assertEqual(
            Kascov("testnet-10").stream_url(),
            "https://kascov.io/data/testnet-10/stream",
        )

    def test_covenant_filter_is_a_query_param_not_a_path_segment(self):
        # the worker takes the filter as ?covenant=; a path-shaped guess 404s
        self.assertEqual(
            Kascov("mainnet").stream_url(COVENANT),
            f"https://kascov.io/data/mainnet/stream?covenant={COVENANT}",
        )

    def test_custom_base_keeps_the_shape(self):
        self.assertEqual(
            Kascov("testnet-10", "http://localhost:8080/").stream_url(),
            "http://localhost:8080/data/testnet-10/stream",
        )

    def test_stream_actually_requests_the_pinned_url(self):
        k = Kascov("testnet-10")
        req = capture(lambda: k.stream(covenant=COVENANT))
        self.assertEqual(
            req.full_url,
            f"https://kascov.io/data/testnet-10/stream?covenant={COVENANT}",
        )


class SseParsing(unittest.TestCase):
    def test_data_lines_parse_and_comments_are_skipped(self):
        lines = [
            b": connected\n",
            b'data: {"kind":"birth","covenant_id":"' + COVENANT.encode() + b'"}\n',
            b": ka\n",
            b'data: {"kind":"move"}\n',
        ]
        with mock.patch(
            "urllib.request.urlopen", lambda req, timeout=None: FakeResponse(lines=lines)
        ):
            events = list(Kascov("testnet-10").stream())
        self.assertEqual([e["kind"] for e in events], ["birth", "move"])
        self.assertEqual(events[0]["covenant_id"], COVENANT)


class LaneToken(unittest.TestCase):
    def test_absent_by_default_on_every_request(self):
        k = Kascov("mainnet")
        for call in (k.live, k.stream):
            req = capture(call)
            self.assertIsNone(req.get_header("X-kascov-lane"))

    def test_rides_on_plain_gets_and_the_stream_when_set(self):
        # the token is minted at kascov.io/lane; capacity only, never verdicts
        k = Kascov("mainnet", lane_token="tok-123")
        for call in (k.live, k.stream):
            req = capture(call)
            self.assertEqual(req.get_header("X-kascov-lane"), "tok-123")


class PollingRoutes(unittest.TestCase):
    def test_endpoints_hit_their_registered_routes(self):
        k = Kascov("testnet-10")
        page = FakeResponse(body=json.dumps({"covenants": []}).encode())
        cases = [
            (lambda: k.live(), "https://kascov.io/data/testnet-10-live.json"),
            (
                lambda: k.coins(limit=5, after_daa=9, after_id="ff"),
                "https://kascov.io/data/testnet-10.json?limit=5&after_daa=9&after_id=ff",
            ),
            (
                lambda: k.coin(COVENANT),
                f"https://kascov.io/data/testnet-10/c/{COVENANT}.json",
            ),
        ]
        for call, url in cases:
            req = capture(call, response=page)
            self.assertEqual(req.full_url, url)


if __name__ == "__main__":
    unittest.main()
