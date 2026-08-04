"""kascov.py — a tiny zero-dependency client for the kascov JSON API.

Python 3.9+, stdlib only (urllib). CORS-open API, no keys.

    from kascov import Kascov
    k = Kascov("testnet-10")
    page = k.coins(limit=100)
    coin = k.coin(page["covenants"][0]["covenant_id"])
    for ev in k.stream():           # live events (SSE), blocks forever
        print(ev["kind"], ev["covenant_id"])
    for ev in k.stream(covenant=cid):   # one coin only

The API needs no token. An optional lane token (minted at kascov.io/lane)
rides along as an X-Kascov-Lane header on every request; it buys extra
capacity on the holder lane and nothing else — the anonymous tier keeps
working without it:

    k = Kascov("mainnet", lane_token="...")

Passport badges verify locally — no request, no trust in the server:

    from kascov import verify_badge
    verify_badge(claim, proof, root)  # True only if the claim is in the tree

Publishing to PyPI is a separate decision — this file is the whole client.
"""
from __future__ import annotations

import hashlib
import json
import re
import urllib.parse
import urllib.request
from typing import Any, Dict, Iterator, Optional, Sequence

DEFAULT_BASE = "https://kascov.io"


class Kascov:
    def __init__(
        self,
        network: str = "mainnet",
        base: str = DEFAULT_BASE,
        lane_token: Optional[str] = None,
    ) -> None:
        self.network = network
        self.base = base.rstrip("/")
        self.lane_token = lane_token

    def _headers(self, accept: str) -> Dict[str, str]:
        # every request goes through here so the lane header can never be
        # forgotten on one endpoint and sent on another
        h = {"accept": accept, "user-agent": "kascov-py"}
        if self.lane_token:
            h["X-Kascov-Lane"] = self.lane_token
        return h

    def _get(self, path: str) -> Dict[str, Any]:
        req = urllib.request.Request(
            f"{self.base}{path}", headers=self._headers("application/json")
        )
        with urllib.request.urlopen(req, timeout=60) as res:
            return json.load(res)

    def live(self) -> Dict[str, Any]:
        """Small fast feed: stats + chain tip + newest ~150 events."""
        return self._get(f"/data/{self.network}-live.json")

    def coins(
        self,
        limit: Optional[int] = None,
        after_daa: Optional[int] = None,
        after_id: Optional[str] = None,
    ) -> Dict[str, Any]:
        """One page of coin summaries, newest first. Pass the previous page's
        next_after_daa / next_after_id to walk older coins."""
        q = {k: v for k, v in {"limit": limit, "after_daa": after_daa, "after_id": after_id}.items() if v is not None}
        qs = f"?{urllib.parse.urlencode(q)}" if q else ""
        return self._get(f"/data/{self.network}.json{qs}")

    def coin(self, covenant_id: str) -> Dict[str, Any]:
        """One coin's full story: events, UTXOs (scripts/reveals), holders."""
        return self._get(f"/data/{self.network}/c/{covenant_id}.json")

    def tx(self, txid: str) -> Dict[str, Any]:
        """Which covenant(s) did this transaction move?"""
        return self._get(f"/data/{self.network}/tx/{txid}.json")

    def address(self, addr_or_pubkey: str) -> Dict[str, Any]:
        """Smart coins an address/pubkey funded, received, or controls."""
        return self._get(f"/data/{self.network}/addr/{urllib.parse.quote(addr_or_pubkey)}.json")

    def digest(self) -> Dict[str, Any]:
        """Last-24h digest: births/moves/burns, value born, headliners."""
        return self._get(f"/data/{self.network}/digest.json")

    def galaxy(self) -> Dict[str, Any]:
        """The whole-network app graph (positions + weighted edges)."""
        return self._get(f"/data/{self.network}/galaxy.json")

    def reorgs(self) -> Dict[str, Any]:
        """Recent chain reorgs the indexer rolled back through."""
        return self._get(f"/data/{self.network}/reorgs.json")

    def templates(self) -> Dict[str, Any]:
        """Contract-type analytics (what's running on this network)."""
        return self._get(f"/data/{self.network}/templates.json")

    def activity(self, range: str = "24h") -> Dict[str, Any]:
        """Births/moves/burns per DAA bucket. range: 1h|6h|24h|48h|all"""
        return self._get(f"/data/{self.network}/activity.json?range={range}")

    def stream_url(self, covenant: Optional[str] = None) -> str:
        """The SSE URL the worker actually serves: /data/{network}/stream, with
        an optional ?covenant=<64 hex> filter narrowing it to one coin.
        Extracted so tests pin the shape against the route in main.rs."""
        q = f"?covenant={urllib.parse.quote(covenant)}" if covenant else ""
        return f"{self.base}/data/{self.network}/stream{q}"

    def stream(self, covenant: Optional[str] = None) -> Iterator[Dict[str, Any]]:
        """Live events (SSE) as an iterator. Hints only — refetch on receipt.
        covenant: exactly 64 hex chars follows ONE coin; anything else is a
        400 from the server, never a silent firehose."""
        req = urllib.request.Request(
            self.stream_url(covenant),
            headers=self._headers("text/event-stream"),
        )
        with urllib.request.urlopen(req, timeout=None) as res:
            for raw in res:
                line = raw.decode("utf-8", "replace").strip()
                if line.startswith("data:"):
                    payload = line[5:].strip()
                    if payload:
                        try:
                            yield json.loads(payload)
                        except json.JSONDecodeError:
                            continue


# ---------------------------------------------------------------------------
# Passport badge verification — pure and local. The scheme (which the bot's
# merkle publisher in scripts/discord-holder-bot.mjs must match exactly — the
# publisher was not yet written when this landed, so this file is the spec
# both sides pin):
#
#     leaf  = sha256(canonical JSON of the claim)   # keys sorted, no spaces
#     node  = sha256(lo + hi)                       # the PAIR sorted as bytes
#     root  = the published 32-byte hex string
#
# Claims stick to strings, integers, booleans and None — floats serialize
# differently across languages and would fork the leaf hash.

_HEX64 = re.compile(r"^[0-9a-f]{64}$")


def canonical_json(value: Any) -> str:
    """Canonical JSON: recursively sorted keys, no whitespace, raw unicode.
    Matches the js client's canonicalJson for claims built from the types
    listed above."""
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


def verify_badge(claim: Any, proof: Sequence[str], root: str) -> bool:
    """Verify a passport badge against a published merkle root, entirely
    locally. claim: the claim object as published; proof: sibling hashes
    leaf->root (64-char hex each); root: the published root (64-char hex).
    An empty proof means the claim IS the whole tree. Malformed input returns
    False — the verifier fails closed, it never raises."""
    if not isinstance(root, str) or not isinstance(proof, (list, tuple)):
        return False
    want = root.lower()
    if not _HEX64.fullmatch(want):
        return False
    cur = hashlib.sha256(canonical_json(claim).encode("utf-8")).hexdigest()
    for step in proof:
        if not isinstance(step, str):
            return False
        sib = step.lower()
        if not _HEX64.fullmatch(sib):
            return False
        # pair-sorted concatenation: lexicographic hex order == byte order here
        lo, hi = sorted((cur, sib))
        cur = hashlib.sha256(bytes.fromhex(lo) + bytes.fromhex(hi)).hexdigest()
    return cur == want
