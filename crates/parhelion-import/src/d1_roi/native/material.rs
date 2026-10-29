use super::super::expression;
use super::*;
use crate::tiger::shader::identity::Stage;

pub(super) struct Equipment {
    pub plates: [String; 3],
    pub dyes: BTreeMap<u8, u32>,
}

/// What the source gear-dye extern holds when a draw selects an unset local dye slot.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Retained {
    /// No earlier draw of the component and stage wrote a dye, so the zeroed allocation remains.
    Zeroed,
    /// Every earlier write stored this local slot's dye and color lane.
    Dye { slot: u8, secondary: bool },
    /// Earlier writes differ, or none of them passes the same draw filters as this draw.
    Unresolved,
}

impl Equipment {
    /// What the gear-dye extern holds for each unset-slot draw of a source model, by stage, dye
    /// byte and material. Source `00862CB2` allocates the extern zeroed for each art component and
    /// stage, and `007F8010` draws the component's meshes in order. `00884FE0` rewrites it when the
    /// dye byte changes: channels 0 to 2 copy their own local slot, channel 4 copies slot 0, and an
    /// unset slot writes only specular and normal W. `007F7EA0` draws a part only if its group
    /// (`+1F`) and flags (`+1C`) pass the view's filters, so an earlier write certainly precedes a
    /// draw only when it shares both. Per-part visibility bits (`+1A`) are taken as set, as in the
    /// default appearance.
    pub fn retained(
        &self,
        source: &Payload,
        meshes: &[usize],
    ) -> Result<BTreeMap<(usize, u8, u32), Retained>> {
        let mut result = BTreeMap::new();
        for stage in 0..19 {
            let mut writes = Vec::new();
            for &mesh in meshes {
                let parts = source.array(mesh + 0x48, 36, Some(0x80801AEF))?;
                let range = usize::from(source.u16(mesh + 0x58 + stage * 2)?)
                    ..usize::from(source.u16(mesh + 0x5A + stage * 2)?);
                for &part in parts
                    .get(range)
                    .context("Source stage exceeds its draw records")?
                {
                    let byte = source.u8(part + 0x1E)?;
                    let channel = byte >> 1;
                    let filter = (source.u16(part + 0x1C)?, source.u8(part + 0x1F)?);
                    let write = match channel {
                        0..=2 => Some(channel),
                        4 => Some(0),
                        _ => None,
                    }
                    .filter(|slot| self.dyes.contains_key(slot));
                    if let Some(slot) = write {
                        // Channel 4 stores the primary color at +40.
                        writes.push((filter, (slot, channel != 4 && byte & 1 != 0)));
                        continue;
                    }
                    if self.dyes.contains_key(&channel) {
                        continue;
                    }
                    let state = match writes.first() {
                        // Other channels do not share this extern's absent-slot path.
                        _ if channel > 2 && !self.dyes.is_empty() => Retained::Unresolved,
                        _ if channel > 2 => Retained::Zeroed,
                        None => Retained::Zeroed,
                        Some(&(_, (slot, secondary)))
                            if writes.iter().all(|w| w.1 == (slot, secondary))
                                && writes.iter().any(|w| w.0 == filter) =>
                        {
                            Retained::Dye { slot, secondary }
                        }
                        Some(_) => Retained::Unresolved,
                    };
                    let entry = result
                        .entry((stage, byte, source.u32(part)?))
                        .or_insert(state);
                    if *entry != state {
                        *entry = Retained::Unresolved;
                    }
                }
            }
        }
        Ok(result)
    }
}

pub(super) struct Draw {
    pub stage: usize,
    pub dye: u8,
    pub simulated: bool,
    pub layout: u16,
    /// The extern this draw sees if its local dye slot is unset.
    pub retained: Retained,
}

