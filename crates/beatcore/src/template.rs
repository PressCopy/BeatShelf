//! Library structure templates and `{token}` naming.
//!
//! Tokens: {name} {bpm} {key} {camelot} {date} {year} {artist}
//! {authors} {coauthors} {producers}.
//! Russian aliases are accepted: {Название} {BPM} {Тональность} {Дата} {Год}
//! {Артист} {Авторы} {Соавторы} {Продюсеры}.
//! Square brackets mark an optional part: `{name}[ (prod. {producers})]`
//! drops the whole bracket when nobody is credited.
//! When a token has no value, the whole `_`/`-`/space-separated segment that
//! contains it is dropped, so `{name}_{bpm}bpm_{key}` without BPM becomes
//! `Night_Am`, not `Night_bpm_Am`.

use crate::daw::AssetKind;
use crate::keys::MusicalKey;
use crate::pathsafe::sanitize_component;
use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BeatSubdirs {
    pub project_fl: String,
    pub project_ableton: String,
    pub bounces: String,
    pub stems: String,
    pub artwork: String,
    pub notes: String,
    #[serde(default = "default_contracts_dir")]
    pub contracts: String,
}

fn default_contracts_dir() -> String {
    "Contracts".into()
}

impl BeatSubdirs {
    pub fn all(&self) -> [&str; 7] {
        [
            &self.project_fl,
            &self.project_ableton,
            &self.bounces,
            &self.stems,
            &self.artwork,
            &self.notes,
            &self.contracts,
        ]
    }
    pub fn get(&self, slot: SubdirSlot) -> &str {
        match slot {
            SubdirSlot::ProjectFl => &self.project_fl,
            SubdirSlot::ProjectAbleton => &self.project_ableton,
            SubdirSlot::Bounces => &self.bounces,
            SubdirSlot::Stems => &self.stems,
            SubdirSlot::Artwork => &self.artwork,
            SubdirSlot::Notes => &self.notes,
            SubdirSlot::Contracts => &self.contracts,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SubdirSlot {
    ProjectFl,
    ProjectAbleton,
    Bounces,
    Stems,
    Artwork,
    Notes,
    Contracts,
}

/// What a library item is. All three share fields, player and sorting;
/// they differ in where their folders live.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Beat,
    Loop,
    Starter,
    /// Producer voice tags: WAV, MP3 and DAW projects, kept flat in the item folder.
    Voicetag,
}

impl ItemKind {
    pub const ALL: [ItemKind; 4] = [ItemKind::Beat, ItemKind::Loop, ItemKind::Starter, ItemKind::Voicetag];

    /// Voice tags hold WAV and MP3 audio plus DAW projects (.flp, zipped
    /// loop packages, .als and Ableton project folders). No contracts,
    /// artwork or other files. Every other kind takes anything.
    pub fn accepts(&self, asset: AssetKind, path: &std::path::Path) -> bool {
        match self {
            ItemKind::Voicetag => match asset {
                AssetKind::Audio => path
                    .extension()
                    .map(|e| {
                        let e = e.to_string_lossy().to_ascii_lowercase();
                        e == "wav" || e == "mp3"
                    })
                    .unwrap_or(false),
                AssetKind::FlProject | AssetKind::FlZip | AssetKind::AbletonProject | AssetKind::AbletonSet => true,
                _ => false,
            },
            _ => true,
        }
    }

    /// Flat kinds keep files directly in the item folder, without subfolders.
    pub fn is_flat(&self) -> bool {
        matches!(self, ItemKind::Voicetag)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            ItemKind::Beat => "beat",
            ItemKind::Loop => "loop",
            ItemKind::Starter => "starter",
            ItemKind::Voicetag => "voicetag",
        }
    }
    pub fn parse(s: &str) -> Option<ItemKind> {
        match s {
            "beat" => Some(ItemKind::Beat),
            "loop" => Some(ItemKind::Loop),
            "starter" => Some(ItemKind::Starter),
            "voicetag" => Some(ItemKind::Voicetag),
            _ => None,
        }
    }
}

fn default_loop_dir() -> String {
    "06_Loops_Starters/Loops/{name}_{bpm}bpm_{key}".into()
}
fn default_voicetag_dir() -> String {
    "07_Voicetags/{name}".into()
}
fn default_starter_dir() -> String {
    "06_Loops_Starters/Starters/{name}_{bpm}bpm_{key}".into()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LibraryTemplate {
    pub id: String,
    pub name: String,
    pub inbox: String,
    pub archive: String,
    /// Extra folders, relative, `/`-separated.
    pub static_dirs: Vec<String>,
    /// Beat folder pattern, e.g. `01_Beats/{year}/{name}_{bpm}bpm_{key}`.
    pub beat_dir: String,
    pub beat_subdirs: BeatSubdirs,
    /// File name pattern without extension.
    pub file_name: String,
    /// Folder pattern for loops.
    #[serde(default = "default_loop_dir")]
    pub loop_dir: String,
    /// Folder pattern for starters (unfinished beats to send or finish later).
    #[serde(default = "default_starter_dir")]
    pub starter_dir: String,
    /// Folder pattern for voice tags.
    #[serde(default = "default_voicetag_dir")]
    pub voicetag_dir: String,
}

#[derive(Debug, Clone, Default)]
pub struct TokenValues {
    pub name: String,
    pub bpm: Option<f32>,
    pub key: Option<String>,
    pub date: Option<NaiveDate>,
    pub artist: Option<String>,
    pub authors: Vec<String>,
    pub coauthors: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeLine {
    pub depth: usize,
    pub name: String,
    /// "root" | "dir" | "beat" | "sub" | "example"
    pub role: &'static str,
}

fn standard_subdirs() -> BeatSubdirs {
    BeatSubdirs {
        project_fl: "Project_FL".into(),
        project_ableton: "Project_Ableton".into(),
        bounces: "Bounces".into(),
        stems: "Stems_Trackouts".into(),
        artwork: "Artwork".into(),
        notes: "Notes".into(),
        contracts: "Contracts".into(),
    }
}

pub fn default_template() -> LibraryTemplate {
    LibraryTemplate {
        id: "standard".into(),
        name: "Standard".into(),
        inbox: "00_Inbox".into(),
        archive: "99_Archive".into(),
        static_dirs: [
            "02_Samples_Kits/Drums",
            "02_Samples_Kits/Loops",
            "02_Samples_Kits/One-shots",
            "02_Samples_Kits/MIDI",
            "02_Samples_Kits/Presets",
            "02_Samples_Kits/FL Presets",
            "02_Samples_Kits/Ableton Racks",
            "03_Leased_Sold/Leases",
            "03_Leased_Sold/Exclusive",
            "04_Collabs",
            "05_Contracts",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect(),
        beat_dir: "01_Beats/{year}/{name}_{bpm}bpm_{key}".into(),
        beat_subdirs: standard_subdirs(),
        file_name: "{name}_{bpm}bpm_{key}".into(),
        loop_dir: default_loop_dir(),
        starter_dir: default_starter_dir(),
        voicetag_dir: default_voicetag_dir(),
    }
}

pub fn minimal_template() -> LibraryTemplate {
    LibraryTemplate {
        id: "minimal".into(),
        name: "Minimal".into(),
        inbox: "Inbox".into(),
        archive: "Archive".into(),
        static_dirs: vec!["Samples".into()],
        beat_dir: "Beats/{name} {bpm} {key}".into(),
        beat_subdirs: BeatSubdirs {
            project_fl: "FL".into(),
            project_ableton: "Ableton".into(),
            bounces: "Bounces".into(),
            stems: "Stems".into(),
            artwork: "Artwork".into(),
            notes: "Notes".into(),
            contracts: "Contracts".into(),
        },
        file_name: "{name} {bpm} {key}".into(),
        loop_dir: "Loops/{name} {bpm} {key}".into(),
        starter_dir: "Starters/{name} {bpm} {key}".into(),
        voicetag_dir: "Voicetags/{name}".into(),
    }
}

pub fn builtin_templates() -> Vec<LibraryTemplate> {
    vec![default_template(), minimal_template()]
}

fn canonical_token(raw: &str) -> Option<&'static str> {
    let t = raw.trim().to_lowercase();
    Some(match t.as_str() {
        "name" | "название" | "имя" => "name",
        "bpm" | "темп" => "bpm",
        "key" | "тональность" => "key",
        "camelot" => "camelot",
        "date" | "дата" => "date",
        "year" | "год" => "year",
        "artist" | "артист" => "artist",
        "authors" | "author" | "авторы" | "автор" => "authors",
        "coauthors" | "coauthor" | "соавторы" | "соавтор" => "coauthors",
        "producers" | "prod" | "продюсеры" => "producers",
        _ => return None,
    })
}

fn format_bpm(v: f32) -> String {
    if (v - v.round()).abs() < 0.01 {
        format!("{}", v.round() as i64)
    } else {
        format!("{:.1}", v)
    }
}

fn token_value(token: &str, v: &TokenValues) -> Option<String> {
    let date = v.date.unwrap_or_else(|| chrono::Local::now().date_naive());
    match token {
        "name" => Some(v.name.trim().to_string()).filter(|s| !s.is_empty()),
        "bpm" => v.bpm.map(format_bpm),
        "key" => v.key.clone(),
        "camelot" => v.key.as_deref().and_then(MusicalKey::parse).map(|k| k.camelot_code()),
        "date" => Some(date.format("%Y-%m-%d").to_string()),
        "year" => Some(date.year().to_string()),
        "artist" => v.artist.clone().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()),
        "authors" => join_names(v.authors.iter()),
        "coauthors" => join_names(v.coauthors.iter()),
        "producers" => join_names(v.authors.iter().chain(v.coauthors.iter())),
        _ => None,
    }
}

/// "maks x lowkey": the way credits are written in beat names.
fn join_names<'a>(names: impl Iterator<Item = &'a String>) -> Option<String> {
    let mut seen: Vec<String> = Vec::new();
    let mut out: Vec<String> = Vec::new();
    for n in names {
        let n = n.trim();
        if n.is_empty() || seen.contains(&n.to_lowercase()) {
            continue;
        }
        seen.push(n.to_lowercase());
        out.push(n.to_string());
    }
    if out.is_empty() {
        None
    } else {
        Some(out.join(" x "))
    }
}

/// Resolves `[...]` groups: the group vanishes if any token inside has no
/// value, otherwise its tokens are filled in and the brackets dropped.
fn expand_optional(pattern: &str, v: &TokenValues) -> String {
    let mut out = String::new();
    let mut rest = pattern;
    while let Some(open) = rest.find('[') {
        out.push_str(&rest[..open]);
        let Some(close_rel) = rest[open..].find(']') else {
            out.push_str(&rest[open..]);
            return out;
        };
        let inner = &rest[open + 1..open + close_rel];
        let mut text = String::new();
        let mut missing = false;
        let mut r = inner;
        while let Some(o) = r.find('{') {
            text.push_str(&r[..o]);
            let Some(c) = r[o..].find('}') else {
                text.push_str(&r[o..]);
                r = "";
                break;
            };
            let raw = &r[o + 1..o + c];
            match canonical_token(raw) {
                Some(tok) => match token_value(tok, v) {
                    Some(val) => text.push_str(&val.replace(['{', '}', '[', ']'], "")),
                    None => missing = true,
                },
                None => text.push_str(raw),
            }
            r = &r[o + c + 1..];
        }
        text.push_str(r);
        if !missing {
            out.push_str(&text);
        }
        rest = &rest[open + close_rel + 1..];
    }
    out.push_str(rest);
    out
}

fn is_seg_sep(c: char) -> bool {
    c == '_' || c == '-' || c == ' '
}

/// Render one path component. Unknown tokens are kept literally (minus braces).
pub fn render_component(pattern: &str, v: &TokenValues) -> String {
    let expanded = expand_optional(pattern, v);
    let pattern = expanded.as_str();
    // Split into (separator, segment) pairs.
    let mut segments: Vec<(String, String)> = Vec::new();
    let mut sep = String::new();
    let mut seg = String::new();
    let mut depth = 0;
    for c in pattern.chars() {
        if c == '{' {
            depth += 1;
        } else if c == '}' && depth > 0 {
            depth -= 1;
        }
        if depth == 0 && is_seg_sep(c) {
            if seg.is_empty() {
                sep.push(c);
            } else {
                segments.push((std::mem::take(&mut sep), std::mem::take(&mut seg)));
                sep.push(c);
            }
        } else {
            seg.push(c);
        }
    }
    if !seg.is_empty() {
        segments.push((sep, seg));
    }

    // Render every segment; remember which ones lost a token.
    let mut rendered: Vec<(String, bool, bool)> = Vec::new(); // (text, dropped, has_token)
    for (_, seg) in segments.iter() {
        let mut text = String::new();
        let mut dropped = false;
        let mut has_token = false;
        let mut rest = seg.as_str();
        while let Some(open) = rest.find('{') {
            text.push_str(&rest[..open]);
            let Some(close_rel) = rest[open..].find('}') else {
                text.push_str(&rest[open..]);
                rest = "";
                break;
            };
            let raw = &rest[open + 1..open + close_rel];
            match canonical_token(raw) {
                Some(tok) => {
                    has_token = true;
                    match token_value(tok, v) {
                        Some(val) => text.push_str(&val),
                        None => dropped = true,
                    }
                }
                None => text.push_str(raw),
            }
            rest = &rest[open + close_rel + 1..];
        }
        text.push_str(rest);
        rendered.push((text, dropped, has_token));
    }
    // A literal unit glued by a single space to a missing token goes too:
    // "{bpm} BPM" or "BPM {bpm}" without a BPM leaves nothing behind.
    let n = rendered.len();
    let mut drop: Vec<bool> = rendered.iter().map(|r| r.1).collect();
    for i in 0..n {
        if !rendered[i].1 {
            continue;
        }
        if i + 1 < n && segments[i + 1].0 == " " && !rendered[i + 1].2 {
            drop[i + 1] = true;
        }
        if i > 0 && segments[i].0 == " " && !rendered[i - 1].2 {
            drop[i - 1] = true;
        }
    }
    let mut out = String::new();
    for (i, (text, _, _)) in rendered.iter().enumerate() {
        if drop[i] {
            continue;
        }
        if !out.is_empty() {
            out.push_str(&segments[i].0);
        }
        out.push_str(text);
    }
    let out = out.trim_matches(|c: char| is_seg_sep(c)).to_string();
    if out.is_empty() {
        "Untitled".to_string()
    } else {
        sanitize_component(&out)
    }
}

/// Render a `/`-separated pattern into path components.
pub fn render_path(pattern: &str, v: &TokenValues) -> Vec<String> {
    pattern
        .split('/')
        .filter(|s| !s.is_empty())
        .map(|c| render_component(c, v))
        .collect()
}

fn static_prefix_of(pattern: &str) -> Vec<String> {
    pattern
        .split('/')
        .filter(|s| !s.is_empty())
        .take_while(|c| !c.contains('{') && !c.contains('['))
        .map(|s| s.to_string())
        .collect()
}

impl LibraryTemplate {
    /// Number of path components in a beat folder (3 for the standard template).
    pub fn beat_depth(&self) -> usize {
        self.beat_dir.split('/').filter(|s| !s.is_empty()).count()
    }

