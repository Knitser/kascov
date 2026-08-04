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
from kascov import Kascov, canonical_json, verify_badge

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


class BadgeVerification(unittest.TestCase):
    """The scheme here is the spec the bot's merkle publisher
    (scripts/discord-holder-bot.mjs) must match: sha256 over canonical
    sorted-key JSON for the leaf, pair-sorted concatenation up the tree.
    The tree is built with hashlib, independently of the client, and the
    fixture literals are shared with kascov.test.mjs so the python and js
    implementations cannot drift apart."""

    CLAIMS = [
        {"address": "kaspa:qq0badge0", "role": "verified-holder", "since": 1},
        {"address": "kaspa:qq1badge1", "role": "auditor", "since": 2},
        {"address": "kaspa:qq2badge2", "role": "voter", "since": 3},
        {"address": "kaspa:qq3badge3", "role": "watchtower", "since": 4},
    ]
    # computed once with node:crypto; pinned verbatim in both clients' tests
    FIXTURE_PROOF_0 = [
        "66d4d1b4cf99b94342e50ad8f7703a045562bf8b129e7e7a1a9ecffa0f4e9a04",
        "8825df4c9aac329f511ffd1f81a0934f1524667fe0d5f02ef8ff974cfb579e7d",
    ]
    FIXTURE_ROOT = "064e13348496f570aa3b2d0a9fa33b25f7be66842bef99c7dbba9a4a65e95d4b"

    @staticmethod
    def _sha(data: bytes) -> str:
        import hashlib

        return hashlib.sha256(data).hexdigest()

    @classmethod
    def _pair(cls, a: str, b: str) -> str:
        lo, hi = sorted((a, b))
        return cls._sha(bytes.fromhex(lo) + bytes.fromhex(hi))

    @classmethod
    def _tree(cls):
        leaves = [cls._sha(canonical_json(c).encode("utf-8")) for c in cls.CLAIMS]
        n01 = cls._pair(leaves[0], leaves[1])
        n23 = cls._pair(leaves[2], leaves[3])
        return leaves, n01, n23, cls._pair(n01, n23)

    def test_canonical_json_pins_the_exact_wire_bytes(self):
        self.assertEqual(
            canonical_json(self.CLAIMS[0]),
            '{"address":"kaspa:qq0badge0","role":"verified-holder","since":1}',
        )
        # key order in the source dict must not matter
        scrambled = {"since": 1, "role": "verified-holder", "address": "kaspa:qq0badge0"}
        self.assertEqual(canonical_json(scrambled), canonical_json(self.CLAIMS[0]))
        self.assertEqual(
            canonical_json({"b": [1, {"z": None, "a": True}]}),
            '{"b":[1,{"a":true,"z":null}]}',
        )

    def test_known_good_badge_round_trips_for_every_leaf(self):
        leaves, n01, n23, root = self._tree()
        proofs = [
            [leaves[1], n23],
            [leaves[0], n23],
            [leaves[3], n01],
            [leaves[2], n01],
        ]
        for i, claim in enumerate(self.CLAIMS):
            self.assertTrue(verify_badge(claim, proofs[i], root), f"leaf {i}")

    def test_empty_proof_means_the_claim_is_the_whole_tree(self):
        root = self._sha(canonical_json(self.CLAIMS[0]).encode("utf-8"))
        self.assertTrue(verify_badge(self.CLAIMS[0], [], root))
        self.assertFalse(verify_badge(self.CLAIMS[1], [], root))

    def test_tampering_rejects(self):
        leaves, n01, n23, root = self._tree()
        proof = [leaves[1], n23]
        self.assertTrue(verify_badge(self.CLAIMS[0], proof, root), "baseline")
        forged = dict(self.CLAIMS[0], role="auditor")
        self.assertFalse(verify_badge(forged, proof, root))
        self.assertFalse(verify_badge(self.CLAIMS[0], [leaves[2], n23], root))
        flipped = ("1" if root[0] == "0" else "0") + root[1:]
        self.assertFalse(verify_badge(self.CLAIMS[0], proof, flipped))
        self.assertFalse(verify_badge(self.CLAIMS[0], proof[:1], root))

    def test_malformed_input_fails_closed_never_raises(self):
        _, _, _, root = self._tree()
        self.assertFalse(verify_badge(self.CLAIMS[0], "not-a-list", root))
        self.assertFalse(verify_badge(self.CLAIMS[0], [], "too-short"))
        self.assertFalse(verify_badge(self.CLAIMS[0], [42], root))
        self.assertFalse(verify_badge(self.CLAIMS[0], ["zz" * 32], root))
        self.assertFalse(verify_badge(self.CLAIMS[0], [], None))

    def test_cross_client_fixture_verifies(self):
        # the same literals live in kascov.test.mjs
        self.assertTrue(
            verify_badge(self.CLAIMS[0], self.FIXTURE_PROOF_0, self.FIXTURE_ROOT)
        )
        _, _, _, root = self._tree()
        self.assertEqual(root, self.FIXTURE_ROOT, "rebuilt tree matches the pinned root")
        # uppercase hex is accepted — the wire may shout, the bytes are the same
        self.assertTrue(
            verify_badge(
                self.CLAIMS[0],
                [h.upper() for h in self.FIXTURE_PROOF_0],
                self.FIXTURE_ROOT.upper(),
            )
        )


if __name__ == "__main__":
    unittest.main()
