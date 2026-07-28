//! Reading a launchpad's published token list, and checking it against chain.
//!
//! Covenant tokens carry no name. KCC-0020 gives a token an identity and an
//! amount, and nothing else, so kascov calls one `patient-copper-lemur` while
//! its holders call it KASBTC. Some deployers write a KCC-0021 object into the
//! genesis payload and kascov reads the name from chain; the launchpads that
//! predate that convention publish a list instead.
//!
//! A published list is somebody's word, and kascov's promise is that it never
//! takes a third party's word for a fact. That promise is kept here by never
//! asking the list for a fact. A list entry carries, alongside the name, a set
//! of structural claims: which transaction was the genesis, which covenant
//! holds the curve inventory, which key took the genesis allocation. Every one
//! of those kascov already proved from chain, so every one can be checked. What
//! survives is a name and a logo, which nothing can verify and which are
//! therefore shown as a claim, attributed and dated, exactly like a name read
//! out of a genesis payload.
//!
//! The checking is the point. A list that agrees with the chain on the parts
//! that can be tested has earned a reader's attention for the parts that
//! cannot; a list that disagrees is worth surfacing loudly. Both mismatches
//! observed on mainnet were the chain being MORE informative than the list, not
//! the list being wrong: one creator had sold the allocation the list still
//! credits them with, and one token had graduated off the curve the list names.
//! That is the useful direction, and it only exists because the check runs.

use anyhow::Result;
use serde::Serialize;

/// Where the list is fetched from. Overridable so an operator can point kascov
/// at a mirror or at their own list without a rebuild; the shape is the
/// tokenlists.org convention plus a covenant-specific `extensions` object.
const DEFAULT_LIST_URL: &str = "https://api.kron.technology/api/registry/tokenlist";

/// A hostile or broken endpoint must not be able to spend our memory. The
/// observed list is a few tens of KB.
const MAX_LIST_BYTES: usize = 512 * 1024;

const FETCH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// How long a successful fetch is reused. The published list sends
/// `cache-control: max-age=60`, so polling faster only burns their bandwidth.
pub const LIST_TTL_OK: std::time::Duration = std::time::Duration::from_secs(300);
/// A failure is retried sooner than a success is refreshed, but not so fast
/// that an outage on their side turns into a request flood from ours.
pub const LIST_TTL_ERR: std::time::Duration = std::time::Duration::from_secs(60);

/// The result of testing one published statement against kascov's own index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Checked {
    /// The list says something and the chain agrees.
    Match,
    /// The list says something and the chain says otherwise. Not necessarily
    /// the list lying: it is also what a stale list looks like, and what a
    /// token that has moved on since publication looks like.
    Differ,
    /// The list does not make this statement, so there is nothing to test.
    NotStated,
    /// kascov cannot test it, because it has not proven the corresponding fact
    /// itself. Never counted as agreement.
    Unproven,
}

/// One entry of a published list, reduced to what kascov can either verify or
/// display. Unknown fields are ignored rather than rejected, so a list that
/// grows new fields keeps working.
#[derive(Clone, Debug, Default)]
pub struct ListedToken {
    pub covenant_id: String,
    pub name: Option<String>,
    pub ticker: Option<String>,
    pub image: Option<String>,
    pub decimals: Option<u8>,
    pub genesis_txid: Option<String>,
    pub curve_covenant_id: Option<String>,
    pub pool_covenant_id: Option<String>,
    pub creator_pubkey: Option<String>,
}

/// What kascov proved about a covenant on its own, ahead of reading anybody's
/// list. Assembled from the store so the comparison itself stays pure.
#[derive(Clone, Debug, Default)]
pub struct ChainFacts {
    /// kascov has this covenant indexed as a KCC20 token.
    pub known: bool,
    pub genesis_txid: Option<String>,
    /// Owner keys of record, as `hex(identifier_type || owner_identifier)`.
    pub owners: Vec<String>,
    /// Owners written at genesis, same encoding. A creator key that has since
    /// been spent away is here but not in `owners`.
    pub genesis_owners: Vec<String>,
}

/// One entry after checking. `name`/`ticker`/`image` are carried through
/// unverified by construction: no chain fact exists to test them against.
#[derive(Clone, Debug, Serialize)]
pub struct CheckedEntry {
    pub covenant_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ticker: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decimals: Option<u8>,
    /// False when kascov has never seen this covenant, in which case every
    /// check below is `Unproven` and the name is shown for nothing.
    pub known: bool,
    pub genesis_txid: Checked,
    pub curve_covenant: Checked,
    pub creator_key: Checked,
    /// True only when every statement the list made was testable and agreed.
    pub all_checks_passed: bool,
}

