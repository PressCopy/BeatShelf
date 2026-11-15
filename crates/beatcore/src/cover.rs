//! Cover art for beats.
//!
//! Two jobs: turn any picture into a cover of a sane size, and write that
//! cover, with the beat's title and credits, into a *copy* of an MP3 for
//! sending. The MP3 in the library is only ever read.

use crate::error::{Error, Result};
use id3::frame::{Picture, PictureType};
use id3::{Tag, TagLike, Version};
use std::path::Path;

/// Longest side of an embedded cover, in pixels. Big enough for any player,
/// small enough not to bloat the MP3.
pub const COVER_MAX: u32 = 1000;

/// File name a beat's cover is stored under (inside its Artwork folder).
pub const COVER_FILE: &str = "cover.jpg";

fn msg(e: impl std::fmt::Display) -> Error {
    Error::Msg(e.to_string())
}

/// Decode a JPG, PNG or WebP and return it as a JPEG no larger than
/// [`COVER_MAX`] on its longest side. The aspect ratio is kept.
pub fn normalized_cover(path: &Path) -> Result<Vec<u8>> {
    let img = image::ImageReader::open(path)?.with_guessed_format()?.decode().map_err(msg)?;
    let img = if img.width().max(img.height()) > COVER_MAX {
        img.resize(COVER_MAX, COVER_MAX, image::imageops::FilterType::Lanczos3)
    } else {
        img
    };
    let rgb = img.to_rgb8();
    let mut out = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, 90).encode_image(&rgb).map_err(msg)?;
    Ok(out)
}

/// What gets written into the exported MP3's tags.
#[derive(Debug, Clone, Default)]
pub struct Mp3Meta {
    pub title: String,
    /// "maks x lowkey"
    pub artist: Option<String>,
    pub bpm: Option<f64>,
    pub key: Option<String>,
    pub genre: Option<String>,
}

fn filled(s: &Option<String>) -> Option<&str> {
    s.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

/// A copy of a read-only file must still be writable for tagging.
fn make_writable(p: &Path) -> std::io::Result<()> {
    let mut perms = std::fs::metadata(p)?.permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        perms.set_mode(perms.mode() | 0o200);
    }
    #[cfg(not(unix))]
    {
        #[allow(clippy::permissions_set_readonly_false)]
        perms.set_readonly(false);
    }
    std::fs::set_permissions(p, perms)
}

