//! What kind of file is this, which DAW made it, and is it safe to move.

use crate::error::Result;
use crate::pathsafe::{fold_key, is_within, nfc};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Daw {
    Fl,
    Ableton,
}

impl Daw {
    pub fn as_str(&self) -> &'static str {
        match self {
            Daw::Fl => "fl",
            Daw::Ableton => "ableton",
        }
    }
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "fl" => Some(Daw::Fl),
            "ableton" => Some(Daw::Ableton),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetKind {
    /// `.flp` — references samples by absolute path.
    FlProject,
    /// FL "Zipped loop package": `.zip` with a `.flp` inside, self-contained.
    FlZip,
    /// A whole Ableton project folder (has "Ableton Project Info").
    AbletonProject,
    /// An `.als` outside a recognisable project folder.
    AbletonSet,
    Audio,
    Midi,
    Image,
    Document,
    /// A plain folder (e.g. stems dropped into the Inbox).
    Folder,
    Other,
}

impl AssetKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            AssetKind::FlProject => "fl_project",
            AssetKind::FlZip => "fl_zip",
            AssetKind::AbletonProject => "ableton_project",
            AssetKind::AbletonSet => "ableton_set",
            AssetKind::Audio => "audio",
            AssetKind::Midi => "midi",
            AssetKind::Image => "image",
            AssetKind::Document => "document",
            AssetKind::Folder => "folder",
            AssetKind::Other => "other",
        }
    }
    pub fn from_str(s: &str) -> Self {
        match s {
            "fl_project" => AssetKind::FlProject,
            "fl_zip" => AssetKind::FlZip,
            "ableton_project" => AssetKind::AbletonProject,
            "ableton_set" => AssetKind::AbletonSet,
            "audio" => AssetKind::Audio,
            "midi" => AssetKind::Midi,
            "image" => AssetKind::Image,
            "document" => AssetKind::Document,
            "folder" => AssetKind::Folder,
            _ => AssetKind::Other,
        }
    }
    pub fn daw(&self) -> Option<Daw> {
        match self {
            AssetKind::FlProject | AssetKind::FlZip => Some(Daw::Fl),
            AssetKind::AbletonProject | AssetKind::AbletonSet => Some(Daw::Ableton),
            _ => None,
        }
    }
    pub fn is_project(&self) -> bool {
        self.daw().is_some()
    }
}

pub const AUDIO_EXTS: [&str; 9] = ["wav", "mp3", "flac", "aif", "aiff", "ogg", "m4a", "aac", "alac"];

