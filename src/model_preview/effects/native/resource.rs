//! Plan both shader stages' bindings before retaining their decoded images.
use super::*;

/// Resource tables may interleave textures and samplers. Keep the stored row
/// identities while packing only decoded samplers into the renderer's array.
pub(super) fn samplers(
    manager: &PackageManager,
    bytes: &[u8],
    stage: usize,
) -> Result<
    (
        Vec<texture::Sampler>,
        std::collections::BTreeMap<usize, usize>,
    ),
    String,
> {
    let (count, rows) = super::super::vertex::table(bytes, stage + 0x40, 0x8080_73F3, 16, 256)?;
    let mut values = Vec::new();
    let mut indices = std::collections::BTreeMap::new();
    for row in 0..count {
        if let Some(value) = texture::sampler(manager, u32_at(bytes, rows + row * 16)?) {
            indices.insert(row, values.len());
            values.push(value);
        }
    }
    Ok((values, indices))
}

pub(super) struct Plan<'a> {
    manager: &'a PackageManager,
    model: &'a mut Model,
    pub bindings: Vec<Binding>,
    pub pending: Vec<texture::Texture>,
}

impl<'a> Plan<'a> {
    pub fn finish(self) -> (Vec<Binding>, Vec<texture::Texture>) {
        (self.bindings, self.pending)
    }

    pub fn new(manager: &'a PackageManager, model: &'a mut Model) -> Self {
        Self {
            manager,
            model,
            bindings: Vec::new(),
            pending: Vec::new(),
        }
    }

    /// `expression` is the stage's material expression, whose sampler bindings name the table
    /// row each sampler register takes.
    pub fn stage(
        &mut self,
        bytes: &[u8],
        code: &program::Program,
        expression: Option<&Program>,
        samplers: &std::collections::BTreeMap<usize, usize>,
        vertex: bool,
        sampler_offset: usize,
    ) -> Result<(), String> {
        let explicit = explicit(bytes, if vertex { 0x50 } else { 0x2D0 })?;
        let dye = code
            .buffers
            .iter()
            .find(|(slot, _)| (5..=7).contains(slot))
            .map(|(slot, _)| *slot);
        for resource in &code.resources {
            let sampler_rows: std::collections::BTreeMap<_, _> =
                sampling_for(code, expression, resource, samplers)?
                    .into_iter()
                    .map(|(register, row)| (register, row + sampler_offset))
                    .collect();
            let mut binding = if let Some(&tag) = explicit.get(&resource.slot) {
                self.image(tag, resource)?
            } else if vertex {
                return Err(format!(
                    "The vertex texture slot {} has no explicit binding",
                    resource.slot
                ));
            } else {
                implicit(code, resource, dye, expression)?
            };
            binding.sampler = sampler_rows.values().next().copied();
            binding.sampler_rows = sampler_rows;
            binding.vertex = vertex;
            self.bindings.push(binding);
        }
        if self.bindings.len() > 9 {
            return Err("The effect texture count exceeds preview limits".into());
        }
        Ok(())
    }

