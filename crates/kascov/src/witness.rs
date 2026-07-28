//! Witnessed launchpad logos: kascov's own dated copy of art nobody committed
//! to chain.
//!
//! Vocabulary matters here, because kascov already sells one word. `pinned`
//! means a sha256 committed on chain; `verified` means bytes that matched it.
//! Neither applies to a launchpad's `logoURI`, so this module says WITNESSED:
//! kascov fetched the bytes on a recorded date, kept its own copy, and
//! re-checks the source daily. That is a first-party observation, not a chain
//! fact, and every reader-facing surface says so.
//!
//! Why witness at all, rather than hotlink or abstain: of the 19 logos KRON
//! published on the day this was written, five were already dead (Discord
//! signs its attachment URLs and expires them) and three were HTML pages. A
//! hotlink leaks every reader's IP to a third-party host and shows swapped art
//! silently; abstaining shows nothing either way. A witnessed copy keeps the
//! art alive after its source dies, and a later swap at the source becomes a
//! RECORDED event instead of a silent one. kcc20.info has the two dead logos
//! today only because it mirrored while they were alive; that window does not
//! reopen.
//!
//! Storage is split on purpose. The witness RECORD (urls, hashes, dates,
//! change history) lives in the per-network archive database, because it is
//! evidence and rides the off-site backups. The BLOBS live in a separate
//! `{net}-media.db` that is deliberately never backed up: losing it costs a
//! refetch, and the archive's 5-minute off-site copies must not grow by
//! megabytes of thumbnails per snapshot forever.

use std::path::{Path, PathBuf};

use anyhow::Result;
use sha2::Digest;

/// Source bytes larger than this are rejected before decoding. Set above the
/// largest real logo observed (2.6 MiB) rather than at a tidy 2 MiB that
/// would silently drop three live ones; the decode limits below are what
/// actually stop a bomb.
pub const MAX_SOURCE_BYTES: usize = 4 * 1024 * 1024;
/// Decoded dimensions above this are refused outright.
const MAX_DIMENSION: u32 = 8192;
/// Thumbnails fit inside this square.
const THUMB_EDGE: u32 = 128;
/// A PNG thumb larger than this re-encodes as JPEG instead.
const THUMB_PNG_CEILING: usize = 48 * 1024;
/// Witness rows per network. A list bigger than this is either an attack or a
/// different product; either way growth stops rather than the database.
const MAX_WITNESS_ROWS: i64 = 5_000;
/// Change-history rows kept per covenant.
const MAX_CHANGES_PER_COVENANT: i64 = 32;
/// A healthy source is re-checked this often.
pub const RECHECK_MS: i64 = 24 * 60 * 60 * 1000;
/// First retry after a failure; doubles per consecutive failure up to RECHECK_MS.
const BACKOFF_BASE_MS: i64 = 60 * 60 * 1000;

/// Magic-byte sniff. The Content-Type header is never consulted: one live
/// mainnet logo declares image/png and serves JPEG bytes, and the dead ones
/// declare images while serving text.
pub fn sniff(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("gif")
    } else if bytes.len() > 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else {
        None
    }
}

/// A processed thumbnail plus the hashes that identify it. `source_sha256` is
/// over the ORIGINAL fetched body, so anyone can re-derive it from the source
/// URL themselves; the served bytes are the thumbnail, and no one is told
/// otherwise.
pub struct Thumb {
    pub bytes: Vec<u8>,
    pub content_type: &'static str,
    pub source_sha256: String,
    pub thumb_sha256: String,
}

