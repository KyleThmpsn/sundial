//! Decode modern Wwise Opus into the extensible PCM RIFF format accepted by
//! the legacy PCM loader. Playback also requires an authored bank and runtime event.
use anyhow::{Context, Result, bail, ensure};
use std::{
    io::Write,
    process::{Command, Stdio},
};

fn word(bytes: &[u8], at: usize) -> Result<u16> {
    Ok(u16::from_le_bytes(
        bytes
            .get(at..at + 2)
            .context("short WEM word")?
            .try_into()?,
    ))
}

fn dword(bytes: &[u8], at: usize) -> Result<u32> {
    Ok(u32::from_le_bytes(
        bytes
            .get(at..at + 4)
            .context("short WEM dword")?
            .try_into()?,
    ))
}

fn chunks(bytes: &[u8]) -> Result<Vec<(&[u8; 4], &[u8])>> {
    ensure!(
        bytes.get(..4) == Some(b"RIFF") && bytes.get(8..12) == Some(b"WAVE"),
        "Wwise media is not RIFF/WAVE"
    );
    ensure!(
        dword(bytes, 4)? as usize + 8 == bytes.len(),
        "Wwise RIFF size disagrees with payload"
    );
    let mut at = 12;
    let mut result = Vec::new();
    while at < bytes.len() {
        let name: &[u8; 4] = bytes
            .get(at..at + 4)
            .context("truncated WEM chunk")?
            .try_into()?;
        let size = dword(bytes, at + 4)? as usize;
        let start = at.checked_add(8).context("WEM chunk overflow")?;
        let end = start.checked_add(size).context("WEM chunk overflow")?;
        result.push((
            name,
            bytes.get(start..end).context("WEM chunk exceeds media")?,
        ));
        // Some shipped media omit the optional pad byte after the final data chunk.
        at = if end == bytes.len() {
            end
        } else {
            end.checked_add(size & 1).context("WEM chunk overflow")?
        };
    }
    ensure!(at == bytes.len(), "WEM chunk padding exceeds media");
    Ok(result)
}

fn opus_samples(packet: &[u8]) -> Result<u32> {
    let toc = *packet.first().context("empty Opus packet")?;
    let config = toc >> 3;
    let frame = if config < 12 {
        [480, 960, 1920, 2880][(config & 3) as usize]
    } else if config < 16 {
        [480, 960][(config & 1) as usize]
    } else {
        [120, 240, 480, 960][(config & 3) as usize]
    };
    let count = match toc & 3 {
        0 => 1,
        1 | 2 => 2,
        _ => u32::from(*packet.get(1).context("Opus packet lacks frame count")? & 0x3f),
    };
    let duration = frame * count;
    ensure!(
        count > 0 && duration <= 5760,
        "invalid Opus packet duration"
    );
    Ok(duration)
}

