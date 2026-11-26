//! Extracts title, BPM and key from file and folder names such as
//! `Night_140bpm_Am`, `Dark Trap 140 BPM (C#m)`, `Ocean 8A 92bpm`.
//!
//! The parser is deliberately conservative: a wrong BPM is worse than an
//! empty field, because the user trusts what is pre-filled.

use crate::keys::{note_pc, MusicalKey};
use regex::Regex;
use serde::Serialize;
use std::sync::OnceLock;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ParsedName {
    pub title: String,
    pub bpm: Option<f32>,
    pub key: Option<MusicalKey>,
    /// `Some(true)` for "tagged", `Some(false)` for "untagged"/"notag".
    pub tagged: Option<bool>,
    /// Names from a credit such as "(prod. maks x lowkey)".
    pub producers: Vec<String>,
}

#[derive(Debug, Clone)]
struct Token {
    text: String,
    start: usize,
    end: usize,
}

fn bpm_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(r"(?i)(?:^|[^0-9.,])(\d{2,3}(?:[.,]\d{1,2})?)\s*[-_ ]?\s*bpm(?:[^a-z]|$)|(?:^|[^a-z])bpm\s*[-_: ]?\s*(\d{2,3}(?:[.,]\d{1,2})?)(?:[^0-9]|$)")
            .unwrap()
    })
}

fn is_sep(c: char) -> bool {
    c.is_whitespace() || matches!(c, '_' | '-' | '.' | '(' | ')' | '[' | ']' | '{' | '}' | ',' | '+')
}

fn tokenize(s: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in s.char_indices() {
        if is_sep(c) {
            if let Some(st) = start.take() {
                out.push(Token { text: s[st..i].to_string(), start: st, end: i });
            }
        } else if start.is_none() {
            start = Some(i);
        }
    }
    if let Some(st) = start {
        out.push(Token { text: s[st..].to_string(), start: st, end: s.len() });
    }
    out
}

/// Strong key token: uppercase note letter, optional accidental, and either a
/// suffix or an accidental (`Am`, `C#`, `Ebmaj`, `F#min`), lowercase letter
/// only with a long suffix (`f#min`), or a Camelot code (`8A`).
/// Returns (key, is_plain_letter).
fn key_from_token(t: &str) -> Option<(MusicalKey, bool)> {
    // Camelot code, uppercase letter only (avoid "1a" style noise)
    if t.is_ascii() && (2..=3).contains(&t.len()) {
        let (digits, letter) = t.split_at(t.len() - 1);
        if (letter == "A" || letter == "B") && digits.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(n) = digits.parse::<u8>() {
                if let Some(k) = MusicalKey::from_camelot(n, letter.chars().next().unwrap()) {
                    return Some((k, false));
                }
            }
        }
    }
    let mut chars = t.chars();
    let letter = chars.next()?;
    if !matches!(letter.to_ascii_uppercase(), 'A'..='G') {
        return None;
    }
    let rest: String = chars.collect();
    let (acc, suffix) = match rest.chars().next() {
        Some(c @ ('#' | '♯' | 'b' | '♭')) => (Some(c), rest[c.len_utf8()..].to_string()),
        _ => (None, rest.clone()),
    };
    let suffix_lower = suffix.to_lowercase();
    let (minor, long_suffix) = match suffix.as_str() {
        "" => (false, false),
        "m" => (true, false),
        "M" => (false, false),
        _ => match suffix_lower.as_str() {
            "min" | "minor" => (true, true),
            "maj" | "major" => (false, true),
            _ => return None,
        },
    };
    if letter.is_ascii_lowercase() && !long_suffix {
        return None;
    }
    let pc = note_pc(letter, acc)?;
    let plain = acc.is_none() && suffix.is_empty();
    Some((MusicalKey::new(pc, minor), plain))
}

fn mode_word(t: &str) -> Option<bool> {
    match t.to_lowercase().as_str() {
        "minor" | "min" | "moll" => Some(true),
        "major" | "maj" | "dur" => Some(false),
        _ => None,
    }
}

fn tag_word(t: &str) -> Option<bool> {
    match t.to_lowercase().as_str() {
        "tagged" | "tag" => Some(true),
        "untagged" | "notag" | "notagged" => Some(false),
        _ => None,
    }
}