    /// Leading components without tokens (`01_Beats`).
    pub fn beat_static_prefix(&self) -> Vec<String> {
        static_prefix_of(&self.beat_dir)
    }

    /// Every folder the library needs at creation time, parents first.
    pub fn dirs_to_create(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        let mut push = |p: &str| {
            let mut acc = String::new();
            for part in p.split('/').filter(|s| !s.is_empty()) {
                if !acc.is_empty() {
                    acc.push('/');
                }
                acc.push_str(part);
                if !out.contains(&acc) {
                    out.push(acc.clone());
                }
            }
        };
        push(&self.inbox);
        for kind in ItemKind::ALL {
            let prefix = self.static_prefix(kind).join("/");
            if !prefix.is_empty() {
                push(&prefix);
            }
        }
        for d in &self.static_dirs {
            push(d);
        }
        push(&self.archive);
        out
    }

    pub fn beat_dir_for(&self, v: &TokenValues) -> Vec<String> {
        render_path(&self.beat_dir, v)
    }

    pub fn dir_pattern(&self, kind: ItemKind) -> &str {
        match kind {
            ItemKind::Beat => &self.beat_dir,
            ItemKind::Loop => &self.loop_dir,
            ItemKind::Starter => &self.starter_dir,
            ItemKind::Voicetag => &self.voicetag_dir,
        }
    }

