//! Technical audio data (duration, sample rate, channels, codec) and
//! waveform peaks. Peaks are computed once in Rust and cached, so the list
//! view never decodes audio in the webview.

use crate::error::{Error, Result};
use std::fs::File;
use std::path::Path;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymError;
use symphonia::core::formats::{FormatOptions, FormatReader};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

pub const PEAK_BUCKETS: usize = 1000;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct AudioInfo {
    pub duration_ms: Option<i64>,
    pub sample_rate: Option<i64>,
    pub channels: Option<i64>,
    pub codec: Option<String>,
}

fn open(path: &Path) -> Result<Box<dyn FormatReader>> {
    let file = File::open(path)?;
    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| Error::Audio(e.to_string()))?;
    Ok(probed.format)
}

/// Header-only probe: cheap enough to run on every file during a scan.
pub fn probe(path: &Path) -> Result<AudioInfo> {
    let format = open(path)?;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| Error::Audio("no audio track".into()))?;
    let p = &track.codec_params;
    let duration_ms = match (p.n_frames, p.time_base, p.sample_rate) {
        (Some(n), Some(tb), _) => {
            let t = tb.calc_time(n);
            Some((t.seconds as f64 * 1000.0 + t.frac * 1000.0).round() as i64)
        }
        (Some(n), None, Some(sr)) if sr > 0 => Some((n as f64 * 1000.0 / sr as f64).round() as i64),
        _ => None,
    };
    let codec = symphonia::default::get_codecs()
        .get_codec(p.codec)
        .map(|d| d.short_name.to_string());
    Ok(AudioInfo {
        duration_ms,
        sample_rate: p.sample_rate.map(|v| v as i64),
        channels: p.channels.map(|c| c.count() as i64),
        codec,
    })
}

/// Decode the file and return `PEAK_BUCKETS` absolute peaks scaled to 0..=255,
/// plus the decoded duration.
pub fn compute_peaks(path: &Path) -> Result<(Vec<u8>, Option<i64>)> {
    let mut format = open(path)?;
    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| Error::Audio("no audio track".into()))?
        .clone();
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| Error::Audio(e.to_string()))?;

    const BLOCK: usize = 256;
    let mut blocks: Vec<f32> = Vec::new();
    let mut block_max = 0f32;
    let mut in_block = 0usize;
    let mut frames: u64 = 0;
    let mut sample_rate = track.codec_params.sample_rate.unwrap_or(0);
    let mut buf: Option<SampleBuffer<f32>> = None;
    let mut buf_frames: u64 = 0;
    let mut buf_ch: usize = 0;

    loop {
        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(SymError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(SymError::ResetRequired) => break,
            Err(e) => return Err(Error::Audio(e.to_string())),
        };
        if packet.track_id() != track.id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(SymError::DecodeError(_)) => continue,
            Err(e) => return Err(Error::Audio(e.to_string())),
        };
        let spec = *decoded.spec();
        sample_rate = spec.rate;
        let ch = spec.channels.count().max(1);
        let needed = decoded.capacity() as u64;
        if buf.is_none() || needed > buf_frames || ch != buf_ch {
            buf = Some(SampleBuffer::<f32>::new(needed, spec));
            buf_frames = needed;
            buf_ch = ch;
        }
        let sb = buf.as_mut().unwrap();
        sb.copy_interleaved_ref(decoded);
        for frame in sb.samples().chunks(ch) {
            let m = frame.iter().fold(0f32, |acc, s| acc.max(s.abs()));
            block_max = block_max.max(m);
            in_block += 1;
            frames += 1;
            if in_block == BLOCK {
                blocks.push(block_max);
                block_max = 0.0;
                in_block = 0;
            }
        }
    }
    if in_block > 0 {
        blocks.push(block_max);
    }

    let mut peaks = vec![0u8; PEAK_BUCKETS];
    if !blocks.is_empty() {
        for (i, p) in peaks.iter_mut().enumerate() {
            let a = i * blocks.len() / PEAK_BUCKETS;
            let b = (((i + 1) * blocks.len()) / PEAK_BUCKETS).max(a + 1).min(blocks.len());
            let m = blocks[a.min(blocks.len() - 1)..b].iter().fold(0f32, |acc, v| acc.max(*v));
            *p = (m.clamp(0.0, 1.0) * 255.0).round() as u8;
        }
    }
    let duration = if sample_rate > 0 { Some((frames as f64 * 1000.0 / sample_rate as f64).round() as i64) } else { None };
    Ok((peaks, duration))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    /// Minimal 16-bit PCM WAV with a sine that fades out.
    pub fn write_test_wav(path: &Path, seconds: f32, sr: u32) {
        let n = (seconds * sr as f32) as u32;
        let mut data = Vec::with_capacity(n as usize * 4);
        for i in 0..n {
            let amp = 1.0 - i as f32 / n as f32;
            let v = ((i as f32 * 440.0 * 2.0 * std::f32::consts::PI / sr as f32).sin() * amp * 30000.0) as i16;
            data.extend_from_slice(&v.to_le_bytes());
            data.extend_from_slice(&v.to_le_bytes());
        }
        let mut f = File::create(path).unwrap();
        let byte_rate = sr * 4;
        f.write_all(b"RIFF").unwrap();
        f.write_all(&(36 + data.len() as u32).to_le_bytes()).unwrap();
        f.write_all(b"WAVEfmt ").unwrap();
        f.write_all(&16u32.to_le_bytes()).unwrap();
        f.write_all(&1u16.to_le_bytes()).unwrap();
        f.write_all(&2u16.to_le_bytes()).unwrap();
        f.write_all(&sr.to_le_bytes()).unwrap();
        f.write_all(&byte_rate.to_le_bytes()).unwrap();
        f.write_all(&4u16.to_le_bytes()).unwrap();
        f.write_all(&16u16.to_le_bytes()).unwrap();
        f.write_all(b"data").unwrap();
        f.write_all(&(data.len() as u32).to_le_bytes()).unwrap();
        f.write_all(&data).unwrap();
    }

    #[test]
    fn probe_and_peaks() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("tone.wav");
        write_test_wav(&p, 2.0, 44100);
        let info = probe(&p).unwrap();
        assert_eq!(info.sample_rate, Some(44100));
        assert_eq!(info.channels, Some(2));
        assert_eq!(info.duration_ms, Some(2000));
        let (peaks, dur) = compute_peaks(&p).unwrap();
        assert_eq!(peaks.len(), PEAK_BUCKETS);
        assert_eq!(dur, Some(2000));
        assert!(peaks[0] > 200, "start is loud: {}", peaks[0]);
        assert!(peaks[PEAK_BUCKETS - 1] < 20, "end is quiet: {}", peaks[PEAK_BUCKETS - 1]);
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("fake.wav");
        std::fs::write(&p, b"not audio at all").unwrap();
        assert!(probe(&p).is_err());
        assert!(compute_peaks(&p).is_err());
    }
}
