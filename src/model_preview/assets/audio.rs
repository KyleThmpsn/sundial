//! Bounded decoding of packaged Wwise Vorbis into ordinary PCM WAV for the inspector.
mod wave;

use std::io::{self, Cursor, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use wave::Wave;

const MAX_PCM: usize = 64 * 1024 * 1024;

pub(crate) fn wave_duration(bytes: &[u8]) -> Option<Duration> {
    let wave = Wave::parse(bytes)?;
    let rate = wave.pcm_byte_rate()?;
    (!wave.data.is_empty())
        .then(|| Duration::from_secs_f64(wave.data.len() as f64 / f64::from(rate)))
}

pub(super) fn wave_info(bytes: &[u8]) -> Option<(u16, u16, u32)> {
    let wave = Wave::parse(bytes)?;
    Some((wave.codec, wave.channels, wave.sample_rate))
}

pub(crate) fn decoded_wave(bytes: &[u8]) -> Result<Vec<u8>, String> {
    decoded_wave_cancelable(bytes, &AtomicBool::new(false))
}

pub(crate) fn decoded_wave_cancelable(
    bytes: &[u8],
    cancel: &AtomicBool,
) -> Result<Vec<u8>, String> {
    check_cancel(cancel)?;
    let wave = Wave::parse(bytes).ok_or("Audio is not a valid RIFF/WAVE payload")?;
    match wave.codec {
        1 => {
            if wave.bytes.len() > MAX_PCM + 44 || wave.data.is_empty() {
                return Err("PCM audio is empty or exceeds the preview budget".into());
            }
            Ok(wave.bytes.to_vec())
        }
        0xFFFF => {
            let default = ww2ogg::CodebookLibrary::default_codebooks()
                .map_err(|error| format!("Could not load Wwise codebooks: {error}"))?;
            let ogg = convert(wave.bytes, default, cancel).or_else(|first| {
                check_cancel(cancel)?;
                let alternate = ww2ogg::CodebookLibrary::aotuv_codebooks()
                    .map_err(|error| format!("Could not load alternate codebooks: {error}"))?;
                convert(wave.bytes, alternate, cancel).map_err(|second| {
                    format!(
                        "Wwise Vorbis conversion failed: {first}. Alternate codebooks: {second}"
                    )
                })
            })?;
            decode_ogg(&ogg, cancel)
        }
        codec => Err(format!("Audio codec 0x{codec:04X} cannot be previewed")),
    }
}

fn check_cancel(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        Err("Audio decoding canceled".into())
    } else {
        Ok(())
    }
}

fn convert(
    bytes: &[u8],
    codebooks: ww2ogg::CodebookLibrary,
    cancel: &AtomicBool,
) -> Result<Vec<u8>, String> {
    check_cancel(cancel)?;
    let mut converter = ww2ogg::WwiseRiffVorbis::new(Cursor::new(bytes), codebooks)
        .map_err(|error| error.to_string())?;
    let mut ogg = ConversionOutput {
        bytes: Vec::new(),
        cancel,
    };
    converter
        .generate_ogg(&mut ogg)
        .map_err(|error| error.to_string())?;
    check_cancel(cancel)?;
    ww2ogg::validate(&ogg.bytes).map_err(|error| error.to_string())?;
    Ok(ogg.bytes)
}

struct ConversionOutput<'a> {
    bytes: Vec<u8>,
    cancel: &'a AtomicBool,
}

impl Write for ConversionOutput<'_> {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        check_cancel(self.cancel).map_err(io::Error::other)?;
        if bytes.len() > MAX_PCM.saturating_sub(self.bytes.len()) {
            return Err(io::Error::other(
                "Converted audio exceeds the preview budget",
            ));
        }
        self.bytes
            .try_reserve(bytes.len())
            .map_err(io::Error::other)?;
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        check_cancel(self.cancel).map_err(io::Error::other)
    }
}