fn clean_title(s: &str) -> String {
    static EMPTY_BRACKETS: OnceLock<Regex> = OnceLock::new();
    let re = EMPTY_BRACKETS.get_or_init(|| Regex::new(r"\(\s*\)|\[\s*\]|\{\s*\}").unwrap());
    let s = s.replace('_', " ");
    let s = re.replace_all(&s, " ");
    let mut out = String::new();
    let mut last_space = true;
    for c in s.chars() {
        if c.is_whitespace() {
            if !last_space {
                out.push(' ');
            }
            last_space = true;
        } else {
            out.push(c);
            last_space = false;
        }
    }
    out.trim_matches(|c: char| c.is_whitespace() || matches!(c, '-' | '.' | ',' | '+'))
        .to_string()
}

fn credits_regex() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    R.get_or_init(|| {
        Regex::new(
            r"(?i)[(\[]\s*prod(?:uced)?\.?(?:\s+by)?\s*[.:]?\s*([^)\]]+)[)\]]|(?:^|[\s_\-]+)prod(?:uced)?\.?(?:[\s_]+by)?[\s_.:]+(.+)$",
        )
        .unwrap()
    })
}

fn split_names(s: &str) -> Vec<String> {
    static R: OnceLock<Regex> = OnceLock::new();
    let re = R.get_or_init(|| Regex::new(r"(?i)\s+x\s+|_x_|\s*&\s*|\s*,\s*|\s+\+\s+|\s+and\s+|\s+и\s+").unwrap());
    let mut out: Vec<String> = Vec::new();
    for n in re.split(s) {
        let n = n.trim_matches(|c: char| c.is_whitespace() || c == '_' || c == '-' || c == '.').to_string();
        if !n.is_empty() && !out.iter().any(|o| o.to_lowercase() == n.to_lowercase()) {
            out.push(n);
        }
    }
    out
}

/// Cuts a producer credit out of a name: "Night (prod. maks x lowkey)" gives
/// ("Night ", ["maks", "lowkey"]). The trailing form ("Night prod. maks") is
/// only trusted when nothing that looks like a BPM follows it.
pub fn take_credits(input: &str) -> (String, Vec<String>) {
    let Some(c) = credits_regex().captures(input) else { return (input.to_string(), vec![]) };
    let whole = c.get(0).unwrap();
    let names = match (c.get(1), c.get(2)) {
        (Some(m), _) => m.as_str(),
        (None, Some(m)) if !bpm_regex().is_match(m.as_str()) => m.as_str(),
        _ => return (input.to_string(), vec![]),
    };
    let producers = split_names(names);
    if producers.is_empty() {
        return (input.to_string(), vec![]);
    }
    let rest = format!("{} {}", &input[..whole.start()], &input[whole.end()..]);
    (rest, producers)
}

pub fn parse_name(input: &str) -> ParsedName {
    let (rest, producers) = take_credits(input);
    let mut p = parse_core(&rest);
    p.producers = producers;
    p
}

/// A project folder may put producer handles before a spaced dash. Keep
/// other uses of @ (including a handle inside a title) as ordinary text.
pub fn parse_folder_name(input: &str) -> ParsedName {
    for delimiter in [" - ", " — ", " – "] {
        if let Some((prefix, title)) = input.split_once(delimiter) {
            let handles: Vec<_> = prefix.split_whitespace().collect();
            if !title.trim().is_empty() && !handles.is_empty() && handles.iter().all(|h| {
                h.strip_prefix('@').is_some_and(|v| !v.is_empty() && v.chars().all(|c| c.is_alphanumeric() || c == '_'))
            }) {
                let mut parsed = parse_name(title);
                let mut producers: Vec<_> = handles.into_iter().map(str::to_owned).collect();
                producers.append(&mut parsed.producers);
                parsed.producers = producers;
                return parsed;
            }
        }
    }
    parse_name(input)
}

