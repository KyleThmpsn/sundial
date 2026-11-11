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
    /// In every replay that draws it, the last write before it stored this local slot's dye and
    /// color lane.
    Dye { slot: u8, secondary: bool },
    /// The replays that draw it disagree, or a record's detail category is outside the table.
    Unresolved,
}

/// What the gear-dye extern holds at each unset-slot draw of a source model.
pub(super) struct RetainedDraws {
    /// By mesh ordinal and draw index.
    states: BTreeMap<(usize, usize), Retained>,
    /// Stage, dye byte and material of draws that see different states.
    split: BTreeSet<(usize, u8, u32)>,
}

impl RetainedDraws {
    pub fn get(&self, mesh: usize, draw: usize) -> Retained {
        self.states
            .get(&(mesh, draw))
            .copied()
            .unwrap_or(Retained::Unresolved)
    }

    /// What keeps apart the materials of draws that share a material and dye byte but see
    /// different states. Empty when every such draw sees the same one.
    pub fn variant(&self, stage: usize, dye: u8, material: u32, state: Retained) -> String {
        if !self.split.contains(&(stage, dye, material)) {
            return String::new();
        }
        match state {
            Retained::Zeroed => "-zeroed".to_owned(),
            Retained::Dye { slot, secondary } => {
                format!("-retained-{slot}-{}", u8::from(secondary))
            }
            Retained::Unresolved => "-unresolved".to_owned(),
        }
    }
}

/// How a source gear component's records reach its color passes. Every pass of one component and
/// stage shares the gear-dye extern, which `00884FE0` finds through the draw context at `+20`.
#[derive(Clone, Copy)]
pub(super) enum Route {
    /// Armor, the type-0 lane route `0086FF80`. Its first pass, through `00885610`, writes depth
    /// and stencil without the dye writer. The color passes then draw records whose flags masked
    /// by `0x90` are zero, then those for which it is `0x10`.
    Lanes,
    /// Weapon parts, the type-1 route `0087E6F0`. One pass draws records whose flags have bit
    /// `0x100` clear, or bit `0x200` clear in view 8, which are two different views.
    Parts,
}

impl Route {
    /// The route's views, each its color passes in order as flag mask and value.
    fn views(self) -> &'static [&'static [(u16, u16)]] {
        match self {
            Self::Lanes => &[&[(0x90, 0), (0x90, 0x10)]],
            Self::Parts => &[&[(0x100, 0)], &[(0x200, 0)]],
        }
    }
}

/// Whether detail predicate `007F7E20` draws a record of `category` at normal-table row `row`, or
/// `None` for a category outside its ten columns. The table at `0127B010` draws each category over
/// one interval of the 17 rows. Mask mode, which view types 0, 1, 2 and 12 select, shifts the
/// category's mask at `0127B168` by the row and reads the window from result table `0127B180`.
/// A window wholly inside a mask gives `10` and draws the record fully, and every such state draws
/// exactly the categories of one table row. Other nonzero results dither a record while one detail
/// level fades into the next.
fn drawn(row: u32, category: i8) -> Option<bool> {
    const INTERVALS: [(u32, u32); 10] = [
        (0, 1),
        (0, 5),
        (0, 10),
        (0, 14),
        (2, 5),
        (2, 10),
        (2, 14),
        (6, 10),
        (6, 14),
        (11, 14),
    ];
    let (first, last) = *INTERVALS.get(usize::try_from(category).ok()?)?;
    Some((first..=last).contains(&row))
}

/// One record of a stage as `007F7EA0` visits it: the detail category comes from the head of its
/// run, `+1F`, whose length is at `+20`.
struct Record {
    mesh: usize,
    index: usize,
    flags: u16,
    category: i8,
    byte: u8,
    material: u32,
}

/// What the extern holds between records.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Held {
    Zeroed,
    Dye(u8, bool),
    /// A write whose category is outside the table may or may not have run.
    Unknown,
}

impl Equipment {
    /// What the gear-dye extern holds for each unset-slot draw of a source model. Source
    /// `00862CB2` allocates the extern zeroed for each art component and stage, and `007F8010`
    /// draws the component's meshes in order in each pass of its route. `00884FE0` rewrites the
    /// extern when the dye byte changes: channels 0 to 2 copy their own local slot, channel 4 copies
    /// slot 0, and an unset slot writes only specular and normal W. A record is drawn when its run's
    /// detail category passes the predicate and its flags (`+1C`) pass the pass's filter.
    ///
    /// Each detail row the table distinguishes and each view of the route is replayed, and a draw
    /// takes the state every replay that draws it agrees on. The rows are the states in which no
    /// record is dithered. While one detail level fades into the next, mask mode can draw records
    /// of both levels, and an unset-slot draw there can follow a write the steady rows never put
    /// before it. Per-part visibility bits (`+1A`) are taken as set, as in the default appearance.
    pub fn retained(
        &self,
        source: &Payload,
        meshes: &[usize],
        route: Route,
    ) -> Result<RetainedDraws> {
        let mut states = BTreeMap::<(usize, usize), Retained>::new();
        let mut keys = BTreeMap::new();
        for stage in 0..19 {
            let records = stage_records(source, meshes, stage)?;
            for passes in route.views() {
                for row in 0..17 {
                    let mut held = Held::Zeroed;
                    for &(mask, value) in *passes {
                        for record in &records {
                            let drawn = drawn(row, record.category);
                            if drawn == Some(false) || record.flags & mask != value {
                                continue;
                            }
                            let Some(state) = self.unset_draw(record, drawn, &mut held) else {
                                continue;
                            };
                            let entry = states.entry((record.mesh, record.index)).or_insert(state);
                            if *entry != state {
                                *entry = Retained::Unresolved;
                            }
                            keys.insert(
                                (record.mesh, record.index),
                                (stage, record.byte, record.material),
                            );
                        }
                    }
                }
            }
        }
        let mut seen = BTreeMap::new();
        let mut split = BTreeSet::new();
        for (draw, key) in keys {
            let state = states[&draw];
            if *seen.entry(key).or_insert(state) != state {
                split.insert(key);
            }
        }
        Ok(RetainedDraws { states, split })
    }

