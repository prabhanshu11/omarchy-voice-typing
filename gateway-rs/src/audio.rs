use std::io::Cursor;
use std::path::Path;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as BASE64;

use crate::error::GatewayError;

/// Build a WAV file in memory from raw PCM16 audio data.
///
/// Uses the `hound` crate for correct RIFF header generation (the Go code
/// did this manually with `binary.Write`). Returns the complete WAV bytes.
pub fn build_wav(pcm: &[u8], sample_rate: u32) -> Result<Vec<u8>, GatewayError> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    let mut cursor = Cursor::new(Vec::new());
    {
        let mut writer = hound::WavWriter::new(&mut cursor, spec)
            .map_err(|e| GatewayError::Audio(format!("failed to create WAV writer: {e}")))?;

        // PCM16 = 2 bytes per sample, little-endian
        for chunk in pcm.chunks_exact(2) {
            let sample = i16::from_le_bytes([chunk[0], chunk[1]]);
            writer
                .write_sample(sample)
                .map_err(|e| GatewayError::Audio(format!("failed to write WAV sample: {e}")))?;
        }

        // Handle trailing byte (odd-length buffer) — shouldn't happen with PCM16 but be safe
        if pcm.len() % 2 != 0 {
            tracing::warn!(
                trailing_byte = pcm[pcm.len() - 1],
                "PCM buffer has odd length, ignoring trailing byte"
            );
        }

        writer
            .finalize()
            .map_err(|e| GatewayError::Audio(format!("failed to finalize WAV: {e}")))?;
    }

    Ok(cursor.into_inner())
}

/// Write a WAV file to disk from raw PCM16 audio data.
pub fn write_wav(path: &Path, pcm: &[u8], sample_rate: u32) -> Result<(), GatewayError> {
    let wav_data = build_wav(pcm, sample_rate)?;
    std::fs::write(path, wav_data)?;
    Ok(())
}

/// Decode base64-encoded audio into raw PCM bytes.
///
/// This matches the Go handler's `base64.StdEncoding.DecodeString(event.Audio)` call.
pub fn decode_base64_audio(b64: &str) -> Result<Vec<u8>, GatewayError> {
    BASE64
        .decode(b64)
        .map_err(|e| GatewayError::Audio(format!("base64 decode failed: {e}")))
}

/// Compute RMS (root mean square) of PCM16 audio data.
///
/// Returns the RMS value as an f64. For 16-bit audio:
/// - Silence: < 50
/// - Background noise: 50-200
/// - Quiet speech: 200-1000
/// - Normal speech: 1000-5000
pub fn compute_rms_i16(pcm: &[u8]) -> f64 {
    if pcm.len() < 2 {
        return 0.0;
    }
    let num_samples = pcm.len() / 2;
    let sum_sq: f64 = pcm
        .chunks_exact(2)
        .map(|chunk| {
            let sample = i16::from_le_bytes([chunk[0], chunk[1]]) as f64;
            sample * sample
        })
        .sum();
    (sum_sq / num_samples as f64).sqrt()
}

/// Threshold below which audio is considered silence (no usable speech).
/// Based on empirical data: broken BT mic produces RMS 0-27, normal speech > 500.
pub const SILENCE_RMS_THRESHOLD: f64 = 100.0;

