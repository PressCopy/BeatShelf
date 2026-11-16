//! SQLite storage. Paths inside the library are stored relative to the
//! library root with `/` separators, so the database survives a moved root.

use crate::error::{Error, Result};
use crate::keys::MusicalKey;
use crate::ops::{BatchStatus, DoneOp, Journal};
use crate::pathsafe::rel_string;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{de::DeserializeOwned, Deserialize, Deserializer, Serialize};
use std::path::{Path, PathBuf};

const SCHEMA_VERSION: i64 = 3;

/// v3: items made with the "New folder" button. `draft` = the item has no
/// folder on disk yet; `auto_name` = its name is still the placeholder and
/// will be taken from the first file added.
const MIGRATION_3: &str = "
ALTER TABLE beats ADD COLUMN draft INTEGER NOT NULL DEFAULT 0;
ALTER TABLE beats ADD COLUMN auto_name INTEGER NOT NULL DEFAULT 0;
";

const SCHEMA: &str = r#"
CREATE TABLE settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);

CREATE TABLE beats(
  id INTEGER PRIMARY KEY,
  name TEXT NOT NULL,
  folder TEXT UNIQUE,
  bpm REAL,
  musical_key TEXT,
  camelot TEXT,
  genre TEXT,
  mood TEXT,
  type_beat TEXT,
  status TEXT NOT NULL DEFAULT 'idea',
  rating INTEGER NOT NULL DEFAULT 0,
  color TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE assets(
  id INTEGER PRIMARY KEY,
  beat_id INTEGER REFERENCES beats(id) ON DELETE CASCADE,
  path TEXT NOT NULL UNIQUE,
  kind TEXT NOT NULL,
  daw TEXT,
  is_dir INTEGER NOT NULL DEFAULT 0,
  is_backup INTEGER NOT NULL DEFAULT 0,
  size INTEGER,
  mtime INTEGER,
  duration_ms INTEGER,
  sample_rate INTEGER,
  channels INTEGER,
  codec TEXT,
  peaks BLOB
);
CREATE INDEX assets_beat ON assets(beat_id);

CREATE TABLE inbox(
  id INTEGER PRIMARY KEY,
  path TEXT NOT NULL UNIQUE,
  source TEXT NOT NULL,
  kind TEXT NOT NULL,
  daw TEXT,
  is_dir INTEGER NOT NULL DEFAULT 0,
  size INTEGER,
  mtime INTEGER,
  title TEXT,
  bpm REAL,
  musical_key TEXT,
  tagged INTEGER,
  state TEXT NOT NULL DEFAULT 'pending',
  detected_at INTEGER NOT NULL
);

CREATE TABLE batches(
  id INTEGER PRIMARY KEY,
  label TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  status TEXT NOT NULL,
  undo_of INTEGER,
  undone_by INTEGER
);
CREATE TABLE ops(
  batch_id INTEGER NOT NULL REFERENCES batches(id) ON DELETE CASCADE,
  seq INTEGER NOT NULL,
  op TEXT NOT NULL,
  PRIMARY KEY(batch_id, seq)
);

CREATE VIRTUAL TABLE beats_fts USING fts5(
  name, genre, mood, type_beat, musical_key, camelot, status,
  content='beats', content_rowid='id', tokenize='unicode61 remove_diacritics 2'
);
CREATE TRIGGER beats_ai AFTER INSERT ON beats BEGIN
  INSERT INTO beats_fts(rowid, name, genre, mood, type_beat, musical_key, camelot, status)
  VALUES (new.id, replace(replace(new.name, 'ё', 'е'), 'Ё', 'Е'), replace(replace(new.genre, 'ё', 'е'), 'Ё', 'Е'), replace(replace(new.mood, 'ё', 'е'), 'Ё', 'Е'), replace(replace(new.type_beat, 'ё', 'е'), 'Ё', 'Е'), new.musical_key, new.camelot, new.status);
END;
CREATE TRIGGER beats_ad AFTER DELETE ON beats BEGIN
  INSERT INTO beats_fts(beats_fts, rowid, name, genre, mood, type_beat, musical_key, camelot, status)
  VALUES ('delete', old.id, replace(replace(old.name, 'ё', 'е'), 'Ё', 'Е'), replace(replace(old.genre, 'ё', 'е'), 'Ё', 'Е'), replace(replace(old.mood, 'ё', 'е'), 'Ё', 'Е'), replace(replace(old.type_beat, 'ё', 'е'), 'Ё', 'Е'), old.musical_key, old.camelot, old.status);
END;
CREATE TRIGGER beats_au AFTER UPDATE ON beats BEGIN
  INSERT INTO beats_fts(beats_fts, rowid, name, genre, mood, type_beat, musical_key, camelot, status)
  VALUES ('delete', old.id, replace(replace(old.name, 'ё', 'е'), 'Ё', 'Е'), replace(replace(old.genre, 'ё', 'е'), 'Ё', 'Е'), replace(replace(old.mood, 'ё', 'е'), 'Ё', 'Е'), replace(replace(old.type_beat, 'ё', 'е'), 'Ё', 'Е'), old.musical_key, old.camelot, old.status);
  INSERT INTO beats_fts(rowid, name, genre, mood, type_beat, musical_key, camelot, status)
  VALUES (new.id, replace(replace(new.name, 'ё', 'е'), 'Ё', 'Е'), replace(replace(new.genre, 'ё', 'е'), 'Ё', 'Е'), replace(replace(new.mood, 'ё', 'е'), 'Ё', 'Е'), replace(replace(new.type_beat, 'ё', 'е'), 'Ё', 'Е'), new.musical_key, new.camelot, new.status);
END;
"#;


/// v2: loops and starters (`kind`), authors and co-authors (searchable).
const MIGRATION_2: &str = r#"
ALTER TABLE beats ADD COLUMN kind TEXT NOT NULL DEFAULT 'beat';
ALTER TABLE beats ADD COLUMN authors TEXT;
ALTER TABLE beats ADD COLUMN coauthors TEXT;
DROP TRIGGER beats_ai;
DROP TRIGGER beats_ad;
DROP TRIGGER beats_au;
DROP TABLE beats_fts;
CREATE VIRTUAL TABLE beats_fts USING fts5(
  name, genre, mood, type_beat, musical_key, camelot, status, authors, coauthors,
  content='beats', content_rowid='id', tokenize='unicode61 remove_diacritics 2'
);
CREATE TRIGGER beats_ai AFTER INSERT ON beats BEGIN
  INSERT INTO beats_fts(rowid, name, genre, mood, type_beat, musical_key, camelot, status, authors, coauthors) VALUES (new.id, replace(replace(new.name, 'ё', 'е'), 'Ё', 'Е'), replace(replace(new.genre, 'ё', 'е'), 'Ё', 'Е'), replace(replace(new.mood, 'ё', 'е'), 'Ё', 'Е'), replace(replace(new.type_beat, 'ё', 'е'), 'Ё', 'Е'), new.musical_key, new.camelot, new.status, replace(replace(new.authors, 'ё', 'е'), 'Ё', 'Е'), replace(replace(new.coauthors, 'ё', 'е'), 'Ё', 'Е'));
END;
CREATE TRIGGER beats_ad AFTER DELETE ON beats BEGIN
  INSERT INTO beats_fts(beats_fts, rowid, name, genre, mood, type_beat, musical_key, camelot, status, authors, coauthors) VALUES ('delete', old.id, replace(replace(old.name, 'ё', 'е'), 'Ё', 'Е'), replace(replace(old.genre, 'ё', 'е'), 'Ё', 'Е'), replace(replace(old.mood, 'ё', 'е'), 'Ё', 'Е'), replace(replace(old.type_beat, 'ё', 'е'), 'Ё', 'Е'), old.musical_key, old.camelot, old.status, replace(replace(old.authors, 'ё', 'е'), 'Ё', 'Е'), replace(replace(old.coauthors, 'ё', 'е'), 'Ё', 'Е'));
END;
CREATE TRIGGER beats_au AFTER UPDATE ON beats BEGIN
  INSERT INTO beats_fts(beats_fts, rowid, name, genre, mood, type_beat, musical_key, camelot, status, authors, coauthors) VALUES ('delete', old.id, replace(replace(old.name, 'ё', 'е'), 'Ё', 'Е'), replace(replace(old.genre, 'ё', 'е'), 'Ё', 'Е'), replace(replace(old.mood, 'ё', 'е'), 'Ё', 'Е'), replace(replace(old.type_beat, 'ё', 'е'), 'Ё', 'Е'), old.musical_key, old.camelot, old.status, replace(replace(old.authors, 'ё', 'е'), 'Ё', 'Е'), replace(replace(old.coauthors, 'ё', 'е'), 'Ё', 'Е'));
  INSERT INTO beats_fts(rowid, name, genre, mood, type_beat, musical_key, camelot, status, authors, coauthors) VALUES (new.id, replace(replace(new.name, 'ё', 'е'), 'Ё', 'Е'), replace(replace(new.genre, 'ё', 'е'), 'Ё', 'Е'), replace(replace(new.mood, 'ё', 'е'), 'Ё', 'Е'), replace(replace(new.type_beat, 'ё', 'е'), 'Ё', 'Е'), new.musical_key, new.camelot, new.status, replace(replace(new.authors, 'ё', 'е'), 'Ё', 'Е'), replace(replace(new.coauthors, 'ё', 'е'), 'Ё', 'Е'));
END;
INSERT INTO beats_fts(rowid, name, genre, mood, type_beat, musical_key, camelot, status, authors, coauthors) SELECT id, replace(replace(name, 'ё', 'е'), 'Ё', 'Е'), replace(replace(genre, 'ё', 'е'), 'Ё', 'Е'), replace(replace(mood, 'ё', 'е'), 'Ё', 'Е'), replace(replace(type_beat, 'ё', 'е'), 'Ё', 'Е'), musical_key, camelot, status, replace(replace(authors, 'ё', 'е'), 'Ё', 'Е'), replace(replace(coauthors, 'ё', 'е'), 'Ё', 'Е') FROM beats;
"#;

/// Names are stored as "maks, lowkey"; commas can't be part of a name.
fn join_people(names: &[String]) -> Option<String> {
    let mut out: Vec<String> = Vec::new();
    for n in names {
        let n = n.replace(',', " ").trim().to_string();
        if !n.is_empty() && !out.iter().any(|o| o.to_lowercase() == n.to_lowercase()) {
            out.push(n);
        }
    }
    if out.is_empty() {
        None
    } else {
        Some(out.join(", "))
    }
}

fn split_people(s: Option<String>) -> Vec<String> {
    s.map(|s| s.split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect())
        .unwrap_or_default()
}

pub const KINDS: [&str; 4] = ["beat", "loop", "starter", "voicetag"];

pub const STATUSES: [&str; 6] = ["idea", "in_progress", "ready", "sent", "leased", "sold_exclusive"];

pub fn now_ts() -> i64 {
    chrono::Utc::now().timestamp()
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BeatRow {
    pub id: i64,
    pub name: String,
    pub folder: Option<String>,
    pub bpm: Option<f64>,
    pub key: Option<String>,
    pub camelot: Option<String>,
    pub genre: Option<String>,
    pub mood: Option<String>,
    pub type_beat: Option<String>,
    pub status: String,
    pub rating: i64,
    pub color: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub has_fl: bool,
    pub has_ableton: bool,
    pub latest_daw: Option<String>,
    pub preview_asset_id: Option<i64>,
    pub duration_ms: Option<i64>,
    /// "beat" | "loop" | "starter" | "voicetag"
    pub kind: String,
    pub authors: Vec<String>,
    pub coauthors: Vec<String>,
    /// Cover image: a file named `cover.*` in the item's folder (relative path).
    pub cover: Option<String>,
    /// Changes when the cover file does; lets the UI refresh a cached picture.
    pub cover_v: Option<i64>,
}

fn double_option<'de, D, T>(d: D) -> std::result::Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

/// Partial update. A field that is absent is left alone; `null` clears it.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BeatPatch {
    pub name: Option<String>,
    #[serde(default, deserialize_with = "double_option")]
    pub bpm: Option<Option<f64>>,
    #[serde(default, deserialize_with = "double_option")]
    pub key: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub genre: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub mood: Option<Option<String>>,
    #[serde(default, deserialize_with = "double_option")]
    pub type_beat: Option<Option<String>>,
    pub status: Option<String>,
    pub rating: Option<i64>,
    #[serde(default, deserialize_with = "double_option")]
    pub color: Option<Option<String>>,
    pub kind: Option<String>,
    pub authors: Option<Vec<String>>,
    pub coauthors: Option<Vec<String>>,
}

#[derive(Debug, Clone, Default)]
pub struct NewBeat {
    pub name: String,
    pub folder: Option<String>,
    pub bpm: Option<f64>,
    pub key: Option<MusicalKey>,
    pub status: Option<String>,
    /// "beat" when empty.
    pub kind: Option<String>,
    pub authors: Vec<String>,
    pub coauthors: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssetRow {
    pub id: i64,
    pub beat_id: Option<i64>,
    pub path: String,
    pub name: String,
    pub kind: String,
    pub daw: Option<String>,
    pub is_dir: bool,
    pub is_backup: bool,
    pub size: Option<i64>,
    pub mtime: Option<i64>,
    pub duration_ms: Option<i64>,
    pub sample_rate: Option<i64>,
    pub channels: Option<i64>,
    pub codec: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct AssetUpsert {
    pub beat_id: Option<i64>,
    pub path: String,
    pub kind: String,
    pub daw: Option<String>,
    pub is_dir: bool,
    pub is_backup: bool,
    pub size: Option<i64>,
    pub mtime: Option<i64>,
    pub duration_ms: Option<i64>,
    pub sample_rate: Option<i64>,
    pub channels: Option<i64>,
    pub codec: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxItem {
    pub id: i64,
    pub path: String,
    pub name: String,
    pub source: String,
    pub kind: String,
    pub daw: Option<String>,
    pub is_dir: bool,
    pub size: Option<i64>,
    pub mtime: Option<i64>,
    pub title: Option<String>,
    pub bpm: Option<f64>,
    pub key: Option<String>,
    pub camelot: Option<String>,
    pub tagged: Option<bool>,
    pub state: String,
    pub detected_at: i64,
}

#[derive(Debug, Clone, Default)]
pub struct InboxUpsert {
    pub path: String,
    pub source: String,
    pub kind: String,
    pub daw: Option<String>,
    pub is_dir: bool,
    pub size: Option<i64>,
    pub mtime: Option<i64>,
    pub title: Option<String>,
    pub bpm: Option<f64>,
    pub key: Option<String>,
    pub tagged: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchRow {
    pub id: i64,
    pub label: String,
    pub created_at: i64,
    pub status: String,
    pub undo_of: Option<i64>,
    pub undone_by: Option<i64>,
    pub op_count: i64,
    pub can_undo: bool,
}

pub struct Db {
    pub conn: Connection,
}

fn file_name_of(rel: &str) -> String {
    rel.rsplit('/').next().unwrap_or(rel).to_string()
}

fn key_fields(key: Option<&str>) -> (Option<String>, Option<String>) {
    match key.and_then(MusicalKey::parse) {
        Some(k) => (Some(k.name().to_string()), Some(k.camelot_code())),
        None => (None, None),
    }
}

/// Turn free text into an FTS5 prefix query plus numeric BPM terms.
fn split_query(q: &str) -> (String, Vec<i64>) {
    let mut fts = Vec::new();
    let mut nums = Vec::new();
    for raw in q.split_whitespace() {
        // FTS stores "ё" as "е" (see triggers), so fold the query the same way.
        let term: String = raw
            .chars()
            .filter(|c| c.is_alphanumeric())
            .map(|c| match c {
                'ё' => 'е',
                'Ё' => 'Е',
                c => c,
            })
            .collect();
        if term.is_empty() {
            continue;
        }
        if term.chars().all(|c| c.is_ascii_digit()) && term.len() <= 3 {
            if let Ok(n) = term.parse() {
                nums.push(n);
                continue;
            }
        }
        fts.push(format!("\"{}\"*", term));
    }
    (fts.join(" "), nums)
}

const BEAT_SELECT: &str = r#"
SELECT b.id, b.name, b.folder, b.bpm, b.musical_key, b.camelot, b.genre, b.mood, b.type_beat,
       b.status, b.rating, b.color, b.created_at, b.updated_at,
       EXISTS(SELECT 1 FROM assets a WHERE a.beat_id = b.id AND a.daw = 'fl' AND a.is_backup = 0),
       EXISTS(SELECT 1 FROM assets a WHERE a.beat_id = b.id AND a.daw = 'ableton' AND a.is_backup = 0),
       (SELECT a.daw FROM assets a WHERE a.beat_id = b.id AND a.daw IS NOT NULL AND a.is_backup = 0
          ORDER BY a.mtime DESC LIMIT 1),
       (SELECT a.id FROM assets a WHERE a.beat_id = b.id AND a.kind = 'audio' AND a.is_backup = 0
          ORDER BY a.mtime DESC LIMIT 1),
       (SELECT a.duration_ms FROM assets a WHERE a.beat_id = b.id AND a.kind = 'audio' AND a.is_backup = 0
          ORDER BY a.mtime DESC LIMIT 1),
       b.kind, b.authors, b.coauthors,
       (SELECT a.path FROM assets a WHERE a.beat_id = b.id AND a.kind = 'image' AND a.is_backup = 0 AND (lower(a.path) LIKE '%/cover.jpg' OR lower(a.path) LIKE '%/cover.jpeg' OR lower(a.path) LIKE '%/cover.png' OR lower(a.path) LIKE '%/cover.webp')
          ORDER BY a.mtime DESC LIMIT 1),
       (SELECT a.mtime FROM assets a WHERE a.beat_id = b.id AND a.kind = 'image' AND a.is_backup = 0 AND (lower(a.path) LIKE '%/cover.jpg' OR lower(a.path) LIKE '%/cover.jpeg' OR lower(a.path) LIKE '%/cover.png' OR lower(a.path) LIKE '%/cover.webp')
          ORDER BY a.mtime DESC LIMIT 1)
FROM beats b
"#;

fn beat_from_row(r: &rusqlite::Row) -> rusqlite::Result<BeatRow> {
    Ok(BeatRow {
        id: r.get(0)?,
        name: r.get(1)?,
        folder: r.get(2)?,
        bpm: r.get(3)?,
        key: r.get(4)?,
        camelot: r.get(5)?,
        genre: r.get(6)?,
        mood: r.get(7)?,
        type_beat: r.get(8)?,
        status: r.get(9)?,
        rating: r.get(10)?,
        color: r.get(11)?,
        created_at: r.get(12)?,
        updated_at: r.get(13)?,
        has_fl: r.get(14)?,
        has_ableton: r.get(15)?,
        latest_daw: r.get(16)?,
        preview_asset_id: r.get(17)?,
        duration_ms: r.get(18)?,
        kind: r.get(19)?,
        authors: split_people(r.get(20)?),
        coauthors: split_people(r.get(21)?),
        cover: r.get(22)?,
        cover_v: r.get(23)?,
    })
}

const ASSET_SELECT: &str = "SELECT id, beat_id, path, kind, daw, is_dir, is_backup, size, mtime, duration_ms, sample_rate, channels, codec FROM assets";

fn asset_from_row(r: &rusqlite::Row) -> rusqlite::Result<AssetRow> {
    let path: String = r.get(2)?;
    Ok(AssetRow {
        id: r.get(0)?,
        beat_id: r.get(1)?,
        name: file_name_of(&path),
        path,
        kind: r.get(3)?,
        daw: r.get(4)?,
        is_dir: r.get(5)?,
        is_backup: r.get(6)?,
        size: r.get(7)?,
        mtime: r.get(8)?,
        duration_ms: r.get(9)?,
        sample_rate: r.get(10)?,
        channels: r.get(11)?,
        codec: r.get(12)?,
    })
}

const INBOX_SELECT: &str = "SELECT id, path, source, kind, daw, is_dir, size, mtime, title, bpm, musical_key, tagged, state, detected_at FROM inbox";

fn inbox_from_row(r: &rusqlite::Row) -> rusqlite::Result<InboxItem> {
    let path: String = r.get(1)?;
    let key: Option<String> = r.get(10)?;
    let camelot = key.as_deref().and_then(MusicalKey::parse).map(|k| k.camelot_code());
    let name = Path::new(&path).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    Ok(InboxItem {
        id: r.get(0)?,
        name,
        path,
        source: r.get(2)?,
        kind: r.get(3)?,
        daw: r.get(4)?,
        is_dir: r.get(5)?,
        size: r.get(6)?,
        mtime: r.get(7)?,
        title: r.get(8)?,
        bpm: r.get(9)?,
        key,
        camelot,
        tagged: r.get(11)?,
        state: r.get(12)?,
        detected_at: r.get(13)?,
    })
}

impl Db {
    pub fn open(path: &Path) -> Result<Db> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Db> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Db> {
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL;")?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version < 1 {
            conn.execute_batch(&format!("BEGIN; {SCHEMA} PRAGMA user_version = 1; COMMIT;"))?;
        }
        if version < 2 {
            conn.execute_batch(&format!("BEGIN; {MIGRATION_2} PRAGMA user_version = 2; COMMIT;"))?;
        }
        if version < 3 {
            conn.execute_batch(&format!("BEGIN; {MIGRATION_3} PRAGMA user_version = {SCHEMA_VERSION}; COMMIT;"))?;
        }
        // Batches left "running" by a crash keep their recorded ops (undoable).
        conn.execute("UPDATE batches SET status = 'failed' WHERE status = 'running'", [])?;
        Ok(Db { conn })
    }

    // ── settings ──
    pub fn get_setting<T: DeserializeOwned>(&self, key: &str) -> Result<Option<T>> {
        let v: Option<String> = self
            .conn
            .query_row("SELECT value FROM settings WHERE key = ?1", [key], |r| r.get(0))
            .optional()?;
        Ok(match v {
            Some(s) => Some(serde_json::from_str(&s)?),
            None => None,
        })
    }

    pub fn set_setting<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings(key, value) VALUES (?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, serde_json::to_string(value)?],
        )?;
        Ok(())
    }

    // ── beats ──
    pub fn create_beat(&self, b: &NewBeat) -> Result<i64> {
        let now = now_ts();
        self.conn.execute(
            "INSERT INTO beats(name, folder, bpm, musical_key, camelot, status, created_at, updated_at, kind, authors, coauthors)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, ?8, ?9, ?10)",
            params![
                b.name,
                b.folder,
                b.bpm,
                b.key.map(|k| k.name().to_string()),
                b.key.map(|k| k.camelot_code()),
                b.status.clone().unwrap_or_else(|| "idea".into()),
                now,
                b.kind.clone().filter(|k| KINDS.contains(&k.as_str())).unwrap_or_else(|| "beat".into()),
                join_people(&b.authors),
                join_people(&b.coauthors),
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn beat_id_by_folder(&self, folder: &str) -> Result<Option<i64>> {
        Ok(self
            .conn
            .query_row("SELECT id FROM beats WHERE folder = ?1", [folder], |r| r.get(0))
            .optional()?)
    }

    pub fn get_beat(&self, id: i64) -> Result<Option<BeatRow>> {
        Ok(self
            .conn
            .query_row(&format!("{BEAT_SELECT} WHERE b.id = ?1"), [id], beat_from_row)
            .optional()?)
    }

    pub fn list_beats(&self, query: &str) -> Result<Vec<BeatRow>> {
        let (fts, nums) = split_query(query);
        let mut sql = String::from(BEAT_SELECT);
        let mut conds: Vec<String> = vec!["b.folder IS NOT NULL".into()];
        let mut args: Vec<rusqlite::types::Value> = Vec::new();
        if !fts.is_empty() {
            args.push(fts.into());
            conds.push(format!("b.id IN (SELECT rowid FROM beats_fts WHERE beats_fts MATCH ?{})", args.len()));
        }
        for n in nums {
            args.push(n.into());
            let i = args.len();
            // A number matches the BPM, or a word in the name ("Beat 2").
            conds.push(format!(
                "(CAST(round(b.bpm) AS INTEGER) = ?{i} OR (' ' || b.name || ' ') LIKE '% ' || ?{i} || ' %')"
            ));
        }
        sql.push_str(" WHERE ");
        sql.push_str(&conds.join(" AND "));
        sql.push_str(" ORDER BY b.updated_at DESC, b.id DESC");
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map(rusqlite::params_from_iter(args), beat_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn update_beat(&self, id: i64, p: &BeatPatch) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        if let Some(name) = &p.name {
            let name = name.trim();
            if !name.is_empty() {
                // A name someone typed is never replaced by a file's name.
                tx.execute("UPDATE beats SET name = ?2, auto_name = 0 WHERE id = ?1", params![id, name])?;
            }
        }
        if let Some(bpm) = p.bpm {
            let bpm = bpm.filter(|v| (20.0..=999.0).contains(v));
            tx.execute("UPDATE beats SET bpm = ?2 WHERE id = ?1", params![id, bpm])?;
        }
        if let Some(key) = &p.key {
            // A typo must not silently wipe the key: reject what we can't read.
            if let Some(raw) = key.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                if MusicalKey::parse(raw).is_none() {
                    return Err(Error::Msg(format!("bad_key:{raw}")));
                }
            }
            let (name, camelot) = key_fields(key.as_deref());
            tx.execute("UPDATE beats SET musical_key = ?2, camelot = ?3 WHERE id = ?1", params![id, name, camelot])?;
        }
        let clean = |v: &Option<String>| v.as_ref().map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
        if let Some(v) = &p.genre {
            tx.execute("UPDATE beats SET genre = ?2 WHERE id = ?1", params![id, clean(v)])?;
        }
        if let Some(v) = &p.mood {
            tx.execute("UPDATE beats SET mood = ?2 WHERE id = ?1", params![id, clean(v)])?;
        }
        if let Some(v) = &p.type_beat {
            tx.execute("UPDATE beats SET type_beat = ?2 WHERE id = ?1", params![id, clean(v)])?;
        }
        if let Some(s) = &p.status {
            if STATUSES.contains(&s.as_str()) {
                tx.execute("UPDATE beats SET status = ?2 WHERE id = ?1", params![id, s])?;
            }
        }
        if let Some(r) = p.rating {
            tx.execute("UPDATE beats SET rating = ?2 WHERE id = ?1", params![id, r.clamp(0, 5)])?;
        }
        if let Some(c) = &p.color {
            tx.execute("UPDATE beats SET color = ?2 WHERE id = ?1", params![id, clean(c)])?;
        }
        if let Some(kind) = &p.kind {
            if !KINDS.contains(&kind.as_str()) {
                return Err(Error::Msg(format!("bad_kind:{kind}")));
            }
            tx.execute("UPDATE beats SET kind = ?2 WHERE id = ?1", params![id, kind])?;
        }
        if let Some(a) = &p.authors {
            tx.execute("UPDATE beats SET authors = ?2 WHERE id = ?1", params![id, join_people(a)])?;
        }
        if let Some(a) = &p.coauthors {
            tx.execute("UPDATE beats SET coauthors = ?2 WHERE id = ?1", params![id, join_people(a)])?;
        }
        tx.execute("UPDATE beats SET updated_at = ?2 WHERE id = ?1", params![id, now_ts()])?;
        tx.commit()?;
        Ok(())
    }

    /// Items of a kind that live in the library (have a folder).
    pub fn count_kind(&self, kind: &str) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM beats WHERE kind = ?1 AND folder IS NOT NULL",
            [kind],
            |r| r.get(0),
        )?)
    }

    /// An empty item: a row without a folder on disk. The folder appears,
    /// named after the first file, when files are added.
    pub fn create_draft(&self, name: &str, kind: &str, authors: &[String]) -> Result<i64> {
        if !KINDS.contains(&kind) {
            return Err(Error::Msg(format!("bad_kind:{kind}")));
        }
        let now = now_ts();
        self.conn.execute(
            "INSERT INTO beats(name, folder, status, created_at, updated_at, kind, authors, draft, auto_name)
             VALUES (?1, NULL, ?2, ?3, ?3, ?4, ?5, 1, 1)",
            params![name.trim(), if kind == "voicetag" { "ready" } else { "idea" }, now, kind, join_people(authors)],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    /// `(is a draft, name is still automatic)`.
    pub fn draft_state(&self, id: i64) -> Result<Option<(bool, bool)>> {
        Ok(self
            .conn
            .query_row("SELECT draft, auto_name FROM beats WHERE id = ?1", [id], |r| {
                Ok((r.get::<_, i64>(0)? != 0, r.get::<_, i64>(1)? != 0))
            })
            .optional()?)
    }

    /// The draft got its folder: from now on it is an ordinary item.
    pub fn adopt_draft(&self, id: i64, folder: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE beats SET folder = ?2, draft = 0, auto_name = 0, updated_at = ?3 WHERE id = ?1",
            params![id, folder, now_ts()],
        )?;
        Ok(())
    }

    /// Remove an item that has no folder on disk (an empty draft, or a row
    /// whose folder is gone). Refuses anything that has a folder: files are
    /// never dropped from the library this way.
    pub fn delete_draft(&self, id: i64) -> Result<bool> {
        let n = self.conn.execute("DELETE FROM beats WHERE id = ?1 AND folder IS NULL", [id])?;
        if n > 0 {
            self.conn.execute("DELETE FROM assets WHERE beat_id = ?1", [id])?;
        }
        Ok(n > 0)
    }

    /// Items of a kind as the sidebar counts them: everything with a folder, plus drafts.
    pub fn count_items(&self, kind: &str) -> Result<i64> {
        Ok(self.conn.query_row(
            "SELECT COUNT(*) FROM beats WHERE kind = ?1 AND (folder IS NOT NULL OR draft = 1)",
            [kind],
            |r| r.get(0),
        )?)
    }

    pub fn set_beat_folder(&self, id: i64, folder: Option<&str>) -> Result<()> {
        self.conn.execute("UPDATE beats SET folder = ?2 WHERE id = ?1", params![id, folder])?;
        Ok(())
    }

    // ── assets ──
    pub fn asset_state(&self, path: &str) -> Result<Option<(i64, Option<i64>, Option<i64>, Option<i64>)>> {
        Ok(self
            .conn
            .query_row("SELECT id, size, mtime, beat_id FROM assets WHERE path = ?1", [path], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
            })
            .optional()?)
    }

    pub fn upsert_asset(&self, a: &AssetUpsert) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO assets(beat_id, path, kind, daw, is_dir, is_backup, size, mtime, duration_ms, sample_rate, channels, codec)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
             ON CONFLICT(path) DO UPDATE SET beat_id = excluded.beat_id, kind = excluded.kind, daw = excluded.daw,
               is_dir = excluded.is_dir, is_backup = excluded.is_backup, size = excluded.size, mtime = excluded.mtime,
               duration_ms = excluded.duration_ms, sample_rate = excluded.sample_rate, channels = excluded.channels,
               codec = excluded.codec,
               peaks = CASE WHEN assets.size IS excluded.size AND assets.mtime IS excluded.mtime THEN assets.peaks ELSE NULL END",
            params![
                a.beat_id, a.path, a.kind, a.daw, a.is_dir, a.is_backup, a.size, a.mtime, a.duration_ms,
                a.sample_rate, a.channels, a.codec
            ],
        )?;
        Ok(self.conn.query_row("SELECT id FROM assets WHERE path = ?1", [&a.path], |r| r.get(0))?)
    }

    pub fn set_asset_beat(&self, path: &str, beat_id: Option<i64>) -> Result<()> {
        self.conn.execute("UPDATE assets SET beat_id = ?2 WHERE path = ?1", params![path, beat_id])?;
        Ok(())
    }

    pub fn asset_paths_under(&self, prefix: &str) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT path FROM assets WHERE path = ?1 OR substr(path, 1, length(?1) + 1) = ?1 || '/'")?;
        let rows = stmt.query_map([prefix], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<String>>>()?)
    }

    pub fn delete_asset(&self, path: &str) -> Result<()> {
        self.conn.execute("DELETE FROM assets WHERE path = ?1", [path])?;
        Ok(())
    }

    pub fn assets_of_beat(&self, beat_id: i64, include_backups: bool) -> Result<Vec<AssetRow>> {
        let sql = format!(
            "{ASSET_SELECT} WHERE beat_id = ?1 {} ORDER BY kind, path",
            if include_backups { "" } else { "AND is_backup = 0" }
        );
        let mut stmt = self.conn.prepare(&sql)?;
        let rows = stmt.query_map([beat_id], asset_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn asset(&self, id: i64) -> Result<Option<AssetRow>> {
        Ok(self
            .conn
            .query_row(&format!("{ASSET_SELECT} WHERE id = ?1"), [id], asset_from_row)
            .optional()?)
    }

    pub fn peaks(&self, asset_id: i64) -> Result<Option<Vec<u8>>> {
        Ok(self
            .conn
            .query_row("SELECT peaks FROM assets WHERE id = ?1", [asset_id], |r| r.get(0))
            .optional()?
            .flatten())
    }

    pub fn set_peaks(&self, asset_id: i64, peaks: &[u8], duration_ms: Option<i64>) -> Result<()> {
        self.conn.execute(
            "UPDATE assets SET peaks = ?2, duration_ms = COALESCE(?3, duration_ms) WHERE id = ?1",
            params![asset_id, peaks, duration_ms],
        )?;
        Ok(())
    }

    /// Keep relative paths in step with files moved on disk.
    pub fn apply_moves(&self, root: &Path, moves: &[(PathBuf, PathBuf)]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        for (from, to) in moves {
            let (Some(rf), rt) = (rel_string(root, from), rel_string(root, to)) else { continue };
            match rt {
                Some(rt) => {
                    for table_col in [("assets", "path"), ("beats", "folder")] {
                        let (t, c) = table_col;
                        tx.execute(
                            &format!(
                                "UPDATE {t} SET {c} = ?2 || substr({c}, length(?1) + 1)
                                 WHERE {c} = ?1 OR substr({c}, 1, length(?1) + 1) = ?1 || '/'"
                            ),
                            params![rf, rt],
                        )?;
                    }
                }
                None => {
                    tx.execute(
                        "DELETE FROM assets WHERE path = ?1 OR substr(path, 1, length(?1) + 1) = ?1 || '/'",
                        [&rf],
                    )?;
                    tx.execute(
                        "UPDATE beats SET folder = NULL WHERE folder = ?1 OR substr(folder, 1, length(?1) + 1) = ?1 || '/'",
                        [&rf],
                    )?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Drop assets that are no longer inside their beat folder, and beats
    /// that lost both their folder and their files.
    pub fn prune(&self) -> Result<()> {
        self.conn.execute_batch(
            "DELETE FROM assets WHERE NOT EXISTS (
               SELECT 1 FROM beats b WHERE b.id = assets.beat_id AND b.folder IS NOT NULL
               AND substr(assets.path, 1, length(b.folder) + 1) = b.folder || '/');
             DELETE FROM beats WHERE folder IS NULL AND draft = 0 AND NOT EXISTS (SELECT 1 FROM assets WHERE beat_id = beats.id);",
        )?;
        Ok(())
    }

    pub fn beat_folders(&self) -> Result<Vec<(i64, String)>> {
        let mut stmt = self.conn.prepare("SELECT id, folder FROM beats WHERE folder IS NOT NULL")?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    // ── inbox ──
    pub fn upsert_inbox(&self, i: &InboxUpsert) -> Result<bool> {
        let existing: Option<(Option<i64>, Option<i64>, String)> = self
            .conn
            .query_row("SELECT size, mtime, state FROM inbox WHERE path = ?1", [&i.path], |r| {
                Ok((r.get(0)?, r.get(1)?, r.get(2)?))
            })
            .optional()?;
        match existing {
            Some((size, mtime, _)) if size == i.size && mtime == i.mtime => Ok(false),
            Some((_, _, state)) => {
                self.conn.execute(
                    "UPDATE inbox SET size = ?2, mtime = ?3, kind = ?4, daw = ?5, is_dir = ?6 WHERE path = ?1",
                    params![i.path, i.size, i.mtime, i.kind, i.daw, i.is_dir],
                )?;
                Ok(state == "pending")
            }
            None => {
                self.conn.execute(
                    "INSERT INTO inbox(path, source, kind, daw, is_dir, size, mtime, title, bpm, musical_key, tagged, state, detected_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'pending', ?12)",
                    params![
                        i.path, i.source, i.kind, i.daw, i.is_dir, i.size, i.mtime, i.title, i.bpm, i.key, i.tagged,
                        now_ts()
                    ],
                )?;
                Ok(true)
            }
        }
    }

    pub fn list_inbox(&self, state: &str) -> Result<Vec<InboxItem>> {
        let mut stmt = self.conn.prepare(&format!("{INBOX_SELECT} WHERE state = ?1 ORDER BY detected_at DESC, id DESC"))?;
        let rows = stmt.query_map([state], inbox_from_row)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn inbox_item(&self, id: i64) -> Result<Option<InboxItem>> {
        Ok(self
            .conn
            .query_row(&format!("{INBOX_SELECT} WHERE id = ?1"), [id], inbox_from_row)
            .optional()?)
    }

    pub fn inbox_paths(&self) -> Result<Vec<String>> {
        let mut stmt = self.conn.prepare("SELECT path FROM inbox")?;
        let rows = stmt.query_map([], |r| r.get(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<String>>>()?)
    }

    pub fn set_inbox_state(&self, id: i64, state: &str) -> Result<()> {
        self.conn.execute("UPDATE inbox SET state = ?2 WHERE id = ?1", params![id, state])?;
        Ok(())
    }

    pub fn remove_inbox_path(&self, path: &str) -> Result<bool> {
        Ok(self.conn.execute("DELETE FROM inbox WHERE path = ?1", [path])? > 0)
    }

    pub fn pending_inbox_count(&self) -> Result<i64> {
        Ok(self.conn.query_row("SELECT COUNT(*) FROM inbox WHERE state = 'pending'", [], |r| r.get(0))?)
    }

    // ── journal ──
    pub fn list_batches(&self, limit: i64) -> Result<Vec<BatchRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT b.id, b.label, b.created_at, b.status, b.undo_of, b.undone_by,
                    (SELECT COUNT(*) FROM ops o WHERE o.batch_id = b.id)
             FROM batches b ORDER BY b.id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], |r| {
            let status: String = r.get(3)?;
            let undone_by: Option<i64> = r.get(5)?;
            let op_count: i64 = r.get(6)?;
            Ok(BatchRow {
                id: r.get(0)?,
                label: r.get(1)?,
                created_at: r.get(2)?,
                can_undo: undone_by.is_none() && op_count > 0 && status != "rolled_back",
                status,
                undo_of: r.get(4)?,
                undone_by,
                op_count,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn batch_label(&self, id: i64) -> Result<Option<String>> {
        Ok(self.conn.query_row("SELECT label FROM batches WHERE id = ?1", [id], |r| r.get(0)).optional()?)
    }

    pub fn batch_ops(&self, id: i64) -> Result<Vec<DoneOp>> {
        let mut stmt = self.conn.prepare("SELECT op FROM ops WHERE batch_id = ?1 ORDER BY seq")?;
        let rows = stmt.query_map([id], |r| r.get::<_, String>(0))?;
        let mut out = Vec::new();
        for s in rows {
            out.push(serde_json::from_str(&s?)?);
        }
        Ok(out)
    }

    pub fn last_undoable_batch(&self) -> Result<Option<i64>> {
        Ok(self.list_batches(50)?.into_iter().find(|b| b.can_undo).map(|b| b.id))
    }
}

impl Journal for Db {
    fn begin_batch(&mut self, label: &str, undo_of: Option<i64>) -> Result<i64> {
        self.conn.execute(
            "INSERT INTO batches(label, created_at, status, undo_of) VALUES (?1, ?2, 'running', ?3)",
            params![label, now_ts(), undo_of],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    fn record_op(&mut self, batch: i64, seq: usize, op: &DoneOp) -> Result<()> {
        self.conn.execute(
            "INSERT INTO ops(batch_id, seq, op) VALUES (?1, ?2, ?3)",
            params![batch, seq as i64, serde_json::to_string(op)?],
        )?;
        Ok(())
    }

    fn finish_batch(&mut self, batch: i64, status: BatchStatus) -> Result<()> {
        self.conn.execute("UPDATE batches SET status = ?2 WHERE id = ?1", params![batch, status.as_str()])?;
        if status == BatchStatus::Done {
            self.conn.execute(
                "UPDATE batches SET undone_by = ?1 WHERE id = (SELECT undo_of FROM batches WHERE id = ?1)",
                [batch],
            )?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn beats_search_and_patch() {
        let db = Db::open_in_memory().unwrap();
        let id = db
            .create_beat(&NewBeat {
                name: "Night Drive".into(),
                folder: Some("01_Beats/2026/Night_140bpm_Am".into()),
                bpm: Some(140.0),
                key: MusicalKey::parse("Am"),
                status: None,
                ..Default::default()
            })
            .unwrap();
        db.create_beat(&NewBeat { name: "Тёмный лес".into(), folder: Some("01_Beats/2026/Les".into()), ..Default::default() })
            .unwrap();

        assert_eq!(db.list_beats("").unwrap().len(), 2);
        assert_eq!(db.list_beats("nig").unwrap()[0].id, id);
        assert_eq!(db.list_beats("140").unwrap().len(), 1);
        assert_eq!(db.list_beats("8a").unwrap().len(), 1);
        // ё and е are interchangeable in search
        assert_eq!(db.list_beats("темн").unwrap().len(), 1);
        assert_eq!(db.list_beats("тёмн").unwrap().len(), 1);

        let patch: BeatPatch = serde_json::from_str(r#"{"genre":"Drill","key":"F#m","bpm":null,"rating":9}"#).unwrap();
        db.update_beat(id, &patch).unwrap();
        let b = db.get_beat(id).unwrap().unwrap();
        assert_eq!(b.genre.as_deref(), Some("Drill"));
        assert_eq!(b.key.as_deref(), Some("F#m"));
        assert_eq!(b.camelot.as_deref(), Some("11A"));
        assert_eq!(b.bpm, None);
        assert_eq!(b.rating, 5);
        assert_eq!(db.list_beats("drill").unwrap().len(), 1);
    }

    #[test]
    fn moves_update_paths_without_like_wildcards() {
        let db = Db::open_in_memory().unwrap();
        let root = Path::new("/lib");
        let id = db
            .create_beat(&NewBeat { name: "A".into(), folder: Some("01_Beats/2026/A_1".into()), ..Default::default() })
            .unwrap();
        let other = db
            .create_beat(&NewBeat { name: "B".into(), folder: Some("01_Beats/2026/AX1".into()), ..Default::default() })
            .unwrap();
        db.upsert_asset(&AssetUpsert {
            beat_id: Some(id),
            path: "01_Beats/2026/A_1/Bounces/a.wav".into(),
            kind: "audio".into(),
            ..Default::default()
        })
        .unwrap();
        db.apply_moves(root, &[(root.join("01_Beats/2026/A_1"), root.join("01_Beats/2026/A_2"))]).unwrap();
        assert_eq!(db.get_beat(id).unwrap().unwrap().folder.as_deref(), Some("01_Beats/2026/A_2"));
        // '_' must not act as a wildcard and touch "AX1"
        assert_eq!(db.get_beat(other).unwrap().unwrap().folder.as_deref(), Some("01_Beats/2026/AX1"));
        assert_eq!(db.assets_of_beat(id, true).unwrap()[0].path, "01_Beats/2026/A_2/Bounces/a.wav");
        db.prune().unwrap();
        assert_eq!(db.assets_of_beat(id, true).unwrap().len(), 1);
    }
}

#[cfg(test)]
mod key_validation_tests {
    use super::*;

    #[test]
    fn bad_key_is_rejected_and_old_value_kept() {
        let db = Db::open_in_memory().unwrap();
        let id = db
            .create_beat(&NewBeat { name: "Night".into(), folder: Some("01_Beats/Night".into()), key: MusicalKey::parse("Am"), ..Default::default() })
            .unwrap();
        let err = db.update_beat(id, &BeatPatch { key: Some(Some("Q#z".into())), ..Default::default() });
        assert!(matches!(err, Err(Error::Msg(m)) if m.starts_with("bad_key:")));
        assert_eq!(db.get_beat(id).unwrap().unwrap().key.as_deref(), Some("Am"));
        db.update_beat(id, &BeatPatch { key: Some(None), ..Default::default() }).unwrap();
        assert_eq!(db.get_beat(id).unwrap().unwrap().key, None);
    }
}

#[cfg(test)]
mod v2_tests {
    use super::*;

    #[test]
    fn kinds_and_people_roundtrip_and_search() {
        let db = Db::open_in_memory().unwrap();
        let id = db
            .create_beat(&NewBeat {
                name: "Ёлка".into(),
                folder: Some("06_Loops_Starters/Loops/Ёлка".into()),
                kind: Some("loop".into()),
                authors: vec!["maks".into(), " Maks ".into()],
                coauthors: vec!["Lowkey".into()],
                ..Default::default()
            })
            .unwrap();
        let b = db.get_beat(id).unwrap().unwrap();
        assert_eq!(b.kind, "loop");
        assert_eq!(b.authors, vec!["maks"]);
        assert_eq!(b.coauthors, vec!["Lowkey"]);
        assert_eq!(db.list_beats("lowkey").unwrap().len(), 1);
        assert_eq!(db.list_beats("елка").unwrap().len(), 1);
        db.update_beat(id, &BeatPatch { kind: Some("starter".into()), coauthors: Some(vec![]), ..Default::default() }).unwrap();
        let b = db.get_beat(id).unwrap().unwrap();
        assert_eq!(b.kind, "starter");
        assert!(b.coauthors.is_empty());
        assert_eq!(db.list_beats("lowkey").unwrap().len(), 0);
        assert!(db.update_beat(id, &BeatPatch { kind: Some("song".into()), ..Default::default() }).is_err());
    }

    #[test]
    fn v1_database_is_migrated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("old.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(&format!("BEGIN; {SCHEMA} PRAGMA user_version = 1; COMMIT;")).unwrap();
            conn.execute(
                "INSERT INTO beats(name, folder, status, created_at, updated_at) VALUES ('Старый', '01_Beats/2026/Old', 'idea', 1, 1)",
                [],
            )
            .unwrap();
        }
        let db = Db::open(&path).unwrap();
        let rows = db.list_beats("старый").unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, "beat");
        assert!(rows[0].authors.is_empty());
    }
}

#[cfg(test)]
mod cover_tests {
    use super::*;

    #[test]
    fn cover_is_the_image_named_cover_in_the_beat_folder() {
        let db = Db::open_in_memory().unwrap();
        let folder = "01_Beats/2026/Night";
        let id = db.create_beat(&NewBeat { name: "Night".into(), folder: Some(folder.into()), ..Default::default() }).unwrap();
        assert_eq!(db.get_beat(id).unwrap().unwrap().cover, None);
        let image = |path: &str, mtime: i64| AssetUpsert {
            beat_id: Some(id),
            path: format!("{folder}/{path}"),
            kind: "image".into(),
            mtime: Some(mtime),
            ..Default::default()
        };
        db.upsert_asset(&image("Artwork/sketch.png", 9)).unwrap();
        assert_eq!(db.get_beat(id).unwrap().unwrap().cover, None, "other pictures are not the cover");
        db.upsert_asset(&image("Artwork/Cover.JPG", 5)).unwrap();
        let b = db.get_beat(id).unwrap().unwrap();
        assert_eq!(b.cover.as_deref(), Some("01_Beats/2026/Night/Artwork/Cover.JPG"));
        assert_eq!(b.cover_v, Some(5));
    }
}

#[cfg(test)]
mod draft_tests {
    use super::*;

    #[test]
    fn drafts_survive_cleanup_and_become_ordinary_items() {
        let db = Db::open_in_memory().unwrap();
        let id = db.create_draft("Без названия", "beat", &["maks".into()]).unwrap();
        db.prune().unwrap();
        let b = db.get_beat(id).unwrap().expect("an empty draft is not cleaned up");
        assert_eq!((b.name.as_str(), b.folder.as_deref(), b.kind.as_str()), ("Без названия", None, "beat"));
        assert_eq!(db.draft_state(id).unwrap(), Some((true, true)));
        assert_eq!(db.count_items("beat").unwrap(), 1);
        assert_eq!(db.count_kind("beat").unwrap(), 0, "no folder yet, so it does not lock the template");

        // Typing a name turns the automatic name off.
        db.update_beat(id, &BeatPatch { name: Some("Night".into()), ..Default::default() }).unwrap();
        assert_eq!(db.draft_state(id).unwrap(), Some((true, false)));

        db.adopt_draft(id, "01_Beats/2026/Night").unwrap();
        assert_eq!(db.draft_state(id).unwrap(), Some((false, false)));
        assert_eq!(db.get_beat(id).unwrap().unwrap().folder.as_deref(), Some("01_Beats/2026/Night"));
        assert!(!db.delete_draft(id).unwrap(), "an item with a folder is never deleted this way");
        assert!(db.get_beat(id).unwrap().is_some());

        let empty = db.create_draft("Без названия", "voicetag", &[]).unwrap();
        assert_eq!(db.get_beat(empty).unwrap().unwrap().status, "ready");
        assert!(db.delete_draft(empty).unwrap());
        assert!(db.get_beat(empty).unwrap().is_none());
        assert!(db.create_draft("x", "song", &[]).is_err());
    }
}
