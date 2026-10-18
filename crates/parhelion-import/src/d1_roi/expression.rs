//! Preserve D1 material arithmetic and bind its source equipment inputs explicitly.
use super::*;
use crate::tiger::reader::Reader;

pub(super) struct Globals {
    pub channels: BTreeMap<u8, Channel>,
    pub scopes: BTreeMap<u8, u8>,
    pub dyes: [DyeScope; 3],
    pub depth: Depth,
}

#[derive(Clone, Copy)]
pub(super) struct Depth {
    pub index: u32,
    pub source: u32,
    pub native: u32,
}

// Only vector zero has an equivalent producer in both transparent scopes.
// Validate instruction boundaries, including every write, before using it.
fn depth_producer(scope: &Payload, field: usize, write: u8) -> Result<()> {
    let range = scope.array_range(field, 1, Some(0x80800009))?;
    let code = &scope.0[range];
    ensure!(
        code.len().is_multiple_of(5),
        "Transparent scope program differs"
    );
    let mut found = false;
    for pair in code.chunks_exact(5) {
        match (pair[0], pair[3]) {
            (0x3F, op) if op == write + 4 => {}
            (0x3D, op) if op == write => {
                if pair[4] == 0 {
                    ensure!(
                        !found && pair[1..3] == [3, 0],
                        "Transparent depth vector producer differs"
                    );
                    found = true;
                }
            }
            _ => anyhow::bail!("Unsupported transparent scope instruction"),
        }
    }
    ensure!(found, "Transparent depth vector has no producer");
    Ok(())
}

#[derive(Clone, Copy)]
pub(super) struct DyeScope {
    pub tag: u32,
    pub index: u32,
    pub slot: u32,
}

pub(super) enum Channel {
    Native(u8),
    SourceDefault([u8; 16]),
}

fn scopes(data: &Payload) -> Result<Vec<String>> {
    data.array(0x10, 16, None)?
        .into_iter()
        .map(|row| {
            let start = data.pointer(row)?;
            let tail = data.0.get(start..).context("Render scope name")?;
            let length = tail
                .iter()
                .position(|byte| *byte == 0)
                .context("Unterminated render scope")?;
            ensure!(length <= 128, "Render scope name is too long");
            Ok(std::str::from_utf8(&tail[..length])?.to_owned())
        })
        .collect()
}