    fn image(&mut self, tag: u32, resource: &program::Resource) -> Result<Binding, String> {
        if resource.integer {
            return Err("Integer effect images are not supported".into());
        }
        let header = self.manager.read_tag(tag)?;
        let color = matches!(u32_at(&header, 4)?, 29 | 72 | 75 | 78 | 91 | 93 | 99);
        let (cube, layered, image) = match resource.dimension {
            6 => {
                let (cube, image) = cube::Cube::load(self.manager, tag)?;
                (Some(cube), None, image)
            }
            5 | 8 => {
                let (layered, image) =
                    layered::Layered::load(self.manager, tag, resource.dimension == 5)?;
                (None, Some(layered), image)
            }
            _ => (None, None, texture::load(self.manager, tag)?),
        };
        if cube.is_none() && layered.is_none() {
            let source = [
                usize::from(u16_at(&header, 14)?),
                usize::from(u16_at(&header, 16)?),
            ];
            if image.size != source {
                self.model.notices.push(format!("Texture 0x{tag:08X} uses a {} × {} mip instead of {} × {} to fit the preview memory budget.", image.size[0], image.size[1], source[0], source[1]));
            }
        }
        let index = if let Some(index) =
            self.model
                .textures
                .iter()
                .chain(&self.pending)
                .position(|t| {
                    t.tag == tag
                        && t.size == image.size
                        && t.mips.as_ref().map(Vec::len) == image.mips.as_ref().map(Vec::len)
                }) {
            index
        } else {
            if self.model.textures.len() + self.pending.len() >= MAX_TEXTURES {
                return Err("The preview texture budget is full".into());
            }
            self.pending.push(image);
            texture::check_pending(self.model, &self.pending)?;
            self.model.textures.len() + self.pending.len() - 1
        };
        Ok(Binding {
            slot: resource.slot,
            role: Role::Texture(index),
            color,
            cube,
            layered,
            sampler: None,
            sampler_rows: Default::default(),
            vertex: false,
        })
    }
}

pub(super) fn explicit(
    bytes: &[u8],
    descriptor: usize,
) -> Result<std::collections::BTreeMap<usize, u32>, String> {
    let (count, rows) = super::super::vertex::table(bytes, descriptor, 0x8080_7211, 8, 32)?;
    let mut explicit = std::collections::BTreeMap::new();
    for row in (0..count).map(|i| rows + i * 8) {
        if explicit
            .insert(u32_at(bytes, row)? as usize, u32_at(bytes, row + 4)?)
            .is_some()
        {
            return Err(
                "The effect binds a texture slot more than once in one shader stage".into(),
            );
        }
    }
    Ok(explicit)
}

fn implicit(
    code: &program::Program,
    resource: &program::Resource,
    dye: Option<usize>,
    expression: Option<&Program>,
) -> Result<Binding, String> {
    let role = if resource.dimension == 3
        && !resource.integer
        && expression.is_some_and(|program| program.scene_normal(resource.slot))
    {
        Role::SceneNormal
    } else if resource.dimension == 3
        && !resource.integer
        && expression.is_some_and(|program| program.sky_hemisphere(resource.slot))
    {
        Role::Sky
    } else if (12..=15).contains(&resource.slot)
        && resource.dimension == 3
        && code
            .buffers
            .iter()
            .any(|&(slot, count)| slot == 8 && (5..=8).contains(&count))
    {
        if resource.slot == 15 {
            Role::SunVisibility
        } else {
            Role::Irradiance
        }
    } else if matches!(
        (resource.slot, resource.dimension),
        (15 | 16 | 18, 3) | (17, 5)
    ) && code
        .buffers
        .iter()
        .any(|&(slot, count)| slot == 12 && matches!(count, 13 | 14))
        && code
            .buffers
            .iter()
            .any(|&(slot, count)| slot == 13 && matches!(count, 1 | 2))
    {
        if resource.slot == 18 {
            Role::SceneColor
        } else {
            Role::Scene
        }
    } else {
        if resource.dimension != 3 {
            return Err("The effect texture has no explicit binding".into());
        }
        let detail = dye.map(|slot| 3 + (7 - slot) * 2);
        match resource.slot {
            3 if resource.integer => Role::Mask,
            0 => Role::Albedo,
            1 => Role::Normal,
            2 => Role::Gear,
            10 => Role::Depth,
            slot if Some(slot) == detail => Role::Detail,
            slot if Some(slot) == detail.map(|v| v + 1) => Role::DetailNormal,
            _ => {
                return Err(format!(
                    "The effect texture slot {} has no preview binding",
                    resource.slot
                ));
            }
        }
    };
    Ok(Binding {
        slot: resource.slot,
        role,
        color: matches!(role, Role::Albedo | Role::Detail),
        cube: None,
        layered: None,
        sampler: None,
        sampler_rows: Default::default(),
        vertex: false,
    })
}
