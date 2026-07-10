"""kascov.py — a tiny zero-dependency client for the kascov JSON API.

Python 3.9+, stdlib only (urllib). CORS-open API, no keys.

    from kascov import Kascov
    k = Kascov("testnet-10")
    page = k.coins(limit=100)
    coin = k.coin(page["covenants"][0]["covenant_id"])
    for ev in k.stream():           # live events (SSE), blocks forever
        print(ev["kind"], ev["covenant_id"])

Publishing to PyPI is a separate decision — this file is the whole client.
"""
from __future__ import annotations

import json
import urllib.parse
import urllib.request
from typing import Any, Dict, Iterator, Optional

DEFAULT_BASE = "https://kascov.io"


class Kascov:
    def __init__(self, network: str = "mainnet", base: str = DEFAULT_BASE) -> None:
        self.network = network
        self.base = base.rstrip("/")

    def _get(self, path: str) -> Dict[str, Any]:
        req = urllib.request.Request(
            f"{self.base}{path}", headers={"accept": "application/json", "user-agent": "kascov-py"}
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

    def stream(self) -> Iterator[Dict[str, Any]]:
        """Live events (SSE) as an iterator. Hints only — refetch on receipt."""
        req = urllib.request.Request(
            f"{self.base}/data/{self.network}/stream",
            headers={"accept": "text/event-stream", "user-agent": "kascov-py"},
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