impl Globals {
    pub fn read(source: &Source, native: &mut Reader) -> Result<Self> {
        let bootstrap = source.read(source.named("client_bootstrap_patchable")?)?;
        let from = source.read(bootstrap.u32(0x3C)?)?;
        let defaults = source.read(from.u32(0x34)?)?;
        let tag = native
            .manager
            .lookup
            .named_tags
            .iter()
            .find(|tag| tag.name == "client_bootstrap_patchable")
            .context("Native render bootstrap")?
            .hash
            .0;
        let bootstrap = native.tag(tag, None)?;
        let to = native.tag(bootstrap.u32(0x4C)?, None)?;
        let native_defaults = native.tag(to.u32(0x34)?, None)?;
        let native_names = native_defaults
            .array(8, 4, None)?
            .into_iter()
            .enumerate()
            .map(|(i, row)| Ok((native_defaults.u32(row)?, u8::try_from(i)?)))
            .collect::<Result<BTreeMap<_, _>>>()?;
        let names = defaults.array(8, 4, None)?;
        let values = defaults.array(24, 16, None)?;
        ensure!(
            names.len() == values.len(),
            "Source global channel arrays differ"
        );
        let mut channels = BTreeMap::new();
        for (i, (name, value)) in names.into_iter().zip(values).enumerate() {
            let channel = match native_names.get(&defaults.u32(name)?) {
                Some(index) => Channel::Native(*index),
                None => Channel::SourceDefault(defaults.bytes(value)?),
            };
            channels.insert(u8::try_from(i)?, channel);
        }
        let native_scopes = scopes(&to)?;
        let scope_rows = to.array(0x10, 16, None)?;
        let mut dyes = Vec::new();
        for channel in 0..3 {
            let index = native_scopes
                .iter()
                .position(|name| *name == format!("gear_dye_{channel}"))
                .context("Native armor dye scope is missing")?;
            ensure!(index < 32, "Native armor dye scope index exceeds its mask");
            let tag = to.u32(scope_rows[index] + 12)?;
            let scope = native.tag(tag, Some(0x808071F3))?;
            ensure!(
                scope.array(0x88, 16, Some(0x80800090))?.len() == 27,
                "Native armor dye vectors differ"
            );
            let slot = scope.u32(0xB8)?;
            ensure!(
                (1..=9).contains(&slot) && !dyes.iter().any(|d: &DyeScope| d.slot == slot),
                "Native armor dye buffer conflicts with another bank"
            );
            dyes.push(DyeScope {
                tag,
                index: index as u32,
                slot,
            });
        }
        let source_scopes = scopes(&from)?;
        let source_index = source_scopes
            .iter()
            .position(|name| name == "transparent")
            .context("Source transparent scope is missing")?;
        let native_index = native_scopes
            .iter()
            .position(|name| name == "transparent")
            .context("Native transparent scope is missing")?;
        ensure!(
            source_index < 32 && native_index < 32,
            "Transparent scope exceeds its mask"
        );
        let source_rows = from.array(0x10, 16, None)?;
        let source_depth = source.read(from.u32(source_rows[source_index] + 12)?)?;
        let native_depth = native.tag(to.u32(scope_rows[native_index] + 12)?, Some(0x808071F3))?;
        ensure!(
            source_depth.array(0x68, 16, Some(0x80800090))?.len() == 6
                && native_depth.array(0x88, 16, Some(0x80800090))?.len() == 6,
            "Transparent scope vector bank differs"
        );
        depth_producer(&source_depth, 0x38, 0x42)?;
        depth_producer(&native_depth, 0x58, 0x43)?;
        let depth = Depth {
            index: source_index as u32,
            source: source_depth.u32(0x90)?,
            native: native_depth.u32(0xB8)?,
        };
        ensure!(
            (1..=9).contains(&depth.source)
                && (1..=9).contains(&depth.native)
                && !dyes.iter().any(|d| d.slot == depth.native),
            "Transparent depth buffer conflicts with another bank"
        );
        let scopes = source_scopes
            .into_iter()
            .enumerate()
            .filter_map(|(i, name)| {
                native_scopes
                    .iter()
                    .position(|n| *n == name)
                    .map(|n| (i as u8, n as u8))
            })
            .collect();
        Ok(Self {
            channels,
            scopes,
            dyes: dyes.try_into().ok().context("Native dye banks")?,
            depth,
        })
    }

    pub fn scope_mask(&self, source: u32) -> Result<u32> {
        let mut result = 0;
        for bit in 0..32 {
            if source & (1 << bit) != 0 {
                result |= 1
                    << self
                        .scopes
                        .get(&bit)
                        .context("Source render scope has no native equivalent")?;
            }
        }
        Ok(result)
    }
}

/// Channels carrying the Power level: `2C0A60A9` is the level itself under namespace
/// `5F6317D5`, and `0E36F79F` is what the stock rocket launcher bank `8152A4F7` computes from
/// it (0.65 times the level), the same way D1's `80AA2D66` does. D1 sight glows and armor
/// glows read these as object inputs through curves authored for D1's cap.
const POWER_CHANNELS: [u32; 2] = [0x0E36_F79F, 0x2C0A_60A9];
/// D1 Rise of Iron's Power cap over Shadowkeep's current level (400 over 1060).
const POWER_SCALE: f32 = 400.0 / 1060.0;

pub(super) struct Inputs<'a> {
    pub globals: &'a Globals,
    pub dye: Option<&'a Payload>,
    pub secondary: bool,
    pub plates: &'a [String; 3],
    pub detail: &'a [String; 2],
    pub objects: usize,
    /// The body's channel names by ordinal, which object inputs index.
    pub object_channels: &'a [u32],
    pub outputs: usize,
    pub samplers: usize,
    pub stage: u8,
    pub reticle: bool,
}

