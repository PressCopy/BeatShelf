//! Indexes beat folders into the database. Only technical data is read from
//! files; musical fields come from the folder name or the user.

use crate::audio;
use crate::daw::{classify_file, ext_lower, is_ableton_project_dir, is_backup_path, is_junk, AssetKind};
use crate::db::{AssetUpsert, Db, NewBeat};
use crate::error::Result;
use crate::parse::parse_name;
use crate::pathsafe::{from_rel, rel_string};
use crate::template::{ItemKind, LibraryTemplate};
use std::collections::HashSet;
use std::path::Path;
use std::time::UNIX_EPOCH;

#[derive(Debug, Default, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScanReport {
    pub beat_folders: usize,
    pub beats_added: usize,
    pub assets_updated: usize,
    pub assets_removed: usize,
}

pub fn mtime_secs(m: &std::fs::Metadata) -> Option<i64> {
    m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs() as i64)
}

fn ableton_project_mtime(dir: &Path) -> Option<i64> {
    std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|e| ext_lower(&e.path()) == "als")
        .filter_map(|e| e.metadata().ok().as_ref().and_then(mtime_secs))
        .max()
}

/// Item folder (beat, loop or starter; relative) that contains `rel`.
pub fn beat_folder_of(tpl: &LibraryTemplate, rel: &str) -> Option<String> {
    tpl.item_folder_of(rel).map(|(f, _)| f)
}

/// Strip " Project" that Ableton appends to project folder names.
pub fn display_stem(path: &Path) -> String {
    let name = if path.is_dir() {
        path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
    } else {
        path.file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()
    };
    name.strip_suffix(" Project").map(|s| s.to_string()).unwrap_or(name)
}

#[derive(Debug, Default)]
pub struct FolderStats {
    pub beat_added: bool,
    pub updated: usize,
    pub removed: usize,
}

/// Bring one beat folder in sync. Creates the beat row if needed and sets
/// `folder = NULL` when the folder is gone.
pub fn refresh_beat_folder(db: &Db, root: &Path, rel: &str, kind: ItemKind) -> Result<FolderStats> {
    let mut stats = FolderStats::default();
    let abs = from_rel(root, rel);
    let existing = db.beat_id_by_folder(rel)?;
    if !abs.is_dir() {
        if let Some(id) = existing {
            db.set_beat_folder(id, None)?;
        }
        for p in db.asset_paths_under(rel)? {
            db.delete_asset(&p)?;
            stats.removed += 1;
        }
        return Ok(stats);
    }
    let beat_id = match existing {
        Some(id) => id,
        None => {
            let parsed = parse_name(&display_stem(&abs));
            stats.beat_added = true;
            db.create_beat(&NewBeat {
                name: parsed.title,
                folder: Some(rel.to_string()),
                bpm: parsed.bpm.map(|v| v as f64),
                key: parsed.key,
                status: None,
                kind: Some(kind.as_str().into()),
                authors: parsed.producers,
                coauthors: vec![],
            })?
        }
    };

    let mut seen: HashSet<String> = HashSet::new();
    let mut it = walkdir::WalkDir::new(&abs).min_depth(1).follow_links(false).into_iter();
    while let Some(entry) = it.next() {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if is_junk(path) {
            if entry.file_type().is_dir() {
                it.skip_current_dir();
            }
            continue;
        }
        let Some(asset_rel) = rel_string(root, path) else { continue };
        let Ok(meta) = entry.metadata() else { continue };
        let backup = is_backup_path(Path::new(&asset_rel));
        if entry.file_type().is_dir() {
            if is_ableton_project_dir(path) {
                it.skip_current_dir();
                let mtime = ableton_project_mtime(path).or_else(|| mtime_secs(&meta));
                seen.insert(asset_rel.clone());
                let prev = db.asset_state(&asset_rel)?;
                if prev.map(|(_, _, m, b)| m != mtime || b != Some(beat_id)).unwrap_or(true) {
                    db.upsert_asset(&AssetUpsert {
                        beat_id: Some(beat_id),
                        path: asset_rel,
                        kind: AssetKind::AbletonProject.as_str().into(),
                        daw: Some("ableton".into()),
                        is_dir: true,
                        is_backup: backup,
                        mtime,
                        ..Default::default()
                    })?;
                    stats.updated += 1;
                }
            }
            continue;
        }
        seen.insert(asset_rel.clone());
        let size = Some(meta.len() as i64);
        let mtime = mtime_secs(&meta);
        let prev = db.asset_state(&asset_rel)?;
        if let Some((_, s, m, b)) = prev {
            if s == size && m == mtime && b == Some(beat_id) {
                continue;
            }
        }
        let kind = classify_file(path);
        let info = if kind == AssetKind::Audio { audio::probe(path).unwrap_or_default() } else { Default::default() };
        db.upsert_asset(&AssetUpsert {
            beat_id: Some(beat_id),
            path: asset_rel,
            kind: kind.as_str().into(),
            daw: kind.daw().map(|d| d.as_str().into()),
            is_dir: false,
            is_backup: backup,
            size,
            mtime,
            duration_ms: info.duration_ms,
            sample_rate: info.sample_rate,
            channels: info.channels,
            codec: info.codec,
        })?;
        stats.updated += 1;
    }
    for p in db.asset_paths_under(rel)? {
        if !seen.contains(&p) {
            db.delete_asset(&p)?;
            stats.removed += 1;
        }
    }
    Ok(stats)
}