    /// Folder (as path components) for an item of this kind.
    pub fn dir_for(&self, kind: ItemKind, v: &TokenValues) -> Vec<String> {
        render_path(self.dir_pattern(kind), v)
    }

    /// Leading components without tokens for a kind (`06_Loops_Starters/Loops`).
    pub fn static_prefix(&self, kind: ItemKind) -> Vec<String> {
        static_prefix_of(self.dir_pattern(kind))
    }

    pub fn depth(&self, kind: ItemKind) -> usize {
        self.dir_pattern(kind).split('/').filter(|s| !s.is_empty()).count()
    }

    /// Which item folder (and of which kind) contains `rel`.
    pub fn item_folder_of(&self, rel: &str) -> Option<(String, ItemKind)> {
        let parts: Vec<&str> = rel.split('/').filter(|s| !s.is_empty()).collect();
        // Longest prefix first, so nested layouts resolve to the most specific kind.
        let mut kinds: Vec<(ItemKind, Vec<String>, usize)> =
            ItemKind::ALL.iter().map(|k| (*k, self.static_prefix(*k), self.depth(*k))).collect();
        kinds.sort_by_key(|(_, p, _)| std::cmp::Reverse(p.len()));
        for (kind, prefix, depth) in kinds {
            if parts.len() < depth || depth <= prefix.len() {
                continue;
            }
            if prefix.iter().zip(parts.iter()).all(|(a, b)| a.eq_ignore_ascii_case(b)) {
                return Some((parts[..depth].join("/"), kind));
            }
        }
        None
    }