/// A covenant id or txid as this module compares them: 64 lowercase hex chars.
fn norm_id(s: &str) -> Option<String> {
    let s = s.trim().to_ascii_lowercase();
    (s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())).then_some(s)
}

/// An x-only pubkey as the list publishes it: 64 lowercase hex chars.
fn norm_key(s: &str) -> Option<String> {
    norm_id(s)
}

/// Same rule KCC-0021 imposes on a genesis `image`, applied to a listed one so
/// a scheme that could execute never reaches the page from this path either.
fn safe_image(u: &str) -> Option<String> {
    let u = u.trim();
    let lower = u.to_ascii_lowercase();
    (u.chars().count() <= 256
        && !u.chars().any(|c| c.is_control())
        && (lower.starts_with("https://") || lower.starts_with("ipfs://")))
    .then(|| u.to_string())
}

fn clean_text(s: &str, max: usize) -> Option<String> {
    let s = s.trim();
    (!s.is_empty() && s.chars().count() <= max && !s.chars().any(|c| c.is_control()))
        .then(|| s.to_string())
}

/// Parse the published document. Returns the entries for `network` only: a
/// list is published per network and mixing them would attach a mainnet name
/// to a testnet covenant that happens to share an id prefix.
pub fn parse_list(body: &str, network: &str) -> Result<Vec<ListedToken>> {
    let doc: serde_json::Value = serde_json::from_str(body)?;
    if let Some(n) = doc.get("network").and_then(|v| v.as_str()) {
        if !n.eq_ignore_ascii_case(network) {
            anyhow::bail!("list is for network {n}, not {network}");
        }
    }
    let Some(items) = doc.get("tokens").and_then(|v| v.as_array()) else {
        anyhow::bail!("list has no `tokens` array");
    };
    let mut out = Vec::with_capacity(items.len());
    for t in items {
        // An entry naming no covenant cannot be attached to anything.
        let Some(covenant_id) = t.get("covenantId").and_then(|v| v.as_str()).and_then(norm_id)
        else {
            continue;
        };
        let ext = t.get("extensions");
        let ext_str = |k: &str| {
            ext.and_then(|e| e.get(k)).and_then(|v| v.as_str()).map(str::to_string)
        };
        out.push(ListedToken {
            covenant_id,
            name: t.get("name").and_then(|v| v.as_str()).and_then(|s| clean_text(s, 48)),
            ticker: t.get("symbol").and_then(|v| v.as_str()).and_then(|s| clean_text(s, 12)),
            image: t.get("logoURI").and_then(|v| v.as_str()).and_then(safe_image),
            decimals: t.get("decimals").and_then(|v| v.as_u64()).filter(|d| *d <= 255).map(|d| d as u8),
            genesis_txid: ext_str("genesisTxid").as_deref().and_then(norm_id),
            curve_covenant_id: ext_str("curveCovenantId").as_deref().and_then(norm_id),
            pool_covenant_id: ext_str("poolCovenantId").as_deref().and_then(norm_id),
            creator_pubkey: ext_str("creatorPubkey").as_deref().and_then(norm_key),
        })
    }
    Ok(out)
}

