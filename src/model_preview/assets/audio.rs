//! Bounded decoding of packaged Wwise Vorbis into ordinary PCM WAV for the inspector.
use std::io::Cursor;
use std::time::Duration;

const MAX_PCM: usize = 64 * 1024 * 1024;

pub(crate) fn wave_duration(bytes: &[u8]) -> Option<Duration> {
    if bytes.get(..4)? != b"RIFF" || bytes.get(8..12)? != b"WAVE" {
        return None;
    }
    let mut at = 12usize;
    let mut rate = None;
    let mut data = None;
    while at.checked_add(8)? <= bytes.len() {
        let kind = bytes.get(at..at + 4)?;
        let size = u32::from_le_bytes(bytes.get(at + 4..at + 8)?.try_into().ok()?) as usize;
        let start = at + 8;
        let end = start.checked_add(size)?;
        if end > bytes.len() {
            return None;
        }
        if kind == b"fmt " && size >= 16 {
            let bytes_per_second =
                u32::from_le_bytes(bytes.get(start + 8..start + 12)?.try_into().ok()?);
            if bytes_per_second == 0 {
                return None;
            }
            rate = Some(bytes_per_second);
        } else if kind == b"data" {
            data = Some(size);
        }
        at = end.checked_add(size & 1)?;
    }
    let seconds = data? as f64 / rate? as f64;
    (seconds.is_finite() && seconds > 0.0).then(|| Duration::from_secs_f64(seconds))
}

pub(crate) fn decoded_wave(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let (codec, _, _) = super::wave_info(bytes).ok_or("Audio is not RIFF/WAVE")?;
    match codec {
        1 => Ok(bytes.to_vec()),
        0xFFFF => {
            let default = ww2ogg::CodebookLibrary::default_codebooks()
                .map_err(|error| format!("Could not load Wwise codebooks: {error}"))?;
            let ogg = convert(bytes, default).or_else(|first| {
                let alternate = ww2ogg::CodebookLibrary::aotuv_codebooks()
                    .map_err(|error| format!("Could not load alternate codebooks: {error}"))?;
                convert(bytes, alternate).map_err(|second| {
                    format!(
                        "Wwise Vorbis conversion failed: {first}. Alternate codebooks: {second}"
                    )
                })
            })?;
            decode_ogg(&ogg)
        }
        _ => Err(format!("Audio codec 0x{codec:04X} cannot be previewed")),
    }
}

fn convert(bytes: &[u8], codebooks: ww2ogg::CodebookLibrary) -> Result<Vec<u8>, String> {
    let mut converter = ww2ogg::WwiseRiffVorbis::new(Cursor::new(bytes), codebooks)
        .map_err(|error| error.to_string())?;
    let mut ogg = Vec::new();
    converter
        .generate_ogg(&mut ogg)
        .map_err(|error| error.to_string())?;
    if ogg.len() > MAX_PCM {
        return Err("Converted audio exceeds the preview budget".into());
    }
    ww2ogg::validate(&ogg).map_err(|error| error.to_string())?;
    Ok(ogg)
}

fn decode_ogg(ogg: &[u8]) -> Result<Vec<u8>, String> {
    let mut decoder = lewton::inside_ogg::OggStreamReader::new(Cursor::new(ogg))
        .map_err(|error| format!("Could not read converted audio: {error}"))?;
    let channels = u16::from(decoder.ident_hdr.audio_channels);
    let sample_rate = decoder.ident_hdr.audio_sample_rate;
    if !(1..=8).contains(&channels) || !(8_000..=192_000).contains(&sample_rate) {
        return Err("Converted audio has invalid channel or sample rate".into());
    }
    let mut pcm = Vec::new();
    while let Some(packet) = decoder
        .read_dec_packet_itl()
        .map_err(|error| format!("Could not decode audio packet: {error}"))?
    {
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
    use super::wave_duration;
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
}
