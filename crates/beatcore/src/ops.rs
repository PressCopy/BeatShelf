//! Plan → validate → confirm → execute → journal → undo.
//!
//! Nothing touches the disk until a [`Plan`] without blockers is executed.
//! Execution re-validates first (the disk may have changed while the user
//! was reading the plan), records every completed operation in a
//! [`Journal`], and rolls back the finished part of the batch if a later
//! step fails.
//!
//! Safety rules enforced here:
//! * never overwrite: a destination that exists (case-insensitively) blocks
//!   the plan, and is re-checked right before each rename;
//! * nothing is deleted outside the OS trash. The only hard deletes are of
//!   empty folders this app created, and of temporary copies it made itself;
//! * an `.als` is never separated from its Ableton project folder;
//! * moves across volumes are copy → verify (BLAKE3) → rename into place →
//!   original to trash.

use crate::daw::{
    ableton_external_samples, ableton_project_root, classify_file, ext_lower, is_ableton_project_dir, is_junk,
    AssetKind,
};
use crate::error::{Error, Result};
use crate::pathsafe::{
    exists_ignoring_case, fold_key, is_cloud_placeholder, is_within, path_len_chars, same_path_ignoring_case,
    LONG_PATH_WARN_CHARS,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Step {
    /// Ensure the folder exists (parents included).
    CreateDir { path: PathBuf },
    /// Atomically claim a fresh service directory. Its parent must exist.
    CreateNewDir { path: PathBuf },
    /// Move or rename a file or a whole folder.
    Move { from: PathBuf, to: PathBuf },
    /// Remove a folder only if it is empty (used by undo).
    RemoveEmptyDir { path: PathBuf },
    /// Copy a file or folder into the library. The original stays where it
    /// is, so this is how files from anywhere (drag and drop, "Add files")
    /// enter the library.
    Copy { from: PathBuf, to: PathBuf },
    /// Send an item to the OS trash. Only produced by undo of a `Copy`.
    Trash { path: PathBuf },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum IssueCode {
    // Blockers
    SourceMissing,
    DestinationExists,
    DestinationDuplicate,
    OutsideLibrary,
    IntoItself,
    AbletonTear,
    CloudPlaceholder,
    ParentMissing,
    NotADirectory,
    /// Library rule, not a disk state: voice tags take WAV, MP3 and DAW projects.
    VoicetagFileType,
    // Warnings
    LongPath,
    AbletonExternalSamples,
    AbletonLooseSet,
    FlSamplePaths,
    MaybeOpenInDaw,
    DirNotEmpty,
    /// Undo can't pull things back out of the OS trash; restore by hand.
    InTrash,
    /// The folder going to the trash still has files in it (`detail` = how many).
    DeletesFiles,
}

impl IssueCode {
    /// Rules of the library itself. They don't go away when the disk
    /// changes, so `execute` refuses a plan that carries one.
    pub fn is_policy(&self) -> bool {
        matches!(self, IssueCode::VoicetagFileType)
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub code: IssueCode,
    pub path: Option<String>,
    pub detail: Option<String>,
    pub items: Vec<String>,
}

impl Issue {
    pub fn new(code: IssueCode, path: &Path) -> Self {
        Issue { code, path: Some(path.to_string_lossy().to_string()), detail: None, items: vec![] }
    }
    pub fn detail(mut self, d: impl Into<String>) -> Self {
        self.detail = Some(d.into());
        self
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub id: String,
    pub label: String,
    pub steps: Vec<Step>,
    pub warnings: Vec<Issue>,
    pub blockers: Vec<Issue>,
    pub undo_of: Option<i64>,
    #[serde(skip)]
    pub roots: Vec<PathBuf>,
}

impl Plan {
    pub fn is_executable(&self) -> bool {
        self.blockers.is_empty()
    }
}

pub struct PlanBuilder {
    label: String,
    steps: Vec<Step>,
    roots: Vec<PathBuf>,
    undo_of: Option<i64>,
    warnings: Vec<Issue>,
    blockers: Vec<Issue>,
}

impl PlanBuilder {
    /// `roots`: folders this plan may touch (library root, Inbox, DAW folders).
    pub fn new(label: impl Into<String>, roots: Vec<PathBuf>) -> Self {
        PlanBuilder { label: label.into(), steps: vec![], roots, undo_of: None, warnings: vec![], blockers: vec![] }
    }
    pub fn undo_of(mut self, batch: i64) -> Self {
        self.undo_of = Some(batch);
        self
    }
    pub fn allow_root(&mut self, p: &Path) -> &mut Self {
        if !self.roots.iter().any(|r| is_within(p, r)) {
            self.roots.push(p.to_path_buf());
        }
        self
    }
    pub fn create_dir(&mut self, p: impl Into<PathBuf>) -> &mut Self {
        self.steps.push(Step::CreateDir { path: p.into() });
        self
    }
    pub fn create_new_dir(&mut self, p: &Path) -> &mut Self {
        if let Some(parent) = p.parent() { self.create_dir(parent); }
        self.steps.push(Step::CreateNewDir { path: p.to_path_buf() });
        self
    }
    pub fn move_item(&mut self, from: impl Into<PathBuf>, to: impl Into<PathBuf>) -> &mut Self {
        self.steps.push(Step::Move { from: from.into(), to: to.into() });
        self
    }
    pub fn copy_item(&mut self, from: impl Into<PathBuf>, to: impl Into<PathBuf>) -> &mut Self {
        self.steps.push(Step::Copy { from: from.into(), to: to.into() });
        self
    }
    pub fn trash_item(&mut self, p: impl Into<PathBuf>) -> &mut Self {
        self.steps.push(Step::Trash { path: p.into() });
        self
    }
    pub fn remove_empty_dir(&mut self, p: impl Into<PathBuf>) -> &mut Self {
        self.steps.push(Step::RemoveEmptyDir { path: p.into() });
        self
    }
    pub fn warn(&mut self, issue: Issue) -> &mut Self {
        self.warnings.push(issue);
        self
    }
    /// Refuse the plan for a reason the disk can't show (a library rule).
    pub fn block(&mut self, issue: Issue) -> &mut Self {
        self.blockers.push(issue);
        self
    }
    pub fn build(self) -> Plan {
        let v = validate(&self.steps, &self.roots, true);
        let mut warnings = self.warnings;
        warnings.extend(v.warnings);
        let mut blockers = self.blockers;
        blockers.extend(v.blockers);
        Plan {
            id: uuid::Uuid::new_v4().to_string(),
            label: self.label,
            steps: v.steps,
            warnings,
            blockers,
            undo_of: self.undo_of,
            roots: self.roots,
        }
    }
}

/// Snapshot of what the disk will look like after the steps so far.
#[derive(Default)]
struct Virtual {
    created: HashSet<String>,
    removed: HashSet<String>,
}

impl Virtual {
    fn exists(&self, p: &Path) -> bool {
        if self.created.contains(&fold_key(p)) {
            return true;
        }
        if p.ancestors().any(|a| self.removed.contains(&fold_key(a))) {
            return false;
        }
        exists_ignoring_case(p)
    }
    fn is_dir(&self, p: &Path) -> bool {
        self.created.contains(&fold_key(p)) || (self.exists(p) && p.is_dir())
    }
    fn add(&mut self, p: &Path) {
        let k = fold_key(p);
        self.removed.remove(&k);
        self.created.insert(k);
    }
    fn remove(&mut self, p: &Path) {
        let k = fold_key(p);
        self.created.remove(&k);
        self.removed.insert(k);
    }
}

struct Validation {
    steps: Vec<Step>,
    warnings: Vec<Issue>,
    blockers: Vec<Issue>,
}

fn within_roots(p: &Path, roots: &[PathBuf]) -> bool {
    roots.iter().any(|r| is_within(p, r))
}

fn recently_modified(p: &Path, window: Duration) -> bool {
    let newest = |m: fs::Metadata| m.modified().ok();
    let check = |t: Option<SystemTime>| {
        t.and_then(|t| SystemTime::now().duration_since(t).ok()).map(|d| d < window).unwrap_or(false)
    };
    if p.is_dir() {
        fs::read_dir(p)
            .map(|rd| rd.flatten().any(|e| ext_lower(&e.path()) == "als" && check(e.metadata().ok().and_then(newest))))
            .unwrap_or(false)
    } else {
        check(fs::metadata(p).ok().and_then(newest))
    }
}

fn longest_path_after_move(from: &Path, to: &Path) -> usize {
    let base = path_len_chars(to);
    if !from.is_dir() {
        return base;
    }
    let from_len = path_len_chars(from);
    walkdir::WalkDir::new(from)
        .into_iter()
        .take(20_000)
        .flatten()
        .map(|e| base + path_len_chars(e.path()).saturating_sub(from_len))
        .max()
        .unwrap_or(base)
}

fn non_junk_entries(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| !is_junk(p)).collect())
        .unwrap_or_default()
}

fn validate(steps: &[Step], roots: &[PathBuf], with_warnings: bool) -> Validation {
    let mut v = Virtual::default();
    let mut out = Validation { steps: vec![], warnings: vec![], blockers: vec![] };
    let mut targets: HashSet<String> = HashSet::new();

    for step in steps {
        match step {
            Step::CreateNewDir { path } if v.exists(path) => {
                out.blockers.push(Issue::new(IssueCode::DestinationExists, path));
            }
            Step::CreateDir { path } | Step::CreateNewDir { path } => {
                if !within_roots(path, roots) {
                    out.blockers.push(Issue::new(IssueCode::OutsideLibrary, path));
                    continue;
                }
                if v.exists(path) {
                    if !v.is_dir(path) {
                        out.blockers.push(Issue::new(IssueCode::NotADirectory, path));
                    }
                    continue; // already there: drop the no-op
                }
                for a in path.ancestors() {
                    if v.exists(a) {
                        break;
                    }
                    v.add(a);
                }
                out.steps.push(step.clone());
            }
            Step::RemoveEmptyDir { path } => {
                if !v.exists(path) {
                    continue;
                }
                let remaining: Vec<PathBuf> = non_junk_entries(path)
                    .into_iter()
                    .filter(|e| !v.removed.contains(&fold_key(e)))
                    .collect();
                let planned_inside = v.created.iter().any(|k| k.starts_with(&(fold_key(path) + "/")));
                if !remaining.is_empty() || planned_inside {
                    if with_warnings {
                        out.warnings.push(Issue::new(IssueCode::DirNotEmpty, path));
                    }
                } else {
                    v.remove(path);
                }
                out.steps.push(step.clone());
            }
            Step::Trash { path } => {
                if !v.exists(path) {
                    continue; // already gone
                }
                if !within_roots(path, roots) {
                    out.blockers.push(Issue::new(IssueCode::OutsideLibrary, path));
                    continue;
                }
                v.remove(path);
                out.steps.push(step.clone());
            }
            Step::Copy { from, to } => {
                let mut blocked = false;
                // The source may live anywhere (Downloads, Desktop); only the
                // destination has to be inside the library.
                if !from.exists() {
                    out.blockers.push(Issue::new(IssueCode::SourceMissing, from));
                    blocked = true;
                }
                if !within_roots(to, roots) {
                    out.blockers.push(Issue::new(IssueCode::OutsideLibrary, to));
                    blocked = true;
                }
                if is_within(to, from) {
                    out.blockers.push(Issue::new(IssueCode::IntoItself, to));
                    blocked = true;
                }
                let tkey = fold_key(to);
                if targets.contains(&tkey) {
                    out.blockers.push(Issue::new(IssueCode::DestinationDuplicate, to));
                    blocked = true;
                } else if v.exists(to) {
                    out.blockers.push(Issue::new(IssueCode::DestinationExists, to));
                    blocked = true;
                }
                if let Some(parent) = to.parent() {
                    if !v.is_dir(parent) {
                        out.blockers.push(Issue::new(IssueCode::ParentMissing, parent));
                        blocked = true;
                    }
                }
                if is_cloud_placeholder(from) {
                    out.blockers.push(Issue::new(IssueCode::CloudPlaceholder, from));
                    blocked = true;
                }
                if with_warnings && !blocked {
                    if !from.is_dir() && matches!(classify_file(from), AssetKind::FlProject) {
                        out.warnings.push(Issue::new(IssueCode::FlSamplePaths, from));
                    }
                    let longest = longest_path_after_move(from, to);
                    if longest > LONG_PATH_WARN_CHARS {
                        out.warnings.push(Issue::new(IssueCode::LongPath, to).detail(longest.to_string()));
                    }
                }
                targets.insert(tkey);
                v.add(to);
                out.steps.push(step.clone());
            }
            Step::Move { from, to } => {
                if from == to {
                    continue;
                }
                let mut blocked = false;
                let mut block = |issue: Issue, out: &mut Validation| {
                    out.blockers.push(issue);
                    blocked = true;
                };
                if !v.exists(from) {
                    block(Issue::new(IssueCode::SourceMissing, from), &mut out);
                }
                if !within_roots(from, roots) || !within_roots(to, roots) {
                    block(Issue::new(IssueCode::OutsideLibrary, to), &mut out);
                }
                let case_only = same_path_ignoring_case(from, to);
                if !case_only && is_within(to, from) {
                    block(Issue::new(IssueCode::IntoItself, to), &mut out);
                }
                let tkey = fold_key(to);
                if targets.contains(&tkey) {
                    block(Issue::new(IssueCode::DestinationDuplicate, to), &mut out);
                } else if !case_only && v.exists(to) {
                    block(Issue::new(IssueCode::DestinationExists, to), &mut out);
                }
                if let Some(parent) = to.parent() {
                    if !v.is_dir(parent) {
                        block(Issue::new(IssueCode::ParentMissing, parent), &mut out);
                    }
                }
                if is_cloud_placeholder(from) {
                    block(Issue::new(IssueCode::CloudPlaceholder, from), &mut out);
                }
                if let Some(proj) = ableton_project_root(from) {
                    if !same_path_ignoring_case(&proj, from) && !is_within(to, &proj) {
                        block(
                            Issue::new(IssueCode::AbletonTear, from).detail(proj.to_string_lossy()),
                            &mut out,
                        );
                    }
                }

                if with_warnings && !blocked {
                    let is_dir = from.is_dir();
                    let kind = if is_dir { None } else { Some(classify_file(from)) };
                    if is_dir && is_ableton_project_dir(from) {
                        if let Ok(ext) = ableton_external_samples(from) {
                            if !ext.is_empty() {
                                let mut issue = Issue::new(IssueCode::AbletonExternalSamples, from)
                                    .detail(ext.len().to_string());
                                issue.items = ext.iter().take(10).map(|e| e.path.clone()).collect();
                                out.warnings.push(issue);
                            }
                        }
                    }
                    match kind {
                        Some(AssetKind::FlProject) => out.warnings.push(Issue::new(IssueCode::FlSamplePaths, from)),
                        Some(AssetKind::AbletonSet) if ableton_project_root(from).is_none() => {
                            out.warnings.push(Issue::new(IssueCode::AbletonLooseSet, from))
                        }
                        _ => {}
                    }
                    let projectish = (is_dir && is_ableton_project_dir(from))
                        || matches!(kind, Some(AssetKind::FlProject) | Some(AssetKind::AbletonSet));
                    if projectish && recently_modified(from, Duration::from_secs(120)) {
                        out.warnings.push(Issue::new(IssueCode::MaybeOpenInDaw, from));
                    }
                    let longest = longest_path_after_move(from, to);
                    if longest > LONG_PATH_WARN_CHARS {
                        out.warnings.push(Issue::new(IssueCode::LongPath, to).detail(longest.to_string()));
                    }
                }

                targets.insert(tkey);
                v.remove(from);
                v.add(to);
                out.steps.push(step.clone());
            }
        }
    }
    out
}

// ───────────────────────────── execution ─────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum DoneOp {
    CreatedDir { path: PathBuf },
    Renamed { from: PathBuf, to: PathBuf },
    CopiedAndTrashed { from: PathBuf, to: PathBuf },
    RemovedDir { path: PathBuf },
    /// A copy that entered the library; the source was left untouched.
    Copied { from: PathBuf, to: PathBuf },
    Trashed { path: PathBuf },
}

impl DoneOp {
    pub fn movement(&self) -> Option<(&Path, &Path)> {
        match self {
            DoneOp::Renamed { from, to } | DoneOp::CopiedAndTrashed { from, to } => Some((from, to)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchStatus {
    Running,
    Done,
    RolledBack,
    Failed,
}

impl BatchStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            BatchStatus::Running => "running",
            BatchStatus::Done => "done",
            BatchStatus::RolledBack => "rolled_back",
            BatchStatus::Failed => "failed",
        }
    }
}

pub trait Journal {
    fn begin_batch(&mut self, label: &str, undo_of: Option<i64>) -> Result<i64>;
    fn record_op(&mut self, batch: i64, seq: usize, op: &DoneOp) -> Result<()>;
    fn finish_batch(&mut self, batch: i64, status: BatchStatus) -> Result<()>;
}

#[derive(Debug, Default, Clone)]
pub struct ExecOptions {
    /// Treat every move as cross-volume (tests, and a "copy" mode later).
    pub force_copy: bool,
    #[doc(hidden)]
    pub fail_at_step: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecReport {
    pub batch_id: Option<i64>,
    pub ops: Vec<DoneOp>,
    pub skipped: Vec<Issue>,
}

impl ExecReport {
    pub fn movements(&self) -> Vec<(PathBuf, PathBuf)> {
        self.ops
            .iter()
            .filter_map(|o| o.movement().map(|(a, b)| (a.to_path_buf(), b.to_path_buf())))
            .collect()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ExecError {
    #[error("the plan is out of date: the files changed after it was made")]
    Stale(Vec<Issue>),
    #[error("step {step} failed: {message}")]
    Failed { step: usize, message: String, rolled_back: bool, batch_id: i64 },
    #[error(transparent)]
    Journal(#[from] Error),
}

/// Move to the OS trash. On macOS this uses NSFileManager instead of the
/// default AppleScript-to-Finder route, which would trigger an automation
/// permission prompt the first time.
pub fn trash_delete(p: &Path) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        use trash::macos::{DeleteMethod, TrashContextExtMacos};
        let mut ctx = trash::TrashContext::default();
        ctx.set_delete_method(DeleteMethod::NsFileManager);
        ctx.delete(p)?;
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        trash::delete(p)?;
        Ok(())
    }
}

#[cfg(unix)]
fn is_cross_device(e: &std::io::Error) -> bool {
    e.raw_os_error() == Some(18) // EXDEV on Linux and macOS
}
#[cfg(windows)]
fn is_cross_device(e: &std::io::Error) -> bool {
    e.raw_os_error() == Some(17) // ERROR_NOT_SAME_DEVICE
}

fn tmp_sibling(target: &Path) -> PathBuf {
    let name = format!(".beatshelf-tmp-{}", uuid::Uuid::new_v4().simple());
    target.parent().map(|p| p.join(&name)).unwrap_or_else(|| PathBuf::from(name))
}

fn hash_file(p: &Path) -> Result<blake3::Hash> {
    let mut f = fs::File::open(p)?;
    let mut h = blake3::Hasher::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize())
}

fn copy_tree(from: &Path, to: &Path) -> Result<()> {
    if from.is_dir() {
        fs::create_dir(to)?;
        for entry in walkdir::WalkDir::new(from).min_depth(1).follow_links(false) {
            let entry = entry.map_err(|e| Error::Msg(e.to_string()))?;
            let rel = entry.path().strip_prefix(from).map_err(|e| Error::Msg(e.to_string()))?;
            let dest = to.join(rel);
            if entry.file_type().is_dir() {
                fs::create_dir(&dest)?;
            } else {
                fs::copy(entry.path(), &dest)?;
            }
        }
    } else {
        fs::copy(from, to)?;
    }
    Ok(())
}

fn verify_tree(from: &Path, to: &Path) -> Result<()> {
    let pairs: Vec<(PathBuf, PathBuf)> = if from.is_dir() {
        walkdir::WalkDir::new(from)
            .min_depth(1)
            .into_iter()
            .flatten()
            .filter(|e| !e.file_type().is_dir())
            .map(|e| {
                let rel = e.path().strip_prefix(from).unwrap().to_path_buf();
                (e.path().to_path_buf(), to.join(rel))
            })
            .collect()
    } else {
        vec![(from.to_path_buf(), to.to_path_buf())]
    };
    for (a, b) in pairs {
        let (ma, mb) = (fs::metadata(&a)?, fs::metadata(&b).map_err(|_| Error::VerifyFailed(b.clone()))?);
        if ma.len() != mb.len() || hash_file(&a)? != hash_file(&b)? {
            return Err(Error::VerifyFailed(b));
        }
    }
    Ok(())
}

fn remove_own_temp(p: &Path) {
    // Only ever called on paths created by this process a moment ago.
    if p.is_dir() {
        let _ = fs::remove_dir_all(p);
    } else {
        let _ = fs::remove_file(p);
    }
}

/// Copy, verify, then atomically rename into place. Leaves nothing behind on failure.
fn copy_verified(from: &Path, to: &Path) -> Result<()> {
    let tmp = tmp_sibling(to);
    let r = copy_tree(from, &tmp).and_then(|_| verify_tree(from, &tmp)).and_then(|_| {
        if exists_ignoring_case(to) {
            return Err(Error::Msg(format!("destination appeared: {}", to.display())));
        }
        fs::rename(&tmp, to).map_err(Error::from)
    });
    if r.is_err() {
        remove_own_temp(&tmp);
    }
    r
}

fn do_create_dir(path: &Path) -> Result<Vec<DoneOp>> {
    let mut missing: Vec<PathBuf> = path.ancestors().take_while(|a| !a.exists()).map(|a| a.to_path_buf()).collect();
    missing.reverse();
    let mut ops = Vec::new();
    for d in missing {
        match fs::create_dir(&d) {
            Ok(()) => ops.push(DoneOp::CreatedDir { path: d }),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => {
                for op in ops.iter().rev() {
                    if let DoneOp::CreatedDir { path } = op {
                        let _ = fs::remove_dir(path);
                    }
                }
                return Err(e.into());
            }
        }
    }
    Ok(ops)
}

fn do_move(from: &Path, to: &Path, opts: &ExecOptions) -> Result<DoneOp> {
    if from.symlink_metadata().is_err() {
        return Err(Error::Msg(format!("source is gone: {}", from.display())));
    }
    if same_path_ignoring_case(from, to) {
        // Case/normalisation-only rename: go through a temporary name so it
        // works on case-insensitive file systems.
        let tmp = tmp_sibling(to);
        fs::rename(from, &tmp)?;
        if let Err(e) = fs::rename(&tmp, to) {
            let _ = fs::rename(&tmp, from);
            return Err(e.into());
        }
        return Ok(DoneOp::Renamed { from: from.into(), to: to.into() });
    }
    // std::fs::rename silently replaces files on every OS: check right before.
    if exists_ignoring_case(to) {
        return Err(Error::Msg(format!("destination appeared: {}", to.display())));
    }
    if !opts.force_copy {
        match fs::rename(from, to) {
            Ok(()) => return Ok(DoneOp::Renamed { from: from.into(), to: to.into() }),
            Err(e) if is_cross_device(&e) => {}
            Err(e) => return Err(e.into()),
        }
    }
    copy_verified(from, to)?;
    if let Err(e) = trash_delete(from) {
        // Original is untouched; drop our own fresh copy and report.
        remove_own_temp(to);
        return Err(e);
    }
    Ok(DoneOp::CopiedAndTrashed { from: from.into(), to: to.into() })
}

fn do_remove_empty_dir(path: &Path) -> Result<Option<DoneOp>> {
    if !path.is_dir() {
        return Ok(None);
    }
    let all: Vec<PathBuf> = fs::read_dir(path)?.flatten().map(|e| e.path()).collect();
    if all.is_empty() {
        fs::remove_dir(path)?;
        return Ok(Some(DoneOp::RemovedDir { path: path.into() }));
    }
    if all.iter().all(|p| is_junk(p) && p.is_file()) {
        // Only .DS_Store / Thumbs.db left: send the folder to the trash.
        trash_delete(path)?;
        return Ok(Some(DoneOp::RemovedDir { path: path.into() }));
    }
    Ok(None)
}

/// Undo the finished part of a failed batch. Returns `true` when everything
/// was put back.
fn rollback(done: &[DoneOp]) -> bool {
    let mut ok = true;
    for op in done.iter().rev() {
        let r: Result<()> = match op {
            DoneOp::CreatedDir { path } => fs::remove_dir(path).map_err(Error::from),
            DoneOp::Renamed { from, to } => {
                if exists_ignoring_case(from) && !same_path_ignoring_case(from, to) {
                    Err(Error::Msg("original location is occupied".into()))
                } else {
                    do_move(to, from, &ExecOptions::default()).map(|_| ())
                }
            }
            DoneOp::CopiedAndTrashed { from, to } => {
                do_move(to, from, &ExecOptions { force_copy: true, ..Default::default() }).map(|_| ())
            }
            DoneOp::RemovedDir { path } => fs::create_dir(path).map_err(Error::from),
            // Our own fresh copy: it goes to the trash, never straight to deletion.
            DoneOp::Copied { to, .. } => trash_delete(to),
            DoneOp::Trashed { path } => Err(Error::Msg(format!("restore from trash by hand: {}", path.display()))),
        };
        if let Err(e) = r {
            log::error!("rollback step failed: {e}");
            ok = false;
        }
    }
    ok
}

pub fn execute(plan: &Plan, journal: &mut dyn Journal, opts: &ExecOptions) -> std::result::Result<ExecReport, ExecError> {
    let policy: Vec<Issue> = plan.blockers.iter().filter(|i| i.code.is_policy()).cloned().collect();
    if !policy.is_empty() {
        return Err(ExecError::Stale(policy));
    }
    let fresh = validate(&plan.steps, &plan.roots, false);
    if !fresh.blockers.is_empty() {
        return Err(ExecError::Stale(fresh.blockers));
    }
    if fresh.steps.is_empty() {
        return Ok(ExecReport { batch_id: None, ops: vec![], skipped: vec![] });
    }

    let batch = journal.begin_batch(&plan.label, plan.undo_of)?;
    let mut done: Vec<DoneOp> = Vec::new();
    let mut skipped: Vec<Issue> = Vec::new();

    for (i, step) in fresh.steps.iter().enumerate() {
        let result: Result<Vec<DoneOp>> = if opts.fail_at_step == Some(i) {
            Err(Error::Msg("injected failure".into()))
        } else {
            match step {
                Step::CreateDir { path } => do_create_dir(path),
                Step::CreateNewDir { path } => fs::create_dir(path)
                    .map(|()| vec![DoneOp::CreatedDir { path: path.clone() }]).map_err(Error::from),
                Step::Move { from, to } => do_move(from, to, opts).map(|op| vec![op]),
                Step::Copy { from, to } => copy_verified(from, to)
                    .map(|_| vec![DoneOp::Copied { from: from.clone(), to: to.clone() }]),
                Step::Trash { path } => trash_delete(path).map(|_| vec![DoneOp::Trashed { path: path.clone() }]),
                Step::RemoveEmptyDir { path } => match do_remove_empty_dir(path) {
                    Ok(Some(op)) => Ok(vec![op]),
                    Ok(None) => {
                        if path.exists() {
                            skipped.push(Issue::new(IssueCode::DirNotEmpty, path));
                        }
                        Ok(vec![])
                    }
                    Err(e) => Err(e),
                },
            }
        };

        let failure = match result {
            Ok(ops) => {
                let mut err = None;
                for op in ops {
                    done.push(op.clone());
                    if let Err(e) = journal.record_op(batch, done.len() - 1, &op) {
                        err = Some(e);
                        break;
                    }
                }
                err
            }
            Err(e) => Some(e),
        };

        if let Some(e) = failure {
            let rolled_back = rollback(&done);
            let status = if rolled_back { BatchStatus::RolledBack } else { BatchStatus::Failed };
            let _ = journal.finish_batch(batch, status);
            return Err(ExecError::Failed { step: i, message: e.to_string(), rolled_back, batch_id: batch });
        }
    }

    journal.finish_batch(batch, BatchStatus::Done)?;
    Ok(ExecReport { batch_id: Some(batch), ops: done, skipped })
}

/// Inverse plan for a finished batch. Shown to the user like any other plan.
pub fn plan_undo(batch_id: i64, label: impl Into<String>, ops: &[DoneOp], roots: Vec<PathBuf>) -> Plan {
    let mut b = PlanBuilder::new(label, roots).undo_of(batch_id);
    for op in ops.iter().rev() {
        match op {
            DoneOp::CreatedDir { path } => {
                b.remove_empty_dir(path.clone());
            }
            DoneOp::Renamed { from, to } | DoneOp::CopiedAndTrashed { from, to } => {
                if let Some(parent) = from.parent() {
                    b.allow_root(parent);
                }
                b.move_item(to.clone(), from.clone());
            }
            DoneOp::RemovedDir { path } => {
                if let Some(parent) = path.parent() {
                    b.allow_root(parent);
                }
                b.create_dir(path.clone());
            }
            DoneOp::Copied { to, .. } => {
                b.trash_item(to.clone());
            }
            DoneOp::Trashed { path } => {
                b.warn(Issue::new(IssueCode::InTrash, path));
            }
        }
    }
    b.build()
}

/// Journal kept in memory, for tests and dry runs.
#[derive(Debug, Default)]
pub struct MemoryJournal {
    pub batches: Vec<(String, Option<i64>, BatchStatus)>,
    pub ops: Vec<(i64, DoneOp)>,
}

impl MemoryJournal {
    pub fn ops_of(&self, batch: i64) -> Vec<DoneOp> {
        self.ops.iter().filter(|(b, _)| *b == batch).map(|(_, o)| o.clone()).collect()
    }
}

impl Journal for MemoryJournal {
    fn begin_batch(&mut self, label: &str, undo_of: Option<i64>) -> Result<i64> {
        self.batches.push((label.to_string(), undo_of, BatchStatus::Running));
        Ok(self.batches.len() as i64)
    }
    fn record_op(&mut self, batch: i64, _seq: usize, op: &DoneOp) -> Result<()> {
        self.ops.push((batch, op.clone()));
        Ok(())
    }
    fn finish_batch(&mut self, batch: i64, status: BatchStatus) -> Result<()> {
        self.batches[(batch - 1) as usize].2 = status;
        Ok(())
    }
}