/// Test one entry's structural statements against what kascov proved itself.
/// Pure: every chain fact is supplied by the caller, so the comparison can be
/// exercised without a database.
pub fn check(entry: &ListedToken, facts: &ChainFacts) -> CheckedEntry {
    let owns = |id: &str| {
        // Owner keys are `identifier_type || owner_identifier`; a covenant
        // owner carries type 0x02, so the id sits in the tail.
        facts.owners.iter().any(|o| o.len() == 66 && &o[2..] == id)
    };
    let genesis_txid = match (&entry.genesis_txid, &facts.genesis_txid) {
        (None, _) => Checked::NotStated,
        (Some(_), None) => Checked::Unproven,
        (Some(a), Some(b)) => {
            if a == b {
                Checked::Match
            } else {
                Checked::Differ
            }
        }
    };
    // A launch names a curve covenant; once it graduates, a pool covenant
    // holds the inventory instead. Either standing as the covenant owner of
    // record satisfies the claim, and a token whose inventory is fully sold
    // has neither, which is why an absent owner is Unproven rather than Differ.
    let curve_covenant = match (&entry.curve_covenant_id, &entry.pool_covenant_id) {
        (None, None) => Checked::NotStated,
        (curve, pool) => {
            let stated: Vec<&String> = curve.iter().chain(pool.iter()).collect();
            if !facts.known {
                Checked::Unproven
            } else if stated.iter().any(|id| owns(id)) {
                Checked::Match
            } else if facts.owners.iter().any(|o| o.starts_with("02")) {
                // Some covenant holds the inventory, and it is not the one named.
                Checked::Differ
            } else {
                Checked::Unproven
            }
        }
    };
    // The creator key is checked against the GENESIS allocation, not against
    // who holds tokens now: a creator who has sold is still the creator, and
    // testing the live balance would report that honest history as a mismatch.
    let creator_key = match &entry.creator_pubkey {
        None => Checked::NotStated,
        Some(key) => {
            if facts.genesis_owners.is_empty() {
                Checked::Unproven
            } else if facts.genesis_owners.iter().any(|o| o.len() == 66 && &o[2..] == key) {
                Checked::Match
            } else {
                Checked::Differ
            }
        }
    };
    let checks = [genesis_txid, curve_covenant, creator_key];
    // "Passed" requires at least one real test: an entry that stated nothing
    // testable has not earned the label by making no claims.
    let all_checks_passed = facts.known
        && checks.iter().any(|c| *c == Checked::Match)
        && checks.iter().all(|c| matches!(c, Checked::Match | Checked::NotStated));
    CheckedEntry {
        covenant_id: entry.covenant_id.clone(),
        name: entry.name.clone(),
        ticker: entry.ticker.clone(),
        image: entry.image.clone(),
        decimals: entry.decimals,
        known: facts.known,
        genesis_txid,
        curve_covenant,
        creator_key,
        all_checks_passed,
    }
}

