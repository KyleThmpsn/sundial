use super::super::expression;
use super::*;
use crate::tiger::shader::identity::Stage;

/// Stock sight materials `80BA7DA2` and `80EF364B`, whose reticle quads match the D1
/// quad's dimensions, subtract this from the quad's z before the reticle projection.
const NATIVE_SIGHT_DEPTH: f32 = 2.5;

pub(super) struct Equipment {
    pub plates: [String; 3],
    pub dyes: BTreeMap<u8, u32>,
}

pub(super) struct Draw {
    pub stage: usize,
    pub dye: u8,
    pub simulated: bool,
    pub layout: u16,
}

fn vectors(data: &Payload, field: usize) -> Result<Vec<u8>> {
    Ok(data
        .array(field, 16, Some(0x80800090))?
        .into_iter()
        .flat_map(|at| data.0[at..at + 16].iter().copied())
        .collect())
}

struct StageInputs {
    textures: BTreeMap<u32, String>,
    samplers: Vec<String>,
    values: Vec<u8>,
    code: Vec<u8>,
    constants: Vec<u8>,
}

fn scope_masks(
    source: &Payload,
    globals: &expression::Globals,
    simulated: bool,
    pass: usize,
    out: &mut [u8],
) -> Result<[u32; 2]> {
    let mut masks = [0u32; 2];
    for (index, at) in [0x18, 0x1C].into_iter().enumerate() {
        let mut mask = globals.scope_mask(source.u32(at)?)?;
        mask &= !0x84;
        mask |= if simulated { 4 } else { 0x80 };
        // Every stock Shadowkeep sight material (18 found by `scan-materials 3f490c47`,
        // such as `80BA7DA2`) requests the gear dye scope (26) at +0x18 and provides the
        // plated-texture and dye scopes (25, 26) at +0x1C, which the gear draw callback keys
        // on. D1 has no scope of those names, so the mapped mask never set them.
        if pass == 16 {
            mask |= if index == 0 {
                1 << 26
            } else {
                (1 << 25) | (1 << 26)
            };
        }
        masks[index] = mask;
        put(
            out,
            at,
            &(mask | if index == 1 { 0x80000000 } else { 0 }).to_le_bytes(),
        )?;
    }
    Ok(masks)
}