/// Full scan of every beat folder. `progress` gets the number of folders done.
pub fn scan_library(db: &Db, root: &Path, tpl: &LibraryTemplate, mut progress: impl FnMut(usize)) -> Result<ScanReport> {
    let mut report = ScanReport::default();
    let mut folders: Vec<(String, ItemKind)> = Vec::new();
    for kind in ItemKind::ALL {
        let prefix = tpl.static_prefix(kind);
        let depth = tpl.depth(kind);
        if depth <= prefix.len() {
            continue;
        }
        let base = from_rel(root, &prefix.join("/"));
        if !base.is_dir() {
            continue;
        }
        let mut it = walkdir::WalkDir::new(&base)
            .min_depth(1)
            .max_depth(depth - prefix.len())
            .follow_links(false)
            .into_iter();
        while let Some(entry) = it.next() {
            let Ok(entry) = entry else { continue };
            if !entry.file_type().is_dir() {
                continue;
            }
            if is_junk(entry.path()) || is_backup_path(entry.path()) {
                it.skip_current_dir();
                continue;
            }
            if entry.depth() == depth - prefix.len() {
                if let Some(rel) = rel_string(root, entry.path()) {
                    // Another kind may claim it when prefixes nest; trust the template.
                    if tpl.item_folder_of(&rel).map(|(f, k)| f == rel && k == kind).unwrap_or(false) {
                        folders.push((rel, kind));
                    }
                }
            }
        }
    }
    let on_disk: HashSet<String> = folders.iter().map(|(f, _)| f.clone()).collect();

    db.conn.execute_batch("BEGIN")?;
    let result = (|| -> Result<()> {
        let mut tx_count = 0;
        for (i, (rel, kind)) in folders.iter().enumerate() {
            let s = refresh_beat_folder(db, root, rel, *kind)?;
            report.beat_folders += 1;
            report.beats_added += s.beat_added as usize;
            report.assets_updated += s.updated;
            report.assets_removed += s.removed;
            tx_count += 1;
            if tx_count >= 200 {
                db.conn.execute_batch("COMMIT; BEGIN")?;
                tx_count = 0;
                progress(i + 1);
            }
        }
        for (id, folder) in db.beat_folders()? {
            if !on_disk.contains(&folder) {
                db.set_beat_folder(id, None)?;
            }
        }
        Ok(())
    })();
    match result {
        Ok(()) => db.conn.execute_batch("COMMIT")?,
        Err(e) => {
            let _ = db.conn.execute_batch("ROLLBACK");
            return Err(e);
        }
    }
    db.prune()?;
    progress(folders.len());
    Ok(report)
}