pub(super) struct Program {
    pub code: Vec<u8>,
    pub constants: Vec<u8>,
    pub textures: BTreeMap<u32, String>,
    /// Renderer globals absent from Shadowkeep retain their source default.
    pub defaulted_globals: Vec<u8>,
    /// The texture register that takes the reticle mask, when the stage binds it.
    pub reticle_mask: Option<u32>,
}

impl Program {
    pub(super) fn initialize(&mut self, values: &[u8]) -> Result<()> {
        if self.code.is_empty() && !values.is_empty() {
            // Native 011D8900 returns before mapping or copying the initial bank
            // when the expression is empty. Preserve vector zero while ensuring
            // that the complete initial bank reaches the writable buffer.
            self.constant(
                values
                    .get(..16)
                    .context("Truncated initial material vector")?
                    .try_into()?,
            )?;
            self.code.extend([0x43, 0]);
        }
        Ok(())
    }

    fn constant(&mut self, value: [u8; 16]) -> Result<()> {
        let index = self
            .constants
            .chunks_exact(16)
            .position(|v| v == value)
            .unwrap_or(self.constants.len() / 16);
        let encoded = u8::try_from(index).context("Translated material exceeds 256 constants")?;
        if index == self.constants.len() / 16 {
            self.constants.extend(value);
        }
        self.code.extend([0x34, encoded]);
        Ok(())
    }
}

fn vector(values: [f32; 4]) -> [u8; 16] {
    let mut bytes = [0; 16];
    for (chunk, value) in bytes.chunks_exact_mut(4).zip(values) {
        chunk.copy_from_slice(&value.to_le_bytes());
    }
    bytes
}

pub(super) fn operand_bytes(op: u8) -> Result<usize> {
    Ok(match op {
        0x01..=0x21 | 0x23..=0x33 => 0,
        0x22 | 0x34..=0x3B | 0x41..=0x47 | 0x49..=0x4B => 1,
        0x3C..=0x40 => 2,
        _ => anyhow::bail!("Unsupported D1 expression opcode {op:02X}"),
    })
}

