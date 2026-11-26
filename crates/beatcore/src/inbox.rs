//! Inbox and DAW-folder watching logic (no threads here, so it is testable).
//!
//! A new file only enters the sorting queue once its size and modification
//! time have stopped changing for `settle` — FL Studio writes renders
//! gradually, and grabbing a half-written WAV is exactly the bug to avoid.

use crate::daw::{
    ableton_project_root, classify_file, ext_lower, is_ableton_project_dir, is_audio, is_backup_path, is_junk,
    AssetKind,
};
use crate::db::InboxUpsert;
use crate::parse::{parse_folder_name, parse_name};
use crate::pathsafe::is_within;
use crate::scan::{display_stem, mtime_secs};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Inbox,
    Fl,
    Ableton,
}

impl Source {
    pub fn as_str(&self) -> &'static str {
        match self {
            Source::Inbox => "inbox",
            Source::Fl => "fl",
            Source::Ableton => "ableton",
        }
    }
}

#[derive(Debug, Clone)]
struct Obs {
    size: u64,
    mtime: Option<SystemTime>,
    since: Instant,
}

#[derive(Debug)]
pub struct StabilityTracker {
    pending: HashMap<PathBuf, Obs>,
    pub settle: Duration,
}

pub type Measure = Option<(u64, Option<SystemTime>)>;

impl StabilityTracker {
    pub fn new(settle: Duration) -> Self {
        StabilityTracker { pending: HashMap::new(), settle }
    }

    /// Something happened at `p`: (re)start watching it.
    pub fn touch(&mut self, p: PathBuf, now: Instant) {
        self.pending.insert(p, Obs { size: u64::MAX, mtime: None, since: now });
    }

    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Returns (stable, gone). Stable paths leave the tracker.
    pub fn poll(&mut self, now: Instant, measure: impl Fn(&Path) -> Measure) -> (Vec<PathBuf>, Vec<PathBuf>) {
        let mut stable = Vec::new();
        let mut gone = Vec::new();
        for (p, obs) in self.pending.iter_mut() {
            match measure(p) {
                None => gone.push(p.clone()),
                Some((size, mtime)) => {
                    if size == obs.size && mtime == obs.mtime {
                        if now.duration_since(obs.since) >= self.settle {
                            stable.push(p.clone());
                        }
                    } else {
                        *obs = Obs { size, mtime, since: now };
                    }
                }
            }
        }
        for p in stable.iter().chain(gone.iter()) {
            self.pending.remove(p);
        }
        (stable, gone)
    }
}

/// Size and newest mtime; for folders, summed over the tree.
pub fn measure(p: &Path) -> Measure {
    let meta = std::fs::metadata(p).ok()?;
    if !meta.is_dir() {
        return Some((meta.len(), meta.modified().ok()));
    }
    let mut size = 0u64;
    let mut newest = meta.modified().ok();
    for e in walkdir::WalkDir::new(p).into_iter().take(50_000).flatten() {
        if let Ok(m) = e.metadata() {
            if m.is_file() {
                size += m.len();
            }
            let t = m.modified().ok();
            if t > newest {
                newest = t;
            }
        }
    }
    Some((size, newest))
}

fn daw_relevant(p: &Path) -> bool {
    matches!(ext_lower(p).as_str(), "flp" | "zip" | "als" | "mid" | "midi") || is_audio(p)
}

fn technical_dir(p: &Path) -> bool {
    let name = p.file_name().unwrap_or_default().to_string_lossy();
    ["Ableton Project Info", "Backup", "Backups", "Auto-Backups", "Project bones"].iter()
        .any(|n| name.eq_ignore_ascii_case(n))
}

/// Folder supplying the original project name. For an export in a subfolder,
/// prefer the closest enclosing FL project within the selected watch root.
pub fn project_name_at(path: &Path, root: &Path) -> String {
    let parent = if path.is_dir() { path } else { path.parent().unwrap_or(path) };
    let dir = if path.is_dir() { path } else {
        parent.ancestors().take_while(|p| p.starts_with(root)).find(|p| {
            std::fs::read_dir(p).ok().is_some_and(|entries| entries.flatten().any(|e| {
                matches!(ext_lower(&e.path()).as_str(), "flp" | "als") && !is_backup_path(&e.path())
            }))
        }).unwrap_or(parent)
    };
    dir.file_name().unwrap_or_default().to_string_lossy().into_owned()
}

/// Which queue item does a file-system event at `path` belong to?
pub fn candidate_for(path: &Path, watch_root: &Path, source: Source) -> Option<PathBuf> {
    if !is_within(path, watch_root) || path == watch_root {
        return None;
    }
    match source {
        Source::Inbox => {
            let rel = path.strip_prefix(watch_root).ok()?;
            let first = rel.components().next()?;
            let top = watch_root.join(first.as_os_str());
            if is_junk(&top) {
                None
            } else {
                Some(top)
            }
        }
        Source::Fl | Source::Ableton => {
            if is_backup_path(path) || path.ancestors().take_while(|p| *p != watch_root)
                .any(|p| is_junk(p) || technical_dir(p) || p.is_symlink()) {
                return None;
            }
            if let Some(proj) = ableton_project_root(path) {
                if is_within(&proj, watch_root) {
                    return Some(proj);
                }
            }
            if daw_relevant(path) {
                Some(path.to_path_buf())
            } else {
                None
            }
        }
    }
}

