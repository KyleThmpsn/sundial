//! A source stat carried by its effect. Both games' stat translators sample a curve per Weapon
//! Stats input and route each output into a component input. Where a native stat's outputs are
//! all copied directly into component inputs that the source also copies directly, and no other
//! route touches those inputs in either table, the donor value is the one whose native outputs
//! come closest to the source outputs at the source value.
use super::*;
use sundial::package_authoring::entity::{
    SANDBOX_PATTERN_ENTITY_ASSIGNMENT_TAG, WEAPON_STAT_TRANSLATOR_COMPONENT_KEY,
    sandbox_pattern_identity_at, weapon_component_bindings, weapon_entity_assignment,
};
use sundial::package_authoring::investment_schema::{
    GLOBALS_SANDBOX_PATTERN_TABLE_SLOT, ITEM_TRANSLATION_BLOCK_CLASS,
    ITEM_TRANSLATION_BLOCK_POINTER_OFFSET, ITEM_TRANSLATION_WEAPON_PATTERN_INDEX_OFFSET,
    investment_globals_table_tag,
};

/// Native translator keys, one per translation group, with the group hash at +0x10, and its
/// tables in the same order: translations at +0 and routes at +0x10.
const KEY_CLASS: u32 = 0x8080_38C7;
const KEY_SIZE: usize = 0x28;
const KEY_HASH: usize = 0x10;
const TABLE_CLASS: u32 = 0x8080_3975;
const TABLE_SIZE: usize = 0x30;

/// Where a game's translator keeps each translation's stat range and sampling mode, and each
/// route's operation and copied output. Both keep a translation's tiers at +0x10, each an array
/// of Float32 outputs, and its Weapon Stats input at +0x20.
struct Layout {
    class: u32,
    size: usize,
    tier: u32,
    low: usize,
    high: usize,
    mode: usize,
    route: u32,
    route_size: usize,
    operation: usize,
    kind: usize,
    output: usize,
    /// Outputs are numbered from here, 0x20 for each translation and one for each column.
    outputs: u32,
    /// Words every supported translation holds, by offset.
    fixed: &'static [(usize, u32)],
}

const NATIVE: Layout = Layout {
    class: 0x8080_3979,
    size: 0x38,
    tier: 0x8080_9F2A,
    low: 0x28,
    high: 0x2C,
    mode: 0x30,
    route: 0x8080_3981,
    route_size: 0x50,
    operation: 0xC,
    kind: 0x10,
    output: 0x1C,
    outputs: 0x1E00,
    fixed: &[],
};

/// Source routes add a name hash before the operation and before each operand. A source
/// translation's domain is the empty name and its flags are clear.
const SOURCE: Layout = Layout {
    class: 0x8080_3F8E,
    size: 0x38,
    tier: 0x8080_BC4D,
    low: 0x2C,
    high: 0x30,
    mode: 0x34,
    route: 0x8080_3F96,
    route_size: 0x60,
    operation: 0x10,
    kind: 0x14,
    output: 0x24,
    outputs: 0x3C00,
    fixed: &[(0x24, 0x811C_9DC5), (0x28, 0)],
};

const INPUT: usize = 0x20;
const OUTPUTS_PER_TRANSLATION: u32 = 0x20;
const OUTPUTS_CLASS: u32 = 0x8080_000F;
/// Copies one translation output as it is.
const DIRECT: u32 = 4;

/// A stat matched by its effect: the source stat, the native stat it becomes and the native
/// Weapon Stats inputs that convert it.
struct Stat {
    source: u32,
    native: u32,
    inputs: &'static [u32],
}

/// Handling converts through three inputs. Range changed identity between the games.
const STATS: [Stat; 4] = [
    Stat {
        source: 0x383D_6DBC,
        native: 0x383D_6DBC,
        inputs: &[14, 15, 16],
    },
    Stat {
        source: 0xF9A0_4987,
        native: 0xF9A0_4987,
        inputs: &[9],
    },
    Stat {
        source: 0x5034_5F6F,
        native: 0x5034_5F6F,
        inputs: &[17],
    },
    Stat {
        source: 0xA1A5_E46F,
        native: 0x49F1_F137,
        inputs: &[7],
    },
];
/// The source numbers each of these Weapon Stats inputs one later, since it adds an input
/// before Stability's recoil scale.
const SOURCE_INPUT_SHIFT: u32 = 1;