/// Archive a recording (WAV + transcript) to disk.
///
/// Matches Go's `archiveRecording()`: saves to `../recordings/` and `../transcripts/`
/// relative to the gateway binary's working directory.
pub fn archive_recording(audio_data: &[u8], transcript: &str, backend: &str) {
    let timestamp = chrono_timestamp();

    if !audio_data.is_empty() {
        let dir = Path::new("../recordings");
        if let Err(e) = std::fs::create_dir_all(dir) {
            tracing::error!(error = %e, "Failed to create recordings dir");
        } else {
            let wav_path = dir.join(format!("{timestamp}_audio.wav"));
            match write_wav(&wav_path, audio_data, 24000) {
                Ok(()) => tracing::info!(path = %wav_path.display(), "Saved audio"),
                Err(e) => tracing::error!(error = %e, "Failed to save WAV"),
            }
        }
    }

    if !transcript.is_empty() {
        let dir = Path::new("../transcripts");
        if let Err(e) = std::fs::create_dir_all(dir) {
            tracing::error!(error = %e, "Failed to create transcripts dir");
        } else {
            let txt_path = dir.join(format!("{timestamp}_{backend}.txt"));
            match std::fs::write(&txt_path, transcript) {
                Ok(()) => tracing::info!(path = %txt_path.display(), "Saved transcript"),
                Err(e) => tracing::error!(error = %e, "Failed to save transcript"),
            }
        }
    }
}

/// Generate a timestamp string in the format used by the Go gateway: "20060102_150405"
fn chrono_timestamp() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();

    // Format as YYYYMMDD_HHMMSS without pulling in the chrono crate
    let secs = now.as_secs();
    // Use libc-free timestamp formatting
    let (year, month, day, hour, min, sec) = timestamp_parts(secs);
    format!("{year:04}{month:02}{day:02}_{hour:02}{min:02}{sec:02}")
}

/// Break Unix timestamp into calendar parts (UTC).
pub fn timestamp_parts(secs: u64) -> (u32, u32, u32, u32, u32, u32) {
    let days = (secs / 86400) as u32;
    let time_of_day = (secs % 86400) as u32;
    let hour = time_of_day / 3600;
    let min = (time_of_day % 3600) / 60;
    let sec = time_of_day % 60;

    // Days since 1970-01-01 to Y-M-D
    let mut y = 1970u32;
    let mut remaining = days;
    loop {
        let days_in_year = if is_leap(y) { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        y += 1;
    }

    let leap = is_leap(y);
    let month_days: [u32; 12] = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];

    let mut m = 0u32;
    for &md in &month_days {
        if remaining < md {
            break;
        }
        remaining -= md;
        m += 1;
    }

    (y, m + 1, remaining + 1, hour, min, sec)
}

pub fn is_leap(y: u32) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_wav_valid() {
        // 100 samples of silence (200 bytes PCM16)
        let pcm = vec![0u8; 200];
        let wav = build_wav(&pcm, 24000).unwrap();

        // Should start with RIFF header
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");

        // Should be parseable by hound
        let reader = hound::WavReader::new(Cursor::new(&wav)).unwrap();
        assert_eq!(reader.spec().sample_rate, 24000);
        assert_eq!(reader.spec().channels, 1);
        assert_eq!(reader.spec().bits_per_sample, 16);
        assert_eq!(reader.len(), 100); // 100 samples
    }

    #[test]
    fn test_build_wav_empty() {
        let wav = build_wav(&[], 24000).unwrap();
        // Valid WAV with 0 samples
        assert_eq!(&wav[..4], b"RIFF");
        let reader = hound::WavReader::new(Cursor::new(&wav)).unwrap();
        assert_eq!(reader.len(), 0);
    }

    #[test]
    fn test_decode_base64_audio() {
        let original = vec![0u8, 1, 2, 3, 4, 5];
        let encoded = BASE64.encode(&original);
        let decoded = decode_base64_audio(&encoded).unwrap();
        assert_eq!(decoded, original);
    }

    #[test]
    fn test_decode_base64_audio_invalid() {
        assert!(decode_base64_audio("not-valid-base64!!!").is_err());
    }

    #[test]
    fn test_timestamp_parts() {
        // 2026-02-18 00:00:00 UTC = 1771372800
        let (y, m, d, h, min, s) = timestamp_parts(1771372800);
        assert_eq!((y, m, d, h, min, s), (2026, 2, 18, 0, 0, 0));
    }

    #[test]
    fn test_timestamp_epoch() {
        let (y, m, d, h, min, s) = timestamp_parts(0);
        assert_eq!((y, m, d, h, min, s), (1970, 1, 1, 0, 0, 0));
    }
}
