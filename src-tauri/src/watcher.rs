//! Native events and explicit scans share the same stable-write detector.
use beatcore::db::Db;
use beatcore::inbox::{candidate_for, describe, measure, scan_candidates, Source, StabilityTracker};
use beatcore::pathsafe::is_within;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

type Completion = Sender<Result<(), String>>;

enum Work {
    Event(PathBuf),
    Scan(Completion),
    Stop,
}

pub struct WatchService {
    _watcher: RecommendedWatcher,
    work: Sender<Work>,
}

impl WatchService {
    pub fn rescan(&self) -> Result<Receiver<Result<(), String>>, String> {
        let (tx, rx) = channel();
        self.work.send(Work::Scan(tx)).map_err(|_| "daw_unavailable".to_owned())?;
        Ok(rx)
    }
}

impl Drop for WatchService {
    fn drop(&mut self) { let _ = self.work.send(Work::Stop); }
}

pub struct Target {
    pub root: PathBuf,
    pub source: Source,
}

fn excluded(path: &Path, target: &Target, library: &Path) -> bool {
    target.source != Source::Inbox && is_within(path, library)
}

pub fn start(app: AppHandle, db_path: PathBuf, library: PathBuf, targets: Vec<Target>) -> notify::Result<WatchService> {
    let (work, rx) = channel();
    let events = work.clone();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(ev) = res {
            // Opening a file to recognise or measure it also generates access events.
            if matches!(ev.kind, EventKind::Access(_)) { return; }
            for p in ev.paths { let _ = events.send(Work::Event(p)); }
        }
    })?;
    for target in &targets {
        if target.root.is_dir() {
            if let Err(e) = watcher.watch(&target.root, RecursiveMode::Recursive) {
                log::warn!("cannot watch {}: {e}", target.root.display());
            }
        }
    }
    std::thread::spawn(move || {
        let db = match Db::open(&db_path) {
            Ok(db) => db,
            Err(e) => { log::error!("watcher database: {e}"); return; }
        };
        let mut tracker = StabilityTracker::new(Duration::from_secs(3));
        let mut origin = HashMap::new();
        let mut requests: Vec<(HashSet<PathBuf>, Completion)> = Vec::new();
        let mut last_poll = Instant::now();
        let mut initial = true;
        loop {
            let msg = if initial { None } else {
                match rx.recv_timeout(Duration::from_millis(250)) {
                    Ok(Work::Stop) | Err(RecvTimeoutError::Disconnected) => break,
                    Ok(msg) => Some(msg),
                    Err(RecvTimeoutError::Timeout) => None,
                }
            };
            let mut nominate = |p: PathBuf, i: usize| {
                if excluded(&p, &targets[i], &library) { return; }
                origin.insert(p.clone(), i);
                tracker.touch(p, Instant::now());
            };
            match msg {
                Some(Work::Event(path)) => {
                    for (i, t) in targets.iter().enumerate() {
                        if !path.starts_with(&t.root) || excluded(&path, t, &library) { continue; }
                        if let Some(p) = candidate_for(&path, &t.root, t.source) {
                            nominate(p, i);
                        } else if path.is_dir() && t.source != Source::Inbox {
                            // A folder moved into the watched tree can arrive as a single event.
                            for p in scan_candidates(&path, t.source) {
                                if let Some(c) = candidate_for(&p, &t.root, t.source) { nominate(c, i); }
                            }
                        }
                    }
                }
                msg if initial || matches!(msg, Some(Work::Scan(_))) => {
                    let mut pending = HashSet::new();
                    let mut missing = None;
                    for (i, t) in targets.iter().enumerate() {
                        if !initial && t.source == Source::Inbox { continue; }
                        if !t.root.is_dir() { missing = Some(t.root.display().to_string()); continue; }
                        for p in scan_candidates(&t.root, t.source) {
                            if excluded(&p, t, &library) { continue; }
                            pending.insert(p.clone());
                            nominate(p, i);
                        }
                    }
                    if let Some(Work::Scan(done)) = msg {
                        if let Some(path) = missing { let _ = done.send(Err(path)); }
                        else if pending.is_empty() { let _ = done.send(Ok(())); }
                        else { requests.push((pending, done)); }
                    }
                    initial = false;
                }
                _ => {}
            }
            if last_poll.elapsed() < Duration::from_millis(800) || tracker.is_empty() { continue; }
            last_poll = Instant::now();
            let (stable, gone) = tracker.poll(last_poll, measure);
            let mut changed = false;
            for (p, exists) in stable.into_iter().map(|p| (p, true)).chain(gone.into_iter().map(|p| (p, false))) {
                let Some(i) = origin.remove(&p) else { continue };
                let result = if exists {
                    let t = &targets[i];
                    match describe(&p, t.source, &t.root) {
                        Some(item) => db.upsert_inbox(&item),
                        None => Ok(false),
                    }
                } else { db.remove_inbox_path(&p.to_string_lossy()) };
                if let Ok(updated) = &result { changed |= *updated; }
                if let Err(e) = &result { log::warn!("queue update: {e}"); }
                requests.retain_mut(|(pending, done)| {
                    if !pending.remove(&p) { return true; }
                    if let Err(e) = &result { let _ = done.send(Err(e.to_string())); return false; }
                    if pending.is_empty() { let _ = done.send(Ok(())); return false; }
                    true
                });
            }
            if changed { let _ = app.emit("inbox-changed", ()); }
        }
    });
    Ok(WatchService { _watcher: watcher, work })
}