/// The source component input that plays a native one's part. The source's component 0 adds an
/// input before native input 12, so its later inputs sit one higher.
fn source_target((component, input): (u32, u32)) -> (u32, u32) {
    if component == 0 && input >= 12 {
        (component, input + 1)
    } else {
        (component, input)
    }
}

struct Translation {
    input: u32,
    range: [f32; 2],
    interpolate: bool,
    tiers: Vec<Vec<f32>>,
}

impl Translation {
    fn read(p: &Payload, at: usize, layout: &Layout) -> Result<Self> {
        for &(offset, word) in layout.fixed {
            ensure!(
                p.u32(at + offset)? == word,
                "Stat translation domain differs"
            );
        }
        let range = [p.f32(at + layout.low)?, p.f32(at + layout.high)?];
        ensure!(range[1] > range[0], "Stat translation range is empty");
        let mode = p.u32(at + layout.mode)?;
        ensure!(mode <= 1, "Unsupported stat translation sampling mode");
        let tiers = p
            .array(at + 0x10, 16, Some(layout.tier))?
            .into_iter()
            .map(|tier| {
                p.array(tier, 4, Some(OUTPUTS_CLASS))?
                    .into_iter()
                    .map(|value| p.f32(value))
                    .collect::<Result<Vec<_>>>()
            })
            .collect::<Result<Vec<_>>>()?;
        ensure!(
            !tiers.is_empty()
                && tiers.len() <= 256
                && !tiers[0].is_empty()
                && tiers.iter().all(|tier| tier.len() == tiers[0].len()),
            "Invalid stat translation tiers"
        );
        Ok(Self {
            input: p.u32(at + INPUT)?,
            range,
            interpolate: mode == 1,
            tiers,
        })
    }

    /// Outputs at `value`, between the tiers either side of it when the mode interpolates and
    /// from the lower one otherwise.
    fn at(&self, value: f32) -> Vec<f32> {
        let [low, high] = self.range;
        let last = self.tiers.len() - 1;
        let x = (value.clamp(low, high) - low) / (high - low) * last as f32;
        let first = (x.floor() as usize).min(last);
        let fraction = if self.interpolate {
            x - first as f32
        } else {
            0.
        };
        let next = &self.tiers[(first + 1).min(last)];
        self.tiers[first]
            .iter()
            .zip(next)
            .map(|(a, b)| a + (b - a) * fraction)
            .collect()
    }
}

/// A table's translations, and the translation output each component input copies, where only
/// one direct route writes that input.
struct Table {
    translations: Vec<Translation>,
    direct: BTreeMap<(u32, u32), (usize, usize)>,
}

impl Table {
    fn read(p: &Payload, translations: usize, routes: usize, layout: &Layout) -> Result<Self> {
        let translations = p
            .array(translations, layout.size, Some(layout.class))?
            .into_iter()
            .map(|at| Translation::read(p, at, layout))
            .collect::<Result<Vec<_>>>()?;
        let mut writes = BTreeMap::<(u32, u32), Vec<Option<(usize, usize)>>>::new();
        for at in p.array(routes, layout.route_size, Some(layout.route))? {
            let output = p.u32(at + layout.output)?.wrapping_sub(layout.outputs);
            let (index, column) = (
                (output / OUTPUTS_PER_TRANSLATION) as usize,
                (output % OUTPUTS_PER_TRANSLATION) as usize,
            );
            let copied = (p.u32(at + layout.operation)? == DIRECT && p.u32(at + layout.kind)? == 0)
                .then_some((index, column))
                .filter(|_| {
                    translations
                        .get(index)
                        .is_some_and(|t| column < t.tiers[0].len())
                });
            writes
                .entry((p.u32(at)?, p.u32(at + 4)?))
                .or_default()
                .push(copied);
        }
        let direct = writes
            .into_iter()
            .filter_map(|(input, writes)| match writes.as_slice() {
                [Some(output)] => Some((input, *output)),
                _ => None,
            })
            .collect();
        Ok(Self {
            translations,
            direct,
        })
    }
}

