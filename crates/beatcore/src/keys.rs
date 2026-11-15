//! Musical keys and the Camelot wheel.
//!
//! A key is stored as a pitch class (0 = C … 11 = B) plus a minor flag.
//! Canonical spelling follows the usual Camelot chart (Ebm, F#m, Bb, …).

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MusicalKey {
    pub pc: u8,
    pub minor: bool,
}

const MINOR_NAMES: [&str; 12] = [
    "Cm", "C#m", "Dm", "Ebm", "Em", "Fm", "F#m", "Gm", "G#m", "Am", "Bbm", "Bm",
];
const MAJOR_NAMES: [&str; 12] = [
    "C", "Db", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B",
];

/// Pitch class of a note letter with an optional accidental.
pub(crate) fn note_pc(letter: char, accidental: Option<char>) -> Option<u8> {
    let base: i32 = match letter.to_ascii_uppercase() {
        'C' => 0,
        'D' => 2,
        'E' => 4,
        'F' => 5,
        'G' => 7,
        'A' => 9,
        'B' => 11,
        _ => return None,
    };
    let shift = match accidental {
        None => 0,
        Some('#') | Some('♯') => 1,
        Some('b') | Some('♭') => -1,
        Some(_) => return None,
    };
    Some(((base + shift).rem_euclid(12)) as u8)
}

impl MusicalKey {
    pub fn new(pc: u8, minor: bool) -> Self {
        MusicalKey { pc: pc % 12, minor }
    }

    pub fn name(&self) -> &'static str {
        if self.minor {
            MINOR_NAMES[self.pc as usize]
        } else {
            MAJOR_NAMES[self.pc as usize]
        }
    }

    /// Camelot number (1..=12) and letter ('A' = minor, 'B' = major).
    pub fn camelot(&self) -> (u8, char) {
        let pc = self.pc as i32;
        // Each step around the wheel is a fifth (7 semitones).
        let steps = if self.minor {
            ((pc + 3) * 7).rem_euclid(12) // Am (pc 9) -> 0
        } else {
            (pc * 7).rem_euclid(12) // C (pc 0) -> 0
        };
        let number = ((steps + 7) % 12 + 1) as u8; // 0 -> 8
        (number, if self.minor { 'A' } else { 'B' })
    }

    pub fn camelot_code(&self) -> String {
        let (n, l) = self.camelot();
        format!("{n}{l}")
    }

    pub fn from_camelot(number: u8, letter: char) -> Option<Self> {
        if !(1..=12).contains(&number) {
            return None;
        }
        let steps = (number as i32 + 4).rem_euclid(12); // inverse of (steps+7)%12+1
        // 7 is its own inverse modulo 12.
        match letter.to_ascii_uppercase() {
            'B' => Some(MusicalKey::new(((steps * 7).rem_euclid(12)) as u8, false)),
            'A' => Some(MusicalKey::new(((steps * 7 - 3).rem_euclid(12)) as u8, true)),
            _ => None,
        }
    }

    /// Permissive parser for user input: "Am", "A minor", "C#m", "Dbmaj",
    /// "F# min", "8A", "11b", "ля минор" is not supported.
    pub fn parse(input: &str) -> Option<Self> {
        let s: String = input
            .trim()
            .replace('♯', "#")
            .replace('♭', "b")
            .chars()
            .filter(|c| !c.is_whitespace() && *c != '_' && *c != '-')
            .collect();
        if s.is_empty() {
            return None;
        }
        // Camelot code
        if let Some(letter) = s.chars().last() {
            let digits = &s[..s.len() - letter.len_utf8()];
            if letter.is_ascii_alphabetic() && !digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()) {
                if let Ok(n) = digits.parse::<u8>() {
                    return Self::from_camelot(n, letter);
                }
            }
        }
        let mut chars = s.chars();
        let letter = chars.next()?;
        let rest: String = chars.collect();
        let (acc, suffix) = match rest.chars().next() {
            Some(c @ ('#' | 'b')) => {
                // "Bb" is B-flat; but "Bbm" / "Bbmaj" also start with b.
                (Some(c), rest[1..].to_string())
            }
            _ => (None, rest),
        };
        let minor = match suffix.as_str() {
            "" | "M" => false,
            "m" => true,
            other => match other.to_lowercase().as_str() {
                "min" | "minor" | "mi" | "moll" => true,
                "maj" | "major" | "ma" | "dur" => false,
                _ => return None,
            },
        };
        let pc = note_pc(letter, acc)?;
        Some(MusicalKey::new(pc, minor))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn camelot_roundtrip_all_24() {
        for pc in 0..12 {
            for minor in [false, true] {
                let k = MusicalKey::new(pc, minor);
                let (n, l) = k.camelot();
                assert_eq!(MusicalKey::from_camelot(n, l), Some(k), "{}", k.name());
            }
        }
    }

    #[test]
    fn camelot_known_values() {
        let cases = [
            ("Am", "8A"),
            ("C", "8B"),
            ("Em", "9A"),
            ("G#m", "1A"),
            ("B", "1B"),
            ("F#m", "11A"),
            ("C#m", "12A"),
            ("E", "12B"),
            ("Ebm", "2A"),
            ("F#", "2B"),
            ("F", "7B"),
            ("Dm", "7A"),
        ];
        for (name, code) in cases {
            let k = MusicalKey::parse(name).unwrap();
            assert_eq!(k.camelot_code(), code, "{name}");
        }
    }

    #[test]
    fn parse_variants() {
        let am = MusicalKey::new(9, true);
        for s in ["Am", "A minor", "amin", "A min", "8A", "8a", "A-minor"] {
            assert_eq!(MusicalKey::parse(s), Some(am), "{s}");
        }
        assert_eq!(MusicalKey::parse("Bb").unwrap().name(), "Bb");
        assert_eq!(MusicalKey::parse("Bbm").unwrap().name(), "Bbm");
        assert_eq!(MusicalKey::parse("A#m").unwrap().name(), "Bbm");
        assert_eq!(MusicalKey::parse("Db maj").unwrap().name(), "Db");
        assert_eq!(MusicalKey::parse("C♯m").unwrap().name(), "C#m");
        assert_eq!(MusicalKey::parse("H"), None);
        assert_eq!(MusicalKey::parse("13A"), None);
        assert_eq!(MusicalKey::parse(""), None);
    }
}