fn page(serial: u32, sequence: u32, flags: u8, granule: u64, packet: &[u8]) -> Result<Vec<u8>> {
    let mut lacing = vec![255u8; packet.len() / 255];
    lacing.push((packet.len() % 255) as u8);
    ensure!(
        lacing.len() <= 255,
        "Opus packet needs unsupported continued Ogg pages"
    );
    let mut bytes = Vec::with_capacity(27 + lacing.len() + packet.len());
    bytes.extend_from_slice(b"OggS\0");
    bytes.push(flags);
    bytes.extend_from_slice(&granule.to_le_bytes());
    bytes.extend_from_slice(&serial.to_le_bytes());
    bytes.extend_from_slice(&sequence.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.push(lacing.len() as u8);
    bytes.extend_from_slice(&lacing);
    bytes.extend_from_slice(packet);
    let mut crc = 0u32;
    for byte in &bytes {
        crc ^= u32::from(*byte) << 24;
        for _ in 0..8 {
            crc = if crc & 0x8000_0000 != 0 {
                (crc << 1) ^ 0x04c1_1db7
            } else {
                crc << 1
            };
        }
    }
    bytes[22..26].copy_from_slice(&crc.to_le_bytes());
    Ok(bytes)
}

/// Repackage Wwise Opus packets as a standards-compliant Ogg Opus stream.
/// Wwise's `seek` chunk contains the packet byte lengths in playback order.
pub fn opus_ogg(media: &[u8]) -> Result<Vec<u8>> {
    let chunks = chunks(media)?;
    let get = |id| {
        chunks
            .iter()
            .find(|(name, _)| **name == id)
            .map(|(_, body)| *body)
    };
    let fmt = get(*b"fmt ").context("Wwise Opus format chunk missing")?;
    ensure!(word(fmt, 0)? == 0x3041, "Wwise media is not Opus 0x3041");
    ensure!(fmt.len() >= 36, "short Wwise Opus format chunk");
    let channels = word(fmt, 2)?;
    let mapping = opus_mapping(fmt)?;
    let rate = dword(fmt, 4)?;
    ensure!(rate == 48_000, "unsupported Wwise Opus sample rate");
    let sample_count = dword(fmt, 24)?;
    let pre_skip = word(fmt, 32)?;
    let seek = get(*b"seek").context("Wwise Opus packet index missing")?;
    ensure!(
        !seek.is_empty() && seek.len() % 2 == 0,
        "invalid Wwise Opus packet index"
    );
    ensure!(
        dword(fmt, 28)? as usize == seek.len() / 2,
        "Wwise Opus packet count disagrees with index"
    );
    let data = get(*b"data").context("Wwise Opus packet data missing")?;
    let mut packets = Vec::new();
    let mut at = 0usize;
    let mut total_samples = 0u64;
    for index in (0..seek.len()).step_by(2) {
        let size = word(seek, index)? as usize;
        ensure!(size > 0, "empty indexed Wwise Opus packet");
        let end = at.checked_add(size).context("Opus packet size overflow")?;
        let packet = data
            .get(at..end)
            .context("Opus packet exceeds Wwise media")?;
        total_samples += u64::from(opus_samples(packet)?);
        packets.push(packet);
        at = end;
    }
    ensure!(
        at == data.len(),
        "Wwise Opus packet index does not cover media"
    );
    let final_granule = u64::from(sample_count) + u64::from(pre_skip);
    ensure!(
        final_granule <= total_samples && total_samples - final_granule < 5760,
        "Wwise Opus sample count disagrees with packets"
    );
    let mut head = Vec::with_capacity(19);
    head.extend_from_slice(b"OpusHead");
    head.extend_from_slice(&[1, channels as u8]);
    head.extend_from_slice(&pre_skip.to_le_bytes());
    head.extend_from_slice(&rate.to_le_bytes());
    head.extend_from_slice(&[0, 0]); // Output gain.
    head.extend_from_slice(&mapping);
    let mut tags = Vec::new();
    tags.extend_from_slice(b"OpusTags");
    tags.extend_from_slice(&9u32.to_le_bytes());
    tags.extend_from_slice(b"Parhelion");
    tags.extend_from_slice(&0u32.to_le_bytes());
    let mut out = page(1, 0, 2, 0, &head)?;
    out.extend(page(1, 1, 0, 0, &tags)?);
    let mut granule = 0u64;
    for (index, packet) in packets.iter().enumerate() {
        granule += u64::from(opus_samples(packet)?);
        let last = index + 1 == packets.len();
        out.extend(page(
            1,
            (index + 2) as u32,
            u8::from(last) * 4,
            if last { final_granule } else { granule },
            packet,
        )?);
    }
    Ok(out)
}

/// Wwise 0x3041 stores an implicit stream map, not an OpusHead map.
/// The standard side-quad layout consists of two coupled streams in channel
/// order. Other multichannel layouts need their own validated stream mapping.
/// Format reference: vgmstream/src/meta/wwise.c, OPUSWW ChannelConfigToMapping.
fn opus_mapping(fmt: &[u8]) -> Result<Vec<u8>> {
    let channels = word(fmt, 2)?;
    ensure!(fmt.get(34) == Some(&1), "unsupported Wwise Opus version");
    let config = dword(fmt, 20)?;
    ensure!(
        config & 0xff == u32::from(channels) && (config >> 8) & 0xf == 1,
        "unsupported Wwise Opus channel configuration"
    );
    match (channels, fmt.get(35), config >> 12) {
        (1, Some(0), 4) | (2, Some(0), 3) => Ok(vec![0]),
        // A mono low-frequency stream is written as mapping family 255, but one uncoupled stream
        // carries it, so it decodes exactly as the family 0 mono stream does.
        (1, Some(255), 8) => Ok(vec![0]),
        (4, Some(1), 0x603) => Ok(vec![1, 2, 2, 0, 1, 2, 3]),
        _ => bail!("unsupported Wwise Opus channel mapping"),
    }
}

/// Decode Wwise Opus into 16-bit PCM RIFF media with ffmpeg. The output is not
/// independently playable in Dawn until its legacy bank and event are authored.
pub fn pcm_wem(media: &[u8]) -> Result<Vec<u8>> {
    crate::cancellation::check()?;
    let ogg = opus_ogg(media)?;
    let chunks = chunks(media)?;
    let fmt = chunks
        .iter()
        .find(|(id, _)| **id == *b"fmt ")
        .context("source format missing")?
        .1;
    let channels = word(fmt, 2)?;
    let rate = dword(fmt, 4)?;
    let mut command = Command::new("ffmpeg");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut process = command
        .args([
            "-v",
            "error",
            "-nostdin",
            "-i",
            "pipe:0",
            "-c:a",
            "pcm_s16le",
            "-f",
            "s16le",
            "pipe:1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("launch ffmpeg for Wwise Opus decode")?;
    let mut input = process.stdin.take().context("ffmpeg stdin missing")?;
    // Large cues can fill stdout while ffmpeg is still reading stdin. Feed
    // input concurrently with draining both output pipes to avoid a deadlock.
    let (output, written) = std::thread::scope(|scope| {
        let writer = scope.spawn(move || input.write_all(&ogg));
        (
            crate::cancellation::wait_with_output(process),
            writer.join(),
        )
    });
    let output = output?;
    if !output.status.success() {
        bail!(
            "ffmpeg could not decode Wwise Opus: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    written
        .map_err(|_| anyhow::anyhow!("ffmpeg input writer panicked"))?
        .context("write Wwise Opus to ffmpeg")?;
    let data = &output.stdout;
    ensure!(
        !data.is_empty() && data.len() % (usize::from(channels) * 2) == 0,
        "decoded PCM samples are incomplete"
    );
    ensure!(
        data.len() / (usize::from(channels) * 2) == dword(fmt, 24)? as usize,
        "decoded PCM frame count disagrees with Wwise source"
    );
    riff_pcm_layout(channels, rate, dword(fmt, 20)? >> 12, data)
}

const PCM_SUBFORMAT: [u8; 16] = [
    1, 0, 0, 0, 0, 0, 0x10, 0, 0x80, 0, 0, 0xAA, 0, 0x38, 0x9B, 0x71,
];

/// The native PCM loaders require format 0xFFFE and read the channel mask at
/// fmt +0x14. A conventional format-1 WAV is rejected before any samples play.
/// Preserve the samples in older prepared graphs while upgrading their header.
pub fn normalize_pcm_wem(media: &[u8]) -> Result<Vec<u8>> {
    let chunks = chunks(media)?;
    let fields = |id| {
        chunks
            .iter()
            .filter(|(name, _)| **name == id)
            .map(|(_, data)| *data)
            .collect::<Vec<_>>()
    };
    let formats = fields(*b"fmt ");
    let samples = fields(*b"data");
    ensure!(
        formats.len() == 1 && samples.len() == 1,
        "PCM media needs one format and sample chunk"
    );
    let fmt = formats[0];
    let channels = word(fmt, 2)?;
    let rate = dword(fmt, 4)?;
    ensure!(
        matches!(channels, 1 | 2 | 4) && (8_000..=192_000).contains(&rate),
        "unsupported PCM channel count or sample rate"
    );
    ensure!(
        word(fmt, 14)? == 16
            && word(fmt, 12)? == channels * 2
            && dword(fmt, 8)? == rate * u32::from(channels) * 2,
        "invalid 16-bit PCM sample layout"
    );
    let mask = match word(fmt, 0)? {
        1 => {
            ensure!(
                matches!(channels, 1 | 2)
                    && (fmt.len() == 16 || (fmt.len() == 18 && word(fmt, 16)? == 0)),
                "unsupported PCM format extension"
            );
            channel_mask(channels)
        }
        0xFFFE => {
            ensure!(
                fmt.len() == 40
                    && word(fmt, 16)? == 22
                    && word(fmt, 18)? == 16
                    && fmt[24..40] == PCM_SUBFORMAT,
                "unsupported extensible PCM format"
            );
            dword(fmt, 20)?
        }
        _ => bail!("media is not PCM"),
    };
    ensure!(
        chunks
            .iter()
            .all(|(id, _)| matches!(*id, b"fmt " | b"data" | b"JUNK" | b"LIST")),
        "PCM media has unsupported playback metadata"
    );
    riff_pcm_layout(channels, rate, mask, samples[0])
}

fn channel_mask(channels: u16) -> u32 {
    if channels == 1 { 4 } else { 3 }
}

/// A 16-bit WAVEFORMATEXTENSIBLE PCM WEM.
fn riff_pcm(channels: u16, rate: u32, data: &[u8]) -> Result<Vec<u8>> {
    riff_pcm_layout(channels, rate, channel_mask(channels), data)
}

fn riff_pcm_layout(channels: u16, rate: u32, mask: u32, data: &[u8]) -> Result<Vec<u8>> {
    ensure!(
        // Mono center, mono low frequency, stereo and side quad.
        matches!((channels, mask), (1, 4) | (1, 8) | (2, 3) | (4, 0x603))
            && !data.is_empty()
            && data.len().is_multiple_of(usize::from(channels) * 2),
        "unsupported PCM speaker layout or incomplete samples"
    );
    let mut wem = Vec::with_capacity(68 + data.len());
    wem.extend_from_slice(b"RIFF");
    wem.extend_from_slice(
        &(60u32
            .checked_add(u32::try_from(data.len())?)
            .context("PCM WEM too large")?)
        .to_le_bytes(),
    );
    wem.extend_from_slice(b"WAVEfmt ");
    wem.extend_from_slice(&40u32.to_le_bytes());
    wem.extend_from_slice(&0xFFFEu16.to_le_bytes());
    wem.extend_from_slice(&channels.to_le_bytes());
    wem.extend_from_slice(&rate.to_le_bytes());
    wem.extend_from_slice(&(rate * u32::from(channels) * 2).to_le_bytes());
    wem.extend_from_slice(&(channels * 2).to_le_bytes());
    wem.extend_from_slice(&16u16.to_le_bytes());
    wem.extend_from_slice(&22u16.to_le_bytes());
    wem.extend_from_slice(&16u16.to_le_bytes());
    wem.extend_from_slice(&mask.to_le_bytes());
    wem.extend_from_slice(&PCM_SUBFORMAT);
    wem.extend_from_slice(b"data");
    wem.extend_from_slice(&u32::try_from(data.len())?.to_le_bytes());
    wem.extend_from_slice(data);
    ensure!(wem.len() == 68 + data.len(), "PCM WEM length mismatch");
    Ok(wem)
}

/// Mix PCM WEMs that play together into one, as Wwise sums simultaneous layers.
///
/// Layers start together and the mix lasts as long as the longest. A mono layer is spread
/// across every channel. Layers must share a sample rate, since nothing here resamples.
pub fn mix_pcm(layers: &[Vec<u8>]) -> Result<Vec<u8>> {
    ensure!(!layers.is_empty(), "nothing to mix");
    let normalized = layers
        .iter()
        .map(|layer| normalize_pcm_wem(layer))
        .collect::<Result<Vec<_>>>()?;
    let mut decoded = Vec::new();
    for layer in &normalized {
        let chunks = chunks(layer)?;
        let fmt = chunks
            .iter()
            .find(|(id, _)| **id == *b"fmt ")
            .context("PCM format missing")?
            .1;
        let data = chunks
            .iter()
            .find(|(id, _)| **id == *b"data")
            .context("PCM data missing")?
            .1;
        ensure!(
            word(fmt, 0)? == 0xFFFE && word(fmt, 14)? == 16 && matches!(word(fmt, 2)?, 1 | 2),
            "only mono or stereo 16-bit PCM layers mix"
        );
        decoded.push((word(fmt, 2)?, dword(fmt, 4)?, data));
    }
    let rate = decoded[0].1;
    ensure!(
        decoded.iter().all(|(_, other, _)| *other == rate),
        "layers differ in sample rate"
    );
    let channels = decoded
        .iter()
        .map(|(channels, _, _)| *channels)
        .max()
        .unwrap_or(1);
    ensure!(
        decoded
            .iter()
            .all(|(layer, _, _)| *layer == channels || *layer == 1),
        "layers differ in channel layout"
    );
    let frames = decoded
        .iter()
        .map(|(layer, _, data)| data.len() / (usize::from(*layer) * 2))
        .max()
        .unwrap_or(0);
    let width = usize::from(channels);
    let mut sum = vec![0i32; frames * width];
    for (layer, _, data) in &decoded {
        let layer = usize::from(*layer);
        for (index, sample) in data.chunks_exact(2).enumerate() {
            let value = i32::from(i16::from_le_bytes([sample[0], sample[1]]));
            let frame = index / layer;
            if layer == width {
                sum[frame * width + index % layer] += value;
            } else {
                for channel in 0..width {
                    sum[frame * width + channel] += value;
                }
            }
        }
    }
    let data = sum
        .into_iter()
        .flat_map(|value| {
            (value.clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16).to_le_bytes()
        })
        .collect::<Vec<_>>();
    riff_pcm(channels, rate, &data)
}

/// Adapt a Dawn-version Vorbis bank template for PCM media while preserving
/// its HIRC object graph. The caller must assign private bank and media IDs.
pub fn pcm_bank_template(bank: &[u8]) -> Result<Vec<u8>> {
    ensure!(bank.get(..4) == Some(b"BKHD"), "legacy bank lacks BKHD");
    ensure!(
        dword(bank, 8)? == 113,
        "bank is not Dawn's Wwise version 113"
    );
    let mut copy = bank.to_vec();
    let mut at = 0usize;
    let mut changed = 0usize;
    while at < copy.len() {
        let name = copy
            .get(at..at + 4)
            .context("short bank section name")?
            .to_vec();
        let size = dword(&copy, at + 4)? as usize;
        let begin = at.checked_add(8).context("bank section overflow")?;
        let end = begin.checked_add(size).context("bank section overflow")?;
        ensure!(end <= copy.len(), "bank section exceeds payload");
        if name == b"HIRC" {
            let count = dword(&copy, begin)? as usize;
            let mut object = begin + 4;
            for _ in 0..count {
                let kind = *copy.get(object).context("short bank HIRC object")?;
                let length = dword(&copy, object + 1)? as usize;
                let next = object
                    .checked_add(5)
                    .and_then(|start| start.checked_add(length))
                    .context("bank HIRC object overflow")?;
                ensure!(next <= end, "bank HIRC object exceeds section");
                if kind == 2 {
                    ensure!(length >= 8, "short Wwise sound source object");
                    let plugin = dword(&copy, object + 9)?;
                    ensure!(
                        plugin == 0x0004_0001,
                        "legacy sound source does not use Vorbis"
                    );
                    copy[object + 9..object + 13].copy_from_slice(&0x0001_0001u32.to_le_bytes());
                    changed += 1;
                }
                object = next;
            }
            ensure!(
                object == end,
                "bank HIRC object count disagrees with section size"
            );
        }
        at = end;
    }
    ensure!(changed > 0, "legacy bank has no Vorbis source to adapt");
    Ok(copy)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_pcm_upgrade_preserves_samples_and_rejects_mislabelled_audio() {
        // A conventional WAV from an older prepared graph. Its PCM samples must
        // survive the container upgrade without another lossy decode or mix.
        let samples = [1000i16, -500, i16::MIN, i16::MAX]
            .map(i16::to_le_bytes)
            .concat();
        let mut wav = b"RIFF\x2c\0\0\0WAVEfmt \x10\0\0\0\x01\0\x02\0\x80\xbb\0\0\0\xee\x02\0\x04\0\x10\0data\x08\0\0\0".to_vec();
        wav.extend_from_slice(&samples);
        let upgraded = normalize_pcm_wem(&wav).unwrap();
        let parsed = chunks(&upgraded).unwrap();
        assert_eq!(parsed[1].1, samples);
        assert_eq!(word(parsed[0].1, 0).unwrap(), 0xFFFE);
        assert_eq!(dword(parsed[0].1, 20).unwrap(), 3);
        assert_eq!(normalize_pcm_wem(&upgraded).unwrap(), upgraded);
        let mut compressed = wav.clone();
        compressed[20..22].copy_from_slice(&2u16.to_le_bytes());
        assert!(normalize_pcm_wem(&compressed).is_err());
        wav[32..34].copy_from_slice(&2u16.to_le_bytes());
        assert!(normalize_pcm_wem(&wav).is_err());
        let mut float = upgraded;
        float[44] = 3;
        assert!(normalize_pcm_wem(&float).is_err());
    }

    #[test]
    fn layers_sum_with_clamping_and_mono_spread() {
        let stereo = riff_pcm(
            2,
            48_000,
            &[1000i16, -1000, 30_000, 0].map(i16::to_le_bytes).concat(),
        )
        .unwrap();
        let mono = riff_pcm(
            1,
            48_000,
            &[500i16, 10_000, 7].map(i16::to_le_bytes).concat(),
        )
        .unwrap();
        let mixed = mix_pcm(&[stereo, mono]).unwrap();
        let chunks = chunks(&mixed).unwrap();
        assert_eq!(word(chunks[0].1, 2).unwrap(), 2);
        let samples = chunks[1]
            .1
            .chunks_exact(2)
            .map(|pair| i16::from_le_bytes([pair[0], pair[1]]))
            .collect::<Vec<_>>();
        assert_eq!(samples, [1500, -500, i16::MAX, 10_000, 7, 7]);
        let other_rate = riff_pcm(1, 44_100, &[0i16].map(i16::to_le_bytes).concat()).unwrap();
        assert!(mix_pcm(&[mixed, other_rate]).is_err());
    }

    #[test]
    fn accepts_unpadded_final_data_chunk() {
        let mut wem = b"RIFF\0\0\0\0WAVEdata\x01\0\0\0\x7f".to_vec();
        let size = (wem.len() - 8) as u32;
        wem[4..8].copy_from_slice(&size.to_le_bytes());
        assert_eq!(chunks(&wem).unwrap()[0].1, [0x7f]);
    }

    #[test]
    #[ignore = "Requires explicitly configured modern Wwise Opus media"]
    fn configured_modern_media_decodes_to_pcm_wem() {
        let path = std::env::var_os("PARHELION_AUDIO_OPUS_WEM").expect("configured modern WEM");
        let media = std::fs::read(path).unwrap();
        let pcm = pcm_wem(&media).unwrap();
        let source = chunks(&media).unwrap();
        let source_fmt = source.iter().find(|(id, _)| **id == *b"fmt ").unwrap().1;
        let chunks = chunks(&pcm).unwrap();
        assert_eq!(word(chunks[0].1, 0).unwrap(), 0xFFFE);
        assert_eq!(
            chunks[1].1.len(),
            dword(source_fmt, 24).unwrap() as usize * word(source_fmt, 2).unwrap() as usize * 2
        );
    }
}