/// The native state a D1 decal's blend state becomes, and whether it replaces target 0's color.
/// Material `+0x20` selects a blend state with its high bit. Both executables give states 22
/// and 23 the premultiplied blend `src + dst * src.a` that keeps the destination alpha, on
/// target 0 or targets 0 and 1, as native vehicle decal programs such as `815B9A49` expect.
/// D1 state 25 writes target 0's color without blending, where native state 25 blends three
/// targets, so it becomes native 22 with zero inverse coverage.
fn decal_state(state: u8) -> Result<(u8, bool)> {
    Ok(match state {
        0x96 | 0x97 => (state, false),
        0x99 => (0x96, true),
        _ => anyhow::bail!("Untranslated source decal blend state {state:02X}"),
    })
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

/// The native stage and packed stage number of a material's shader slot.
fn shader_slot(index: usize) -> (Stage, u8) {
    match index {
        0 => (Stage::Vertex, 2),
        1 => (Stage::Hull, 4),
        2 => (Stage::Domain, 6),
        3 => (Stage::Geometry, 3),
        4 => (Stage::Pixel, 1),
        _ => unreachable!(),
    }
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

    /// The sight quad's z is offset by this row before view matrix six, which both builds fill
    /// from the corresponding camera block. The depth is each weapon's reticle size (0.15 on a
    /// Rise of Iron sidearm to 26 on its snipers, 1.2 to 25 across stock Shadowkeep sights), so
    /// the source value is kept.
    fn sight_depth(&mut self, shader: u32, values: &[u8]) -> Result<Value> {
        let program = self.source.read(shader)?;
        let range = program.array_range(8, 1, Some(0x80800009))?;
        let row = usize::from(super::super::shader::sight_depth_row(&program.0[range])?);
        let at = row * 16;
        ensure!(
            values.len() >= at + 16,
            "Source sight depth row exceeds the initial vectors"
        );
        let depth = f32::from_le_bytes(values[at..at + 4].try_into()?);
        ensure!(
            depth.is_finite() && depth > 0.0,
            "Source sight depth is not positive"
        );
        Ok(json!({"stage":"vertex","sight_depth_row":row,"sight_depth":depth}))
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
            retained,
        } = draw;
        ensure!(
            self.source.reference(tag)? == 0x80801C32,
            "Source material class differs"
        );
        let source = self.source.read(tag)?;
        // The palette holds one dye per local slot. A draw through an unset slot reads what an
        // earlier draw of its component and stage left in the gear-dye extern.
        let selected = equipment.dyes.get(&(dye / 2)).copied();
        let (dye_tag, secondary) = match (selected, retained) {
            (Some(tag), _) => (Some(tag), dye % 2 != 0),
            (None, Retained::Dye { slot, secondary }) => {
                (equipment.dyes.get(&slot).copied(), secondary)
            }
            (None, _) => (None, false),
        };
        let dye_data = dye_tag.map(|tag| self.source.read(tag)).transpose()?;
        let detail = self.detail_textures(dye_data.as_ref())?;
        let dye_input = match (selected, retained, dye_data.as_ref()) {
            (Some(_), _, Some(dye)) => expression::Dye::Selected { dye, secondary },
            (None, Retained::Zeroed | Retained::Dye { .. }, data) => {
                expression::Dye::Unset(data.map(|dye| (dye, secondary)))
            }
            _ => expression::Dye::Unresolved,
        };
        let mut out = vec![0; 0x408];
        put(&mut out, 8, &source.bytes::<32>(8)?)?;
        // Platform pipeline cache bits are not authored D3D state selectors.
        put(
            &mut out,
            0xC,
            &(source.u32(0xC)? & 0x00FF_FFFF).to_le_bytes(),
        )?;
        out[0x27] = 0;
        let decal = (pass == 1)
            .then(|| decal_state(source.u8(0x20)?))
            .transpose()?;
        if let Some((state, _)) = decal {
            out[0x20] = state;
        }
        let gbuffer = match pass {
            0 => Some(super::resource::Gbuffer::Surface),
            1 => decal.map(|(_, replace)| super::resource::Gbuffer::Decal { replace }),
            _ => None,
        };
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
            let (stage, packed) = shader_slot(index);
            let StageInputs {
                mut textures,
                samplers,
                values,
                code,
                constants,
            } = self.material_inputs(&source, from)?;
            if pass == 16 && index == 0 {
                evidence.push(self.sight_depth(shader, &values)?);
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
                    dye: dye_input,
                    plates: &equipment.plates,
                    detail: &detail,
                    objects,
                    object_channels: &self.object_channels,
                    outputs: values.len() / 16,
                    samplers: samplers.len(),
                    stage: packed,
                    reticle: pass == 16,
                    light: false,
                },
            )?;
            program.initialize(&values)?;
            let shader = self.shader(
                shader,
                super::resource::Shader {
                    stage,
                    gbuffer: gbuffer.filter(|_| index == 4),
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