    /// The dye a draw reads and its color lane: its own local slot's when set, or the one an
    /// earlier draw left in the extern.
    fn dye_source(&self, dye: u8, retained: Retained) -> (Option<u32>, bool) {
        match (self.dyes.get(&(dye / 2)).copied(), retained) {
            (Some(tag), _) => (Some(tag), !dye.is_multiple_of(2)),
            (None, Retained::Dye { slot, secondary }) => (self.dyes.get(&slot).copied(), secondary),
            (None, _) => (None, false),
        }
    }

    /// The extern state an unset-slot draw sees, or `None` when the record writes the extern
    /// or reads a set slot. A write updates `held`, and a write whose detail row fades leaves
    /// it unknown.
    fn unset_draw(
        &self,
        record: &Record,
        drawn: Option<bool>,
        held: &mut Held,
    ) -> Option<Retained> {
        let channel = record.byte >> 1;
        let write = match channel {
            0..=2 => Some(channel),
            4 => Some(0),
            _ => None,
        }
        .filter(|slot| self.dyes.contains_key(slot));
        if let Some(slot) = write {
            // Channel 4 stores the primary color at +40.
            *held = if drawn.is_some() {
                Held::Dye(slot, channel != 4 && record.byte & 1 != 0)
            } else {
                Held::Unknown
            };
            return None;
        }
        if self.dyes.contains_key(&channel) {
            return None;
        }
        Some(match *held {
            // Other channels do not share this extern's absent-slot path.
            _ if channel > 2 && !self.dyes.is_empty() => Retained::Unresolved,
            _ if channel > 2 => Retained::Zeroed,
            _ if drawn.is_none() => Retained::Unresolved,
            Held::Zeroed => Retained::Zeroed,
            Held::Dye(slot, secondary) => Retained::Dye { slot, secondary },
            Held::Unknown => Retained::Unresolved,
        })
    }
}

/// The gear-dye extern fields a draw's expression reads.
fn dye_input(
    selected: bool,
    retained: Retained,
    data: Option<&Payload>,
    secondary: bool,
) -> expression::Dye<'_> {
    match (selected, retained, data) {
        (true, _, Some(dye)) => expression::Dye::Selected { dye, secondary },
        (false, Retained::Zeroed | Retained::Dye { .. }, data) => {
            expression::Dye::Unset(data.map(|dye| (dye, secondary)))
        }
        _ => expression::Dye::Unresolved,
    }
}

/// Every draw record of one stage across the source meshes, in draw order. Each mesh's stage
/// range at `+58`/`+5A` holds runs whose first record gives the run length (`+20`) and the
/// detail category (`+1F`) the whole run shares.
fn stage_records(source: &Payload, meshes: &[usize], stage: usize) -> Result<Vec<Record>> {
    let mut records = Vec::new();
    for (mesh, &at) in meshes.iter().enumerate() {
        let parts = source.array(at + 0x48, 36, Some(0x80801AEF))?;
        let end = usize::from(source.u16(at + 0x5A + stage * 2)?);
        let mut head = usize::from(source.u16(at + 0x58 + stage * 2)?);
        ensure!(
            head <= end && end <= parts.len(),
            "Source stage exceeds its draw records"
        );
        while head < end {
            let run = usize::from(source.u8(parts[head] + 0x20)?);
            ensure!(
                run > 0 && head + run <= end,
                "Source draw run leaves its stage"
            );
            let category = i8::from_ne_bytes([source.u8(parts[head] + 0x1F)?]);
            for (index, &part) in parts.iter().enumerate().skip(head).take(run) {
                records.push(Record {
                    mesh,
                    index,
                    flags: source.u16(part + 0x1C)?,
                    category,
                    byte: source.u8(part + 0x1E)?,
                    material: source.u32(part)?,
                });
            }
            head += run;
        }
    }
    Ok(records)
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
        let selected = equipment.dyes.contains_key(&(dye / 2));
        let (dye_tag, secondary) = equipment.dye_source(dye, retained);
        let dye_data = dye_tag.map(|tag| self.source.read(tag)).transpose()?;
        let detail = self.detail_textures(dye_data.as_ref())?;
        let dye_input = dye_input(selected, retained, dye_data.as_ref(), secondary);
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
                    bloom_limit: pass == 7 && index == 4,
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
            if !program.defaulted_frame.is_empty() {
                self.approximations.insert("Source frame wind and displacement inputs take the values of a frame without wind, so wind does not move the parts that read them.".to_owned());
            }
            evidence.push(json!({"stage":stage,"object_inputs":objects,"defaulted_source_globals":program.defaulted_globals,"defaulted_frame_fields":program.defaulted_frame}));
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
