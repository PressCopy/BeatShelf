use beatcore::db::Db;
use beatcore::inbox::Source;
use beatcore::keys::MusicalKey;
use beatcore::ops::Plan;
use beatcore::template::LibraryTemplate;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

pub const CONFIG_KEY: &str = "config";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WatchFolder {
    pub path: String,
    /// Only files changed after this moment are queued, so adding a DAW folder
    /// with years of old projects does not flood the queue.
    pub since: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    pub root: Option<String>,
    pub template: Option<LibraryTemplate>,
    pub fl_folders: Vec<WatchFolder>,
    pub ableton_folders: Vec<WatchFolder>,
    pub lang: String,
    pub theme: String,
    pub show_backups: bool,
    /// Your producer name; new beats get it as the author.
    pub producer_name: Option<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            root: None,
            template: None,
            fl_folders: vec![],
            ableton_folders: vec![],
            lang: "ru".into(),
            theme: "dark".into(),
            show_backups: false,
            producer_name: None,
        }
    }
}

impl Config {
    pub fn onboarded(&self) -> bool {
        self.root.is_some() && self.template.is_some()
    }
    pub fn root_path(&self) -> Option<PathBuf> {
        self.root.as_ref().map(PathBuf::from)
    }
    pub fn inbox_path(&self) -> Option<PathBuf> {
        Some(self.root_path()?.join(&self.template.as_ref()?.inbox))
    }
    /// Every folder plans may touch.
    pub fn plan_roots(&self) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = self.root_path().into_iter().collect();
        for f in self.fl_folders.iter().chain(self.ableton_folders.iter()) {
            v.push(PathBuf::from(&f.path));
        }
        v
    }
    pub fn watch_targets(&self) -> Vec<(PathBuf, Source, Option<i64>)> {
        let mut v = Vec::new();
        if let Some(inbox) = self.inbox_path() {
            v.push((inbox, Source::Inbox, None));
        }
        for f in &self.fl_folders {
            v.push((PathBuf::from(&f.path), Source::Fl, Some(f.since)));
        }
        for f in &self.ableton_folders {
            v.push((PathBuf::from(&f.path), Source::Ableton, Some(f.since)));
        }
        v
    }
}

#[derive(Debug, Clone)]
/// A draft (made with "New folder") that receives its first files: the
/// folder it gets and the fields read from the first file's name.
pub struct DraftFill {
    pub id: i64,
    pub name: String,
    pub bpm: Option<f64>,
    pub key: Option<MusicalKey>,
    pub authors: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct NewBeatMeta {
    pub name: String,
    pub bpm: Option<f64>,
    pub key: Option<MusicalKey>,
    pub kind: beatcore::template::ItemKind,
    pub authors: Vec<String>,
    pub coauthors: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum AfterExec {
    Nothing,
    Onboarding(Config),
    SortIntoBeat { folder: String, new_beat: Option<NewBeatMeta>, inbox_ids: Vec<i64>, draft: Option<DraftFill> },
}

pub struct PendingPlan {
    pub plan: Plan,
    pub after: AfterExec,
}

pub struct AppState {
    pub db_path: PathBuf,
    pub db: Mutex<Db>,
    pub plans: Mutex<HashMap<String, PendingPlan>>,
    pub watcher: Mutex<Option<crate::watcher::WatchService>>,
    pub scanning: AtomicBool,
    /// Files dropped on the window or picked in a dialog, by drop ID. The UI
    /// refers to them by ID, so paths never travel back from the UI.
    pub drops: Mutex<HashMap<String, Vec<PathBuf>>>,
    /// Set while we are dragging a file out, so our own window ignores it.
    pub dragging_out: AtomicBool,
}

impl AppState {
    pub fn config(&self) -> Config {
        let db = self.db.lock().unwrap();
        db.get_setting::<Config>(CONFIG_KEY).ok().flatten().unwrap_or_default()
    }
    pub fn save_config(&self, cfg: &Config) -> Result<(), String> {
        let db = self.db.lock().unwrap();
        db.set_setting(CONFIG_KEY, cfg).map_err(|e| e.to_string())
    }
}