pub fn ext_lower(p: &Path) -> String {
    p.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

pub fn is_audio(p: &Path) -> bool {
    AUDIO_EXTS.contains(&ext_lower(p).as_str())
}

/// Files nobody wants in a library view or queue.
pub fn is_junk(p: &Path) -> bool {
    let Some(name) = p.file_name().and_then(|n| n.to_str()) else {
        return true;
    };
    let lower = name.to_ascii_lowercase();
    name.starts_with('.')
        || name.starts_with("~$")
        || matches!(lower.as_str(), "desktop.ini" | "thumbs.db" | "icon\r")
        || lower.ends_with(".asd") // Ableton analysis file, regenerated on demand
        || lower.ends_with(".tmp")
        || lower.ends_with(".part")
        || lower.ends_with(".crdownload")
}

pub fn zip_contains_flp(p: &Path) -> bool {
    let Ok(f) = File::open(p) else { return false };
    let Ok(mut z) = zip::ZipArchive::new(BufReader::new(f)) else { return false };
    (0..z.len()).any(|i| {
        z.by_index_raw(i)
            .map(|e| e.name().map(|n| n.to_ascii_lowercase().ends_with(".flp")).unwrap_or(false))
            .unwrap_or(false)
    })
}

pub fn classify_file(p: &Path) -> AssetKind {
    match ext_lower(p).as_str() {
        "flp" => AssetKind::FlProject,
        "zip" if zip_contains_flp(p) => AssetKind::FlZip,
        "als" => AssetKind::AbletonSet,
        "mid" | "midi" => AssetKind::Midi,
        "png" | "jpg" | "jpeg" | "webp" | "gif" | "psd" | "heic" => AssetKind::Image,
        "txt" | "md" | "rtf" | "pdf" | "doc" | "docx" | "pages" => AssetKind::Document,
        _ if is_audio(p) => AssetKind::Audio,
        _ => AssetKind::Other,
    }
}

/// An Ableton project folder: has "Ableton Project Info", or holds an `.als`
/// next to a "Samples" folder (projects copied without the info folder).
pub fn is_ableton_project_dir(dir: &Path) -> bool {
    if !dir.is_dir() {
        return false;
    }
    if dir.join("Ableton Project Info").is_dir() {
        return true;
    }
    let has_samples = dir.join("Samples").is_dir();
    has_samples
        && std::fs::read_dir(dir)
            .map(|rd| rd.flatten().any(|e| ext_lower(&e.path()) == "als"))
            .unwrap_or(false)
}

/// Nearest ancestor (or `p` itself) that is an Ableton project folder.
pub fn ableton_project_root(p: &Path) -> Option<PathBuf> {
    let mut cur = if p.is_dir() { Some(p) } else { p.parent() };
    while let Some(d) = cur {
        if is_ableton_project_dir(d) {
            return Some(d.to_path_buf());
        }
        cur = d.parent();
    }
    None
}

/// Ableton `Backup` folders and FL autosave/overwrite backups.
pub fn is_backup_path(p: &Path) -> bool {
    if p.components().any(|c| c.as_os_str().to_string_lossy().eq_ignore_ascii_case("backup")) {
        return true;
    }
    let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    name.contains("(autosave)")
        || name.contains("_autosave")
        || name.contains("(overwritten at")
        || name.contains("(backup")
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ExternalSample {
    pub path: String,
    pub exists: bool,
    pub set_file: String,
}

/// Samples referenced by the project's `.als` files that live outside the
/// project folder. Factory content (Live Packs, Core Library) is ignored:
/// Ableton finds it by itself.
pub fn ableton_external_samples(project_dir: &Path) -> Result<Vec<ExternalSample>> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(project_dir) else { return Ok(out) };
    for e in rd.flatten() {
        let p = e.path();
        if ext_lower(&p) != "als" || is_backup_path(&p) {
            continue;
        }
        let set_name = p.file_name().unwrap().to_string_lossy().to_string();
        for r in read_als_sample_refs(&p)? {
            if r.pack_name.as_deref().map(|s| !s.is_empty()).unwrap_or(false) {
                continue;
            }
            // Project-relative reference that resolves inside the project.
            if r.relative_type == Some(3) {
                if let Some(rel) = &r.relative_path {
                    if !rel.is_empty() && project_dir.join(rel).exists() {
                        continue;
                    }
                }
            }
            let Some(abs) = r.path.filter(|s| !s.is_empty()) else { continue };
            let norm = abs.replace('\\', "/");
            let lower = norm.to_lowercase();
            if lower.contains("/core library/") || lower.contains("/factory packs/") {
                continue;
            }
            let abs_path = PathBuf::from(&norm);
            if is_within(&abs_path, project_dir) || fold_key(&abs_path).starts_with(&fold_key(project_dir)) {
                continue;
            }
            let item = ExternalSample { path: nfc(&norm), exists: abs_path.exists(), set_file: set_name.clone() };
            if !out.contains(&item) {
                out.push(item);
            }
        }
    }
    Ok(out)
}

#[derive(Debug, Default, Clone)]
struct SampleRef {
    path: Option<String>,
    relative_path: Option<String>,
    relative_type: Option<i32>,
    pack_name: Option<String>,
}

fn read_als_sample_refs(als: &Path) -> Result<Vec<SampleRef>> {
    use quick_xml::events::Event;
    let mut raw = Vec::new();
    File::open(als)?.read_to_end(&mut raw)?;
    // .als is gzip-compressed XML; fall back to plain XML just in case.
    let xml = if raw.starts_with(&[0x1f, 0x8b]) {
        let mut d = flate2::read::GzDecoder::new(&raw[..]);
        let mut s = Vec::new();
        if d.read_to_end(&mut s).is_err() {
            return Ok(Vec::new());
        }
        s
    } else {
        raw
    };
    let mut reader = quick_xml::Reader::from_reader(&xml[..]);
    let mut buf = Vec::new();
    let mut in_sample_ref = 0usize;
    let mut in_file_ref = false;
    let mut cur = SampleRef::default();
    let mut refs = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => match e.name().as_ref() {
                b"SampleRef" => in_sample_ref += 1,
                b"FileRef" if in_sample_ref > 0 => {
                    in_file_ref = true;
                    cur = SampleRef::default();
                }
                _ => {}
            },
            Ok(Event::Empty(e)) if in_file_ref => {
                let value = e
                    .try_get_attribute("Value")
                    .ok()
                    .flatten()
                    .and_then(|a| a.unescape_value().ok().map(|v| v.to_string()));
                match e.name().as_ref() {
                    b"Path" => cur.path = value,
                    b"RelativePath" => cur.relative_path = value,
                    b"RelativePathType" => cur.relative_type = value.and_then(|v| v.parse().ok()),
                    b"LivePackName" => cur.pack_name = value,
                    _ => {}
                }
            }
            Ok(Event::End(e)) => match e.name().as_ref() {
                b"SampleRef" => in_sample_ref = in_sample_ref.saturating_sub(1),
                b"FileRef" if in_file_ref => {
                    in_file_ref = false;
                    refs.push(std::mem::take(&mut cur));
                }
                _ => {}
            },
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(refs)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use flate2::write::GzEncoder;
    use std::io::Write;

    pub fn write_als(path: &Path, sample_paths: &[(&str, &str)]) {
        let mut xml = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?><Ableton><LiveSet>");
        for (abs, rel) in sample_paths {
            xml.push_str(&format!(
                "<SampleRef><FileRef><RelativePathType Value=\"3\" /><RelativePath Value=\"{rel}\" /><Path Value=\"{abs}\" /><LivePackName Value=\"\" /></FileRef></SampleRef>"
            ));
        }
        xml.push_str("</LiveSet></Ableton>");
        let f = File::create(path).unwrap();
        let mut gz = GzEncoder::new(f, flate2::Compression::fast());
        gz.write_all(xml.as_bytes()).unwrap();
        gz.finish().unwrap();
    }

    pub fn make_ableton_project(parent: &Path, name: &str, external: &[&str]) -> PathBuf {
        let proj = parent.join(format!("{name} Project"));
        std::fs::create_dir_all(proj.join("Ableton Project Info")).unwrap();
        std::fs::create_dir_all(proj.join("Samples/Imported")).unwrap();
        std::fs::write(proj.join("Samples/Imported/kick.wav"), b"RIFF").unwrap();
        let mut refs: Vec<(String, String)> = vec![(
            proj.join("Samples/Imported/kick.wav").to_string_lossy().replace('\\', "/"),
            "Samples/Imported/kick.wav".into(),
        )];
        for e in external {
            refs.push((e.to_string(), String::new()));
        }
        let pairs: Vec<(&str, &str)> = refs.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        write_als(&proj.join(format!("{name}.als")), &pairs);
        proj
    }

    #[test]
    fn detects_project_and_external_samples() {
        let dir = tempfile::tempdir().unwrap();
        let outside = dir.path().join("Splice/snare.wav");
        std::fs::create_dir_all(outside.parent().unwrap()).unwrap();
        std::fs::write(&outside, b"x").unwrap();
        let outside_s = outside.to_string_lossy().replace('\\', "/");
        let proj = make_ableton_project(
            dir.path(),
            "Night",
            &[&outside_s, "/Users/x/Music/Ableton/Factory Packs/Drums/a.wav", "/gone/hat.wav"],
        );
        assert!(is_ableton_project_dir(&proj));
        assert_eq!(ableton_project_root(&proj.join("Samples/Imported/kick.wav")), Some(proj.clone()));
        let ext = ableton_external_samples(&proj).unwrap();
        assert_eq!(ext.len(), 2, "{ext:?}");
        assert!(ext.iter().any(|e| e.path.ends_with("Splice/snare.wav") && e.exists));
        assert!(ext.iter().any(|e| e.path == "/gone/hat.wav" && !e.exists));
    }

    #[test]
    fn classifies_files() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("pack.zip");
        {
            let f = File::create(&zip_path).unwrap();
            let mut z = zip::ZipWriter::new(f);
            z.start_file("Night.flp", zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored)).unwrap();
            z.write_all(b"FLhd").unwrap();
            z.finish().unwrap();
        }
        let plain_zip = dir.path().join("kit.zip");
        {
            let f = File::create(&plain_zip).unwrap();
            let mut z = zip::ZipWriter::new(f);
            z.start_file("kick.wav", zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored)).unwrap();
            z.write_all(b"RIFF").unwrap();
            z.finish().unwrap();
        }
        assert_eq!(classify_file(&zip_path), AssetKind::FlZip);
        assert_eq!(classify_file(&plain_zip), AssetKind::Other);
        assert_eq!(classify_file(Path::new("a/Night.FLP")), AssetKind::FlProject);
        assert_eq!(classify_file(Path::new("a/Night.wav")), AssetKind::Audio);
        assert!(is_backup_path(Path::new("/x/Night Project/Backup/Night [2026-01-01].als")));
        assert!(is_backup_path(Path::new("/x/Night (autosave).flp")));
        assert!(!is_backup_path(Path::new("/x/Night.flp")));
        assert!(is_junk(Path::new("/x/kick.wav.asd")));
        assert!(is_junk(Path::new("/x/.DS_Store")));
    }
}