fn parse_core(input: &str) -> ParsedName {
    let s = input.replace('♯', "#").replace('♭', "b");
    let mut remove: Vec<(usize, usize)> = Vec::new();

    // 1. Explicit BPM ("140bpm", "140 BPM", "bpm140", "92.5bpm").
    let mut bpm: Option<f32> = None;
    let mut bpm_span: Option<(usize, usize)> = None;
    if let Some(caps) = bpm_regex().captures(&s) {
        let m = caps.get(1).or_else(|| caps.get(2)).unwrap();
        if let Ok(v) = m.as_str().replace(',', ".").parse::<f32>() {
            if (40.0..=300.0).contains(&v) {
                bpm = Some(v);
                // Span of number + "bpm" word, without the separators around it.
                let whole = caps.get(0).unwrap();
                // ASCII lowercasing keeps byte offsets identical to `s`.
                let rel = s[whole.start()..whole.end()].to_ascii_lowercase();
                let bpm_pos = rel.find("bpm").map(|p| p + whole.start()).unwrap();
                let start = m.start().min(bpm_pos);
                let end = (m.end()).max(bpm_pos + 3);
                bpm_span = Some((start, end));
                remove.push((start, end));
            }
        }
    }

    let tokens: Vec<Token> = tokenize(&s)
        .into_iter()
        .filter(|t| match bpm_span {
            Some((a, b)) => t.end <= a || t.start >= b,
            None => true,
        })
        .collect();

    // Index of the token right after / before the BPM span, for plain letters.
    let adjacent_to_bpm = |t: &Token| -> bool {
        match bpm_span {
            None => false,
            Some((a, b)) => {
                let between_after = s.get(b..t.start).map(|x| x.chars().all(is_sep)).unwrap_or(false);
                let between_before = s.get(t.end..a).map(|x| x.chars().all(is_sep)).unwrap_or(false);
                (t.start >= b && between_after) || (t.end <= a && between_before)
            }
        }
    };

    // 2. Key: scan from the end, metadata usually trails the title.
    let mut key: Option<MusicalKey> = None;
    let mut i = tokens.len();
    while i > 0 && key.is_none() {
        i -= 1;
        let t = &tokens[i];
        // Two-token form: "A minor", "C# maj"
        if let Some(minor) = mode_word(&t.text) {
            if i > 0 {
                let prev = &tokens[i - 1];
                let mut pc_chars = prev.text.chars();
                let l = pc_chars.next();
                let acc_s: String = pc_chars.collect();
                if let Some(l) = l {
                    let acc = match acc_s.as_str() {
                        "" => Some(None),
                        "#" => Some(Some('#')),
                        "b" => Some(Some('b')),
                        _ => None,
                    };
                    if l.is_ascii_uppercase() {
                        if let Some(acc) = acc {
                            if let Some(pc) = note_pc(l, acc) {
                                key = Some(MusicalKey::new(pc, minor));
                                remove.push((prev.start, t.end));
                                break;
                            }
                        }
                    }
                }
            }
            continue;
        }
        if let Some((k, plain)) = key_from_token(&t.text) {
            if plain && !adjacent_to_bpm(t) {
                continue;
            }
            key = Some(k);
            remove.push((t.start, t.end));
        }
    }

    // 3. Implicit BPM: a single standalone 60..=200 number when a key was found.
    if bpm.is_none() && key.is_some() {
        let nums: Vec<&Token> = tokens
            .iter()
            .filter(|t| {
                t.text.len() >= 2
                    && t.text.len() <= 3
                    && !t.text.starts_with('0')
                    && t.text.chars().all(|c| c.is_ascii_digit())
                    && !remove.iter().any(|(a, b)| t.start >= *a && t.end <= *b)
            })
            .collect();
        let candidates: Vec<&&Token> = nums
            .iter()
            .filter(|t| t.text.parse::<u32>().map(|v| (60..=200).contains(&v)).unwrap_or(false))
            .collect();
        if candidates.len() == 1 {
            let t = candidates[0];
            bpm = t.text.parse::<f32>().ok();
            remove.push((t.start, t.end));
        }
    }

    // 4. Tagged / untagged marker.
    let mut tagged = None;
    for t in &tokens {
        if let Some(v) = tag_word(&t.text) {
            tagged = Some(v);
            remove.push((t.start, t.end));
        }
    }

    // 5. Title = input minus the recognised spans.
    remove.sort();
    let mut title = String::new();
    let mut pos = 0;
    for (a, b) in remove {
        if a >= pos {
            title.push_str(&s[pos..a]);
            title.push(' ');
            pos = b;
        } else if b > pos {
            pos = b;
        }
    }
    title.push_str(&s[pos.min(s.len())..]);
    let mut title = clean_title(&title);
    if title.is_empty() {
        title = clean_title(&s);
    }

    ParsedName { title, bpm, key, tagged, producers: vec![] }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(s: &str) -> Option<MusicalKey> {
        MusicalKey::parse(s)
    }

    #[test]
    fn classic_names() {
        let p = parse_name("Night_140bpm_Am");
        assert_eq!((p.title.as_str(), p.bpm, p.key), ("Night", Some(140.0), k("Am")));

        let p = parse_name("Dark Trap Beat 140 BPM (C#m)");
        assert_eq!((p.title.as_str(), p.bpm, p.key), ("Dark Trap Beat", Some(140.0), k("C#m")));

        let p = parse_name("ocean_92.5bpm_F#min_untagged");
        assert_eq!(p.bpm, Some(92.5));
        assert_eq!(p.key, k("F#m"));
        assert_eq!(p.tagged, Some(false));
        assert_eq!(p.title, "ocean");

        let p = parse_name("Lights - BPM 128 - A minor");
        assert_eq!((p.title.as_str(), p.bpm, p.key), ("Lights", Some(128.0), k("Am")));

        let p = parse_name("Ocean 8A 92");
        assert_eq!((p.title.as_str(), p.bpm, p.key), ("Ocean", Some(92.0), k("Am")));

        let p = parse_name("Snow_Ebm_150");
        assert_eq!((p.title.as_str(), p.bpm, p.key), ("Snow", Some(150.0), k("Ebm")));
    }

    #[test]
    fn plain_letter_needs_bpm_neighbour() {
        // "A" as an article must not become a key
        let p = parse_name("A_Dream_140bpm");
        assert_eq!(p.key, None);
        assert_eq!(p.bpm, Some(140.0));
        assert_eq!(p.title, "A Dream");
        // ...but right next to the BPM it is a key
        let p = parse_name("Dream_140bpm_C");
        assert_eq!(p.key, k("C"));
        assert_eq!(p.title, "Dream");
    }

    #[test]
    fn no_false_positives() {
        let p = parse_name("Mix 2024 final");
        assert_eq!((p.bpm, p.key), (None, None));
        assert_eq!(p.title, "Mix 2024 final");

        let p = parse_name("i am here");
        assert_eq!(p.key, None);

        // numbers without key are not BPM
        let p = parse_name("Track 140");
        assert_eq!(p.bpm, None);

        // out-of-range BPM is ignored
        let p = parse_name("Loop 999bpm");
        assert_eq!(p.bpm, None);

        // Two candidate numbers: ambiguous, keep empty
        let p = parse_name("Beat 120 140 Am");
        assert_eq!(p.bpm, None);
        assert_eq!(p.key, k("Am"));
    }

    #[test]
    fn cyrillic_titles() {
        let p = parse_name("Ночь_140bpm_Am");
        assert_eq!((p.title.as_str(), p.bpm, p.key), ("Ночь", Some(140.0), k("Am")));
        let p = parse_name("Зима 128 BPM");
        assert_eq!((p.title.as_str(), p.bpm), ("Зима", Some(128.0)));
    }

    #[test]
    fn folder_handles_and_unicode_punctuation() {
        let p = parse_folder_name("@presscopy @prodamory — Alice 130 Gm");
        assert_eq!(p.producers, vec!["@presscopy", "@prodamory"]);
        assert_eq!((p.title.as_str(), p.bpm, p.key), ("Alice", Some(130.0), k("Gm")));
        let p = parse_folder_name("Night — final");
        assert_eq!((p.bpm, p.key), (None, None));
    }

    #[test]
    fn producer_credits() {
        let p = parse_name("Night (prod. maks x lowkey)_140bpm_Am");
        assert_eq!(p.producers, vec!["maks", "lowkey"]);
        assert_eq!(p.title, "Night");
        assert_eq!(p.bpm, Some(140.0));
        let p = parse_name("Ночь 150bpm F#m [prod by Kid & 808mafia]");
        assert_eq!(p.producers, vec!["Kid", "808mafia"]);
        assert_eq!(p.title, "Ночь");
        let p = parse_name("Velvet 92bpm prod. maks");
        assert_eq!(p.producers, vec!["maks"]);
        assert_eq!(p.title, "Velvet");
        // A trailing credit followed by a BPM is not trusted.
        let p = parse_name("Product prod 140bpm");
        assert!(p.producers.is_empty());
        let p = parse_name("Production Line");
        assert!(p.producers.is_empty());
    }
}
