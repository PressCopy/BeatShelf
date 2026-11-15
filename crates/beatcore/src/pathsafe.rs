//! File names that work on both Windows and macOS.
//!
//! * Windows forbids `<>:"/\|?*`, control characters, trailing dots/spaces
//!   and reserved device names (CON, NUL, COM1…), even with an extension.
//! * NTFS and APFS (default) are case-insensitive, so `Beat.wav` and
//!   `beat.wav` collide.
//! * macOS may hand us decomposed Unicode (NFD): Cyrillic "й" arrives as
//!   "и" + combining breve. We normalise to NFC before comparing or writing.

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use unicode_normalization::UnicodeNormalization;

/// Max bytes per path component we generate. APFS allows 255 UTF-8 bytes,
/// NTFS 255 UTF-16 units; we leave room for " (12)" suffixes.
pub const MAX_COMPONENT_BYTES: usize = 200;

/// Above this many characters FL Studio, Ableton and Explorer start to fail
/// on Windows even though the file system itself copes.
pub const LONG_PATH_WARN_CHARS: usize = 240;

const RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

pub fn nfc(s: &str) -> String {
    s.nfc().collect()
}

pub fn is_reserved_name(name: &str) -> bool {
    let base = name.split('.').next().unwrap_or("").trim_end();
    RESERVED.iter().any(|r| r.eq_ignore_ascii_case(base))
}

