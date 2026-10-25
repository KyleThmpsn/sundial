//! Checked mip-major arrays and volumes, packed into a bounded atlas for both renderers.
use super::*;
mod sample;
pub(super) use sample::GLSL;

#[derive(Clone, Copy, Debug)]
pub(in crate::model_preview) struct Layered {
    pub size: [usize; 3],
    pub levels: usize,
    pub columns: usize,
    pub volume: bool,
}

impl Layered {
    pub(super) fn load(
        manager: &PackageManager,
        tag: u32,
        volume: bool,
    ) -> Result<(Self, texture::Texture), String> {
        let entry = manager
            .get_entry(tag)
            .ok_or("Layered texture header is missing")?;
        if entry.file_type != 32 || !matches!(entry.file_subtype, 1..=3) {
            return Err("Unsupported layered texture header type".into());
        }
        let header = manager.read_tag(tag)?;
        let width = usize::from(u16_at(&header, 14)?);
        let height = usize::from(u16_at(&header, 16)?);
        let depth = usize::from(u16_at(&header, 18)?);
        let layers = usize::from(u16_at(&header, 20)?);
        let levels = usize::from(*header.get(23).ok_or("Truncated layered texture header")?);
        let size = [width, height, if volume { depth } else { layers }];
        if size.contains(&0)
            || width > 4096
            || height > 4096
            || size[2] > 2048
            || if volume { layers != 1 } else { depth != 1 }
            || levels == 0
            || levels
                > width
                    .max(height)
                    .max(if volume { depth } else { 1 })
                    .ilog2() as usize
                    + 1
        {
            return Err("Unsupported layered texture dimensions".into());
        }
        let image = Self {
            size,
            levels,
            columns: (size[2] as f32).sqrt().ceil() as usize,
            volume,
        };
        let atlas = [
            width * image.columns,
            (0..levels)
                .map(|i| {
                    let size = image.level(i);
                    size[1] * size[2].div_ceil(image.columns)
                })
                .sum(),
        ];
        if atlas[0] > 8192 || atlas[1] > 8192 || atlas[0] * atlas[1] > 4096 * 4096 {
            return Err("Layered texture exceeds the preview pixel budget".into());
        }
        let large = u32_at(&header, 36)?;
        let payload = if matches!(large, 0 | u32::MAX) {
            entry.reference
        } else {
            large
        };
        let mut bytes = Vec::new();
        for source in
            std::iter::once(payload).chain((payload != entry.reference).then_some(entry.reference))
        {
            let part = manager
                .get_entry(source)
                .ok_or("Layered texture payload is missing")?;
            if bytes.len().saturating_add(part.file_size as usize) > 128 * 1024 * 1024 {
                return Err("Layered texture exceeds the preview payload budget".into());
            }
            bytes.extend(manager.read_tag(source)?);
        }
        let format = u32_at(&header, 4)?;
        let mut rgba = vec![0; atlas[0] * atlas[1] * 4];
        let mut linear = matches!(format, 10 | 26).then(|| vec![[0.0; 4]; atlas[0] * atlas[1]]);
        let mut offset = 0usize;
        for level in 0..levels {
            let size = image.level(level);
            let length = texture::surface_length(format, [size[0], size[1]])?;
            for slice in 0..size[2] {
                let data = bytes
                    .get(offset..offset + length)
                    .ok_or("Layered texture mip or slice is truncated")?;
                let pixels = texture::decode(data, format, size[0], size[1])?;
                let floats = texture::float::decode(data, format, size[0], size[1])?;
                let origin = image.origin(level, slice);
                for y in 0..size[1] {
                    let to = (origin[1] + y) * atlas[0] + origin[0];
                    let from = y * size[0];
                    rgba[to * 4..(to + size[0]) * 4]
                        .copy_from_slice(&pixels[from * 4..(from + size[0]) * 4]);
                    if let (Some(atlas), Some(pixels)) = (&mut linear, &floats) {
                        atlas[to..to + size[0]].copy_from_slice(&pixels[from..from + size[0]]);
                    }
                }
                offset += length;
            }
        }
        Ok((
            image,
            texture::Texture {
                tag,
                size: atlas,
                rgba,
                linear,
                mips: Some(Vec::new()),
            },
        ))
    }

    pub(super) fn level(self, mip: usize) -> [usize; 3] {
        std::array::from_fn(|axis| {
            if axis == 2 && !self.volume {
                self.size[axis]
            } else {
                (self.size[axis] >> mip).max(1)
            }
        })
    }

    fn origin(self, mip: usize, slice: usize) -> [usize; 2] {
        let top: usize = (0..mip)
            .map(|i| {
                let size = self.level(i);
                size[1] * size[2].div_ceil(self.columns)
            })
            .sum();
        [
            slice % self.columns * self.size[0],
            top + slice / self.columns * self.level(mip)[1],
        ]
    }

    pub(super) fn dimensions(self, mip: u32) -> [u32; 4] {
        let size = if mip < self.levels as u32 {
            self.level(mip as usize)
        } else {
            [0, 0, if self.volume { 0 } else { self.size[2] }]
        };
        [
            size[0] as u32,
            size[1] as u32,
            size[2] as u32,
            self.levels as u32,
        ]
    }

    pub(super) fn load_texel(
        self,
        texture: &texture::Texture,
        position: [i32; 4],
        offset: [i32; 3],
        color: bool,
    ) -> [f32; 4] {
        let level = position[3];
        if level < 0 || level as usize >= self.levels {
            return [0.0; 4];
        }
        let p = std::array::from_fn(|i| {
            position[i].saturating_add(if i < 2 || self.volume { offset[i] } else { 0 })
        });
        self.texel(texture, level as usize, p, color)
            .unwrap_or([0.0; 4])
    }

    fn texel(
        self,
        texture: &texture::Texture,
        level: usize,
        p: [i32; 3],
        color: bool,
    ) -> Option<[f32; 4]> {
        let size = self.level(level);
        if (0..3).any(|i| p[i] < 0 || p[i] as usize >= size[i]) {
            return None;
        }
        let origin = self.origin(level, p[2] as usize);
        Some(texel(
            texture,
            [origin[0] + p[0] as usize, origin[1] + p[1] as usize],
            color,
        ))
    }
}

pub(super) fn texel(texture: &texture::Texture, p: [usize; 2], color: bool) -> [f32; 4] {
    let index = p[1] * texture.size[0] + p[0];
    if let Some(pixels) = &texture.linear {
        return pixels[index];
    }
    std::array::from_fn(|lane| {
        let value = f32::from(texture.rgba[index * 4 + lane]) / 255.0;
        if color && lane < 3 {
            shader::linear(value)
        } else {
            value
        }
    })
}
