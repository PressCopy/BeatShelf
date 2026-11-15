//! Watches the Inbox and the DAW folders. Events only nominate candidates;
//! a candidate is queued once its writes have settled (see beatcore::inbox).

use beatcore::db::Db;
use beatcore::inbox::{candidate_for, describe, measure, scan_candidates, Source, StabilityTracker};
use beatcore::pathsafe::is_within;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter};

pub struct WatchService {
    _watcher: RecommendedWatcher,
    stop: Arc<AtomicBool>,
}

impl Drop for WatchService {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

pub struct Target {
    pub root: PathBuf,
    pub source: Source,
    pub since: Option<i64>,
}

pub fn start(
    app: AppHandle,
    db_path: PathBuf,
    library_root: PathBuf,
    targets: Vec<Target>,
) -> notify::Result<WatchService> {
    let (tx, rx) = channel::<(PathBuf, usize)>();
    let targets = Arc::new(targets);

    let event_targets = targets.clone();
    let event_tx = tx.clone();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        let Ok(ev) = res else { return };
        for p in ev.paths {
            for (i, t) in event_targets.iter().enumerate() {
                if let Some(c) = candidate_for(&p, &t.root, t.source) {
                    let _ = event_tx.send((c, i));
                }
            }
        }
    })?;
    for t in targets.iter() {
        if t.root.is_dir() {
            if let Err(e) = watcher.watch(&t.root, RecursiveMode::Recursive) {
                log::warn!("cannot watch {}: {e}", t.root.display());
            }
        }
    }

    let stop = Arc::new(AtomicBool::new(false));
    let stop_thread = stop.clone();
    let initial_targets = targets.clone();
    std::thread::spawn(move || {
        // Catch up with what changed while the app was closed.
        for (i, t) in initial_targets.iter().enumerate() {
            for c in scan_candidates(&t.root, t.source) {
                let _ = tx.send((c, i));
            }
        }
        let db = match Db::open(&db_path) {
            Ok(db) => db,
            Err(e) => {
                log::error!("watcher cannot open database: {e}");
                return;
            }
        };
        let mut tracker = StabilityTracker::new(Duration::from_secs(3));
        let mut origin: HashMap<PathBuf, usize> = HashMap::new();
        let mut last_poll = Instant::now();
        while !stop_thread.load(Ordering::SeqCst) {
            match rx.recv_timeout(Duration::from_millis(400)) {
                Ok((p, i)) => {
                    origin.insert(p.clone(), i);
                    tracker.touch(p, Instant::now());
                    continue_draining(&rx, &mut tracker, &mut origin);
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            if last_poll.elapsed() < Duration::from_millis(900) || tracker.is_empty() {
                continue;
            }
            last_poll = Instant::now();
            let (stable, gone) = tracker.poll(Instant::now(), measure);
            let mut changed = false;
            for p in gone {
                origin.remove(&p);
                if db.remove_inbox_path(&p.to_string_lossy()).unwrap_or(false) {
                    changed = true;
                }
            }
            for p in stable {
                let Some(i) = origin.remove(&p) else { continue };
                let t = &targets[i];
                // A DAW folder inside the library must not re-queue sorted beats.
                if t.source != Source::Inbox && is_within(&p, &library_root) {
                    continue;
                }
                let Some(item) = describe(&p, t.source) else { continue };
                if let (Some(since), Some(mtime)) = (t.since, item.mtime) {
                    if mtime <= since {
                        continue;
                    }
                }
                if db.upsert_inbox(&item).unwrap_or(false) {
                    changed = true;
                }
            }
            if changed {
                let _ = app.emit("inbox-changed", ());
            }
        }
    });

    Ok(WatchService { _watcher: watcher, stop })
}

fn continue_draining(
    rx: &std::sync::mpsc::Receiver<(PathBuf, usize)>,
    tracker: &mut StabilityTracker,
    origin: &mut HashMap<PathBuf, usize>,
) {
    while let Ok((p, i)) = rx.try_recv() {
        origin.insert(p.clone(), i);
        tracker.touch(p, Instant::now());
    }
}