/// Decode, bound, shrink, re-encode. Pure, so the whole policy is testable
/// without a network. Errors are static strings because they end up in a
/// status column, not in anyone's face.
pub fn process_image(original: &[u8]) -> std::result::Result<Thumb, &'static str> {
    if original.len() > MAX_SOURCE_BYTES {
        return Err("source too large");
    }
    sniff(original).ok_or("not an image")?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_DIMENSION);
    limits.max_image_height = Some(MAX_DIMENSION);
    limits.max_alloc = Some(256 * 1024 * 1024);
    let mut reader = image::ImageReader::new(std::io::Cursor::new(original))
        .with_guessed_format()
        .map_err(|_| "unreadable")?;
    reader.limits(limits);
    let img = reader.decode().map_err(|_| "decode failed")?;
    let thumb = img.thumbnail(THUMB_EDGE, THUMB_EDGE);

    let mut png: Vec<u8> = Vec::new();
    thumb
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|_| "encode failed")?;
    let (bytes, content_type) = if png.len() <= THUMB_PNG_CEILING {
        (png, "image/png")
    } else {
        // Photographic art compresses badly as PNG. JPEG has no alpha, so
        // transparency is composited onto the site's background rather than
        // defaulting to black.
        let rgba = thumb.to_rgba8();
        let (w, h) = rgba.dimensions();
        let mut rgb = image::RgbImage::new(w, h);
        const BG: [u16; 3] = [0x0a, 0x10, 0x0f];
        for (x, y, px) in rgba.enumerate_pixels() {
            let a = px[3] as u16;
            let blend = |c: u8, b: u16| (((c as u16) * a + b * (255 - a)) / 255) as u8;
            rgb.put_pixel(x, y, image::Rgb([blend(px[0], BG[0]), blend(px[1], BG[1]), blend(px[2], BG[2])]));
        }
        let mut jpg: Vec<u8> = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut std::io::Cursor::new(&mut jpg), 80)
            .encode_image(&image::DynamicImage::ImageRgb8(rgb))
            .map_err(|_| "encode failed")?;
        (jpg, "image/jpeg")
    };
    Ok(Thumb {
        source_sha256: hex::encode(sha2::Sha256::digest(original)),
        thumb_sha256: hex::encode(sha2::Sha256::digest(&bytes)),
        bytes,
        content_type,
    })
}

/// One witness row as the state machine sees it.
#[derive(Clone, Debug, Default)]
pub struct WitnessRow {
    pub covenant_id: String,
    pub source_url: String,
    pub observed_sha256: Option<String>,
    pub thumb_sha256: Option<String>,
    pub content_type: Option<String>,
    pub first_seen_ms: Option<i64>,
    pub last_checked_ms: Option<i64>,
    pub last_change_ms: Option<i64>,
    pub change_count: i64,
    pub fail_count: i64,
    pub next_check_ms: i64,
    /// witnessed | unavailable | not_an_image | delisted
    pub state: String,
    /// A NEW source hash seen exactly once. Adoption needs two consecutive
    /// checks agreeing, so a single poisoned response can never swap the art.
    pub pending_sha256: Option<String>,
}

/// What one check of the source produced.
pub enum Checked {
    /// Fetched and processed.
    Image(Thumb),
    /// Fetched, but the bytes are not an image (an HTML page, an expiry note).
    NotAnImage,
    /// Could not fetch at all.
    Failed,
}