impl Assembly<'_> {
    fn detail_textures(&mut self, dye: Option<&Payload>) -> Result<[String; 2]> {
        let mut detail = [String::new(), String::new()];
        if let Some(data) = dye {
            for (index, symbol) in detail.iter_mut().enumerate() {
                let tag = data.u32(0x60 + index * 4)?;
                if !matches!(tag, 0 | u32::MAX) {
                    *symbol = self.texture(tag)?;
                }
            }
        }
        Ok(detail)
    }

    fn material_inputs(&mut self, source: &Payload, from: usize) -> Result<StageInputs> {
        let mut textures = BTreeMap::new();
        for row in source.array(from + 0x10, 8, Some(0x80801B28))? {
            let tag = source.u32(row + 4)?;
            if !matches!(tag, 0 | u32::MAX) {
                textures.insert(source.u32(row)?, self.texture(tag)?);
            }
        }
        let mut samplers = Vec::new();
        for row in source.array(from + 0x48, 16, Some(0x80801ACC))? {
            samplers.push(self.sampler(source.u32(row)?)?);
        }
        let external = source.u32(from + 0x84)?;
        let values = if matches!(external, 0 | u32::MAX) {
            vectors(source, from + 0x58)?
        } else {
            self.source.constant_buffer(external)?.1
        };
        let code = source
            .array(from + 0x28, 1, Some(0x80800009))?
            .into_iter()
            .map(|at| source.u8(at))
            .collect::<Result<Vec<_>>>()?;
        let constants = vectors(source, from + 0x38)?;
        Ok(StageInputs {
            textures,
            samplers,
            values,
            code,
            constants,
        })
    }

    pub(super) fn material(
        &mut self,
        tag: u32,
        symbol: &str,
        draw: Draw,
        equipment: &Equipment,
    ) -> Result<String> {
        if self.symbols.contains(symbol) {
            return Ok(symbol.to_owned());
        }
        let Draw {
            stage: pass,
            dye,
            simulated,
            layout,
        } = draw;
        ensure!(
            self.source.reference(tag)? == 0x80801C32,
            "Source material class differs"
        );
        let source = self.source.read(tag)?;
        let dye_tag = equipment.dyes.get(&(dye / 2)).copied();
        let dye_data = dye_tag.map(|tag| self.source.read(tag)).transpose()?;
        let detail = self.detail_textures(dye_data.as_ref())?;
        let mut out = vec![0; 0x408];
        put(&mut out, 8, &source.bytes::<32>(8)?)?;
        // Platform pipeline cache bits are not authored D3D state selectors.
        put(
            &mut out,
            0xC,
            &(source.u32(0xC)? & 0x00FF_FFFF).to_le_bytes(),
        )?;
        out[0x27] = 0;
        put(&mut out, 0x28, &u32::MAX.to_le_bytes())?;
        let masks = scope_masks(&source, &self.globals, simulated, pass, &mut out)?;
        let depth = (source.u32(0x1C)? & (1 << self.globals.depth.index) != 0)
            .then_some(self.globals.depth);
        let mut patches = Vec::new();
        let mut evidence = Vec::new();
        for (index, (from, to)) in [0x28, 0xC8, 0x168, 0x208, 0x2A8]
            .into_iter()
            .zip([0x48, 0xE8, 0x188, 0x228, 0x2C8])
            .enumerate()
        {
            put(&mut out, to, &u32::MAX.to_le_bytes())?;
            put(&mut out, to + 0x80, &u32::MAX.to_le_bytes())?;
            put(&mut out, to + 0x84, &u32::MAX.to_le_bytes())?;
            let shader = source.u32(from)?;
            if matches!(shader, 0 | u32::MAX) {
                continue;
            }
            let (stage, packed) = match index {
                0 => (Stage::Vertex, 2),
                1 => (Stage::Hull, 4),
                2 => (Stage::Domain, 6),
                3 => (Stage::Geometry, 3),
                4 => (Stage::Pixel, 1),
                _ => unreachable!(),
            };
            let StageInputs {
                mut textures,
                samplers,
                mut values,
                code,
                constants,
            } = self.material_inputs(&source, from)?;
            if pass == 16 && index == 0 {
                // The sight quad's z is offset by this row before reticle matrix six. Stock
                // sights carry a per-weapon constant (2.5 on the two stock sights whose quad
                // has this quad's dimensions, 1.2 to 25 across all 18), and the game builds
                // the matrix for that plane. D1's own 1.0 belongs to D1's projection and
                // drew the quad 2.5 times too large with a corner on the lens centre.
                let program = self.source.read(shader)?;
                let range = program.array_range(8, 1, Some(0x80800009))?;
                let row = usize::from(super::super::shader::sight_depth_row(&program.0[range])?);
                let at = row * 16;
                ensure!(
                    values.len() >= at + 16,
                    "Source sight depth row exceeds the initial vectors"
                );
                values[at..at + 4].copy_from_slice(&NATIVE_SIGHT_DEPTH.to_le_bytes());
                evidence.push(json!({"stage":"vertex","sight_depth_row":row,
                    "native_sight_depth":NATIVE_SIGHT_DEPTH,"approximation":"stock_sight_with_matching_quad"}));
            }
            let objects = source.u32(from + 0x78)? as usize;
            ensure!(
                objects <= 256,
                "Source object input count exceeds native indices"
            );
            let mut program = expression::translate(
                &code,
                constants,
                &expression::Inputs {
                    globals: &self.globals,
                    dye: dye_data.as_ref(),
                    empty_palette: equipment.dyes.is_empty(),
                    secondary: dye % 2 != 0,
                    plates: &equipment.plates,
                    detail: &detail,
                    objects,
                    object_channels: &self.object_channels,
                    outputs: values.len() / 16,
                    samplers: samplers.len(),
                    stage: packed,
                    reticle: pass == 16,
                },
            )?;
            program.initialize(&values)?;
            let shader = self.shader(
                shader,
                super::resource::Shader {
                    stage,
                    deferred: index == 4 && matches!(pass, 0 | 1),
                    layout,
                    depth: depth.filter(|_| index == 4),
                    reticle_mask: program.reticle_mask.filter(|_| index == 4),
                },
            )?;
            patches.push((to, shader));
            textures.extend(program.textures);
            let rows = textures
                .keys()
                .flat_map(|slot| slot.to_le_bytes().into_iter().chain(u32::MAX.to_le_bytes()))
                .collect::<Vec<_>>();
            let start = array(&mut out, to + 8, 0x80807211, &rows, 8)?;
            for (index, texture) in textures.values().enumerate() {
                patches.push((start + index * 8 + 4, texture.clone()));
            }
            // The resource set hash includes every actual bound texture identity.
            let mut hash = 0xCBF29CE484222325u64;
            for (slot, symbol) in &textures {
                for byte in slot.to_le_bytes().into_iter().chain(symbol.bytes()) {
                    hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001B3);
                }
            }
            put(&mut out, to + 0x18, &hash.to_le_bytes())?;
            array(&mut out, to + 0x20, 0x80800009, &program.code, 1)?;
            array(&mut out, to + 0x30, 0x80800090, &program.constants, 16)?;
            let rows = samplers
                .iter()
                .flat_map(|_| u32::MAX.to_le_bytes().into_iter().chain([0; 12]))
                .collect::<Vec<_>>();
            let start = array(&mut out, to + 0x40, 0x808073F3, &rows, 16)?;
            for (index, sampler) in samplers.into_iter().enumerate() {
                patches.push((start + index * 16, sampler));
            }
            array(&mut out, to + 0x50, 0x80800090, &values, 16)?;
            put(&mut out, to + 0x70, &(objects as u32).to_le_bytes())?;
            // The initialized expression also satisfies the native upload gate.
            out[to + 0x74] = if values.is_empty() { 0 } else { 0x10 };
            put(
                &mut out,
                to + 0x80,
                &(if values.is_empty() { u32::MAX } else { 0 }).to_le_bytes(),
            )?;
            evidence.push(json!({"stage":stage,"object_inputs":objects,"defaulted_source_globals":program.defaulted_globals}));
        }
        put(&mut out, 0x368, &u32::MAX.to_le_bytes())?;
        put(&mut out, 0x3E8, &u32::MAX.to_le_bytes())?;
        put(&mut out, 0x3EC, &u32::MAX.to_le_bytes())?;
        size(&mut out)?;
        let template = self.class_template(0x808071E8)?;
        self.evidence.push(json!({"material":tag,"symbol":symbol,"pass":pass,"dye":dye_tag,"secondary":dye%2!=0,"simulated":simulated,"scopes":masks,"stages":evidence}));
        self.node(symbol, template, &out, None, &patches)
    }
}