/// Fetch the published list. Bounded in time and size; every failure is just
/// an error, because this feature degrades to showing nothing.
pub async fn fetch_list(client: &reqwest::Client) -> Result<String> {
    let url = std::env::var("KASCOV_REGISTRY_URL")
        .unwrap_or_else(|_| DEFAULT_LIST_URL.to_string());
    let res = client
        .get(&url)
        .header(reqwest::header::ACCEPT, "application/json")
        .timeout(FETCH_TIMEOUT)
        .send()
        .await?;
    if !res.status().is_success() {
        anyhow::bail!("list endpoint returned {}", res.status());
    }
    // Content-Length is a hint, not a promise, so the body is also capped as
    // it is read.
    if res.content_length().is_some_and(|n| n as usize > MAX_LIST_BYTES) {
        anyhow::bail!("list is larger than {MAX_LIST_BYTES} bytes");
    }
    let bytes = res.bytes().await?;
    if bytes.len() > MAX_LIST_BYTES {
        anyhow::bail!("list is larger than {MAX_LIST_BYTES} bytes");
    }
    Ok(String::from_utf8(bytes.to_vec())?)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CURVE: &str = "bd1a035f502f62053c4a9aea894b0f1ac2c4138fe2c5701bddfd186b32f57cf6";
    const TOKEN: &str = "caa73fcc3081b1477f14ea03e45af81e21b1371dde7b2c45871eae0384e52bce";
    const GENESIS: &str = "227008606c3ce1e32614702075c53086d657274977c14dbf4e16f19cf36d043f";
    const CREATOR: &str = "af84fec297a9650ad6d6bafcf2c8bf33a75b2cde7f42dae77746164fba57144e";

    fn entry() -> ListedToken {
        ListedToken {
            covenant_id: TOKEN.into(),
            name: Some("PUBIC HAIR".into()),
            ticker: Some("PUBIC".into()),
            genesis_txid: Some(GENESIS.into()),
            curve_covenant_id: Some(CURVE.into()),
            creator_pubkey: Some(CREATOR.into()),
            ..Default::default()
        }
    }

    fn facts() -> ChainFacts {
        ChainFacts {
            known: true,
            genesis_txid: Some(GENESIS.into()),
            owners: vec![format!("02{CURVE}"), format!("03{CREATOR}")],
            genesis_owners: vec![format!("02{CURVE}"), format!("03{CREATOR}")],
        }
    }

    #[test]
    fn an_entry_that_agrees_with_the_chain_passes_every_check() {
        let c = check(&entry(), &facts());
        assert!(c.known);
        assert_eq!(c.genesis_txid, Checked::Match);
        assert_eq!(c.curve_covenant, Checked::Match);
        assert_eq!(c.creator_key, Checked::Match);
        assert!(c.all_checks_passed);
    }

    /// The mainnet case that made this worth building: a creator who has sold
    /// the allocation they were given at genesis. The list still names them,
    /// and it is still right, so checking against the live balance would report
    /// honest history as a lie.
    #[test]
    fn a_creator_who_sold_still_matches() {
        let mut f = facts();
        f.owners.retain(|o| !o.ends_with(CREATOR)); // sold, gone from the frontier
        let c = check(&entry(), &f);
        assert_eq!(c.creator_key, Checked::Match, "genesis is what fixes the creator");
        assert!(c.all_checks_passed);
    }

    /// The other mainnet case: a token that has graduated, so a pool covenant
    /// holds the inventory the list still attributes to the curve.
    #[test]
    fn a_graduated_token_matches_on_its_pool() {
        const POOL: &str = "10f2155ebcd2e0cd55cc9354f78d43fa4e8b9f93de98683919d1f490da726c4f";
        let mut e = entry();
        e.pool_covenant_id = Some(POOL.into());
        let mut f = facts();
        f.owners = vec![format!("02{POOL}")];
        assert_eq!(check(&e, &f).curve_covenant, Checked::Match);
    }

    #[test]
    fn a_wrong_statement_is_reported_rather_than_ignored() {
        let mut e = entry();
        e.genesis_txid = Some("11".repeat(32));
        let c = check(&e, &facts());
        assert_eq!(c.genesis_txid, Checked::Differ);
        assert!(!c.all_checks_passed);

        let mut e = entry();
        e.creator_pubkey = Some("22".repeat(32));
        assert_eq!(check(&e, &facts()).creator_key, Checked::Differ);
    }

    /// Saying nothing testable must never read as having been verified.
    #[test]
    fn an_entry_stating_nothing_earns_nothing() {
        let bare = ListedToken {
            covenant_id: TOKEN.into(),
            name: Some("Anything At All".into()),
            ..Default::default()
        };
        let c = check(&bare, &facts());
        assert_eq!(c.genesis_txid, Checked::NotStated);
        assert_eq!(c.curve_covenant, Checked::NotStated);
        assert_eq!(c.creator_key, Checked::NotStated);
        assert!(!c.all_checks_passed, "an entry that claims nothing proves nothing");
    }

    /// A covenant kascov has never indexed cannot be vouched for at all, no
    /// matter how confident the list is.
    #[test]
    fn an_unknown_covenant_passes_nothing() {
        let c = check(&entry(), &ChainFacts::default());
        assert!(!c.known);
        assert_eq!(c.curve_covenant, Checked::Unproven);
        assert_eq!(c.creator_key, Checked::Unproven);
        assert!(!c.all_checks_passed);
    }

    #[test]
    fn parsing_keeps_only_what_it_can_use() {
        let body = serde_json::json!({
            "name": "KRON", "network": "mainnet",
            "tokens": [
                { "covenantId": TOKEN, "symbol": "PUBIC", "name": "PUBIC HAIR",
                  "decimals": 0, "logoURI": "https://example.test/a.png",
                  "extensions": { "genesisTxid": GENESIS, "curveCovenantId": CURVE,
                                  "poolCovenantId": null, "creatorPubkey": CREATOR } },
                // no covenant id: attachable to nothing, dropped
                { "symbol": "GHOST", "name": "Ghost" },
                // a scheme that could execute must not survive the parse
                { "covenantId": "ab".repeat(32), "symbol": "XSS",
                  "logoURI": "javascript:alert(1)" },
            ]
        })
        .to_string();
        let list = parse_list(&body, "mainnet").unwrap();
        assert_eq!(list.len(), 2, "the entry naming no covenant is dropped");
        assert_eq!(list[0].ticker.as_deref(), Some("PUBIC"));
        assert_eq!(list[0].curve_covenant_id.as_deref(), Some(CURVE));
        assert_eq!(list[0].pool_covenant_id, None, "a null extension is absent, not empty");
        assert_eq!(list[1].image, None, "javascript: is dropped, the entry survives");
        assert_eq!(list[1].ticker.as_deref(), Some("XSS"));
    }

    #[test]
    fn a_list_for_another_network_is_refused_whole() {
        let body = serde_json::json!({ "network": "testnet-10", "tokens": [] }).to_string();
        assert!(parse_list(&body, "mainnet").is_err());
    }
}