/// Write `dest` as a copy of the MP3 at `src` with title, credits and
/// (optionally) cover art in its ID3 tag.
///
/// `src` is never modified. The copy is tagged under a temporary name next
/// to `dest` and renamed into place at the end, so a failure leaves nothing
/// behind. An existing `dest` is replaced: the caller has already asked the
/// user (the system "Save as" dialog does).
pub fn export_mp3(src: &Path, dest: &Path, meta: &Mp3Meta, cover: Option<&Path>) -> Result<()> {
    if src == dest {
        return Err(Error::Msg("source and destination are the same file".into()));
    }
    let parent = dest.parent().ok_or_else(|| Error::Msg("destination has no folder".into()))?;
    let tmp = parent.join(format!(".beatshelf-export-{}.tmp", uuid::Uuid::new_v4().simple()));

    let run = || -> Result<()> {
        // Decode the picture first: a bad image should fail before any copying.
        let art = match cover {
            Some(c) => Some(normalized_cover(c)?),
            None => None,
        };
        std::fs::copy(src, &tmp)?;
        make_writable(&tmp)?;
        // Keep whatever tags the file already has; a file without a tag gets a new one.
        let mut tag = Tag::read_from_path(&tmp).unwrap_or_else(|_| Tag::new());
        if !meta.title.trim().is_empty() {
            tag.set_title(meta.title.trim());
        }
        if let Some(a) = filled(&meta.artist) {
            tag.set_artist(a);
        }
        if let Some(bpm) = meta.bpm {
            tag.set_text("TBPM", format!("{}", bpm.round() as i64));
        }
        if let Some(k) = filled(&meta.key) {
            tag.set_text("TKEY", k);
        }
        if let Some(g) = filled(&meta.genre) {
            tag.set_genre(g);
        }
        if let Some(data) = art {
            tag.remove_all_pictures();
            tag.add_frame(Picture {
                mime_type: "image/jpeg".to_string(),
                picture_type: PictureType::CoverFront,
                description: String::new(),
                data,
            });
        }
        // ID3v2.3 is the version every player and messenger reads.
        tag.write_to_path(&tmp, Version::Id3v23).map_err(msg)?;
        std::fs::rename(&tmp, dest)?;
        Ok(())
    };

    let result = run();
    if result.is_err() {
        // Our own temporary copy, never a user's file.
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_mp3() -> Vec<u8> {
        let mut v = vec![0xFF, 0xFB, 0x90, 0x00];
        v.extend(std::iter::repeat(7u8).take(4096));
        v
    }

    fn meta() -> Mp3Meta {
        Mp3Meta {
            title: "Night".into(),
            artist: Some("maks x lowkey".into()),
            bpm: Some(140.0),
            key: Some("Am".into()),
            genre: Some("Trap".into()),
        }
    }

    #[test]
    fn export_tags_a_copy_and_leaves_the_source_alone() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("Night_tagged.mp3");
        std::fs::write(&src, fake_mp3()).unwrap();
        let png = dir.path().join("art.png");
        image::RgbImage::from_pixel(2000, 1200, image::Rgb([200, 30, 30])).save(&png).unwrap();
        let out_dir = dir.path().join("Downloads");
        std::fs::create_dir_all(&out_dir).unwrap();
        let dest = out_dir.join("Night.mp3");

        export_mp3(&src, &dest, &meta(), Some(&png)).unwrap();

        assert_eq!(std::fs::read(&src).unwrap(), fake_mp3(), "the library file is untouched");
        let tag = Tag::read_from_path(&dest).unwrap();
        assert_eq!(tag.title(), Some("Night"));
        assert_eq!(tag.artist(), Some("maks x lowkey"));
        assert_eq!(tag.genre(), Some("Trap"));
        assert_eq!(tag.get("TBPM").and_then(|f| f.content().text()), Some("140"));
        assert_eq!(tag.get("TKEY").and_then(|f| f.content().text()), Some("Am"));
        let pic = tag.pictures().next().expect("cover embedded");
        assert_eq!(pic.mime_type, "image/jpeg");
        assert_eq!(pic.picture_type, PictureType::CoverFront);
        let img = image::load_from_memory(&pic.data).unwrap();
        assert_eq!((img.width(), img.height()), (1000, 600), "scaled down, aspect kept");
        assert!(std::fs::read(&dest).unwrap().ends_with(&fake_mp3()), "audio follows the tag unchanged");
        assert_eq!(std::fs::read_dir(&out_dir).unwrap().count(), 1, "no temporary files left");
    }

    #[test]
    fn export_without_a_cover_still_writes_credits_and_replaces_a_cover_on_reexport() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("a.mp3");
        std::fs::write(&src, fake_mp3()).unwrap();
        let first = dir.path().join("first.mp3");
        export_mp3(&src, &first, &meta(), None).unwrap();
        let tag = Tag::read_from_path(&first).unwrap();
        assert_eq!(tag.title(), Some("Night"));
        assert_eq!(tag.pictures().count(), 0);

        // Tagging an already tagged file keeps one cover, not two.
        let small = dir.path().join("small.png");
        image::RgbImage::from_pixel(300, 300, image::Rgb([10, 200, 90])).save(&small).unwrap();
        let second = dir.path().join("second.mp3");
        export_mp3(&first, &second, &meta(), Some(&small)).unwrap();
        let third = dir.path().join("third.mp3");
        export_mp3(&second, &third, &meta(), Some(&small)).unwrap();
        let tag = Tag::read_from_path(&third).unwrap();
        assert_eq!(tag.pictures().count(), 1);
        let img = image::load_from_memory(&tag.pictures().next().unwrap().data).unwrap();
        assert_eq!((img.width(), img.height()), (300, 300), "small covers are not enlarged");
    }

    #[test]
    fn a_broken_image_fails_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("a.mp3");
        std::fs::write(&src, fake_mp3()).unwrap();
        let not_image = dir.path().join("cover.jpg");
        std::fs::write(&not_image, b"this is not a picture").unwrap();
        let out_dir = dir.path().join("out");
        std::fs::create_dir_all(&out_dir).unwrap();
        let dest = out_dir.join("a.mp3");
        assert!(export_mp3(&src, &dest, &meta(), Some(&not_image)).is_err());
        assert_eq!(std::fs::read_dir(&out_dir).unwrap().count(), 0, "nothing written, nothing left behind");
        assert!(export_mp3(&src, &src, &meta(), None).is_err(), "never tags the source in place");
        assert_eq!(std::fs::read(&src).unwrap(), fake_mp3());
    }
}