fn decode_ogg(ogg: &[u8], cancel: &AtomicBool) -> Result<Vec<u8>, String> {
    let mut decoder = lewton::inside_ogg::OggStreamReader::new(Cursor::new(ogg))
        .map_err(|error| format!("Could not read converted audio: {error}"))?;
    let channels = u16::from(decoder.ident_hdr.audio_channels);
    let sample_rate = decoder.ident_hdr.audio_sample_rate;
    if !(1..=8).contains(&channels) || !(8_000..=192_000).contains(&sample_rate) {
        return Err("Converted audio has invalid channel or sample rate".into());
    }
    let mut pcm = Vec::new();
    loop {
        check_cancel(cancel)?;
        let Some(packet) = decoder
            .read_dec_packet_itl()
            .map_err(|error| format!("Could not decode audio packet: {error}"))?
        else {
            break;
        };
        let bytes = packet
            .len()
            .checked_mul(2)
            .and_then(|size| pcm.len().checked_add(size))
            .ok_or("Decoded audio is too large")?;
        if bytes > MAX_PCM {
            return Err("Decoded audio exceeds the preview budget".into());
        }
        pcm.extend(packet.into_iter().flat_map(i16::to_le_bytes));
    }
    if pcm.is_empty() {
        return Err("Decoded audio has no samples".into());
    }
    let data_size = u32::try_from(pcm.len()).map_err(|_| "Decoded audio is too large")?;
    let byte_rate = sample_rate
        .checked_mul(u32::from(channels))
        .and_then(|rate| rate.checked_mul(2))
        .ok_or("Decoded audio has invalid rate")?;
    let mut wav = Vec::with_capacity(44 + pcm.len());
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data_size).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&byte_rate.to_le_bytes());
    wav.extend_from_slice(&(channels * 2).to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&data_size.to_le_bytes());
    wav.extend_from_slice(&pcm);
    Ok(wav)
}

#[cfg(test)]
mod tests {
    use super::{decoded_wave, wave_duration};
    use std::time::Duration;

    #[test]
    fn wav_duration_uses_pcm_data_rate_and_rejects_truncation() {
        let mut wave = Vec::new();
        wave.extend_from_slice(b"RIFF");
        wave.extend_from_slice(&16_036u32.to_le_bytes());
        wave.extend_from_slice(b"WAVEfmt ");
        wave.extend_from_slice(&16u32.to_le_bytes());
        wave.extend_from_slice(&1u16.to_le_bytes());
        wave.extend_from_slice(&1u16.to_le_bytes());
        wave.extend_from_slice(&8_000u32.to_le_bytes());
        wave.extend_from_slice(&16_000u32.to_le_bytes());
        wave.extend_from_slice(&2u16.to_le_bytes());
        wave.extend_from_slice(&16u16.to_le_bytes());
        wave.extend_from_slice(b"data");
        wave.extend_from_slice(&16_000u32.to_le_bytes());
        wave.resize(wave.len() + 16_000, 0);
        assert_eq!(wave_duration(&wave), Some(Duration::from_secs(1)));
        wave.pop();
        assert_eq!(wave_duration(&wave), None);
    }

    fn pcm_fixture() -> Vec<u8> {
        let mut bytes = b"RIFF\0\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0".to_vec();
        bytes.extend_from_slice(&8_000u32.to_le_bytes());
        bytes.extend_from_slice(&16_000u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&16_000u32.to_le_bytes());
        bytes.extend((0..16_000).map(|index| (index % 251) as u8));
        set_extent(&mut bytes);
        bytes
    }

    fn set_extent(bytes: &mut [u8]) {
        let size = u32::try_from(bytes.len() - 8).unwrap();
        bytes[4..8].copy_from_slice(&size.to_le_bytes());
    }