/// What the caller must persist after a check.
pub struct Effect {
    pub row: WitnessRow,
    /// Store this blob (adoption or first witness).
    pub store_blob: Option<(String, &'static str, Vec<u8>)>,
    /// Record (from, to) in the change log.
    pub log_change: Option<(String, String)>,
}

/// The whole re-check policy in one pure function.
///
/// The rules, each of which exists because of a real failure mode:
/// - a source dying NEVER un-witnesses (five logos died within a day of
///   publication; the copy is the product);
/// - new bytes at an UNCHANGED url are adopted only after two consecutive
///   checks agree, so a swap is delayed by at least a day and a one-off
///   poisoned response is ignored;
/// - a CHANGED url in the published list is the publisher explicitly updating
///   the logo (KRON's own UI shows "logo changed 27m ago"), so it adopts on
///   the first good fetch — `source_replaced` — but still goes on the record;
/// - an adoption is counted and dated forever. A re-skin is a fact.
pub fn apply_check(
    mut row: WitnessRow,
    outcome: Checked,
    now_ms: i64,
    source_replaced: bool,
) -> Effect {
    let mut store_blob = None;
    let mut log_change = None;
    match outcome {
        Checked::Image(t) => {
            row.last_checked_ms = Some(now_ms);
            if source_replaced {
                if let Some(seen) = row.observed_sha256.clone() {
                    if seen != t.source_sha256 {
                        log_change = Some((seen, t.source_sha256.clone()));
                        row.change_count += 1;
                        row.last_change_ms = Some(now_ms);
                    }
                    row.observed_sha256 = Some(t.source_sha256.clone());
                    row.thumb_sha256 = Some(t.thumb_sha256.clone());
                    row.content_type = Some(t.content_type.to_string());
                    row.state = "witnessed".into();
                    row.fail_count = 0;
                    row.pending_sha256 = None;
                    row.next_check_ms = now_ms + RECHECK_MS;
                    store_blob = Some((t.thumb_sha256, t.content_type, t.bytes));
                    return Effect { row, store_blob, log_change };
                }
                // never witnessed before: fall through to the first-sighting path
            }
            match &row.observed_sha256 {
                None => {
                    // first sighting
                    row.first_seen_ms = Some(now_ms);
                    row.observed_sha256 = Some(t.source_sha256.clone());
                    row.thumb_sha256 = Some(t.thumb_sha256.clone());
                    row.content_type = Some(t.content_type.to_string());
                    row.state = "witnessed".into();
                    row.fail_count = 0;
                    row.pending_sha256 = None;
                    store_blob = Some((t.thumb_sha256, t.content_type, t.bytes));
                }
                Some(seen) if *seen == t.source_sha256 => {
                    row.fail_count = 0;
                    row.pending_sha256 = None; // a flapping source settles back
                }
                Some(seen) => {
                    if row.pending_sha256.as_deref() == Some(t.source_sha256.as_str()) {
                        // second consecutive sighting: adopt, on the record
                        log_change = Some((seen.clone(), t.source_sha256.clone()));
                        row.observed_sha256 = Some(t.source_sha256.clone());
                        row.thumb_sha256 = Some(t.thumb_sha256.clone());
                        row.content_type = Some(t.content_type.to_string());
                        row.change_count += 1;
                        row.last_change_ms = Some(now_ms);
                        row.pending_sha256 = None;
                        store_blob = Some((t.thumb_sha256, t.content_type, t.bytes));
                    } else {
                        // first sighting of new bytes: hold the line
                        row.pending_sha256 = Some(t.source_sha256.clone());
                    }
                    row.fail_count = 0;
                }
            }
            row.next_check_ms = now_ms + RECHECK_MS;
        }
        Checked::NotAnImage => {
            row.last_checked_ms = Some(now_ms);
            row.fail_count += 1;
            if row.observed_sha256.is_none() {
                row.state = "not_an_image".into();
            } // else: keep serving the witnessed copy — the source rotted, we did not
            row.next_check_ms = now_ms + backoff(row.fail_count);
        }
        Checked::Failed => {
            row.last_checked_ms = Some(now_ms);
            row.fail_count += 1;
            if row.observed_sha256.is_none() {
                row.state = "unavailable".into();
            }
            row.next_check_ms = now_ms + backoff(row.fail_count);
        }
    }
    Effect { row, store_blob, log_change }
}

fn backoff(fails: i64) -> i64 {
    (BACKOFF_BASE_MS << fails.clamp(0, 5).saturating_sub(1)).min(RECHECK_MS)
}

// ---------------------------------------------------------------------------
// storage

/// The witness record lives in the ARCHIVE db: it is evidence, and it rides
/// the off-site backups. Tiny rows only — no blobs, ever.
pub fn ensure_witness_schema(conn: &rusqlite::Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS listed_image_witness (
            covenant_id TEXT PRIMARY KEY,
            source_url TEXT NOT NULL,
            observed_sha256 TEXT,
            thumb_sha256 TEXT,
            content_type TEXT,
            first_seen_ms INTEGER,
            last_checked_ms INTEGER,
            last_change_ms INTEGER,
            change_count INTEGER NOT NULL DEFAULT 0,
            fail_count INTEGER NOT NULL DEFAULT 0,
            next_check_ms INTEGER NOT NULL DEFAULT 0,
            state TEXT NOT NULL,
            pending_sha256 TEXT
        );
        CREATE TABLE IF NOT EXISTS listed_image_change (
            covenant_id TEXT NOT NULL,
            at_ms INTEGER NOT NULL,
            from_sha256 TEXT NOT NULL,
            to_sha256 TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS liw_change_cov ON listed_image_change(covenant_id, at_ms);",
    )?;
    Ok(())
}

/// Blob store in `{net}-media.db`, which the backup scripts never touch:
/// losing it costs a refetch, and the archive must not carry megabytes of
/// thumbnails in every 5-minute off-site snapshot forever.
pub fn media_db_path(base_dir: &Path, network: &str) -> PathBuf {
    base_dir.join(format!("{network}-media.db"))
}

pub fn open_media_db(path: &Path) -> Result<rusqlite::Connection> {
    let conn = rusqlite::Connection::open(path)?;
    conn.busy_timeout(std::time::Duration::from_millis(5000))?;
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         CREATE TABLE IF NOT EXISTS listed_image_blob (
            thumb_sha256 TEXT PRIMARY KEY,
            content_type TEXT NOT NULL,
            bytes BLOB NOT NULL,
            byte_len INTEGER NOT NULL
         );",
    )?;
    Ok(conn)
}