pub(super) fn translate(code: &[u8], constants: Vec<u8>, inputs: &Inputs<'_>) -> Result<Program> {
    ensure!(
        constants.len().is_multiple_of(16) && constants.len() / 16 <= 256,
        "Invalid source expression constants"
    );
    let mut result = Program {
        code: Vec::new(),
        constants,
        textures: BTreeMap::new(),
        defaulted_globals: Vec::new(),
        reticle_mask: None,
    };
    let mut at = 0;
    while at < code.len() {
        let op = code[at];
        at += 1;
        let size = operand_bytes(op)?;
        let args = code
            .get(at..at + size)
            .context("Truncated D1 material expression")?;
        at += size;
        match op {
            0x3F => {
                ensure!(
                    code.get(at) == Some(&0x46),
                    "Source equipment texture has a nonlocal binding"
                );
                let slot = *code
                    .get(at + 1)
                    .context("Truncated equipment texture binding")?;
                ensure!(
                    slot >> 5 == inputs.stage,
                    "Source texture binds another shader stage"
                );
                if args == [75, 12] {
                    ensure!(
                        inputs.reticle && inputs.stage == 1,
                        "Source reticle mask is unavailable in this pass"
                    );
                    result.code.extend([0x3F, 0x49, 12, 0x47, slot]);
                    result.reticle_mask = Some(u32::from(slot & 31));
                    at += 2;
                    continue;
                }
                let symbol = match args {
                    [25, index @ 0..=2] => &inputs.plates[usize::from(*index)],
                    [27, index @ 0..=1] => &inputs.detail[usize::from(*index)],
                    _ => anyhow::bail!("Unsupported source equipment texture {args:?}"),
                };
                if symbol.is_empty()
                    && let [27, index @ 0..=1] = args
                    && let Some(dye) = inputs.dye
                    && dye.u32(0x60 + usize::from(*index) * 4)? == u32::MAX
                {
                    // Source 008C4080 writes a null resource for the absent tag.
                    // Native 34 pushes its raw vector and 47 binds its low qword.
                    result.constant([0; 16])?;
                    result.code.extend([0x47, slot]);
                    at += 2;
                    continue;
                }
                ensure!(
                    !symbol.is_empty(),
                    "Source material has no required equipment texture"
                );
                result.textures.insert(u32::from(slot & 31), symbol.clone());
                at += 2;
            }
            0x3D => {
                if let [75, field @ 4..=5] = args {
                    ensure!(
                        inputs.reticle,
                        "Source reticle viewport is unavailable in this pass"
                    );
                    result.code.extend([0x3D, 0x49, *field]);
                    continue;
                }
                let value = match args {
                    // Normal production rendering with the source dye-debug display disabled.
                    [31, 0] => vector([0., 1., 1., 1.]),
                    [27, index @ 1..=4] => {
                        if let Some(dye) = inputs.dye {
                            let offset = match *index {
                                1 => 0x70,
                                2 => 0x50,
                                3 => 0x80,
                                4 => {
                                    if inputs.secondary {
                                        0xA0
                                    } else {
                                        0x90
                                    }
                                }
                                _ => unreachable!(),
                            };
                            let mut value = dye.bytes::<16>(offset)?;
                            // The source producer overwrites stored normal W with the missing-dye flag.
                            if *index == 3 {
                                value[12..].fill(0);
                            }
                            value
                        } else {
                            ensure!(*index == 3, "Source material needs an absent equipment dye");
                            vector([0., 0., 0., 1.])
                        }
                    }
                    _ => anyhow::bail!("Unsupported source equipment vector {args:?}"),
                };
                result.constant(value)?;
            }
            0x3C => match args {
                [1, 0 | 1] => result.code.extend([op, args[0], args[1]]),
                // The inspected D1 frame producer writes zero to this fade input.
                [1, 28] => result.constant([0; 16])?,
                _ => anyhow::bail!("Untranslated source frame input {args:?}"),
            },
            // Both view externs provide the reticle projection at matrix six.
            0x3E if args == [2, 6] => result.code.extend([op, 2, 6]),
            0x3E | 0x40 => anyhow::bail!("Untranslated source matrix or integer extern {args:?}"),
            0x4A => {
                ensure!(
                    usize::from(args[0]) < inputs.objects,
                    "Source object input exceeds its declared table"
                );
                result.code.extend([0x4D, args[0]]);
                // The stock weapon bank derives channel 0E36F79F from the Power level
                // (0.65 times the level, normalised against 500 by the sight-glow curves,
                // which cap at 2.0). D1 authored those curves for its 400 cap; Shadowkeep's
                // level is 1060, so the loaded value is scaled back into D1's range.
                if inputs
                    .object_channels
                    .get(usize::from(args[0]))
                    .is_some_and(|name| POWER_CHANNELS.contains(name))
                {
                    result.constant(vector([POWER_SCALE; 4]))?;
                    result.code.push(0x03);
                }
            }
            0x4B => match inputs
                .globals
                .channels
                .get(&args[0])
                .context("Source global channel is absent")?
            {
                Channel::Native(index) => result.code.extend([0x4E, *index]),
                Channel::SourceDefault(value) => {
                    result.constant(*value)?;
                    if !result.defaulted_globals.contains(&args[0]) {
                        result.defaulted_globals.push(args[0]);
                    }
                }
            },
            0x41..=0x45 => {
                if op <= 0x43 {
                    ensure!(
                        usize::from(args[0]) < inputs.outputs,
                        "Expression output exceeds source constant buffer"
                    );
                }
                result.code.extend([op + 1, args[0]]);
            }
            0x46 => anyhow::bail!("Source texture binding lacks a validated producer"),
            0x47 => {
                ensure!(
                    args[0] >> 5 == inputs.stage,
                    "Sampler binds another shader stage"
                );
                result.code.extend([0x49, args[0]]);
            }
            0x49 => {
                ensure!(
                    usize::from(args[0]) < inputs.samplers,
                    "Source sampler exceeds resource table"
                );
                result.code.extend([0x4C, args[0]]);
            }
            _ => {
                if (0x34..=0x3B).contains(&op) {
                    ensure!(
                        usize::from(args[0]) < result.constants.len() / 16,
                        "Source expression constant exceeds its table"
                    );
                }
                result.code.push(op);
                result.code.extend(args);
            }
        }
    }
    Ok(result)
}