/// The donor's native stat translator table, selected by its translation group as the build
/// selects it. None for a donor without a weapon pattern or with no single stat translator.
fn native_table(r: &mut Reader, donor: u32) -> Result<Option<Table>> {
    let definition = crate::tiger::shadowkeep::item_tag(r, donor)?;
    let definition = r.tag(definition, None)?;
    if definition.u64(ITEM_TRANSLATION_BLOCK_POINTER_OFFSET)? == 0 {
        return Ok(None);
    }
    let block = definition.pointer(ITEM_TRANSLATION_BLOCK_POINTER_OFFSET)?;
    ensure!(
        block >= 4 && definition.u32(block - 4)? == ITEM_TRANSLATION_BLOCK_CLASS,
        "Native weapon translation block differs"
    );
    let index = definition.u16(block + ITEM_TRANSLATION_WEAPON_PATTERN_INDEX_OFFSET)?;
    if index == u16::MAX {
        return Ok(None);
    }
    let globals = r
        .manager
        .lookup
        .named_tags
        .iter()
        .find(|t| t.name == "investment_globals")
        .context("native globals")?
        .hash
        .0;
    let globals = r.tag(globals, None)?;
    let patterns = investment_globals_table_tag(&globals.0, GLOBALS_SANDBOX_PATTERN_TABLE_SLOT)
        .map_err(anyhow::Error::msg)?;
    let patterns = r.tag(patterns, None)?;
    let pattern = sandbox_pattern_identity_at(&patterns.0, usize::from(index))
        .map_err(anyhow::Error::msg)?
        .context("Native weapon pattern row")?;
    let assignments = r.tag(SANDBOX_PATTERN_ENTITY_ASSIGNMENT_TAG, None)?;
    let entity = weapon_entity_assignment(&assignments.0, pattern.pattern_global_id_hash)
        .map_err(anyhow::Error::msg)?
        .context("Native weapon pattern entity")?;
    let entity = r.tag(entity, None)?;
    let bindings = weapon_component_bindings(&entity.0, WEAPON_STAT_TRANSLATOR_COMPONENT_KEY)
        .map_err(anyhow::Error::msg)?;
    let [binding] = bindings.as_slice() else {
        return Ok(None);
    };
    let owner = r.tag(binding.owner_tag, None)?;
    let keys = only_array(&owner, KEY_CLASS, KEY_SIZE)?;
    let tables = only_array(&owner, TABLE_CLASS, TABLE_SIZE)?;
    ensure!(
        !keys.is_empty() && keys.len() == tables.len(),
        "Native stat translator keys differ from its tables"
    );
    let mut selected = None;
    for (index, &key) in keys.iter().enumerate() {
        if owner.u32(key + KEY_HASH)? == pattern.weapon_translation_group_hash {
            ensure!(
                selected.replace(index).is_none(),
                "Native stat translator has duplicate group keys"
            );
        }
    }
    // A group the translator has no table for falls back to its first table.
    let table = tables[selected.unwrap_or(0)];
    Table::read(&owner, table, table + 0x10, &NATIVE).map(Some)
}

/// The translator owner's one nonempty array of `class`.
fn only_array(owner: &Payload, class: u32, stride: usize) -> Result<Vec<usize>> {
    let mut found: Option<Vec<usize>> = None;
    for at in (0..owner.0.len().saturating_sub(16)).step_by(8) {
        if owner.u64(at)? == 0 {
            continue;
        }
        let Ok(rows) = owner.array(at, stride, Some(class)) else {
            continue;
        };
        if let Some(other) = &found {
            ensure!(
                other.first() == rows.first(),
                "Native stat translator holds more than one {class:08X} array"
            );
        }
        found = Some(rows);
    }
    found.with_context(|| format!("Native stat translator has no {class:08X} array"))
}

/// Whether this module translates source `stat`, matched or with a reason it could not be.
pub(super) fn translates(stat: u32) -> bool {
    STATS.iter().any(|row| row.source == stat)
}

