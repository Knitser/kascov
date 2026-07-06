#!/usr/bin/env python3
"""Repair a covenant's truncated pre-history using archival REST data.

Walks the chain BACKWARDS from the coin's currently-first event to its true
genesis (every hop chain-verified: the earlier tx's output must carry the
covenant id, and the root must pass KIP-20 genesis recomputation via the
node's own consensus function). Then rewrites the coin's events + utxos in
one transaction: real genesis, real transitions, complete lineage.

Usage: repair-lineage.py <db> <network-rest-base> <covenant_id_hex> [--apply]
"""
import json, pathlib, sqlite3, ssl, subprocess, sys, time, urllib.request

DB, REST, CID = sys.argv[1], sys.argv[2], sys.argv[3].lower()
APPLY = '--apply' in sys.argv
# framework pythons often lack a CA bundle; these are public read-only APIs
CTX = ssl.create_default_context(); CTX.check_hostname = False; CTX.verify_mode = ssl.CERT_NONE
# repo root = two levels up from this script (scripts/repair-lineage.py)
REPO = str(pathlib.Path(__file__).resolve().parent.parent)

def get(url):
    last = None
    for a in range(5):
        try:
            req = urllib.request.Request(url, headers={'User-Agent': 'kascov-repair'})
            with urllib.request.urlopen(req, timeout=30, context=CTX) as r:
                return json.loads(r.read())
        except Exception as e:
            last = e; time.sleep(2 + 2 * a)
    raise last

TX = {}
def tx(txid):
    if txid not in TX:
        TX[txid] = get(f"{REST}/transactions/{txid}?inputs=true&outputs=true&resolve_previous_outpoints=no")
        time.sleep(0.15)
    return TX[txid]

BLK = {}
def block_daa(bhash):
    if bhash not in BLK:
        b = get(f"{REST}/blocks/{bhash}?includeTransactions=false")
        BLK[bhash] = int(b['header']['daaScore'])
        time.sleep(0.1)
    return BLK[bhash]

db = sqlite3.connect(DB)
cid_blob = bytes.fromhex(CID)
first = db.execute(
    "SELECT seq, kind, lower(hex(txid)), lower(hex(accepting_block)), accepting_daa FROM covenant_events WHERE covenant_id=? ORDER BY seq LIMIT 1",
    (cid_blob,)).fetchone()
if not first:
    sys.exit(f"unknown covenant {CID[:12]}")
seq0, kind0, tx0, blk0, daa0 = first
print(f"current first event: seq{seq0} {kind0} tx {tx0[:16]}… daa {daa0}")

# ---- walk backwards ----
chain = []  # oldest last while walking; each: {txid, bound_outs:[(idx,val,script)], spent_by, accepting_block, daa}
cur = tx0
guard = 0
while True:
    guard += 1
    if guard > 50: sys.exit("walked 50 hops — refusing (suspicious)")
    t = tx(cur)
    bound_in = None
    for i in t.get('inputs') or []:
        prev = tx(i['previous_outpoint_hash'])
        pidx = int(i['previous_outpoint_index'])
        po = next((o for o in prev.get('outputs') or [] if int(o['index']) == pidx), None)
        if po and (po.get('covenant_id') or '').lower() == CID:
            bound_in = (i['previous_outpoint_hash'], pidx, i)
            break
    if not bound_in:
        root = cur
        break
    prev_txid = bound_in[0]
    pt = tx(prev_txid)
    if not pt.get('is_accepted'):
        sys.exit(f"hop {prev_txid[:16]} not accepted — refusing")
    chain.append(prev_txid)
    cur = prev_txid

if not chain:
    sys.exit("first event is already the chain root — nothing to repair")
chain.reverse()  # oldest first now
print(f"missing prefix: {len(chain)} tx(s), root {chain[0][:16]}…")

# ---- verify the root is a true KIP-20 genesis ----
rt = tx(chain[0])
bound = [(int(o['index']), int(o['amount']), o['script_public_key']) for o in rt['outputs']
         if (o.get('covenant_id') or '').lower() == CID]
auths = {o.get('covenant_authorizing_input') for o in rt['outputs'] if (o.get('covenant_id') or '').lower() == CID}
assert len(auths) == 1, f"mixed authorizing inputs {auths}"
auth = list(auths)[0]
ain = (rt.get('inputs') or [])[int(auth)]
outpoint = f"{ain['previous_outpoint_hash']}:{ain['previous_outpoint_index']}"
args = [f"{i}:{v}:{s}" for i, v, s in bound]
out = subprocess.run(
    ['cargo', 'run', '-q', '-p', 'kascov-core', '--example', 'covid', '--', outpoint, *args],
    capture_output=True, text=True, cwd=REPO)