    pub fn file_name_for(&self, v: &TokenValues, ext: Option<&str>) -> String {
        let stem = render_component(&self.file_name, v);
        match ext {
            Some(e) if !e.is_empty() => sanitize_component(&format!("{stem}.{e}")),
            _ => stem,
        }
    }

    /// Tree for the onboarding preview, with one example beat expanded.
    pub fn preview(&self, root_name: &str, example: &TokenValues) -> Vec<TreeLine> {
        let mut lines = vec![TreeLine { depth: 0, name: root_name.to_string(), role: "root" }];
        // Build a sorted set of all static dirs plus the example beat path.
        let mut paths: Vec<(Vec<String>, &'static str)> = self
            .dirs_to_create()
            .into_iter()
            .map(|p| (p.split('/').map(|s| s.to_string()).collect(), "dir"))
            .collect();
        let mut beat = self.beat_dir_for(example);
        beat.push(example.name.clone());
        for i in self.beat_static_prefix().len()..beat.len() {
            let role = if i + 1 == beat.len() { "beat" } else { "example" };
            paths.push((beat[..=i].to_vec(), role));
        }
        for sub in self.beat_subdirs.all() {
            let mut p = beat.clone();
            p.push(sub.to_string());
            paths.push((p, "sub"));
        }
        paths.sort_by(|a, b| a.0.cmp(&b.0));
        paths.dedup_by(|a, b| a.0 == b.0);
        for (p, role) in paths {
            lines.push(TreeLine { depth: p.len(), name: p.last().cloned().unwrap_or_default(), role });
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vals(name: &str, bpm: Option<f32>, key: Option<&str>) -> TokenValues {
        TokenValues {
            name: name.into(),
            bpm,
            key: key.map(str::to_owned),
            date: NaiveDate::from_ymd_opt(2026, 11, 13),
            ..Default::default()
        }
    }

    #[test]
    fn credits_and_optional_groups() {
        let mut v = vals("Night", Some(140.0), Some("Am"));
        let pat = "{name}_{bpm}bpm_{key}[ (prod. {producers})]";
        assert_eq!(render_component(pat, &v), "Night_140bpm_Am");
        v.authors = vec!["maks".into()];
        assert_eq!(render_component(pat, &v), "Night_140bpm_Am (prod. maks)");
        v.coauthors = vec!["lowkey".into(), "Maks".into()];
        assert_eq!(render_component(pat, &v), "Night_140bpm_Am (prod. maks x lowkey)");
        assert_eq!(render_component("{name} - {authors}[ feat {coauthors}]", &v), "Night - maks feat lowkey x Maks");
        assert_eq!(render_component("{Название} [{Соавторы}]", &v), "Night lowkey x Maks");
    }

    #[test]
    fn kinds_have_their_own_folders() {
        let t = default_template();
        let v = vals("Loop1", Some(120.0), Some("Cm"));
        assert_eq!(t.dir_for(ItemKind::Loop, &v), vec!["06_Loops_Starters", "Loops", "Loop1_120bpm_Cm"]);
        assert_eq!(t.dir_for(ItemKind::Starter, &v)[1], "Starters");
        assert_eq!(
            t.item_folder_of("06_Loops_Starters/Loops/Loop1_120bpm_Cm/Bounces/a.wav"),
            Some(("06_Loops_Starters/Loops/Loop1_120bpm_Cm".into(), ItemKind::Loop))
        );
        assert_eq!(
            t.item_folder_of("01_Beats/2026/Night/Project_FL/Night.flp"),
            Some(("01_Beats/2026/Night".into(), ItemKind::Beat))
        );
        assert_eq!(t.item_folder_of("06_Loops_Starters/Loops"), None);
        assert!(t.dirs_to_create().contains(&"06_Loops_Starters/Starters".to_string()));
        // Configs saved before loops existed still load, with defaults.
        let mut json = serde_json::to_value(&t).unwrap();
        json.as_object_mut().unwrap().remove("loopDir");
        json["beatSubdirs"].as_object_mut().unwrap().remove("contracts");
        let old: LibraryTemplate = serde_json::from_value(json).unwrap();
        assert_eq!(old.loop_dir, default_loop_dir());
        assert_eq!(old.beat_subdirs.contracts, "Contracts");
    }

    #[test]
    fn renders_full_and_partial() {
        let t = default_template();
        assert_eq!(
            t.beat_dir_for(&vals("Night", Some(140.0), Some("Am"))),
            vec!["01_Beats", "2026", "Night_140bpm_Am"]
        );
        assert_eq!(
            t.beat_dir_for(&vals("Night", None, Some("Am"))),
            vec!["01_Beats", "2026", "Night_Am"]
        );
        assert_eq!(t.beat_dir_for(&vals("Night", None, None))[2], "Night");
        assert_eq!(t.beat_dir_for(&vals("", None, None))[2], "Untitled");
        assert_eq!(
            t.file_name_for(&vals("Ночь: тест", Some(92.5), Some("F#m")), Some("wav")),
            "Ночь- тест_92.5bpm_F#m.wav"
        );
    }

    #[test]
    fn russian_tokens_and_spaces() {
        let v = vals("Snow", Some(150.0), Some("Ebm"));
        assert_eq!(render_component("{Название} - {BPM} BPM - {Тональность}", &v), "Snow - 150 BPM - Ebm");
        let v2 = vals("Snow", None, Some("Ebm"));
        assert_eq!(render_component("{Название} - {BPM} BPM - {Тональность}", &v2), "Snow - Ebm");
        assert_eq!(render_component("{name} BPM {bpm}", &v2), "Snow");
        assert_eq!(render_component("{name}_{camelot}", &v), "Snow_2A");
        assert_eq!(render_component("{name}_{artist}", &v), "Snow");
    }

    #[test]
    fn structure() {
        let t = default_template();
        assert_eq!(t.beat_depth(), 3);
        assert_eq!(t.beat_static_prefix(), vec!["01_Beats"]);
        let dirs = t.dirs_to_create();
        assert_eq!(dirs[0], "00_Inbox");
        assert!(dirs.contains(&"02_Samples_Kits".to_string()));
        assert!(dirs.iter().position(|d| d == "02_Samples_Kits").unwrap()
            < dirs.iter().position(|d| d == "02_Samples_Kits/Drums").unwrap());
        let preview = t.preview("Beats", &vals("Night", Some(140.0), Some("Am")));
        assert!(preview.iter().any(|l| l.name == "Night" && l.role == "beat"));
        assert!(preview.iter().any(|l| l.name == "Night_140bpm_Am" && l.role == "example"));
        assert!(preview.iter().any(|l| l.name == "Project_Ableton" && l.depth == 5));
    }

    #[test]
    fn voicetags_are_flat_and_take_audio_and_projects() {
        use std::path::Path;
        let t = default_template();
        let v = vals("Maks Tag", None, None);
        assert_eq!(t.dir_for(ItemKind::Voicetag, &v), vec!["07_Voicetags", "Maks Tag"]);
        assert_eq!(
            t.item_folder_of("07_Voicetags/Maks Tag/Maks Tag.wav"),
            Some(("07_Voicetags/Maks Tag".into(), ItemKind::Voicetag))
        );
        assert!(t.dirs_to_create().contains(&"07_Voicetags".to_string()));
        let k = ItemKind::Voicetag;
        assert!(k.is_flat() && !ItemKind::Beat.is_flat());
        assert!(k.accepts(AssetKind::Audio, Path::new("a/tag.WAV")));
        assert!(k.accepts(AssetKind::Audio, Path::new("a/tag.mp3")));
        assert!(!k.accepts(AssetKind::Audio, Path::new("a/tag.flac")));
        assert!(k.accepts(AssetKind::FlProject, Path::new("a/tag.flp")));
        assert!(k.accepts(AssetKind::FlZip, Path::new("a/tag.zip")));
        assert!(k.accepts(AssetKind::AbletonSet, Path::new("a/tag.als")));
        assert!(k.accepts(AssetKind::AbletonProject, Path::new("a/Tag Project")));
        assert!(!k.accepts(AssetKind::Document, Path::new("a/contract.pdf")));
        assert!(!k.accepts(AssetKind::Image, Path::new("a/cover.jpg")));
        assert!(!k.accepts(AssetKind::Folder, Path::new("a/stems")));
        assert!(ItemKind::Beat.accepts(AssetKind::Document, Path::new("a/contract.pdf")));
        // Templates saved before voice tags existed still load.
        let mut json = serde_json::to_value(&t).unwrap();
        json.as_object_mut().unwrap().remove("voicetagDir");
        let old: LibraryTemplate = serde_json::from_value(json).unwrap();
        assert_eq!(old.voicetag_dir, "07_Voicetags/{name}");
    }
}