pub fn load_row(conn: &rusqlite::Connection, covenant_id: &str) -> Result<Option<WitnessRow>> {
    let row = conn
        .query_row(
            "SELECT covenant_id, source_url, observed_sha256, thumb_sha256, content_type,
                    first_seen_ms, last_checked_ms, last_change_ms, change_count, fail_count,
                    next_check_ms, state, pending_sha256
             FROM listed_image_witness WHERE covenant_id = ?1",
            [covenant_id],
            |r| {
                Ok(WitnessRow {
                    covenant_id: r.get(0)?,
                    source_url: r.get(1)?,
                    observed_sha256: r.get(2)?,
                    thumb_sha256: r.get(3)?,
                    content_type: r.get(4)?,
                    first_seen_ms: r.get(5)?,
                    last_checked_ms: r.get(6)?,
                    last_change_ms: r.get(7)?,
                    change_count: r.get(8)?,
                    fail_count: r.get(9)?,
                    next_check_ms: r.get(10)?,
                    state: r.get(11)?,
                    pending_sha256: r.get(12)?,
                })
            },
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            e => Err(e),
        })?;
    Ok(row)
}

pub fn save_effect(
    archive: &rusqlite::Connection,
    media: &rusqlite::Connection,
    effect: &Effect,
) -> Result<()> {
    let r = &effect.row;
    let existing: i64 =
        archive.query_row("SELECT COUNT(*) FROM listed_image_witness", [], |x| x.get(0))?;
    let known: bool = archive.query_row(
        "SELECT EXISTS(SELECT 1 FROM listed_image_witness WHERE covenant_id = ?1)",
        [&r.covenant_id],
        |x| x.get(0),
    )?;
    if !known && existing >= MAX_WITNESS_ROWS {
        anyhow::bail!("witness table at capacity");
    }
    archive.execute(
        "INSERT INTO listed_image_witness (covenant_id, source_url, observed_sha256, thumb_sha256,
            content_type, first_seen_ms, last_checked_ms, last_change_ms, change_count, fail_count,
            next_check_ms, state, pending_sha256)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)
         ON CONFLICT(covenant_id) DO UPDATE SET
            source_url=?2, observed_sha256=?3, thumb_sha256=?4, content_type=?5, first_seen_ms=?6,
            last_checked_ms=?7, last_change_ms=?8, change_count=?9, fail_count=?10,
            next_check_ms=?11, state=?12, pending_sha256=?13",
        rusqlite::params![
            r.covenant_id, r.source_url, r.observed_sha256, r.thumb_sha256, r.content_type,
            r.first_seen_ms, r.last_checked_ms, r.last_change_ms, r.change_count, r.fail_count,
            r.next_check_ms, r.state, r.pending_sha256
        ],
    )?;
    if let Some((from, to)) = &effect.log_change {
        archive.execute(
            "INSERT INTO listed_image_change (covenant_id, at_ms, from_sha256, to_sha256)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![r.covenant_id, r.last_change_ms, from, to],
        )?;
        archive.execute(
            "DELETE FROM listed_image_change WHERE covenant_id = ?1 AND rowid NOT IN
             (SELECT rowid FROM listed_image_change WHERE covenant_id = ?1
              ORDER BY at_ms DESC LIMIT ?2)",
            rusqlite::params![r.covenant_id, MAX_CHANGES_PER_COVENANT],
        )?;
    }
    if let Some((sha, ctype, bytes)) = &effect.store_blob {
        media.execute(
            "INSERT OR REPLACE INTO listed_image_blob (thumb_sha256, content_type, bytes, byte_len)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![sha, ctype, bytes, bytes.len() as i64],
        )?;
    }
    Ok(())
}