    #[test]
    fn saved_pcm_clips_share_one_extent_for_inspection_decode_and_duration() {
        use sha2::Digest;
        let directory = tempfile::tempdir().unwrap();
        let original = pcm_fixture();
        let mut padded = original[..12].to_vec();
        padded.extend_from_slice(b"JUNK\x01\0\0\0\x42\0");
        padded.extend_from_slice(&original[12..]);
        set_extent(&mut padded);
        let mut with_trailer = padded.clone();
        with_trailer.extend_from_slice(b"data\xff\xff\xff\xffignored outside RIFF");
        let path = directory.path().join("clip.wav");
        std::fs::write(&path, with_trailer).unwrap();
        let source = std::fs::read(&path).unwrap();
        assert_eq!(super::super::wave_info(&source), Some((1, 1, 8_000)));
        assert_eq!(wave_duration(&source), Some(Duration::from_secs(1)));
        let decoded = decoded_wave(&source).unwrap();
        assert_eq!(decoded, padded);
        std::fs::write(&path, &decoded).unwrap();
        assert_eq!(
            decoded_wave(&std::fs::read(&path).unwrap()).unwrap(),
            decoded
        );
        crate::test_support::artifact(
            "pcm-wave-roundtrip.json",
            &serde_json::json!({
                "channels": 1, "sample_rate": 8000, "duration_ms": 1000,
                "decoded_bytes": decoded.len(), "source_bytes": source.len(),
                "sha256": hex::encode(sha2::Sha256::digest(&decoded)),
            }),
        );
        if let Some(output) = std::env::var_os("SUNDIAL_TEST_ARTIFACTS") {
            std::fs::write(
                std::path::PathBuf::from(output).join("pcm-wave-roundtrip.wav"),
                decoded,
            )
            .unwrap();
        }
    }

    #[test]
    fn malformed_pcm_is_rejected_before_export_or_playback() {
        let mut cases = Vec::new();
        for (name, at, bytes) in [
            ("zero byte rate", 28, 0u32.to_le_bytes().to_vec()),
            ("incorrect byte rate", 28, 15_999u32.to_le_bytes().to_vec()),
            ("incorrect alignment", 32, 1u16.to_le_bytes().to_vec()),
            ("invalid sample width", 34, 0u16.to_le_bytes().to_vec()),
        ] {
            let mut wave = pcm_fixture();
            wave[at..at + bytes.len()].copy_from_slice(&bytes);
            cases.push((name, wave));
        }
        let mut incomplete_frame = pcm_fixture();
        incomplete_frame.pop();
        incomplete_frame[40..44].copy_from_slice(&15_999u32.to_le_bytes());
        incomplete_frame.push(0);
        cases.push(("incomplete sample frame", incomplete_frame));
        let mut duplicate = pcm_fixture();
        let format = duplicate[12..36].to_vec();
        duplicate.extend(format);
        set_extent(&mut duplicate);
        cases.push(("duplicate format", duplicate));
        let mut duplicate = pcm_fixture();
        duplicate.extend_from_slice(b"data\0\0\0\0");
        set_extent(&mut duplicate);
        cases.push(("duplicate data", duplicate));
        let mut missing_pad = pcm_fixture();
        missing_pad.extend_from_slice(b"JUNK\x01\0\0\0\x42");
        set_extent(&mut missing_pad);
        cases.push(("missing odd chunk padding", missing_pad));
        let mut trailing_chunk = pcm_fixture();
        trailing_chunk.extend_from_slice(b"fmt");
        set_extent(&mut trailing_chunk);
        cases.push(("incomplete chunk header", trailing_chunk));
        let mut truncated = pcm_fixture();
        truncated.pop();
        cases.push(("truncated RIFF", truncated));
        let mut rejected = Vec::new();
        for (name, wave) in cases {
            assert!(decoded_wave(&wave).is_err(), "{name}");
            assert_eq!(wave_duration(&wave), None, "{name}");
            rejected.push(name);
        }
        crate::test_support::artifact("pcm-wave-rejections.json", &serde_json::json!(rejected));
    }
}