/// Items already present in a watched folder (start-up and catch-up scan).
pub fn scan_candidates(watch_root: &Path, source: Source) -> Vec<PathBuf> {
    let mut out = Vec::new();
    match source {
        Source::Inbox => {
            if let Ok(rd) = std::fs::read_dir(watch_root) {
                for e in rd.flatten() {
                    if !is_junk(&e.path()) {
                        out.push(e.path());
                    }
                }
            }
        }
        Source::Fl | Source::Ableton => {
            if is_ableton_project_dir(watch_root) {
                return vec![watch_root.to_path_buf()];
            }
            let mut it = walkdir::WalkDir::new(watch_root).min_depth(1).follow_links(false).into_iter();
            while let Some(entry) = it.next() {
                let Ok(entry) = entry else { continue };
                let p = entry.path();
                if entry.file_type().is_symlink() || is_junk(p) || is_backup_path(p) || technical_dir(p) {
                    if entry.file_type().is_dir() {
                        it.skip_current_dir();
                    }
                    continue;
                }
                if entry.file_type().is_dir() {
                    if is_ableton_project_dir(p) {
                        out.push(p.to_path_buf());
                        it.skip_current_dir();
                    }
                } else if daw_relevant(p) {
                    out.push(p.to_path_buf());
                }
            }
        }
    }
    out
}

/// Build the queue record for a stable candidate. DAW folders only feed
/// project and audio files; the Inbox accepts anything.
pub fn describe(path: &Path, source: Source, root: &Path) -> Option<InboxUpsert> {
    let meta = std::fs::metadata(path).ok()?;
    let (kind, is_dir) = if meta.is_dir() {
        if is_ableton_project_dir(path) {
            (AssetKind::AbletonProject, true)
        } else {
            (AssetKind::Folder, true)
        }
    } else {
        (classify_file(path), false)
    };
    if source != Source::Inbox
        && !matches!(
            kind,
            AssetKind::FlProject
                | AssetKind::FlZip
                | AssetKind::AbletonProject
                | AssetKind::AbletonSet
                | AssetKind::Audio
                | AssetKind::Midi
        )
    {
        return None;
    }
    let (size, newest) = measure(path)?;
    let folder_name = if source == Source::Inbox {
        if is_dir { path.file_name().map(|n| n.to_string_lossy().into_owned()) } else { None }
    } else {
        Some(project_name_at(path, root))
    };
    let input = folder_name.clone().unwrap_or_else(|| display_stem(path));
    let title_source = if kind == AssetKind::AbletonProject {
        input.strip_suffix(" Project").unwrap_or(&input)
    } else { &input };
    let parsed = if folder_name.is_some() { parse_folder_name(title_source) } else { parse_name(title_source) };
    let mtime = newest
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .or_else(|| mtime_secs(&meta));
    Some(InboxUpsert {
        path: path.to_string_lossy().to_string(),
        source: source.as_str().into(),
        kind: kind.as_str().into(),
        daw: kind.daw().map(|d| d.as_str().into()),
        is_dir,
        size: Some(size as i64),
        mtime,
        folder_name,
        title: Some(parsed.title),
        bpm: parsed.bpm.map(|v| v as f64),
        key: parsed.key.map(|k| k.name().to_string()),
        tagged: parsed.tagged,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waits_until_writes_settle() {
        let mut t = StabilityTracker::new(Duration::from_secs(3));
        let start = Instant::now();
        let p = PathBuf::from("/in/render.wav");
        t.touch(p.clone(), start);
        // still growing
        let (s, g) = t.poll(start, |_| Some((100, None)));
        assert!(s.is_empty() && g.is_empty());
        let (s, _) = t.poll(start + Duration::from_secs(2), |_| Some((5_000, None)));
        assert!(s.is_empty());
        // unchanged but not long enough
        let (s, _) = t.poll(start + Duration::from_secs(4), |_| Some((5_000, None)));
        assert!(s.is_empty());
        // unchanged for >= 3s since last change at t=2
        let (s, _) = t.poll(start + Duration::from_secs(5), |_| Some((5_000, None)));
        assert_eq!(s, vec![p.clone()]);
        assert!(t.is_empty());
        // deleted before settling
        t.touch(p.clone(), start);
        let (s, g) = t.poll(start, |_| None);
        assert!(s.is_empty());
        assert_eq!(g, vec![p]);
    }

    #[test]
    fn candidates_resolve_to_queue_items() {
        let dir = tempfile::tempdir().unwrap();
        let inbox = dir.path().join("00_Inbox");
        let stems = inbox.join("Stems Night");
        std::fs::create_dir_all(&stems).unwrap();
        assert_eq!(candidate_for(&stems.join("kick.wav"), &inbox, Source::Inbox), Some(stems.clone()));
        assert_eq!(candidate_for(&inbox.join(".DS_Store"), &inbox, Source::Inbox), None);

        let fl = dir.path().join("FL Projects");
        std::fs::create_dir_all(fl.join("Backup")).unwrap();
        assert_eq!(candidate_for(&fl.join("Backup/Night (autosave).flp"), &fl, Source::Fl), None);
        assert_eq!(candidate_for(&fl.join("Night.flp"), &fl, Source::Fl), Some(fl.join("Night.flp")));
        assert_eq!(candidate_for(&fl.join("readme.txt"), &fl, Source::Fl), None);

        let proj = crate::daw::tests::make_ableton_project(&dir.path().join("Live"), "Night", &[]);
        let live = dir.path().join("Live");
        assert_eq!(
            candidate_for(&proj.join("Samples/Imported/kick.wav"), &live, Source::Ableton),
            Some(proj.clone())
        );
        let found = scan_candidates(&live, Source::Ableton);
        assert_eq!(found, vec![proj.clone()]);
        let d = describe(&proj, Source::Ableton, &live).unwrap();
        assert_eq!(d.kind, "ableton_project");
        assert_eq!(d.title.as_deref(), Some("Night"));
    }
}