/// The serving read: witnessed rows only, one blob.
pub fn serve_lookup(
    archive: &rusqlite::Connection,
    media: &rusqlite::Connection,
    covenant_id: &str,
) -> Result<Option<(String, String, Vec<u8>)>> {
    let Some(row) = load_row(archive, covenant_id)? else { return Ok(None) };
    if row.state != "witnessed" {
        return Ok(None);
    }
    let (Some(sha), Some(_ct)) = (row.thumb_sha256, row.content_type) else { return Ok(None) };
    let blob = media
        .query_row(
            "SELECT content_type, bytes FROM listed_image_blob WHERE thumb_sha256 = ?1",
            [&sha],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?)),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            e => Err(e),
        })?;
    Ok(blob.map(|(ct, bytes)| (sha, ct, bytes)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny_png() -> Vec<u8> {
        let img = image::RgbaImage::from_fn(300, 300, |x, y| {
            image::Rgba([(x % 256) as u8, (y % 256) as u8, 128, 255])
        });
        let mut out = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    #[test]
    fn sniffing_trusts_bytes_and_nothing_else() {
        assert_eq!(sniff(&tiny_png()), Some("png"));
        assert_eq!(sniff(b"\xff\xd8\xff\xe0 jfif"), Some("jpeg"));
        // the real failure modes observed live: an expiry note and two HTML pages
        assert_eq!(sniff(b"This content is no longer available."), None);
        assert_eq!(sniff(b"<!doctype html><html>..."), None);
        assert_eq!(sniff(b"<svg xmlns=..."), None, "SVG can script; never witnessed");
    }

    #[test]
    fn processing_shrinks_and_hashes() {
        let t = process_image(&tiny_png()).unwrap();
        assert!(t.bytes.len() < 64 * 1024, "a thumb is small: {}", t.bytes.len());
        assert_eq!(t.content_type, "image/png");
        assert_ne!(t.source_sha256, t.thumb_sha256, "source hash is over the ORIGINAL bytes");
        // deterministic: same input, same hashes
        let t2 = process_image(&tiny_png()).unwrap();
        assert_eq!(t.thumb_sha256, t2.thumb_sha256);
        assert!(process_image(b"<!doctype html>").is_err());
        assert!(process_image(&vec![0u8; MAX_SOURCE_BYTES + 1]).is_err());
    }

    fn fresh(url: &str) -> WitnessRow {
        WitnessRow {
            covenant_id: "c0".repeat(32),
            source_url: url.into(),
            state: "unavailable".into(),
            ..Default::default()
        }
    }

    fn thumb(tag: u8) -> Thumb {
        Thumb {
            bytes: vec![tag; 8],
            content_type: "image/png",
            source_sha256: format!("{:064x}", tag),
            thumb_sha256: format!("{:064x}", 0xf0 + tag as u32),
        }
    }

    #[test]
    fn a_first_sighting_witnesses() {
        let e = apply_check(fresh("https://x/logo.png"), Checked::Image(thumb(1)), 1000, false);
        assert_eq!(e.row.state, "witnessed");
        assert_eq!(e.row.first_seen_ms, Some(1000));
        assert!(e.store_blob.is_some());
        assert!(e.log_change.is_none(), "the first sighting is not a change");
    }

    /// The rule the whole design leans on: a swap at an UNCHANGED url needs
    /// two consecutive checks agreeing, so one poisoned response cannot
    /// replace the art, and an adoption is dated and counted forever.
    #[test]
    fn a_swap_needs_two_agreeing_checks_and_goes_on_the_record() {
        let e1 = apply_check(fresh("https://x/l.png"), Checked::Image(thumb(1)), 1000, false);
        // new bytes appear once: held, old copy still served
        let e2 = apply_check(e1.row, Checked::Image(thumb(2)), 2000, false);
        assert_eq!(e2.row.observed_sha256, Some(format!("{:064x}", 1)));
        assert_eq!(e2.row.pending_sha256, Some(format!("{:064x}", 2)));
        assert!(e2.store_blob.is_none(), "held bytes are not stored");
        assert_eq!(e2.row.change_count, 0);
        // the source flaps back: pending clears, nothing recorded
        let e3 = apply_check(e2.row.clone(), Checked::Image(thumb(1)), 3000, false);
        assert_eq!(e3.row.pending_sha256, None);
        assert_eq!(e3.row.change_count, 0);
        // new bytes twice in a row: adopted, counted, logged
        let e4 = apply_check(e2.row, Checked::Image(thumb(2)), 4000, false);
        assert_eq!(e4.row.observed_sha256, Some(format!("{:064x}", 2)));
        assert_eq!(e4.row.change_count, 1);
        assert_eq!(e4.row.last_change_ms, Some(4000));
        assert!(e4.store_blob.is_some());
        assert_eq!(e4.log_change, Some((format!("{:064x}", 1), format!("{:064x}", 2))));
    }

    /// A url change in the PUBLISHED LIST is the publisher updating the logo
    /// (KRON's own page shows "Logo changed 27m ago"), so it adopts on the
    /// first good fetch instead of waiting a day — but still goes on the
    /// record, because a re-skin is a fact either way.
    #[test]
    fn a_replaced_url_adopts_immediately_and_is_recorded() {
        let e1 = apply_check(fresh("https://x/old.png"), Checked::Image(thumb(1)), 1000, false);
        let mut moved = e1.row;
        moved.source_url = "https://x/new.png".into();
        let e2 = apply_check(moved, Checked::Image(thumb(2)), 2000, true);
        assert_eq!(e2.row.observed_sha256, Some(format!("{:064x}", 2)));
        assert_eq!(e2.row.change_count, 1);
        assert!(e2.store_blob.is_some());
        assert!(e2.log_change.is_some());
        // same bytes at a new url: nothing to record
        let e3 = apply_check(e2.row, Checked::Image(thumb(2)), 3000, true);
        assert_eq!(e3.row.change_count, 1);
        assert!(e3.log_change.is_none());
    }

    /// Five of nineteen live logos died within days of publication. The copy
    /// outliving its source is the entire point.
    #[test]
    fn a_dead_source_never_unwitnesses() {
        let e1 = apply_check(fresh("https://x/l.png"), Checked::Image(thumb(1)), 1000, false);
        let e2 = apply_check(e1.row, Checked::Failed, 2000, false);
        assert_eq!(e2.row.state, "witnessed", "the copy survives the source");
        assert_eq!(e2.row.fail_count, 1);
        let e3 = apply_check(e2.row, Checked::NotAnImage, 3000, false);
        assert_eq!(e3.row.state, "witnessed", "even an expiry page changes nothing");
        // but a source that was NEVER witnessed reports honestly
        let n = apply_check(fresh("https://x/dead.png"), Checked::Failed, 1000, false);
        assert_eq!(n.row.state, "unavailable");
        let h = apply_check(fresh("https://x/page.html"), Checked::NotAnImage, 1000, false);
        assert_eq!(h.row.state, "not_an_image");
    }

    #[test]
    fn failures_back_off_and_recover() {
        let mut row =
            apply_check(fresh("https://x/l.png"), Checked::Image(thumb(1)), 0, false).row;
        let mut last = 0;
        for i in 1..=6 {
            let e = apply_check(row, Checked::Failed, i * 10, false);
            row = e.row;
            let wait = row.next_check_ms - i * 10;
            assert!(wait >= last.min(RECHECK_MS), "backoff never shrinks while failing");
            assert!(wait <= RECHECK_MS, "and never exceeds the healthy cadence");
            last = wait;
        }
        let e = apply_check(row, Checked::Image(thumb(1)), 1000, false);
        assert_eq!(e.row.fail_count, 0, "one good check resets the backoff");
    }

    #[test]
    fn storage_round_trips_and_serves_witnessed_only() {
        let archive = rusqlite::Connection::open_in_memory().unwrap();
        let media = rusqlite::Connection::open_in_memory().unwrap();
        ensure_witness_schema(&archive).unwrap();
        media
            .execute_batch(
                "CREATE TABLE listed_image_blob (thumb_sha256 TEXT PRIMARY KEY,
                 content_type TEXT NOT NULL, bytes BLOB NOT NULL, byte_len INTEGER NOT NULL);",
            )
            .unwrap();
        let cov = "ab".repeat(32);
        let mut row = fresh("https://x/l.png");
        row.covenant_id = cov.clone();
        let e = apply_check(row, Checked::Image(thumb(7)), 1000, false);
        save_effect(&archive, &media, &e).unwrap();
        let (sha, ct, bytes) = serve_lookup(&archive, &media, &cov).unwrap().unwrap();
        assert_eq!(sha, format!("{:064x}", 0xf7));
        assert_eq!(ct, "image/png");
        assert_eq!(bytes, vec![7u8; 8]);
        // a never-witnessed id serves nothing
        assert!(serve_lookup(&archive, &media, &"cd".repeat(32)).unwrap().is_none());
        // delisting stops serving but keeps the record
        archive
            .execute(
                "UPDATE listed_image_witness SET state='delisted' WHERE covenant_id=?1",
                [&cov],
            )
            .unwrap();
        assert!(serve_lookup(&archive, &media, &cov).unwrap().is_none());
        let kept: i64 = archive
            .query_row("SELECT COUNT(*) FROM listed_image_witness", [], |r| r.get(0))
            .unwrap();
        assert_eq!(kept, 1, "the evidence outlives the listing");
    }
}