computed = out.stdout.strip().splitlines()[-1] if out.stdout else ''
print(f"root genesis recomputation: {computed[:16]}… vs {CID[:16]}… -> {'VALID ✓' if computed == CID else 'INVALID ✗'}")
if computed != CID:
    sys.exit("root does not recompute — the walk found the wrong root; refusing to write")

# ---- build rows ----
full = chain + [None]  # None marks the existing-first-event boundary
events, utxos = [], []
for hop, txid in enumerate(chain):
    t = tx(txid)
    ab = t['accepting_block_hash']
    daa = block_daa(ab)
    kind = 'genesis' if hop == 0 else 'transition'
    events.append((hop, kind, txid, ab, daa))
    spender = chain[hop + 1] if hop + 1 < len(chain) else tx0
    st = tx(spender)
    for o in t['outputs']:
        if (o.get('covenant_id') or '').lower() != CID: continue
        oidx = int(o['index'])
        sin = next((i for i in st.get('inputs') or []
                    if i['previous_outpoint_hash'] == txid and int(i['previous_outpoint_index']) == oidx), None)
        if sin is None:
            sys.exit(f"output {txid[:12]}:{oidx} not spent by expected next hop — refusing")
        utxos.append({
            'txid': txid, 'oidx': oidx, 'value': int(o['amount']),
            'script': o['script_public_key'],         'created_block': t['accepting_block_hash'],
            'created_daa': daa, 'spent_block': st['accepting_block_hash'],
            'spent_txid': spender, 'spent_sig': sin.get('signature_script') or '',
            'spent_budget': sin.get('compute_budget'),
        })

print("prefix events:")
for e in events: print(f"   seq{e[0]} {e[1]:10s} tx {e[2][:16]}… daa {e[4]}")
print(f"historical utxos to add: {len(utxos)}")

if not APPLY:
    print("(dry run — pass --apply to write)"); sys.exit(0)

n = len(events)
cur = db.cursor()
cur.execute("BEGIN")
# two-step shift via negative temp values: an in-place seq+n UPDATE can
# transiently collide with its own unshifted rows (PK covenant_id+seq)
cur.execute("UPDATE covenant_events SET seq = -(seq + ?) - 1 WHERE covenant_id = ?", (n, cid_blob))
cur.execute("UPDATE covenant_events SET seq = -seq - 1 WHERE covenant_id = ? AND seq < 0", (cid_blob,))
for seq, kind, txid, ab, daa in events:
    cur.execute(
        "INSERT INTO covenant_events (covenant_id, seq, kind, txid, accepting_block, accepting_daa, payload) VALUES (?,?,?,?,?,?,NULL)",
        (cid_blob, seq, kind, bytes.fromhex(txid), bytes.fromhex(ab), daa))
for u in utxos:
    cur.execute(
        """INSERT OR REPLACE INTO covenant_utxos
           (txid, output_index, covenant_id, value, spk_version, spk_script, created_block, created_daa,
            spent_block, spent_txid, spent_sig, spent_budget, template, revealed_template)
           VALUES (?,?,?,?,0,?,?,?,?,?,?,?,NULL,NULL)""",
        (bytes.fromhex(u['txid']), u['oidx'], cid_blob, u['value'], bytes.fromhex(u['script']),
         bytes.fromhex(u['created_block']), u['created_daa'], bytes.fromhex(u['spent_block']),
         bytes.fromhex(u['spent_txid']), bytes.fromhex(u['spent_sig']) if u['spent_sig'] else None,
         u['spent_budget']))
root_t = tx(chain[0])
cur.execute(
    "UPDATE covenants SET genesis_txid=?, genesis_daa=?, lineage_complete=1, event_count=event_count+? WHERE covenant_id=?",
    (bytes.fromhex(chain[0]), events[0][4], n, cid_blob))
# the previously-first event was labeled genesis by pre-validation code; it is a transition
cur.execute(
    "UPDATE covenant_events SET kind='transition' WHERE covenant_id=? AND seq=? AND kind='genesis'",
    (cid_blob, n))
cur.execute("COMMIT")
print("APPLIED ✓")
row = db.execute("SELECT genesis_daa, event_count FROM covenants WHERE covenant_id=?", (cid_blob,)).fetchone()
print(f"covenant now: genesis_daa={row[0]}, events={row[1]}")