/// Donor investment values for the source stats whose native effect can be matched, keyed by
/// native stat hash, with the evidence for each. Stats that cannot be matched are left out with a
/// reason.
pub(super) fn matched(
    native: &mut Reader,
    donor: u32,
    source: &Payload,
    group: usize,
    values: &[Value],
) -> (BTreeMap<u32, i32>, Vec<Value>, Vec<Value>) {
    let mut chosen = BTreeMap::new();
    let mut evidence = Vec::new();
    let mut omitted = Vec::new();
    let tables = native_table(native, donor).and_then(|table| {
        let table = table.context("The donor has no single native stat translator")?;
        Ok((table, Table::read(source, group, group + 0x10, &SOURCE)?))
    });
    let (table, own) = match tables {
        Ok(tables) => tables,
        Err(error) => {
            for row in &STATS {
                omitted.push(json!({"stat":row.source,"reason":format!("{error:#}")}));
            }
            return (chosen, evidence, omitted);
        }
    };
    for row in &STATS {
        let stat = row.source;
        let Some(value) = values
            .iter()
            .find(|v| v["hash"].as_u64() == Some(u64::from(stat)) && v["literal"] == true)
            .and_then(|v| v["value"].as_i64())
        else {
            omitted
                .push(json!({"stat":stat,"reason":"The source weapon carries no literal value"}));
            continue;
        };
        match effect(&table, &own, row.inputs, value as f32) {
            Ok((matched, outputs, others)) => {
                chosen.insert(row.native, matched);
                evidence.push(json!({"stat":stat,"native":row.native,"source":value,
                    "matched":matched,"outputs":outputs,"other_source_stats":others}));
            }
            Err(reason) => omitted.push(json!({"stat":stat,"reason":reason})),
        }
    }
    (chosen, evidence, omitted)
}

/// The native value whose outputs for `inputs` come closest to the source outputs at `value`,
/// each output weighed by how far the native curve moves it. Outputs the native curve holds
/// constant cannot be matched and are not weighed. An output the source drives from another of
/// its stats is left out and returned with the other stats.
fn effect(
    native: &Table,
    source: &Table,
    inputs: &[u32],
    value: f32,
) -> std::result::Result<(i32, Vec<Value>, Vec<Value>), String> {
    let expected = inputs
        .iter()
        .map(|input| input + SOURCE_INPUT_SHIFT)
        .collect::<Vec<_>>();
    let mut pairs = Vec::new();
    let mut others = Vec::new();
    for (&target, &(index, column)) in &native.direct {
        let translation = &native.translations[index];
        if !inputs.contains(&translation.input) {
            continue;
        }
        if !translation.interpolate {
            return Err("A native curve selects tiers without interpolation".into());
        }
        let Some(&(own, own_column)) = source.direct.get(&source_target(target)) else {
            return Err(format!(
                "The source copies nothing directly into component {} input {}",
                target.0, target.1
            ));
        };
        if !expected.contains(&source.translations[own].input) {
            others.push(json!({"component":target.0,"input":target.1,
                "source_input":source.translations[own].input}));
            continue;
        }
        pairs.push((
            target,
            translation,
            column,
            source.translations[own].at(value)[own_column],
        ));
    }
    if pairs.is_empty() {
        return Err("No native output of this stat is copied directly".into());
    }
    let low = pairs.iter().map(|p| p.1.range[0]).fold(f32::MIN, f32::max);
    let high = pairs.iter().map(|p| p.1.range[1]).fold(f32::MAX, f32::min);
    if high <= low {
        return Err("The native curves cover different stat ranges".into());
    }
    let candidates = (low.ceil() as i32)..=(high.floor() as i32);
    let spans = pairs
        .iter()
        .map(|(_, translation, column, _)| {
            let outputs = candidates
                .clone()
                .map(|x| translation.at(x as f32)[*column])
                .collect::<Vec<_>>();
            let max = outputs.iter().copied().fold(f32::MIN, f32::max);
            let min = outputs.iter().copied().fold(f32::MAX, f32::min);
            max - min
        })
        .collect::<Vec<_>>();
    if spans.iter().all(|span| *span <= f32::EPSILON) {
        return Err("The native curves hold these outputs constant".into());
    }
    let error = |x: i32| -> f32 {
        pairs
            .iter()
            .zip(&spans)
            .filter(|(_, span)| **span > f32::EPSILON)
            .map(|((_, translation, column, wanted), span)| {
                ((translation.at(x as f32)[*column] - wanted) / span).powi(2)
            })
            .sum()
    };
    let matched = candidates
        .clone()
        .min_by(|a, b| error(*a).total_cmp(&error(*b)))
        .ok_or("The native curves cover no whole stat value")?;
    let outputs = pairs
        .iter()
        .map(|((component, input), translation, column, wanted)| {
            json!({"component":component,"input":input,"source":wanted,"native":translation.at(matched as f32)[*column]})
        })
        .collect();
    Ok((matched, outputs, others))
}
