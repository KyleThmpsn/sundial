//! Exact numeric top-mip loads. Color and metadata tables never use preview mip selection.
use super::*;

pub(super) struct Table {
    width: usize,
    height: usize,
    channels: usize,
    stride: usize,
    normalized: bool,
    bytes: Vec<u8>,
}

impl Table {
    pub fn load(manager: &PackageManager, tag: u32, integer: bool) -> Result<Self, String> {
        let entry = manager
            .get_entry(tag)
            .ok_or("The stored attribute texture header is missing")?;
        if entry.file_type != 32 || !matches!(entry.file_subtype, 1..=3) {
            return Err("The stored attribute texture header type is unavailable".into());
        }
        let header = manager.read_tag(tag)?;
        let width = usize::from(u16_at(&header, 14)?);
        let height = usize::from(u16_at(&header, 16)?);
        if width == 0
            || height == 0
            || width > 8192
            || height > 8192
            || width * height > 1024 * 1024
            || u16_at(&header, 18)? != 1
            || u16_at(&header, 20)? != 1
        {
            return Err("The stored attribute texture exceeds the numeric table budget".into());
        }
        let format = u32_at(&header, 4)?;
        let (channels, stride, normalized) = match (format, integer) {
            (30, true) => (4, 1, false),
            (42, true) => (1, 4, false),
            (3, true) => (4, 4, false),
            (17, true) => (2, 4, false),
            (28, false) => (4, 1, true),
            _ => {
                return Err(format!(
                    "The stored attribute texture format {format} is unavailable"
                ));
            }
        };
        let large = u32_at(&header, 36)?;
        let payload = if matches!(large, 0 | u32::MAX) {
            entry.reference
        } else {
            large
        };
        let data_entry = manager
            .get_entry(payload)
            .ok_or("The stored attribute texture pixels are missing")?;
        if data_entry.file_size > 32 * 1024 * 1024 {
            return Err("The stored attribute texture payload exceeds limits".into());
        }
        let bytes = manager.read_tag(payload)?;
        let bytes = bytes
            .get(..width * height * channels * stride)
            .ok_or("The stored attribute texture pixels are truncated")?
            .to_vec();
        Ok(Self {
            width,
            height,
            channels,
            stride,
            normalized,
            bytes,
        })
    }

    pub fn texel(&self, coordinates: [i32; 4], offset: [i32; 3]) -> Result<[u32; 4], String> {
        let x = i64::from(coordinates[0]) + i64::from(offset[0]);
        let y = i64::from(coordinates[1]) + i64::from(offset[1]);
        if coordinates[3] != 0 {
            return Err("The stored attribute texture mip is unavailable".into());
        }
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            // Native SM5 ld returns zero when any coordinate is out of bounds.
            let mut value = [0; 4];
            if self.channels < 4 {
                value[3] = 1;
            }
            return Ok(value);
        }
        let mut values = [0u32; 4];
        if self.channels < 4 {
            values[3] = 1;
        }
        let start = (y as usize * self.width + x as usize) * self.channels * self.stride;
        for (lane, value) in values.iter_mut().enumerate().take(self.channels) {
            let at = start + lane * self.stride;
            *value = if self.stride == 1 {
                let byte = self.bytes[at];
                if self.normalized {
                    (f32::from(byte) / 255.0).to_bits()
                } else {
                    u32::from(byte)
                }
            } else {
                u32_at(&self.bytes, at)?
            };
        }
        Ok(values)
    }
}
