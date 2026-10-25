//! The RIFF extent and chunk layout shared by clip inspection and PCM playback.

pub(super) struct Wave<'a> {
    pub bytes: &'a [u8],
    pub data: &'a [u8],
    pub codec: u16,
    pub channels: u16,
    pub sample_rate: u32,
    byte_rate: u32,
    block_align: u16,
    bits_per_sample: u16,
}

impl<'a> Wave<'a> {
    pub fn parse(bytes: &'a [u8]) -> Option<Self> {
        // Wwise's Vorbis RIFF reader advances by the exact chunk size without padding.
        // Retry that native layout only for its codec, keeping ordinary PCM RIFF strict.
        Self::read(bytes, true)
            .or_else(|| Self::read(bytes, false).filter(|wave| wave.codec == 0xFFFF))
    }

    fn read(bytes: &'a [u8], padded: bool) -> Option<Self> {
        if bytes.get(..4)? != b"RIFF" || bytes.get(8..12)? != b"WAVE" {
            return None;
        }
        let end =
            8usize.checked_add(u32::from_le_bytes(bytes.get(4..8)?.try_into().ok()?) as usize)?;
        let bytes = bytes.get(..end)?;
        let (format, data) = chunks(bytes, padded)?;
        let wave = Self {
            bytes,
            data,
            codec: u16::from_le_bytes(format[0..2].try_into().ok()?),
            channels: u16::from_le_bytes(format[2..4].try_into().ok()?),
            sample_rate: u32::from_le_bytes(format[4..8].try_into().ok()?),
            byte_rate: u32::from_le_bytes(format[8..12].try_into().ok()?),
            block_align: u16::from_le_bytes(format[12..14].try_into().ok()?),
            bits_per_sample: u16::from_le_bytes(format[14..16].try_into().ok()?),
        };
        if !(1..=8).contains(&wave.channels) || !(8_000..=192_000).contains(&wave.sample_rate) {
            return None;
        }
        if wave.codec == 1 {
            wave.pcm_byte_rate()?;
        }
        Some(wave)
    }

    pub fn pcm_byte_rate(&self) -> Option<u32> {
        if self.codec != 1 || !matches!(self.bits_per_sample, 8 | 16 | 24 | 32) {
            return None;
        }
        let alignment = u32::from(self.channels) * u32::from(self.bits_per_sample) / 8;
        if u32::from(self.block_align) != alignment
            || self.sample_rate.checked_mul(alignment)? != self.byte_rate
            || !self
                .data
                .len()
                .is_multiple_of(usize::from(self.block_align))
        {
            return None;
        }
        Some(self.byte_rate)
    }
}

fn chunks(bytes: &[u8], padded: bool) -> Option<(&[u8], &[u8])> {
    let mut at = 12usize;
    let mut format = None;
    let mut data = None;
    while at < bytes.len() {
        let start = at.checked_add(8)?;
        let header = bytes.get(at..start)?;
        let size = u32::from_le_bytes(header[4..8].try_into().ok()?) as usize;
        let end = start.checked_add(size)?;
        let chunk = bytes.get(start..end)?;
        match &header[..4] {
            b"fmt " => {
                if chunk.len() < 16 || format.replace(chunk).is_some() {
                    return None;
                }
            }
            b"data" if data.replace(chunk).is_some() => {
                return None;
            }
            _ => {}
        }
        at = end.checked_add(if padded { size & 1 } else { 0 })?;
    }
    if at != bytes.len() {
        return None;
    }
    Some((format?, data?))
}