/// Make one path component safe on Windows and macOS.
pub fn sanitize_component(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for c in input.nfc() {
        match c {
            '/' | '\\' | '|' | ':' => out.push('-'),
            '"' => out.push('\''),
            '<' | '>' | '?' | '*' => {}
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    let trimmed = out.trim_start().trim_end_matches(|c: char| c == '.' || c == ' ');
    let mut s = trimmed.to_string();
    if s.is_empty() || s == "." || s == ".." {
        s = "_".to_string();
    }
    if is_reserved_name(&s) {
        s.insert(0, '_');
    }
    truncate_component(&s, MAX_COMPONENT_BYTES)
}

/// Truncate at a char boundary, keeping the extension when there is one.
pub fn truncate_component(name: &str, max_bytes: usize) -> String {
    if name.len() <= max_bytes {
        return name.to_string();
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 && name.len() - i <= 10 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    let budget = max_bytes.saturating_sub(ext.len());
    let mut end = 0;
    for (i, c) in stem.char_indices() {
        if i + c.len_utf8() > budget {
            break;
        }
        end = i + c.len_utf8();
    }
    let cut = stem[..end].trim_end_matches(|c: char| c == '.' || c == ' ');
    format!("{cut}{ext}")
}

/// Key for collision checks: NFC + lowercase, `/` separators.
pub fn fold_key(p: &Path) -> String {
    let mut parts: Vec<String> = Vec::new();
    for comp in p.components() {
        match comp {
            Component::Prefix(pre) => parts.push(pre.as_os_str().to_string_lossy().to_lowercase()),
            Component::RootDir => parts.push(String::new()),
            Component::CurDir => {}
            Component::ParentDir => parts.push("..".into()),
            Component::Normal(s) => parts.push(nfc(&s.to_string_lossy()).to_lowercase()),
        }
    }
    parts.join("/")
}

pub fn same_path_ignoring_case(a: &Path, b: &Path) -> bool {
    fold_key(a) == fold_key(b)
}

/// `true` when `child` is `parent` or lives under it (case-insensitive).
pub fn is_within(child: &Path, parent: &Path) -> bool {
    let c = fold_key(child);
    let p = fold_key(parent);
    c == p || c.starts_with(&(p.trim_end_matches('/').to_string() + "/"))
}

/// Path relative to `root` with `/` separators, if `abs` is inside root.
pub fn rel_string(root: &Path, abs: &Path) -> Option<String> {
    if !is_within(abs, root) {
        return None;
    }
    let root_n = root.components().count();
    let rest: Vec<String> = abs
        .components()
        .skip(root_n)
        .map(|c| nfc(&c.as_os_str().to_string_lossy()))
        .collect();
    Some(rest.join("/"))
}

pub fn from_rel(root: &Path, rel: &str) -> PathBuf {
    let mut p = root.to_path_buf();
    for part in rel.split('/').filter(|s| !s.is_empty()) {
        p.push(part);
    }
    p
}

/// Pick a name that collides neither with `taken` (fold keys of names planned
/// in this batch) nor with what is already on disk in `dir`.
pub fn unique_name(dir: &Path, name: &str, taken: &HashSet<String>) -> String {
    let free = |n: &str| {
        let p = dir.join(n);
        !taken.contains(&fold_key(&p)) && !exists_ignoring_case(&p)
    };
    if free(name) {
        return name.to_string();
    }
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => (&name[..i], &name[i..]),
        _ => (name, ""),
    };
    for n in 2..10_000 {
        let candidate = format!("{stem} ({n}){ext}");
        if free(&candidate) {
            return candidate;
        }
    }
    format!("{stem} ({}){ext}", uuid::Uuid::new_v4())
}

/// Existence check that also catches case/normalisation variants on
/// case-sensitive file systems, so plans behave the same on every OS.
pub fn exists_ignoring_case(p: &Path) -> bool {
    if p.symlink_metadata().is_ok() {
        return true;
    }
    let (Some(parent), Some(name)) = (p.parent(), p.file_name()) else {
        return false;
    };
    let want = nfc(&name.to_string_lossy()).to_lowercase();
    match std::fs::read_dir(parent) {
        Ok(rd) => rd
            .flatten()
            .any(|e| nfc(&e.file_name().to_string_lossy()).to_lowercase() == want),
        Err(_) => false,
    }
}

pub fn path_len_chars(p: &Path) -> usize {
    p.to_string_lossy().chars().count()
}

/// Cloud "files on demand" that are not actually on disk. Moving them would
/// trigger a download or fail half-way, so the planner refuses.
pub fn is_cloud_placeholder(p: &Path) -> bool {
    if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
        // iCloud Drive evicted file: ".Name.wav.icloud"
        if name.starts_with('.') && name.ends_with(".icloud") {
            return true;
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_OFFLINE: u32 = 0x1000;
        const FILE_ATTRIBUTE_RECALL_ON_OPEN: u32 = 0x40000;
        const FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS: u32 = 0x400000;
        if let Ok(m) = p.symlink_metadata() {
            let a = m.file_attributes();
            return a & (FILE_ATTRIBUTE_OFFLINE | FILE_ATTRIBUTE_RECALL_ON_OPEN | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS) != 0;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forbidden_chars_and_reserved() {
        assert_eq!(sanitize_component("Prod: Mike / 2024?"), "Prod- Mike - 2024");
        assert_eq!(sanitize_component("CON"), "_CON");
        assert_eq!(sanitize_component("nul.wav"), "_nul.wav");
        assert_eq!(sanitize_component("com1.txt"), "_com1.txt");
        assert_eq!(sanitize_component("Console"), "Console");
        assert_eq!(sanitize_component("beat... "), "beat");
        assert_eq!(sanitize_component("  "), "_");
        assert_eq!(sanitize_component("a\u{0007}b"), "ab");
    }

    #[test]
    fn nfc_normalisation_for_cyrillic() {
        let decomposed = "Мои\u{0306} бит"; // и + combining breve
        assert_eq!(sanitize_component(decomposed), "Мой бит");
        assert_eq!(
            fold_key(Path::new("/x/Мои\u{0306}")),
            fold_key(Path::new("/x/мой"))
        );
    }

    #[test]
    fn truncation_keeps_extension_and_char_boundaries() {
        let long = "Ж".repeat(300) + ".wav";
        let t = sanitize_component(&long);
        assert!(t.len() <= MAX_COMPONENT_BYTES);
        assert!(t.ends_with(".wav"));
        assert!(t.is_char_boundary(t.len()));
    }

    #[test]
    fn within_is_case_insensitive() {
        assert!(is_within(Path::new("/Lib/Beats/a.wav"), Path::new("/lib")));
        assert!(!is_within(Path::new("/Library2/a.wav"), Path::new("/Library")));
        assert_eq!(
            rel_string(Path::new("/lib"), Path::new("/lib/01_Beats/x")).as_deref(),
            Some("01_Beats/x")
        );
    }

    #[test]
    fn unique_names() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("Beat.wav"), b"x").unwrap();
        let mut taken = HashSet::new();
        assert_eq!(unique_name(dir.path(), "beat.wav", &taken), "beat (2).wav");
        taken.insert(fold_key(&dir.path().join("beat (2).wav")));
        assert_eq!(unique_name(dir.path(), "beat.wav", &taken), "beat (3).wav");
    }
}
