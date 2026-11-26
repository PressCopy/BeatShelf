//! Commands called from the UI. The UI refers to beats, assets, inbox items
//! and plans by ID only; every path is resolved and checked here.

use crate::state::{AfterExec, AppState, Config, DraftFill, NewBeatMeta, PendingPlan, WatchFolder};
use beatcore::daw::AssetKind;
use beatcore::db::{AssetRow, BatchRow, BeatPatch, BeatRow, InboxItem, NewBeat};
use beatcore::ops::{self, ExecError, ExecOptions, Issue, Plan, PlanBuilder, Step};
use beatcore::parse::{parse_folder_name, parse_name};
use beatcore::pathsafe::{fold_key, from_rel, is_within, rel_string, unique_name};
use beatcore::scan::{refresh_section_item, scan_library, LAYOUT_MARKER};
use beatcore::template::{builtin_templates, ItemKind, LibraryTemplate, SubdirSlot, TokenValues, TreeLine};
use chrono::TimeZone;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_opener::OpenerExt;

type CmdResult<T> = Result<T, String>;

fn err<E: std::fmt::Display>(e: E) -> String {
    e.to_string()
}

// ───────────────────────────── DTOs ─────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStateDto {
    pub onboarded: bool,
    pub config: Config,
    pub inbox_count: i64,
    pub platform: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StepView {
    pub kind: &'static str,
    pub from: Option<String>,
    pub to: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanView {
    pub id: String,
    pub label: String,
    pub steps: Vec<StepView>,
    pub warnings: Vec<Issue>,
    pub blockers: Vec<Issue>,
    pub is_undo: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecResultDto {
    pub batch_id: Option<i64>,
    pub moved: usize,
    pub created: usize,
    pub skipped: Vec<Issue>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaDto {
    pub path: String,
    pub peaks: Vec<f32>,
    pub duration_ms: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BeatDetail {
    pub beat: BeatRow,
    pub assets: Vec<AssetRow>,
    pub wrong_section: bool,
    /// Full relative path the template expects (kind decides the section).
    pub expected_folder: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingRequest {
    pub root: String,
    pub template_id: String,
    pub fl_folders: Vec<String>,
    pub ableton_folders: Vec<String>,
    pub lang: String,
    pub label: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "type")]
pub enum SortTarget {
    Existing { beat_id: i64 },
    New {
        name: String,
        bpm: Option<f64>,
        key: Option<String>,
        #[serde(default)]
        kind: Option<String>,
        #[serde(default)]
        authors: Vec<String>,
        #[serde(default)]
        coauthors: Vec<String>,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SortRequest {
    pub inbox_ids: Vec<i64>,
    pub target: SortTarget,

    #[serde(default)]
    pub slots: HashMap<String, SubdirSlot>,
    pub label: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrefsPatch {
    pub lang: Option<String>,
    pub theme: Option<String>,
    pub show_backups: Option<bool>,
    pub number_beats: Option<bool>,
    pub number_from: Option<u64>,
    #[serde(default, deserialize_with = "some_option")]
    pub producer_name: Option<Option<String>>,
}

fn some_option<'de, D, T>(d: D) -> Result<Option<Option<T>>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(d).map(Some)
}

// ───────────────────────────── helpers ─────────────────────────────

fn shorten(p: &Path, bases: &[(PathBuf, String)]) -> String {
    for (base, label) in bases {
        if let Some(rel) = rel_string(base, p) {
            return if rel.is_empty() { label.clone() } else { format!("{label}/{rel}") };
        }
    }
    p.to_string_lossy().to_string()
}

fn display_bases(cfg: &Config) -> Vec<(PathBuf, String)> {
    let mut v = Vec::new();
    for p in cfg.plan_roots() {
        let label = p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| p.to_string_lossy().to_string());
        v.push((p, label));
    }
    v
}

fn view_plan(plan: &Plan, cfg: &Config) -> PlanView {
    let bases = {
        let mut b = display_bases(cfg);
        for r in &plan.roots {
            if !b.iter().any(|(p, _)| p == r) {
                b.push((r.clone(), r.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()));
            }
        }
        b
    };
    let short_issue = |i: &Issue| {
        let mut i = i.clone();
        i.path = i.path.map(|p| shorten(Path::new(&p), &bases));
        i
    };
    PlanView {
        id: plan.id.clone(),
        label: plan.label.clone(),
        steps: plan
            .steps
            .iter()
            .map(|s| match s {
                Step::CreateDir { path } | Step::CreateNewDir { path } => StepView { kind: "create_dir", from: None, to: shorten(path, &bases) },
                Step::Move { from, to } => {
                    let same_parent = from.parent() == to.parent();
                    StepView {
                        kind: if same_parent { "rename" } else { "move" },
                        from: Some(shorten(from, &bases)),
                        to: shorten(to, &bases),
                    }
                }
                Step::RemoveEmptyDir { path } => StepView { kind: "remove_dir", from: None, to: shorten(path, &bases) },
                Step::Copy { from, to } => StepView { kind: "copy", from: Some(shorten(from, &bases)), to: shorten(to, &bases) },
                Step::Trash { path } => StepView { kind: "trash", from: None, to: shorten(path, &bases) },
            })
            .collect(),
        warnings: plan.warnings.iter().map(short_issue).collect(),
        blockers: plan.blockers.iter().map(short_issue).collect(),
        is_undo: plan.undo_of.is_some(),
    }
}

fn register(state: &AppState, plan: Plan, after: AfterExec, cfg: &Config) -> PlanView {
    let view = view_plan(&plan, cfg);
    let mut plans = state.plans.lock().unwrap();
    if plans.len() > 50 {
        plans.clear();
    }
    plans.insert(plan.id.clone(), PendingPlan { plan, after });
    view
}

fn library(cfg: &Config) -> CmdResult<(PathBuf, LibraryTemplate)> {
    match (cfg.root_path(), cfg.template.clone()) {
        (Some(r), Some(t)) => Ok((r, t)),
        _ => Err("library is not set up".into()),
    }
}

pub fn allow_scope(app: &AppHandle, cfg: &Config) {
    let scope = app.asset_protocol_scope();
    for p in cfg.plan_roots() {
        if let Err(e) = scope.allow_directory(&p, true) {
            log::warn!("asset scope: {e}");
        }
    }
}

pub fn restart_watcher(app: &AppHandle, state: &AppState, cfg: &Config) {
    let mut guard = state.watcher.lock().unwrap();
    *guard = None; // stops the old thread
    let Some(root) = cfg.root_path() else { return };
    let targets = cfg
        .watch_targets()
        .into_iter()
        .map(|(root, source)| crate::watcher::Target { root, source })
        .collect();
    match crate::watcher::start(app.clone(), state.db_path.clone(), root, targets) {
        Ok(w) => *guard = Some(w),
        Err(e) => log::error!("watcher failed to start: {e}"),
    }
}

pub fn spawn_scan(app: &AppHandle, state: &AppState) {
    if state.scanning.swap(true, Ordering::SeqCst) {
        return;
    }
    let cfg = state.config();
    let Ok((root, tpl)) = library(&cfg) else {
        state.scanning.store(false, Ordering::SeqCst);
        return;
    };
    let db_path = state.db_path.clone();
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<AppState>();
        let _io = state.library_io.lock().unwrap();
        let result = beatcore::db::Db::open(&db_path).and_then(|db| {
            scan_library(&db, &root, &tpl, |done| {
                let _ = app.emit("scan-progress", done);
            })
        });
        if let Err(e) = &result {
            log::error!("scan failed: {e}");
        }
        app.state::<AppState>().scanning.store(false, Ordering::SeqCst);
        let _ = app.emit("library-changed", ());
    });
}

const CONTRACT_WORDS: [&str; 8] = ["contract", "контракт", "договор", "license", "licence", "лиценз", "agreement", "split"];
const TRACKOUT_WORDS: [&str; 6] = ["stem", "trackout", "track out", "стем", "трекаут", "multitrack"];

fn name_has(path: &Path, words: &[&str]) -> bool {
    let n = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    words.iter().any(|w| n.contains(w))
}

/// Best guess for where a file goes inside a beat folder.
fn suggest_slot(path: &Path, kind: &str) -> SubdirSlot {
    let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if name_has(path, &CONTRACT_WORDS) || ["pdf", "doc", "docx", "odt", "pages"].contains(&ext.as_str()) {
        return SubdirSlot::Contracts;
    }
    if name_has(path, &TRACKOUT_WORDS) || ext == "zip" && !matches!(AssetKind::from_str(kind), AssetKind::FlZip) {
        return SubdirSlot::Stems;
    }
    default_slot(kind)
}

fn default_slot(kind: &str) -> SubdirSlot {
    match AssetKind::from_str(kind) {
        AssetKind::FlProject | AssetKind::FlZip => SubdirSlot::ProjectFl,
        AssetKind::AbletonProject | AssetKind::AbletonSet => SubdirSlot::ProjectAbleton,
        AssetKind::Audio => SubdirSlot::Bounces,
        AssetKind::Image => SubdirSlot::Artwork,
        AssetKind::Document => SubdirSlot::Notes,
        AssetKind::Midi | AssetKind::Folder => SubdirSlot::Stems,
        AssetKind::Other => SubdirSlot::Notes,
    }
}

fn people(v: &[String]) -> Vec<String> {
    v.iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

/// Template values for an existing item. The year comes from when it was
/// created, so an old beat is not "misplaced" just because the year changed.
fn values_for(beat: &BeatRow) -> TokenValues {
    TokenValues {
        name: beat.name.clone(),
        bpm: beat.bpm.map(|v| v as f32),
        key: beat.key.clone(),
        date: chrono::Local.timestamp_opt(beat.created_at, 0).single().map(|d| d.date_naive()),
        artist: None,
        authors: beat.authors.clone(),
        coauthors: beat.coauthors.clone(),
    }
}

fn kind_of(beat: &BeatRow) -> ItemKind {
    ItemKind::parse(&beat.kind).unwrap_or(ItemKind::Beat)
}

const COUNTER: &str = "next_beat_number";
const MAX_NUMBER: u64 = 9_007_199_254_740_991;

fn planned_number(db: &beatcore::db::Db, cfg: &Config, kind: ItemKind) -> CmdResult<Option<u64>> {
    if !cfg.number_beats || kind != ItemKind::Beat {
        return Ok(None);
    }
    let next = db.get_setting::<u64>(COUNTER).map_err(err)?.unwrap_or(cfg.number_from);
    if next > MAX_NUMBER { return Err("number_limit".into()); }
    Ok(Some(next))
}

/// Template output is an internal directory. Only one ordinary path component
/// supplied by the user may be appended to it.
fn new_folder(tpl: &LibraryTemplate, root: &Path, kind: ItemKind, values: &TokenValues,
              user: &str, number: Option<u64>) -> CmdResult<String> {
    if Path::new(user).file_name().and_then(|v| v.to_str()) != Some(user)
        || user.contains('\0') || user.is_empty() {
        return Err("bad_folder_name".into());
    }
    #[cfg(windows)]
    if user.chars().any(|c| c.is_control() || r#"<>:"/\|?*"#.contains(c))
        || user.ends_with(['.', ' ']) || beatcore::pathsafe::is_reserved_name(user) {
        return Err("bad_folder_name".into());
    }
    let mut parts = tpl.dir_for(kind, values);
    let leaf = parts.pop().ok_or("bad_pattern")?;
    let parent = from_rel(root, &parts.join("/"));
    parts.push(unique_name(&parent, &leaf, &HashSet::new()));
    let user = match number {
        Some(n) if !user.starts_with(&format!("{n} ")) => format!("{n} {user}"),
        _ => user.to_owned(),
    };
    parts.push(user);
    Ok(parts.join("/"))
}

fn plan_layout(b: &mut PlanBuilder, user: &Path) {
    // Claim the parent before copying or moving any files. A second plan
    // with the same name must never adopt someone else's directory.
    b.create_new_dir(user.parent().expect("a user directory has a service parent"));
    b.create_dir(user.join(LAYOUT_MARKER));
}

/// Where files for an existing item go. A draft (made with "New folder") has
/// no folder yet: it gets one now, named after the first file added, unless
/// someone already typed a name for it.
fn existing_target(
    tpl: &LibraryTemplate,
    root: &Path,
    db: &beatcore::db::Db,
    beat: &BeatRow,
    source_name: Option<&str>,
    number: Option<u64>,
) -> CmdResult<(String, TokenValues, ItemKind, Option<DraftFill>)> {
    let kind = kind_of(beat);
    if let Some(folder) = &beat.folder {
        return Ok((folder.clone(), values_for(beat), kind, None));
    }
    let (is_draft, auto_name) = db.draft_state(beat.id).map_err(err)?.unwrap_or((false, false));
    if !is_draft {
        return Err("this beat has no folder".into());
    }
    let stem = source_name.unwrap_or_default().to_owned();
    let user = if auto_name && !stem.is_empty() { &stem } else { &beat.name };
    let parsed = parse_folder_name(user);
    let tag = kind == ItemKind::Voicetag;
    let mut name = beat.name.clone();
    if auto_name {
        // BPM and key written in the file name go to their own fields, the rest is the name.
        let from_file = if tag || parsed.title.trim().is_empty() { stem.clone() } else { parsed.title.clone() };
        if !from_file.trim().is_empty() {
            name = from_file.trim().to_string();
        }
    }
    let (bpm_edited, key_edited) = db.draft_edits(beat.id).map_err(err)?;
    let first = parse_name(&stem);
    let bpm = if tag || bpm_edited { beat.bpm } else {
        beat.bpm.or(parsed.bpm.or(first.bpm).map(f64::from))
    };
    let key = if tag || key_edited { beat.key.clone() } else {
        beat.key.clone().or_else(|| parsed.key.or(first.key).map(|k| k.name().to_owned()))
    };
    let mut authors = beat.authors.clone();
    if authors.is_empty() && !tag {
        authors = parsed.producers.clone();
    }
    let mut values = values_for(beat);
    values.name = name.clone();
    values.bpm = bpm.map(|v| v as f32);
    values.key = key.clone();
    values.authors = authors.clone();
    let folder = new_folder(tpl, root, kind, &values, user, number)?;
    Ok((folder, values, kind, Some(DraftFill { original: beat.clone(), id: beat.id, name, bpm, key, authors })))
}

/// Folder, template values and row data for an item that doesn't exist yet.
#[allow(clippy::too_many_arguments)]
fn new_item(
    cfg: &Config,
    tpl: &LibraryTemplate,
    name: &str,
    bpm: Option<f64>,
    key: Option<&str>,
    kind: Option<&str>,
    authors: &[String],
    coauthors: &[String],
    user: &str,
    number: Option<u64>,
) -> CmdResult<(String, TokenValues, NewBeatMeta)> {
    let name = name.trim();
    if name.is_empty() {
        return Err("name is empty".into());
    }
    let key = key.filter(|v| !v.is_empty()).map(str::to_owned);
    let kind = kind.and_then(ItemKind::parse).unwrap_or(ItemKind::Beat);
    // A newly typed folder also supplies suggestions. A picked folder already
    // went through the editable suggestion form, including any cleared fields.
    let parsed = (name == user && kind != ItemKind::Voicetag).then(|| parse_folder_name(user));
    let name = parsed.as_ref().map(|p| p.title.as_str()).filter(|s| !s.is_empty()).unwrap_or(name);
    let bpm = bpm.or_else(|| parsed.as_ref().and_then(|p| p.bpm).map(f64::from));
    let key = key.or_else(|| parsed.as_ref().and_then(|p| p.key).map(|k| k.name().to_owned()));
    let mut authors = people(authors);
    if authors.is_empty() {
        if let Some(parsed) = &parsed { authors.clone_from(&parsed.producers); }
    }
    if authors.is_empty() && kind != ItemKind::Voicetag {
        authors.extend(cfg.producer_name.clone().filter(|n| !n.trim().is_empty()));
    }
    let coauthors = people(coauthors);
    let values = TokenValues {
        name: name.into(),
        bpm: bpm.map(|v| v as f32),
        key: key.clone(),
        date: None,
        artist: None,
        authors: authors.clone(),
        coauthors: coauthors.clone(),
    };
    let folder = new_folder(tpl, &cfg.root_path().ok_or("no library")?, kind, &values, user, number)?;
    Ok((folder, values, NewBeatMeta { name: name.into(), bpm, key, kind, authors, coauthors }))
}

/// Database follow-up after files moved: paths, beat rows, inbox rows.
fn after_execution(db: &beatcore::db::Db, cfg: &Config, report: &ops::ExecReport, after: &AfterExec) -> CmdResult<()> {
    let Ok((root, tpl)) = library(cfg) else { return Ok(()) };
    let tx = db.conn.unchecked_transaction().map_err(err)?;
    db.apply_moves(&root, &report.movements()).map_err(err)?;

    if let AfterExec::SortIntoBeat { folder, new_beat, inbox_ids, draft, number } = after {
        if let Some(d) = draft {
            // Before the folder is re-read below, or it would be taken for a new item.
            db.adopt_draft(d.id, folder).map_err(err)?;
            let patch = BeatPatch {
                name: Some(d.name.clone()),
                bpm: Some(d.bpm),
                key: Some(d.key.clone()),
                authors: Some(d.authors.clone()),
                ..Default::default()
            };
            db.update_beat(d.id, &patch).map_err(err)?;
        }
        if let Some(meta) = new_beat {
            db.create_beat(&NewBeat {
                name: meta.name.clone(),
                folder: Some(folder.clone()),
                bpm: meta.bpm,
                key: meta.key.clone(),
                status: Some(if meta.kind == ItemKind::Voicetag { "ready" } else { "in_progress" }.into()),
                kind: Some(meta.kind.as_str().into()),
                authors: meta.authors.clone(),
                coauthors: meta.coauthors.clone(),
            }).map_err(err)?;
        }
        if new_beat.is_some() || draft.is_some() {
            let id = db.beat_id_by_folder(folder).map_err(err)?.ok_or("missing item")?;
            let (service, _) = folder.rsplit_once('/').ok_or("missing service folder")?;
            db.set_service(id, service).map_err(err)?;
            if let Some(n) = number {
                db.set_setting(COUNTER, &(n + 1)).map_err(err)?;
            }
        }
        for id in inbox_ids {
            if let Some(item) = db.inbox_item(*id).map_err(err)? {
                if !Path::new(&item.path).exists() {
                    db.remove_inbox_path(&item.path).map_err(err)?;
                }
            }
        }
    }

    let mut touched: BTreeSet<(String, ItemKind)> = BTreeSet::new();
    for op in &report.ops {
        let paths: Vec<&Path> = match op {
            ops::DoneOp::CreatedDir { path } | ops::DoneOp::RemovedDir { path } | ops::DoneOp::Trashed { path } => vec![path],
            ops::DoneOp::Renamed { from, to } | ops::DoneOp::CopiedAndTrashed { from, to } => vec![from, to],
            ops::DoneOp::Copied { to, .. } => vec![to],
        };
        for p in paths {
            if let Some(rel) = rel_string(&root, p) {
                if let Some(found) = tpl.item_folder_of(&rel) {
                    touched.insert(found);
                }
            }
        }
    }
    for (f, kind) in &touched {
        refresh_section_item(db, &root, f, *kind).map_err(err)?;
    }
    db.prune().map_err(err)?;
    for (from, _) in report.movements() {
        db.remove_inbox_path(&from.to_string_lossy()).map_err(err)?;
    }
    tx.commit().map_err(err)
}

fn peaks_to_f32(p: &[u8]) -> Vec<f32> {
    p.iter().map(|v| *v as f32 / 255.0).collect()
}

// ───────────────────────────── commands ─────────────────────────────

#[tauri::command]
pub fn get_app_state(state: State<'_, AppState>) -> CmdResult<AppStateDto> {
    let cfg = state.config();
    let inbox_count = state.db.lock().unwrap().pending_inbox_count().map_err(err)?;
    Ok(AppStateDto {
        onboarded: cfg.onboarded(),
        config: cfg,
        inbox_count,
        platform: std::env::consts::OS,
    })
}

#[tauri::command]
pub fn list_templates() -> Vec<LibraryTemplate> {
    builtin_templates()
}

#[tauri::command]
pub fn preview_template(template_id: String, root_name: String) -> CmdResult<Vec<TreeLine>> {
    let tpl = builtin_templates().into_iter().find(|t| t.id == template_id).ok_or("unknown template")?;
    let example = TokenValues {
        name: "Night".into(),
        bpm: Some(140.0),
        key: Some("Am".into()),
        ..Default::default()
    };
    Ok(tpl.preview(&root_name, &example))
}

#[tauri::command]
pub async fn plan_create_library(state: State<'_, AppState>, req: OnboardingRequest) -> CmdResult<PlanView> {
    let root = PathBuf::from(&req.root);
    if !root.is_absolute() {
        return Err("choose an absolute folder".into());
    }
    let tpl = builtin_templates().into_iter().find(|t| t.id == req.template_id).ok_or("unknown template")?;
    let now = chrono::Utc::now().timestamp();
    let watch = |v: &Vec<String>| -> Vec<WatchFolder> {
        v.iter().filter(|p| !p.trim().is_empty()).map(|p| WatchFolder { path: p.clone(), since: now }).collect()
    };
    let mut cfg = state.config();
    cfg.root = Some(req.root.clone());
    cfg.template = Some(tpl.clone());
    cfg.fl_folders = watch(&req.fl_folders);
    cfg.ableton_folders = watch(&req.ableton_folders);
    cfg.lang = req.lang.clone();

    let mut b = PlanBuilder::new(req.label, vec![root.clone()]);
    b.create_dir(&root);
    for d in tpl.dirs_to_create() {
        b.create_dir(from_rel(&root, &d));
    }
    let plan = b.build();
    Ok(register(&state, plan, AfterExec::Onboarding(cfg.clone()), &cfg))
}

#[tauri::command]
pub async fn execute_plan(app: AppHandle, state: State<'_, AppState>, plan_id: String) -> CmdResult<ExecResultDto> {
    let pending = state.plans.lock().unwrap().remove(&plan_id).ok_or("this plan has expired, make it again")?;
    let _io = state.library_io.lock().unwrap();
    let cfg = state.config();
    let report = {
        let mut db = state.db.lock().unwrap();
        if let AfterExec::SortIntoBeat { folder, new_beat, draft, number, .. } = &pending.after {
            if let Some(kind) = new_beat.as_ref().map(|m| m.kind).or_else(|| draft.as_ref().map(|d| kind_of(&d.original))) {
                let (root, _) = library(&cfg)?;
                let (service, _) = folder.rsplit_once('/').ok_or("stale")?;
                if beatcore::pathsafe::exists_ignoring_case(&from_rel(&root, service))
                    || planned_number(&db, &cfg, kind)? != *number {
                    return Err("stale".into());
                }
                if let Some(d) = draft {
                    let row = db.get_beat(d.id).map_err(err)?.ok_or("stale")?;
                    let old = &d.original;
                    if row.folder.is_some() || row.name != old.name || row.key != old.key || row.bpm != old.bpm
                        || row.authors != old.authors || row.coauthors != old.coauthors || row.kind != old.kind {
                        return Err("stale".into());
                    }
                }
            }
        }
        let report = match ops::execute(&pending.plan, &mut *db, &ExecOptions::default()) {
            Ok(r) => r,
            Err(ExecError::Stale(_)) => return Err("stale".into()),
            Err(e) => return Err(e.to_string()),
        };
        after_execution(&db, &cfg, &report, &pending.after)?;
        report
    };

    if let AfterExec::Onboarding(cfg) = &pending.after {
        state.save_config(cfg)?;
        allow_scope(&app, cfg);
        restart_watcher(&app, &state, cfg);
    }
    let _ = app.emit("library-changed", ());
    let _ = app.emit("inbox-changed", ());
    if matches!(pending.after, AfterExec::Onboarding(_)) {
        spawn_scan(&app, &state);
    }
    Ok(ExecResultDto {
        batch_id: report.batch_id,
        moved: report.movements().len(),
        created: report.ops.iter().filter(|o| matches!(o, ops::DoneOp::CreatedDir { .. })).count(),
        skipped: report.skipped,
    })
}

#[tauri::command]
pub fn discard_plan(state: State<'_, AppState>, plan_id: String) {
    state.plans.lock().unwrap().remove(&plan_id);
}

#[tauri::command]
pub async fn list_beats(state: State<'_, AppState>, query: String) -> CmdResult<Vec<BeatRow>> {
    let root = state.config().root_path();
    let rows = state.db.lock().unwrap().list_beats(&query).map_err(err)?;
    Ok(match root {
        Some(r) => rows.into_iter().map(|b| with_abs_cover(b, &r)).collect(),
        None => rows,
    })
}

#[tauri::command]
pub fn update_beat(app: AppHandle, state: State<'_, AppState>, id: i64, patch: BeatPatch) -> CmdResult<BeatRow> {
    if let Some(Some(c)) = &patch.color {
        // The colour goes straight into the UI's styles: "#rrggbb" only.
        let ok = c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|ch| ch.is_ascii_hexdigit());
        if !ok {
            return Err("bad_color".into());
        }
    }
    let root = state.config().root_path(); // before the lock: config() takes it too
    let db = state.db.lock().unwrap();
    if patch.kind.as_deref() == Some("voicetag") {
        // An item with contracts, artwork or loose folders can't become a voice tag.
        let files = db.assets_of_beat(id, true).map_err(err)?;
        if files.iter().any(|a| !ItemKind::Voicetag.accepts(AssetKind::from_str(&a.kind), Path::new(&a.path))) {
            return Err("voicetag_file_type".into());
        }
    }
    db.update_beat(id, &patch).map_err(err)?;
    let row = db.get_beat(id).map_err(err)?.ok_or("beat not found")?;
    let _ = app.emit("beat-updated", row.id);
    Ok(match root {
        Some(r) => with_abs_cover(row, &r),
        None => row,
    })
}

#[tauri::command]
pub fn beat_detail(state: State<'_, AppState>, id: i64) -> CmdResult<BeatDetail> {
    let cfg = state.config();
    let (root, tpl) = library(&cfg)?;
    let db = state.db.lock().unwrap();
    let beat = db.get_beat(id).map_err(err)?.ok_or("beat not found")?;
    let assets = db.assets_of_beat(id, cfg.show_backups).map_err(err)?;
    let kind = kind_of(&beat);
    let mut expected_parts = tpl.dir_for(kind, &values_for(&beat));
    let wrong_section = beat.folder.as_deref().and_then(|f| tpl.item_folder_of(f)).is_some_and(|(_, k)| k != kind);
    if wrong_section {
        if let Some(folder) = &beat.folder {
            let leaf = folder.rsplit('/').next().unwrap_or(folder);
            if tpl.item_folder_of(folder).is_some_and(|(s, _)| s == *folder) { expected_parts.pop(); }
            expected_parts.push(leaf.to_owned());
        }
    }
    Ok(BeatDetail {
        wrong_section,
        expected_folder: expected_parts.join("/"),
        beat: with_abs_cover(beat, &root),
        assets,
    })
}

/// Rename the beat folder so it matches the template and the current fields.
#[tauri::command]
pub async fn plan_align_folder(state: State<'_, AppState>, id: i64, label: String) -> CmdResult<PlanView> {
    let cfg = state.config();
    let (root, tpl) = library(&cfg)?;
    let beat = state.db.lock().unwrap().get_beat(id).map_err(err)?.ok_or("beat not found")?;
    let folder = beat.folder.clone().ok_or("this beat has no folder")?;
    let kind = kind_of(&beat);
    let (section, old_kind) = tpl.item_folder_of(&folder).ok_or("no_folder")?;
    if old_kind == kind { return Err("stale".into()); }
    // A category move keeps the visible folder name for both layouts.
    let src = from_rel(&root, &section);
    let mut parts = tpl.dir_for(kind, &values_for(&beat));
    let template_leaf = parts.pop().ok_or("bad_pattern")?;
    let parent = from_rel(&root, &parts.join("/"));
    let leaf = if section == folder { src.file_name().and_then(|s| s.to_str()).ok_or("no_folder")? } else { &template_leaf };
    let dest = parent.join(unique_name(&parent, leaf, &HashSet::new()));
    let mut b = PlanBuilder::new(label, cfg.plan_roots());
    b.create_dir(parent);
    b.move_item(src, dest);
    Ok(register(&state, b.build(), AfterExec::Nothing, &cfg))
}

#[tauri::command]
pub fn list_inbox(state: State<'_, AppState>, ignored: bool) -> CmdResult<Vec<InboxItem>> {
    let db = state.db.lock().unwrap();
    let mut items = db.list_inbox(if ignored { "ignored" } else { "pending" }).map_err(err)?;
    // Drop rows whose file vanished while the app was closed.
    items.retain(|i| {
        let ok = Path::new(&i.path).exists();
        if !ok {
            let _ = db.remove_inbox_path(&i.path);
        }
        ok
    });
    Ok(items)
}

#[tauri::command]
pub fn set_inbox_ignored(app: AppHandle, state: State<'_, AppState>, ids: Vec<i64>, ignored: bool) -> CmdResult<()> {
    let db = state.db.lock().unwrap();
    for id in ids {
        db.set_inbox_state(id, if ignored { "ignored" } else { "pending" }).map_err(err)?;
    }
    let _ = app.emit("inbox-changed", ());
    Ok(())
}

fn inbox_name(item: &InboxItem) -> String {
    let p = Path::new(&item.path);
    if let Some(name) = &item.folder_name {
        name.clone()
    } else if p.is_dir() {
        item.name.clone()
    } else {
        beatcore::scan::display_stem(p)
    }
}

#[tauri::command]
pub async fn plan_sort(state: State<'_, AppState>, req: SortRequest) -> CmdResult<PlanView> {
    let cfg = state.config();
    let (root, tpl) = library(&cfg)?;
    let db = state.db.lock().unwrap();

    let items: Vec<InboxItem> = req
        .inbox_ids
        .iter()
        .filter_map(|id| db.inbox_item(*id).ok().flatten())
        .collect();
    if items.is_empty() {
        return Err("nothing to sort".into());
    }

    let mut draft = None;
    let mut number = None;
    let (folder_rel, _values, new_beat, item_kind) = match &req.target {
        SortTarget::Existing { beat_id } => {
            let beat = db.get_beat(*beat_id).map_err(err)?.ok_or("beat not found")?;
            let first = inbox_name(&items[0]);
            if beat.folder.is_none() { number = planned_number(&db, &cfg, kind_of(&beat))?; }
            let (folder, values, kind, fill) = existing_target(&tpl, &root, &db, &beat, Some(&first), number)?;
            draft = fill;
            (folder, values, None, kind)
        }
        SortTarget::New { name, bpm, key, kind, authors, coauthors } => {
            number = planned_number(&db, &cfg, kind.as_deref().and_then(ItemKind::parse).unwrap_or(ItemKind::Beat))?;
            let source = inbox_name(&items[0]);
            let user = if items[0].title.as_deref() == Some(name.as_str()) { &source } else { name };
            let (folder, values, meta) = new_item(&cfg, &tpl, name, *bpm, key.as_deref(), kind.as_deref(), authors, coauthors, user, number)?;
            let kind = meta.kind;
            (folder, values, Some(meta), kind)
        }
    };
    drop(db);

    let beat_abs = from_rel(&root, &folder_rel);
    let mut b = PlanBuilder::new(req.label, cfg.plan_roots());
    let mut taken: HashSet<String> = HashSet::new();
    if draft.is_some() || new_beat.is_some() {
        plan_layout(&mut b, &beat_abs);
    }
    for item in &items {
        let src = PathBuf::from(&item.path);
        if !item_kind.accepts(AssetKind::from_str(&item.kind), &src) {
            b.block(Issue::new(ops::IssueCode::VoicetagFileType, &src));
            continue;
        }
        let dest_dir = if item_kind.is_flat() {
            beat_abs.clone()
        } else {
            let slot = req.slots.get(&item.id.to_string()).copied().unwrap_or_else(|| suggest_slot(&src, &item.kind));
            beat_abs.join(tpl.beat_subdirs.get(slot))
        };
        b.create_dir(&dest_dir);
        let original = src.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let name = unique_name(&dest_dir, &original, &taken);
        let dest = dest_dir.join(&name);
        taken.insert(fold_key(&dest));
        b.move_item(src, dest);
    }
    let plan = b.build();
    let after = AfterExec::SortIntoBeat { folder: folder_rel, new_beat, inbox_ids: req.inbox_ids.clone(), draft, number };
    Ok(register(&state, plan, after, &cfg))
}

#[tauri::command]
pub fn list_history(state: State<'_, AppState>) -> CmdResult<Vec<BatchRow>> {
    state.db.lock().unwrap().list_batches(200).map_err(err)
}

#[tauri::command]
pub async fn plan_undo(state: State<'_, AppState>, batch_id: Option<i64>, label: String) -> CmdResult<Option<PlanView>> {
    let cfg = state.config();
    let db = state.db.lock().unwrap();
    let id = match batch_id {
        Some(id) => id,
        None => match db.last_undoable_batch().map_err(err)? {
            Some(id) => id,
            None => return Ok(None),
        },
    };
    let original = db.batch_label(id).map_err(err)?.unwrap_or_default();
    let ops_done = db.batch_ops(id).map_err(err)?;
    drop(db);
    let plan = ops::plan_undo(id, format!("{label}: {original}"), &ops_done, cfg.plan_roots());
    Ok(Some(register(&state, plan, AfterExec::Nothing, &cfg)))
}

fn asset_abs(state: &AppState, id: i64) -> CmdResult<(AssetRow, PathBuf)> {
    let cfg = state.config();
    let (root, _) = library(&cfg)?;
    let asset = state.db.lock().unwrap().asset(id).map_err(err)?.ok_or("file not found in the library")?;
    let abs = from_rel(&root, &asset.path);
    Ok((asset, abs))
}

#[tauri::command]
pub async fn asset_media(app: AppHandle, id: i64) -> CmdResult<MediaDto> {
    let state = app.state::<AppState>();
    let (asset, abs) = asset_abs(&state, id)?;
    if asset.kind != "audio" {
        return Err("not an audio file".into());
    }
    let cached = state.db.lock().unwrap().peaks(id).map_err(err)?;
    let (peaks, duration) = match cached {
        Some(p) => (p, asset.duration_ms),
        None => {
            let path = abs.clone();
            let (p, d) = tauri::async_runtime::spawn_blocking(move || beatcore::audio::compute_peaks(&path))
                .await
                .map_err(err)?
                .map_err(err)?;
            state.db.lock().unwrap().set_peaks(id, &p, d).map_err(err)?;
            (p, d.or(asset.duration_ms))
        }
    };
    Ok(MediaDto { path: abs.to_string_lossy().to_string(), peaks: peaks_to_f32(&peaks), duration_ms: duration })
}

#[tauri::command]
pub async fn inbox_media(app: AppHandle, id: i64) -> CmdResult<MediaDto> {
    let state = app.state::<AppState>();
    let item = state.db.lock().unwrap().inbox_item(id).map_err(err)?.ok_or("item not found")?;
    if item.kind != "audio" {
        return Err("not an audio file".into());
    }
    let cfg = state.config();
    let path = PathBuf::from(&item.path);
    if !cfg.plan_roots().iter().any(|r| is_within(&path, r)) {
        return Err("outside watched folders".into());
    }
    let p2 = path.clone();
    let (peaks, duration) = tauri::async_runtime::spawn_blocking(move || beatcore::audio::compute_peaks(&p2))
        .await
        .map_err(err)?
        .map_err(err)?;
    Ok(MediaDto { path: item.path, peaks: peaks_to_f32(&peaks), duration_ms: duration })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum RevealTarget {
    Asset { id: i64 },
    Beat { id: i64 },
    Inbox { id: i64 },
    Root,
}

#[tauri::command]
pub fn reveal(app: AppHandle, state: State<'_, AppState>, target: RevealTarget) -> CmdResult<()> {
    let cfg = state.config();
    let (root, _) = library(&cfg)?;
    let path = match target {
        RevealTarget::Asset { id } => asset_abs(&state, id)?.1,
        RevealTarget::Beat { id } => {
            let beat = state.db.lock().unwrap().get_beat(id).map_err(err)?.ok_or("beat not found")?;
            let path = from_rel(&root, beat.folder.as_deref().ok_or("this beat has no folder")?);
            return app.opener().open_path(path.to_string_lossy(), None::<&str>).map_err(err);
        }
        RevealTarget::Inbox { id } => {
            PathBuf::from(state.db.lock().unwrap().inbox_item(id).map_err(err)?.ok_or("item not found")?.path)
        }
        RevealTarget::Root => root,
    };
    app.opener().reveal_item_in_dir(path).map_err(err)
}

#[tauri::command]
pub fn set_prefs(state: State<'_, AppState>, patch: PrefsPatch) -> CmdResult<Config> {
    let db = state.db.lock().unwrap();
    let tx = db.conn.unchecked_transaction().map_err(err)?;
    let mut cfg = db.get_setting::<Config>(crate::state::CONFIG_KEY).map_err(err)?.unwrap_or_default();
    if let Some(n) = patch.number_from {
        if n > MAX_NUMBER { return Err("number_limit".into()); }
        cfg.number_from = n;
        db.set_setting(COUNTER, &n).map_err(err)?;
    }
    if let Some(on) = patch.number_beats { cfg.number_beats = on; }
    if let Some(l) = patch.lang.filter(|l| l == "ru" || l == "en") {
        cfg.lang = l;
    }
    if let Some(t) = patch.theme.filter(|t| ["dark", "light", "system"].contains(&t.as_str())) {
        cfg.theme = t;
    }
    if let Some(b) = patch.show_backups {
        cfg.show_backups = b;
    }
    if let Some(n) = patch.producer_name {
        cfg.producer_name = n.map(|n| n.replace(',', " ").trim().to_string()).filter(|n| !n.is_empty());
    }
    db.set_setting(crate::state::CONFIG_KEY, &cfg).map_err(err)?;
    tx.commit().map_err(err)?;
    Ok(cfg)
}

#[tauri::command]
pub fn set_watch_folders(
    app: AppHandle,
    state: State<'_, AppState>,
    fl: Vec<String>,
    ableton: Vec<String>,
) -> CmdResult<Config> {
    let db = state.db.lock().unwrap();
    let mut cfg = db.get_setting::<Config>(crate::state::CONFIG_KEY).map_err(err)?.unwrap_or_default();
    let now = chrono::Utc::now().timestamp();
    let merge = |old: &Vec<WatchFolder>, new: &Vec<String>| -> Vec<WatchFolder> {
        new.iter()
            .filter(|p| !p.trim().is_empty())
            .map(|p| old.iter().find(|w| &w.path == p).cloned().unwrap_or(WatchFolder { path: p.clone(), since: now }))
            .collect()
    };
    cfg.fl_folders = merge(&cfg.fl_folders, &fl);
    cfg.ableton_folders = merge(&cfg.ableton_folders, &ableton);
    db.set_setting(crate::state::CONFIG_KEY, &cfg).map_err(err)?;
    drop(db);
    allow_scope(&app, &cfg);
    restart_watcher(&app, &state, &cfg);
    Ok(cfg)
}

#[tauri::command]
pub async fn scan_daw(state: State<'_, AppState>) -> CmdResult<()> {
    let done = state.watcher.lock().unwrap().as_ref().ok_or("daw_unavailable")?.rescan()?;
    tauri::async_runtime::spawn_blocking(move || done.recv().map_err(|_| "daw_unavailable".to_owned()))
        .await.map_err(err)??
}

#[tauri::command]
pub fn rescan(app: AppHandle, state: State<'_, AppState>) {
    spawn_scan(&app, &state);
}

// ───────────────────────── files from outside: drop & pick ─────────────────────────

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DropItem {
    pub index: usize,
    pub name: String,
    pub kind: String,
    pub is_dir: bool,
    pub size: Option<u64>,
    pub slot: SubdirSlot,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct DropDto {
    pub folder_suggestion: Option<ImportSuggestion>,
    pub drop_id: String,
    pub items: Vec<DropItem>,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase", tag = "type")]
enum DragFilesEvent {
    /// Files are over the window; `x`/`y` are CSS pixels.
    Over { x: f64, y: f64 },
    Leave,
    Drop { x: f64, y: f64, drop: DropDto },
}

fn source_stem(p: &Path) -> String {
    let name = if p.is_dir() { p.file_name() } else { p.file_stem() };
    name.map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

fn kind_of_path(p: &Path) -> AssetKind {
    if p.is_dir() {
        if beatcore::daw::is_ableton_project_dir(p) {
            AssetKind::AbletonProject
        } else {
            AssetKind::Folder
        }
    } else {
        beatcore::daw::classify_file(p)
    }
}

/// Remember outside paths under a new ID and describe them for the UI.
fn register_drop(state: &AppState, paths: Vec<PathBuf>) -> Option<DropDto> {
    let paths: Vec<PathBuf> = paths.into_iter().filter(|p| p.exists() && !beatcore::daw::is_junk(p)).collect();
    if paths.is_empty() {
        return None;
    }
    let drop_id = uuid_like();
    let items = paths
        .iter()
        .enumerate()
        .map(|(index, p)| {
            let kind = kind_of_path(p);
            DropItem {
                index,
                name: p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
                kind: kind.as_str().to_string(),
                is_dir: p.is_dir(),
                size: if p.is_file() { p.metadata().ok().map(|m| m.len()) } else { None },
                slot: suggest_slot(p, kind.as_str()),
            }
        })
        .collect();
    let folder_suggestion = paths.first().filter(|p| p.is_dir())
        .map(|p| import_suggestion(source_stem(p), true, beatcore::daw::is_ableton_project_dir(p)));
    let mut drops = state.drops.lock().unwrap();
    if drops.len() > 20 {
        drops.clear();
    }
    drops.insert(drop_id.clone(), paths);
    Some(DropDto { drop_id, items, folder_suggestion })
}

fn uuid_like() -> String {
    format!("{:x}{:x}", chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default(), std::process::id())
}

/// Native drag-and-drop onto the window. Paths stay in Rust; the UI gets an ID
/// plus the drop position so it can tell which beat was under the cursor.
pub fn on_drag_drop(window: &tauri::Window, event: &tauri::DragDropEvent) {
    let app = window.app_handle();
    let state = app.state::<AppState>();
    if state.dragging_out.load(Ordering::SeqCst) {
        return; // our own drag passing over the window
    }
    let scale = window.scale_factor().unwrap_or(1.0);
    let payload = match event {
        tauri::DragDropEvent::Enter { position, .. } | tauri::DragDropEvent::Over { position } => {
            DragFilesEvent::Over { x: position.x / scale, y: position.y / scale }
        }
        tauri::DragDropEvent::Drop { paths, position } => match register_drop(&state, paths.clone()) {
            Some(drop) => DragFilesEvent::Drop { x: position.x / scale, y: position.y / scale, drop },
            None => DragFilesEvent::Leave,
        },
        _ => DragFilesEvent::Leave,
    };
    let _ = app.emit("drag-files", payload);
}

/// "Add files" / "Add folder" button: the dialog runs here, not in the UI.
#[tauri::command]
pub async fn pick_files(app: AppHandle, folders: bool, title: String) -> CmdResult<Option<DropDto>> {
    use tauri_plugin_dialog::DialogExt;
    let dialog = app.dialog().file().set_title(title);
    let picked = if folders { dialog.blocking_pick_folders() } else { dialog.blocking_pick_files() };
    let paths: Vec<PathBuf> = picked.unwrap_or_default().into_iter().filter_map(|p| p.into_path().ok()).collect();
    let state = app.state::<AppState>();
    Ok(register_drop(&state, paths))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", rename_all_fields = "camelCase", tag = "type")]
pub enum AddTarget {
    Beat { beat_id: i64 },
    Inbox,
    /// A new beat, loop, starter or voice tag made from these files.
    New {
        name: String,
        bpm: Option<f64>,
        key: Option<String>,
        #[serde(default)]
        kind: Option<String>,
        #[serde(default)]
        authors: Vec<String>,
        #[serde(default)]
        coauthors: Vec<String>,
    },
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddFilesRequest {
    pub drop_id: String,
    pub user_folder: Option<String>,
    pub target: AddTarget,
    /// Item index → slot; only for `Beat`.
    #[serde(default)]
    pub slots: HashMap<String, SubdirSlot>,
    pub label: String,
}

/// Copy outside files into a beat folder (or into the Inbox for sorting
/// later). Copy, not move: the originals stay where they were.
#[tauri::command]
pub async fn plan_add_files(state: State<'_, AppState>, req: AddFilesRequest) -> CmdResult<PlanView> {
    let cfg = state.config();
    let (root, tpl) = library(&cfg)?;
    let paths = state.drops.lock().unwrap().get(&req.drop_id).cloned().ok_or("stale")?;
    let mut b = PlanBuilder::new(req.label, cfg.plan_roots());
    let mut taken: HashSet<String> = HashSet::new();
    let mut after = AfterExec::Nothing;
    let beat_abs = match &req.target {
        AddTarget::Beat { beat_id } => {
            let db = state.db.lock().unwrap();
            let beat = db.get_beat(*beat_id).map_err(err)?.ok_or("beat not found")?;
            let kind = kind_of(&beat);
            // A draft is named after the first file it can actually take.
            let first = paths.iter().find(|p| kind.accepts(kind_of_path(p), p)).or(paths.first());
            let number = if beat.folder.is_none() { planned_number(&db, &cfg, kind)? } else { None };
            let source = req.user_folder.clone().or_else(|| first.map(|p| source_stem(p)));
            let (folder, _, kind, fill) = existing_target(&tpl, &root, &db, &beat, source.as_deref(), number)?;
            let abs = from_rel(&root, &folder);
            if fill.is_some() {
                plan_layout(&mut b, &abs);
                after = AfterExec::SortIntoBeat { folder, new_beat: None, inbox_ids: vec![], draft: fill, number };
            }
            Some((abs, kind))
        }
        AddTarget::Inbox => None,
        AddTarget::New { name, bpm, key, kind, authors, coauthors } => {
            let number = planned_number(&state.db.lock().unwrap(), &cfg, kind.as_deref().and_then(ItemKind::parse).unwrap_or(ItemKind::Beat))?;
            let user = req.user_folder.as_deref().unwrap_or(name);
            let (folder, _, meta) = new_item(&cfg, &tpl, name, *bpm, key.as_deref(), kind.as_deref(), authors, coauthors, user, number)?;
            let abs = from_rel(&root, &folder);
            let kind = meta.kind;
            plan_layout(&mut b, &abs);
            after = AfterExec::SortIntoBeat { folder, new_beat: Some(meta), inbox_ids: vec![], draft: None, number };
            Some((abs, kind))
        }
    };
    for (i, src) in paths.iter().enumerate() {
        let dest_dir = match &beat_abs {
            Some((beat, item_kind)) => {
                if !item_kind.accepts(kind_of_path(src), src) {
                    b.block(Issue::new(ops::IssueCode::VoicetagFileType, src));
                    continue;
                }
                if item_kind.is_flat() {
                    beat.clone()
                } else {
                    let kind = kind_of_path(src);
                    let slot = req.slots.get(&i.to_string()).copied().unwrap_or_else(|| suggest_slot(src, kind.as_str()));
                    beat.join(tpl.beat_subdirs.get(slot))
                }
            }
            None => cfg.inbox_path().ok_or("library is not set up")?,
        };
        b.create_dir(&dest_dir);
        let original = src.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let name = unique_name(&dest_dir, &original, &taken);
        let dest = dest_dir.join(&name);
        taken.insert(fold_key(&dest));
        b.copy_item(src.clone(), dest);
    }
    Ok(register(&state, b.build(), after, &cfg))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", tag = "type")]
pub enum DragTarget {
    Asset { id: i64 },
    /// The beat's newest audio file.
    Beat { id: i64 },
}

static DRAG_ICON: &[u8] = include_bytes!("../icons/64x64.png");

/// Start a native drag of a library file into FL Studio, Ableton, Telegram, a
/// mail app… Always a copy: the receiving app never moves our file away.
#[tauri::command]
pub fn start_drag(app: AppHandle, window: tauri::WebviewWindow, state: State<'_, AppState>, target: DragTarget) -> CmdResult<()> {
    let id = match target {
        DragTarget::Asset { id } => id,
        DragTarget::Beat { id } => state
            .db
            .lock()
            .unwrap()
            .get_beat(id)
            .map_err(err)?
            .and_then(|b| b.preview_asset_id)
            .ok_or("no audio to drag")?,
    };
    let (_, path) = asset_abs(&state, id)?;
    if !path.exists() {
        return Err("file not found".into());
    }
    state.dragging_out.store(true, Ordering::SeqCst);
    let app2 = app.clone();
    app.run_on_main_thread(move || {
        let item = drag::DragItem::Files(vec![path]);
        let icon = drag::Image::Raw(DRAG_ICON.to_vec());
        let done_app = app2.clone();
        let on_done = move |_r: drag::DragResult, _p: drag::CursorPosition| {
            done_app.state::<AppState>().dragging_out.store(false, Ordering::SeqCst);
        };
        let opts = drag::Options { mode: drag::DragMode::Copy, ..Default::default() };
        #[cfg(target_os = "linux")]
        let result = window
            .gtk_window()
            .map_err(|e| e.to_string())
            .and_then(|w| drag::start_drag(&w, item, icon, on_done, opts).map_err(|e| e.to_string()));
        #[cfg(not(target_os = "linux"))]
        let result = drag::start_drag(&window, item, icon, on_done, opts).map_err(|e| e.to_string());
        if let Err(e) = result {
            log::warn!("drag failed: {e}");
            app2.state::<AppState>().dragging_out.store(false, Ordering::SeqCst);
        }
    })
    .map_err(err)
}

// ───────────────────────────── template editor ─────────────────────────────

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplatePatch {
    pub file_name: Option<String>,
    pub beat_dir: Option<String>,
    pub loop_dir: Option<String>,
    pub starter_dir: Option<String>,
    pub voicetag_dir: Option<String>,
}

fn clean_pattern(p: &str) -> String {
    p.trim().trim_matches('/').split('/').map(str::trim).filter(|s| !s.is_empty()).collect::<Vec<_>>().join("/")
}

/// Change naming patterns. Folder names may always change (old folders get a
/// "rename" button); the folder *structure* (sections and depth) is locked
/// once items of that kind exist, because the library finds them by it.
#[tauri::command]
pub fn update_template(app: AppHandle, state: State<'_, AppState>, patch: TemplatePatch) -> CmdResult<Config> {
    let mut cfg = state.config();
    let (_, mut tpl) = library(&cfg)?;
    if let Some(f) = patch.file_name {
        let f = f.trim().to_string();
        if !f.contains('{') || f.contains('/') {
            return Err("bad_pattern".into());
        }
        tpl.file_name = f;
    }
    for (kind, new) in [
        (ItemKind::Beat, patch.beat_dir),
        (ItemKind::Loop, patch.loop_dir),
        (ItemKind::Starter, patch.starter_dir),
        (ItemKind::Voicetag, patch.voicetag_dir),
    ] {
        let Some(new) = new else { continue };
        let new = clean_pattern(&new);
        if new.is_empty() || !new.rsplit('/').next().unwrap_or("").contains('{') {
            return Err("bad_pattern".into());
        }
        let mut next = tpl.clone();
        match kind {
            ItemKind::Beat => next.beat_dir = new.clone(),
            ItemKind::Loop => next.loop_dir = new.clone(),
            ItemKind::Starter => next.starter_dir = new.clone(),
            ItemKind::Voicetag => next.voicetag_dir = new.clone(),
        }
        let same_shape = next.static_prefix(kind) == tpl.static_prefix(kind) && next.depth(kind) == tpl.depth(kind);
        if !same_shape && state.db.lock().unwrap().count_kind(kind.as_str()).map_err(err)? > 0 {
            return Err("structure_locked".into());
        }
        tpl = next;
    }
    cfg.template = Some(tpl);
    state.save_config(&cfg)?;
    let _ = app.emit("library-changed", ());
    Ok(cfg)
}

/// Live example for the template editor.
#[tauri::command]
pub fn render_preview(state: State<'_, AppState>, pattern: String) -> CmdResult<String> {
    let cfg = state.config();
    let me = cfg.producer_name.clone().unwrap_or_else(|| "maks".into());
    let v = TokenValues {
        name: "Night".into(),
        bpm: Some(140.0),
        key: Some("Am".into()),
        authors: vec![me],
        coauthors: vec!["lowkey".into()],
        ..Default::default()
    };
    let parts = beatcore::template::render_path(&clean_pattern(&pattern), &v);
    Ok(parts.join("/"))
}

// ───────────────────────────── list extras ─────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RowPeaks {
    pub id: i64,
    pub peaks: Vec<u8>,
}

const ROW_POINTS: usize = 96;

fn downsample(p: &[u8], n: usize) -> Vec<u8> {
    if p.is_empty() {
        return vec![];
    }
    (0..n)
        .map(|i| {
            let a = i * p.len() / n;
            let b = ((i + 1) * p.len() / n).max(a + 1).min(p.len());
            p[a.min(p.len() - 1)..b].iter().copied().max().unwrap_or(0)
        })
        .collect()
}

/// Small waveforms for list rows. Reads the cached peaks; a file seen for
/// the first time is decoded once in the background and cached.
#[tauri::command]
pub async fn row_peaks(app: AppHandle, ids: Vec<i64>) -> CmdResult<Vec<RowPeaks>> {
    let state = app.state::<AppState>();
    let mut out = Vec::new();
    for id in ids.into_iter().take(32) {
        let Ok((asset, abs)) = asset_abs(&state, id) else { continue };
        if asset.kind != "audio" {
            continue;
        }
        let cached = state.db.lock().unwrap().peaks(id).map_err(err)?;
        let peaks = match cached {
            Some(p) => p,
            None => {
                let path = abs.clone();
                match tauri::async_runtime::spawn_blocking(move || beatcore::audio::compute_peaks(&path)).await {
                    Ok(Ok((p, d))) => {
                        state.db.lock().unwrap().set_peaks(id, &p, d).map_err(err)?;
                        p
                    }
                    _ => {
                        out.push(RowPeaks { id, peaks: vec![] });
                        continue;
                    }
                }
            }
        };
        out.push(RowPeaks { id, peaks: downsample(&peaks, ROW_POINTS) });
    }
    Ok(out)
}

/// How many items each section of the library holds (for the sidebar).
#[tauri::command]
pub fn library_counts(state: State<'_, AppState>) -> CmdResult<HashMap<String, i64>> {
    let db = state.db.lock().unwrap();
    let mut m = HashMap::new();
    for k in ItemKind::ALL {
        m.insert(k.as_str().to_string(), db.count_items(k.as_str()).map_err(err)?);
    }
    Ok(m)
}

// ───────────────────────────── covers & export ─────────────────────────────

/// The UI shows covers through the asset protocol, which needs absolute paths.
fn with_abs_cover(mut row: BeatRow, root: &Path) -> BeatRow {
    row.cover = row.cover.map(|c| from_rel(root, &c).to_string_lossy().to_string());
    row
}

fn has_ext(p: &Path, ext: &str) -> bool {
    p.extension().map(|e| e.eq_ignore_ascii_case(ext)).unwrap_or(false)
}

/// Pick a picture, bring it to cover size, and plan putting it into the
/// beat's Artwork folder as `cover.jpg`. A previous cover goes to the trash
/// in the same plan, so nothing is ever overwritten.
#[tauri::command]
pub async fn plan_set_cover(app: AppHandle, id: i64, title: String, label: String) -> CmdResult<Option<PlanView>> {
    use tauri_plugin_dialog::DialogExt;
    let state = app.state::<AppState>();
    let cfg = state.config();
    let (root, tpl) = library(&cfg)?;
    let beat = state.db.lock().unwrap().get_beat(id).map_err(err)?.ok_or("beat not found")?;
    if kind_of(&beat).is_flat() {
        return Err("no_cover_for_kind".into());
    }
    let folder = from_rel(&root, beat.folder.as_deref().ok_or("this beat has no folder")?);
    let Some(picked) = app
        .dialog()
        .file()
        .set_title(title)
        .add_filter("JPG, PNG, WebP", &["jpg", "jpeg", "png", "webp"])
        .blocking_pick_file()
    else {
        return Ok(None);
    };
    let src = picked.into_path().map_err(err)?;
    let bytes = tauri::async_runtime::spawn_blocking(move || beatcore::cover::normalized_cover(&src))
        .await
        .map_err(err)?
        .map_err(|_| "bad_image".to_string())?;

    // The prepared JPEG waits in the app's own cache; the plan copies it in.
    let cache = app.path().app_cache_dir().map_err(err)?.join("covers");
    std::fs::create_dir_all(&cache).map_err(err)?;
    if let Ok(entries) = std::fs::read_dir(&cache) {
        // Leftovers of plans that were never run (our own files, an hour old).
        for e in entries.flatten() {
            let age = e.metadata().ok().and_then(|m| m.modified().ok()).and_then(|t| t.elapsed().ok());
            if age.map(|d| d.as_secs() > 3600).unwrap_or(false) {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    let staged = cache.join(format!("cover-{}.jpg", uuid_like()));
    std::fs::write(&staged, &bytes).map_err(err)?;

    let art = folder.join(&tpl.beat_subdirs.artwork);
    let mut b = PlanBuilder::new(label, cfg.plan_roots());
    if let Some(old) = beat.cover.as_deref() {
        b.trash_item(from_rel(&root, old));
    }
    b.create_dir(&art);
    b.copy_item(staged, art.join(beatcore::cover::COVER_FILE));
    Ok(Some(register(&state, b.build(), AfterExec::Nothing, &cfg)))
}

#[tauri::command]
pub async fn plan_remove_cover(state: State<'_, AppState>, id: i64, label: String) -> CmdResult<PlanView> {
    let cfg = state.config();
    let (root, _) = library(&cfg)?;
    let beat = state.db.lock().unwrap().get_beat(id).map_err(err)?.ok_or("beat not found")?;
    let cover = beat.cover.as_deref().ok_or("this beat has no cover")?;
    let mut b = PlanBuilder::new(label, cfg.plan_roots());
    b.trash_item(from_rel(&root, cover));
    Ok(register(&state, b.build(), AfterExec::Nothing, &cfg))
}

/// "Download": save a copy of a beat's MP3 with its cover, title, credits,
/// BPM and key in the tags, ready to send. The library file is only read.
/// Returns the saved file's name, or `None` if the dialog was cancelled.
#[tauri::command]
pub async fn export_mp3(app: AppHandle, id: i64, title: String) -> CmdResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let state = app.state::<AppState>();
    let cfg = state.config();
    let (root, _) = library(&cfg)?;
    let (asset, src) = asset_abs(&state, id)?;
    if !has_ext(&src, "mp3") || !src.is_file() {
        return Err("not an mp3 file".into());
    }
    let beat = match asset.beat_id {
        Some(b) => state.db.lock().unwrap().get_beat(b).map_err(err)?,
        None => None,
    }
    .ok_or("beat not found")?;

    let mut dialog = app.dialog().file().set_title(title).set_file_name(asset.name.clone()).add_filter("MP3", &["mp3"]);
    if let Ok(dir) = app.path().download_dir() {
        dialog = dialog.set_directory(dir);
    }
    let Some(picked) = dialog.blocking_save_file() else {
        return Ok(None);
    };
    let mut dest = picked.into_path().map_err(err)?;
    if !has_ext(&dest, "mp3") {
        let mut name = dest.file_name().map(|n| n.to_os_string()).unwrap_or_default();
        name.push(".mp3");
        dest.set_file_name(name);
    }
    // The library changes only through plans; an export has to land elsewhere.
    if cfg.plan_roots().iter().any(|r| is_within(&dest, r)) {
        return Err("export_inside_library".into());
    }

    let people: Vec<String> = beat.authors.iter().chain(beat.coauthors.iter()).cloned().collect();
    let artist = if people.is_empty() { cfg.producer_name.clone() } else { Some(people.join(" x ")) };
    let meta = beatcore::cover::Mp3Meta {
        title: beat.name.clone(),
        artist,
        bpm: beat.bpm,
        key: beat.key.clone(),
        genre: beat.genre.clone(),
    };
    let cover = beat.cover.as_deref().map(|c| from_rel(&root, c));
    let to = dest.clone();
    tauri::async_runtime::spawn_blocking(move || beatcore::cover::export_mp3(&src, &to, &meta, cover.as_deref()))
        .await
        .map_err(err)?
        .map_err(err)?;
    // Show the file so it can be dragged straight into a chat.
    let _ = app.opener().reveal_item_in_dir(&dest);
    Ok(dest.file_name().map(|n| n.to_string_lossy().to_string()))
}

// ───────────────────────────── "Add folder" button ─────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportDto {
    pub drop: DropDto,
    #[serde(flatten)]
    pub suggestion: ImportSuggestion,
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ImportSuggestion {
    pub user_folder: String,
    pub name: String,
    pub bpm: Option<f64>,
    pub key: Option<String>,
    pub authors: Vec<String>,
}

fn import_suggestion(original: String, folder: bool, live: bool) -> ImportSuggestion {
    let source = if live { original.strip_suffix(" Project").unwrap_or(&original) } else { &original };
    let parsed = if folder { parse_folder_name(source) } else { parse_name(source) };
    ImportSuggestion {
        name: if parsed.title.trim().is_empty() { original.clone() } else { parsed.title },
        user_folder: original,
        bpm: parsed.bpm.map(f64::from),
        key: parsed.key.map(|k| k.name().to_owned()),
        authors: parsed.producers,
    }
}

const IMPORT_MAX_ITEMS: usize = 200;

/// Pick a folder (its contents become the files of one new item) or loose
/// files. Like every picker, it runs here: the UI gets an ID and names.
#[tauri::command]
pub async fn pick_import(app: AppHandle, folder: bool, title: String) -> CmdResult<Option<ImportDto>> {
    use tauri_plugin_dialog::DialogExt;
    let state = app.state::<AppState>();
    let cfg = state.config();
    let (root, _) = library(&cfg)?;
    let dialog = app.dialog().file().set_title(title);

    let (paths, source_name) = if folder {
        let Some(picked) = dialog.blocking_pick_folder() else { return Ok(None) };
        let dir = picked.into_path().map_err(err)?;
        if is_within(&dir, &root) || is_within(&root, &dir) {
            return Err("already_in_library".into());
        }
        let name = dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if beatcore::daw::is_ableton_project_dir(&dir) {
            // An Ableton project travels as one piece.
            (vec![dir], name)
        } else {
            let mut entries: Vec<PathBuf> = std::fs::read_dir(&dir)
                .map_err(err)?
                .flatten()
                .map(|e| e.path())
                // Judge backups by the entry's own name, not by where the folder lives.
                .filter(|p| p.file_name().map(|n| !beatcore::daw::is_backup_path(Path::new(n))).unwrap_or(false))
                .filter(|p| !beatcore::daw::is_junk(p))
                .collect();
            entries.sort();
            (entries, name)
        }
    } else {
        let paths: Vec<PathBuf> =
            dialog.blocking_pick_files().unwrap_or_default().into_iter().filter_map(|p| p.into_path().ok()).collect();
        if paths.is_empty() {
            return Ok(None);
        }
        if paths.iter().any(|p| is_within(p, &root)) {
            return Err("already_in_library".into());
        }
        let name = paths[0].file_stem().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        (paths, name)
    };
    if paths.len() > IMPORT_MAX_ITEMS {
        return Err("too_many_items".into());
    }
    let Some(drop) = register_drop(&state, paths) else {
        return Err("empty_folder".into());
    };
    let live = folder && drop.items.len() == 1 && drop.items[0].kind == "ableton_project" && drop.items[0].name == source_name;
    Ok(Some(ImportDto { drop, suggestion: import_suggestion(source_name, folder, live) }))
}

// ───────────────────────────── delete an item ─────────────────────────────

/// Plan sending an item's whole folder to the OS trash. The plan carries a
/// warning with the number of files inside, so nothing disappears unseen.
/// The item's row goes away when the library re-reads the missing folder.
#[tauri::command]
pub async fn plan_delete_item(state: State<'_, AppState>, id: i64, label: String) -> CmdResult<PlanView> {
    let cfg = state.config();
    let (root, tpl) = library(&cfg)?;
    let beat = state.db.lock().unwrap().get_beat(id).map_err(err)?.ok_or("beat not found")?;
    let abs = from_rel(&root, beat.folder.as_deref().ok_or("no_folder")?);
    let files = walkdir::WalkDir::new(&abs)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .count();
    let mut b = PlanBuilder::new(label, cfg.plan_roots());
    if files > 0 {
        b.warn(Issue::new(ops::IssueCode::DeletesFiles, &abs).detail(files.to_string()));
    }
    b.trash_item(abs.clone());
    let folder = beat.folder.as_deref().unwrap();
    if let Some((service, _)) = tpl.item_folder_of(folder) {
        if service != folder { b.remove_empty_dir(from_rel(&root, &service)); }
    }
    Ok(register(&state, b.build(), AfterExec::Nothing, &cfg))
}

// ───────────────────────────── "New folder" button ─────────────────────────────

/// Make an empty item. Nothing is created on disk: the folder appears, named
/// after the first file, when files are added to it.
#[tauri::command]
pub fn create_item(app: AppHandle, state: State<'_, AppState>, kind: String, name: String) -> CmdResult<BeatRow> {
    let cfg = state.config();
    library(&cfg)?;
    let name = name.trim();
    if name.is_empty() {
        return Err("name is empty".into());
    }
    let authors: Vec<String> = if kind == "voicetag" {
        vec![]
    } else {
        cfg.producer_name.clone().filter(|n| !n.trim().is_empty()).into_iter().collect()
    };
    let db = state.db.lock().unwrap();
    let id = db.create_draft(name, &kind, &authors).map_err(err)?;
    let row = db.get_beat(id).map_err(err)?.ok_or("beat not found")?;
    let _ = app.emit("beat-updated", id);
    Ok(row)
}

/// Remove an empty item made by mistake. Only works while it has no folder.
#[tauri::command]
pub fn delete_draft(app: AppHandle, state: State<'_, AppState>, id: i64) -> CmdResult<()> {
    if !state.db.lock().unwrap().delete_draft(id).map_err(err)? {
        return Err("not_empty".into());
    }
    let _ = app.emit("library-changed", ());
    Ok(())
}


#[cfg(test)]
mod draft_target_tests {
    use super::*;
    use beatcore::db::Db;
    use chrono::Datelike;

    fn setup() -> (tempfile::TempDir, Db, LibraryTemplate, String) {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_in_memory().unwrap();
        let tpl = builtin_templates().remove(0);
        let year = chrono::Local::now().year().to_string();
        (dir, db, tpl, year)
    }

    #[test]
    fn an_empty_item_is_named_after_its_first_file() {
        let (dir, db, tpl, year) = setup();
        let id = db.create_draft("Без названия", "beat", &["maks".into()]).unwrap();
        let beat = db.get_beat(id).unwrap().unwrap();
        let first = Path::new("C:/Downloads/Night_140bpm_Am.wav");
        let (folder, _, kind, fill) = existing_target(&tpl, dir.path(), &db, &beat, Some(&source_stem(first)), None).unwrap();
        let fill = fill.expect("a draft is filled in");
        assert_eq!(kind, ItemKind::Beat);
        assert_eq!(fill.name, "Night");
        assert_eq!(fill.bpm, Some(140.0));
        assert_eq!(fill.key.as_deref(), Some("Am"));
        assert_eq!(fill.authors, vec!["maks"]);
        assert_eq!(folder, format!("01_Beats/{year}/Night_140bpm_Am/Night_140bpm_Am"));

        // A plain name is taken whole, minus the extension.
        let (folder, _, _, fill) =
            existing_target(&tpl, dir.path(), &db, &beat, Some("Тёмный лес"), None).unwrap();
        assert_eq!(fill.unwrap().name, "Тёмный лес");
        assert_eq!(folder, format!("01_Beats/{year}/Тёмный лес/Тёмный лес"));
    }

    #[test]
    fn a_typed_name_wins_and_existing_folders_are_never_merged_into() {
        let (dir, db, tpl, year) = setup();
        let id = db.create_draft("Без названия", "beat", &[]).unwrap();
        db.update_beat(id, &BeatPatch { name: Some("Мой бит".into()), ..Default::default() }).unwrap();
        let beat = db.get_beat(id).unwrap().unwrap();
        let first = Path::new("/x/Night_140bpm_Am.wav");
        let (folder, _, _, fill) = existing_target(&tpl, dir.path(), &db, &beat, Some(&source_stem(first)), None).unwrap();
        let fill = fill.unwrap();
        assert_eq!(fill.name, "Мой бит");
        assert_eq!(fill.bpm, Some(140.0), "BPM and key are still read from the first source");
        assert_eq!(folder, format!("01_Beats/{year}/Мой бит_140bpm_Am/Мой бит"));

        std::fs::create_dir_all(dir.path().join(&folder)).unwrap();
        let (again, _, _, _) = existing_target(&tpl, dir.path(), &db, &beat, Some(&source_stem(first)), None).unwrap();
        assert_eq!(again, format!("01_Beats/{year}/Мой бит_140bpm_Am (2)/Мой бит"));
    }

    #[test]
    fn voice_tags_keep_the_whole_file_name_and_ordinary_items_are_untouched() {
        let (dir, db, tpl, _) = setup();
        let id = db.create_draft("Без названия", "voicetag", &[]).unwrap();
        let beat = db.get_beat(id).unwrap().unwrap();
        let (folder, _, kind, fill) =
            existing_target(&tpl, dir.path(), &db, &beat, Some("Maks Tag"), None).unwrap();
        assert_eq!(kind, ItemKind::Voicetag);
        assert_eq!(fill.unwrap().name, "Maks Tag");
        assert_eq!(folder, "07_Voicetags/Maks Tag/Maks Tag");

        db.adopt_draft(id, &folder).unwrap();
        let beat = db.get_beat(id).unwrap().unwrap();
        let (same, _, _, fill) =
            existing_target(&tpl, dir.path(), &db, &beat, Some("Other"), None).unwrap();
        assert_eq!(same, folder);
        assert!(fill.is_none(), "an item that has a folder keeps its name");
    }
}
