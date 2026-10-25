//! The Magazine count Destiny's inspect display shows. It is the weapon's components' Magazine
//! input 0, not the stat group's display curve: the stat translator writes it from the weapon's
//! stats, through its routing entries and its runtime logic rows in their shared order, then the
//! attached perks' property modifiers change it. The display truncates it, after dividing it by
//! the truncated Barrel input 22, bullets per shot, where the weapon's HUD Content mode row sets
//! byte +0x13D, as fusion rifles' and multi-rocket launchers' do. A count of 0 hides the row.
//!
//! The replay follows the Shadowkeep translator consumer: operations 0 to 10 of a routing entry and
//! logic type 2, round(base * rate / reference) * bullets with 0 bullets kept as 1. Other logic
//! types write Barrel timing inputs, so one that writes an input this count reads is refused rather
//! than guessed. Perk modifiers apply at full weight, additions before factors.
use super::ammo;
use super::rig::{
    TRANSLATOR_KEY_CLASS, TRANSLATOR_KEY_HASH, TRANSLATOR_KEY_SIZE, TRANSLATOR_TABLE_CLASS,
    TRANSLATOR_TABLE_SIZE, translator_array,
};
use crate::tag_payload::{array_at, read_i32, read_u8, read_u32, read_u64};
use crate::{AuthoringResult, error::invalid};
use std::collections::BTreeMap;
use sundial::package_authoring::runtime::{
    load_weapon_runtime_graph_for_entity,
    modifiers::{BARREL_BULLETS_PER_SHOT, BARREL_COMPONENT, MAGAZINE_COMPONENT},
};
use sundial::package_authoring::sandbox_perk::{
    entity::modifiers::discover, load_sandbox_perk_runtime_action,
};
use sundial::package_authoring::{PackageManager, resolve_live_named_tag};

/// One translation: a Weapon Stats input converted at every tier, the signed index offset at +0x24,
/// the stat values its first and last tiers stand for at +0x28 and +0x2C, and whether it blends
/// adjacent tiers (+0x30).
const TRANSLATION_CLASS: u32 = 0x8080_3979;
const TRANSLATION_SIZE: usize = 0x38;
const TIER_CLASS: u32 = 0x8080_9F2A;
const TIER_SIZE: usize = 0x10;
const VALUES_CLASS: u32 = 0x8080_000F;
/// A routing entry writes component +0, input +4 by operation +0xC from operands at +0x10, +0x24
/// and +0x38, in the order at +0x4C.
const ROUTE_CLASS: u32 = 0x8080_3981;
const ROUTE_SIZE: usize = 0x50;
const ROUTE_ORDER: usize = 0x4C;
/// A runtime logic row writes its outputs at +0 from its operands at +0x10, by type +0x20, in the
/// order at +0x24.
const LOGIC_CLASS: u32 = 0x8080_397C;
const LOGIC_SIZE: usize = 0x28;
const LOGIC_OUTPUT_CLASS: u32 = 0x8080_3987;
const LOGIC_OUTPUT_SIZE: usize = 0x0C;
const MAGAZINE_LOGIC: i32 = 2;
/// An operand: a component selector at +0 (0 for a translation output) and its input at +4, the
/// output number at +0xC, and a constant at +0x10, Float32 MAX beside an output it samples.
const OPERAND_CLASS: u32 = 0x8080_3986;
const OPERAND_SIZE: usize = 0x14;
const OUTPUTS: u32 = 0x1E00;
const OUTPUTS_PER_TRANSLATION: u32 = 0x20;
/// The pins a translator key row holds from +0: a signed byte naming a Weapon Stats input and,
/// at +4, the Float32 stat value that input reads for this type instead of the weapon's own.
const PIN_SIZE: usize = 8;
/// The investment stat each Weapon Stats input reads, by definition index. Inputs 2, 12 and 19
/// read none. Past input 5 this follows the translator chapter's numbering, which the client's
/// own is not established to share there.
const INPUT_STATS: [Option<u16>; 20] = [
    Some(14),
    Some(24),
    None,
    Some(32),
    Some(17),
    Some(22),
    Some(20),
    Some(16),
    Some(21),
    Some(27),
    Some(18),
    Some(15),
    None,
    Some(25),
    Some(26),
    Some(26),
    Some(26),
    Some(30),
    Some(31),
    None,
];
/// Magazine input 0, capacity, and Barrel input 22, the bullets per shot the display divides by.
const CAPACITY: (u32, u32) = (MAGAZINE_COMPONENT as u32, 0);
const BULLETS_PER_SHOT: (u32, u32) = (BARREL_COMPONENT as u32, BARREL_BULLETS_PER_SHOT[0] as u32);
/// The HUD Content mode row's key and the byte that makes its count one of shots.
const MODE_CONTENT_KEY: usize = 0x10;
const COUNTS_SHOTS: usize = 0x13D;

#[derive(Clone, Debug, PartialEq)]
struct Translation {
    input: i32,
    offset: i32,
    range: [f32; 2],
    interpolate: bool,
    tiers: Vec<Vec<f32>>,
}

impl Translation {
    /// Column `column` at stat `stat`, sampled as the routing operand sampler does: the position
    /// bounded to the tiers, then the lower tier, or a blend toward the next.
    fn sample(&self, column: usize, stat: f32) -> Option<f32> {
        let last = self.tiers.len().checked_sub(1)?;
        let [low, high] = self.range;
        let span = high - low;
        let bounded = if span == 0.0 {
            0.0
        } else {
            ((stat - low) * last as f32 / span).clamp(0.0, last as f32)
        };
        let position = bounded + self.offset as f32;
        let tier = |at: f32| (at as i64 - i64::from(self.offset)).clamp(0, last as i64) as usize;
        let value = |tier: usize| self.tiers[tier].get(column).copied();
        if !self.interpolate {
            return value(tier(position));
        }
        let lower = tier(position.floor());
        let (from, to) = (value(lower)?, value((lower + 1).min(last))?);
        Some(from + (to - from) * (position - position.floor()))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Operand {
    Literal(f32),
    /// What an earlier step wrote to a component input, 0 before any did.
    Input(u32, u32),
    Output(u32),
    /// An output number the table does not have. Evaluating it gives no count.
    Missing,
}

#[derive(Clone, Debug, PartialEq)]
enum Step {
    Route {
        target: (u32, u32),
        operation: i8,
        operands: [Operand; 3],
    },
    Magazine {
        targets: Vec<(u32, u32)>,
        operands: [Operand; 4],
    },
}

/// A component property modifier a starting perk attaches, at full weight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Modifier {
    pub(crate) perk: u16,
    target: (u32, u32),
    pub(crate) multiplies: bool,
    pub(crate) amount: f32,
}

impl Modifier {
    /// Whether it changes the capacity, rather than the bullets per shot.
    pub(crate) fn capacity(&self) -> bool {
        self.target == CAPACITY
    }
}

/// What the inspect display's Magazine count is computed from, read from the weapon as built.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Magazine {
    translations: Vec<Translation>,
    steps: Vec<Step>,
    pins: Vec<(i32, f32)>,
    /// The output every Barrel bullet input copies, which a Bullets per Shot edit replaces.
    burst: Option<u32>,
    /// Whether the HUD Content mode row counts shots.
    counts_shots: bool,
    pub(crate) modifiers: Vec<Modifier>,
}

// Equal models come from equal package bytes, and packages hold no NaN these fields keep.
impl Eq for Magazine {}

/// How the shown count comes about.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Shown {
    /// Magazine input 0 as the translator writes it, before perks.
    pub(crate) capacity: f32,
    /// The bullets per shot it is divided by, where the HUD counts shots.
    pub(crate) per_shot: Option<i64>,
    pub(crate) count: i64,
}

impl Magazine {
    /// The count for a weapon whose investment stat `stat` holds by definition index, firing
    /// `bullets` per shot where a Bullets per Shot edit sets them, with the starting perks'
    /// modifiers where `perks` asks for them. None where the translator writes no capacity or a step
    /// it reaches cannot be evaluated.
    pub(crate) fn shown(
        &self,
        stat: impl Fn(u16) -> i32,
        bullets: Option<u16>,
        perks: bool,
    ) -> Option<Shown> {
        let mut written = BTreeMap::<(u32, u32), f32>::new();
        let operand = |operand: Operand, written: &BTreeMap<(u32, u32), f32>| match operand {
            Operand::Literal(value) => Some(value),
            Operand::Input(component, input) => {
                Some(written.get(&(component, input)).copied().unwrap_or(0.0))
            }
            Operand::Output(output) if Some(output) == self.burst && bullets.is_some() => {
                bullets.map(f32::from)
            }
            Operand::Output(output) => self.output(output, &stat),
            Operand::Missing => None,
        };
        for step in &self.steps {
            match step {
                Step::Route {
                    target,
                    operation,
                    operands: [a, b, c],
                } => {
                    let (a, b) = (operand(*a, &written)?, operand(*b, &written)?);
                    let value = match *operation {
                        0 => a + b,
                        1 => a - b,
                        2 => a * b,
                        3 => a / b,
                        4 => a,
                        5 => a.ceil(),
                        6 => a.floor(),
                        7 => a.round_ties_even(),
                        8 => operand(*c, &written)?.max(a).min(b),
                        9 => a.max(b),
                        10 => a.min(b),
                        // The native jump table skips the write for any other code.
                        _ => continue,
                    };
                    written.insert(*target, value);
                }
                Step::Magazine { targets, operands } => {
                    let [reference, rate, base, burst] = *operands;
                    let reference = operand(reference, &written)?;
                    let (rate, base) = (operand(rate, &written)?, operand(base, &written)?);
                    let burst = operand(burst, &written)?;
                    if reference == 0.0 {
                        return None;
                    }
                    let burst = if burst == 0.0 { 1.0 } else { burst };
                    let value = (base * rate / reference).round_ties_even() * burst;
                    for target in targets {
                        written.insert(*target, value);
                    }
                }
            }
        }
        let capacity = *written.get(&CAPACITY)?;
        let applied = |target: (u32, u32), value: f32| {
            let mine = || {
                self.modifiers
                    .iter()
                    .filter(move |m| perks && m.target == target)
            };
            let added = mine()
                .filter(|modifier| !modifier.multiplies)
                .fold(value, |value, modifier| value + modifier.amount);
            mine()
                .filter(|modifier| modifier.multiplies)
                .fold(added, |value, modifier| value * modifier.amount)
        };
        let total = applied(CAPACITY, capacity);
        let bullets = applied(
            BULLETS_PER_SHOT,
            written.get(&BULLETS_PER_SHOT).copied().unwrap_or(0.0),
        );
        // A count of 0 bullets never divides.
        let per_shot = Some(bullets.trunc() as i64).filter(|shots| self.counts_shots && *shots > 0);
        let count = (f64::from(total) / per_shot.unwrap_or(1) as f64).floor() as i64;
        Some(Shown {
            capacity,
            per_shot,
            count,
        })
    }

    /// Translation output `output` at the stat its translation converts, or that type's pin.
    fn output(&self, output: u32, stat: &impl Fn(u16) -> i32) -> Option<f32> {
        let index = output.checked_sub(OUTPUTS)?;
        let translation = self
            .translations
            .get((index / OUTPUTS_PER_TRANSLATION) as usize)?;
        let value = self
            .pins
            .iter()
            .find(|(input, _)| *input == translation.input)
            .map(|(_, value)| *value)
            .or_else(|| {
                usize::try_from(translation.input)
                    .ok()
                    .and_then(|input| INPUT_STATS.get(input).copied().flatten())
                    .map(|index| stat(index) as f32)
            })
            .unwrap_or(0.0);
        translation.sample((index % OUTPUTS_PER_TRANSLATION) as usize, value)
    }
}

/// The Magazine count model of a weapon whose stat translator owner is `translator`, converting
/// by the table for translation `group`. `burst` is the output a Bullets per Shot edit replaces.
/// `hud` is the HUD Content owner and the definition its instance points at, whose mode row for
/// content group `content_group` says whether the count is of shots. A weapon without one counts
/// rounds, as the native ammo-per-shot getter returns 1 then.
pub(crate) fn read(
    translator: &[u8],
    (group, burst): (u32, Option<u32>),
    hud: Option<(&[u8], usize)>,
    content_group: u32,
    modifiers: Vec<Modifier>,
) -> AuthoringResult<Magazine> {
    let keys = translator_array(translator, TRANSLATOR_KEY_CLASS)?;
    let tables = translator_array(translator, TRANSLATOR_TABLE_CLASS)?;
    if keys.0 != tables.0 {
        return Err(invalid("The stat translator's keys and tables differ"));
    }
    // A group the translator has no key for converts by its first table, as the client falls back.
    let mut index = 0;
    for key in 0..keys.0 {
        if read_u32(
            translator,
            keys.1 + key * TRANSLATOR_KEY_SIZE + TRANSLATOR_KEY_HASH,
        )? == group
        {
            index = key;
            break;
        }
    }
    let pins = pins(translator, keys.1 + index * TRANSLATOR_KEY_SIZE)?;
    let table = tables.1 + index * TRANSLATOR_TABLE_SIZE;
    let translations = translations(translator, table)?;
    let mut ordered = routes(translator, table + 0x10, translations.len())?;
    ordered.extend(logic(translator, table + 0x20, translations.len())?);
    // Stable, so steps of one order keep their array order, routes first.
    ordered.sort_by_key(|(order, _)| *order);
    Ok(Magazine {
        translations,
        steps: ordered.into_iter().map(|(_, step)| step).collect(),
        pins,
        burst,
        counts_shots: match hud {
            Some((owner, definition)) => counts_shots(owner, definition, content_group)?,
            None => false,
        },
        modifiers,
    })
}

/// The count and first row of the array described at `descriptor`, which must be of `class`
/// unless it is empty.
fn array(owner: &[u8], descriptor: usize, class: u32) -> AuthoringResult<(usize, usize)> {
    if read_u64(owner, descriptor)? == 0 {
        return Ok((0, 0));
    }
    let (count, _, rows, found) = array_at(owner, descriptor)?;
    if found != class {
        return Err(invalid(format!(
            "The stat translator holds class 0x{found:08X} where 0x{class:08X} belongs"
        )));
    }
    Ok((count, rows))
}

fn pins(owner: &[u8], key: usize) -> AuthoringResult<Vec<(i32, f32)>> {
    if read_u64(owner, key)? == 0 {
        return Ok(Vec::new());
    }
    let (count, _, rows, _) = array_at(owner, key)?;
    (0..count)
        .map(|pin| {
            let at = rows + pin * PIN_SIZE;
            Ok((
                i32::from(read_u8(owner, at)? as i8),
                f32::from_bits(read_u32(owner, at + 4)?),
            ))
        })
        .collect()
}

fn translations(owner: &[u8], table: usize) -> AuthoringResult<Vec<Translation>> {
    let (count, first) = array(owner, table, TRANSLATION_CLASS)?;
    (0..count)
        .map(|index| {
            let at = first + index * TRANSLATION_SIZE;
            let (tiers, tier) = array(owner, at + 0x10, TIER_CLASS)?;
            let tiers = (0..tiers)
                .map(|row| {
                    let (values, start) = array(owner, tier + row * TIER_SIZE, VALUES_CLASS)?;
                    (0..values)
                        .map(|value| Ok(f32::from_bits(read_u32(owner, start + value * 4)?)))
                        .collect::<AuthoringResult<Vec<_>>>()
                })
                .collect::<AuthoringResult<Vec<_>>>()?;
            let float = |offset| read_u32(owner, at + offset).map(f32::from_bits);
            Ok(Translation {
                input: read_i32(owner, at + 0x20)?,
                offset: read_i32(owner, at + 0x24)?,
                range: [float(0x28)?, float(0x2C)?],
                interpolate: read_u8(owner, at + 0x30)? != 0,
                tiers,
            })
        })
        .collect()
}

fn operand(owner: &[u8], at: usize, translations: usize) -> AuthoringResult<Operand> {
    let kind = read_u32(owner, at)?;
    if kind != 0 {
        return Ok(Operand::Input(kind, read_u32(owner, at + 4)?));
    }
    let output = read_u32(owner, at + 0xC)?;
    let value = f32::from_bits(read_u32(owner, at + 0x10)?);
    // E83C40 returns any constant other than Float32 MAX before it samples.
    if output == 0 || value != f32::MAX {
        return Ok(Operand::Literal(value));
    }
    let known = output
        .checked_sub(OUTPUTS)
        .is_some_and(|index| ((index / OUTPUTS_PER_TRANSLATION) as usize) < translations);
    Ok(if known {
        Operand::Output(output)
    } else {
        Operand::Missing
    })
}

fn routes(
    owner: &[u8],
    descriptor: usize,
    translations: usize,
) -> AuthoringResult<Vec<(u32, Step)>> {
    let (count, first) = array(owner, descriptor, ROUTE_CLASS)?;
    (0..count)
        .map(|index| {
            let at = first + index * ROUTE_SIZE;
            let operand =
                |slot: usize| operand(owner, at + 0x10 + slot * OPERAND_SIZE, translations);
            Ok((
                read_u32(owner, at + ROUTE_ORDER)?,
                Step::Route {
                    target: (read_u32(owner, at)?, read_u32(owner, at + 4)?),
                    operation: read_u8(owner, at + 0xC)? as i8,
                    operands: [operand(0)?, operand(1)?, operand(2)?],
                },
            ))
        })
        .collect()
}

fn logic(
    owner: &[u8],
    descriptor: usize,
    translations: usize,
) -> AuthoringResult<Vec<(u32, Step)>> {
    let (count, first) = array(owner, descriptor, LOGIC_CLASS)?;
    let mut steps = Vec::new();
    for at in (0..count).map(|index| first + index * LOGIC_SIZE) {
        let (outputs, start) = array(owner, at, LOGIC_OUTPUT_CLASS)?;
        let targets = (0..outputs)
            .map(|output| {
                let at = start + output * LOGIC_OUTPUT_SIZE;
                Ok((read_u32(owner, at)?, read_u32(owner, at + 4)?))
            })
            .collect::<AuthoringResult<Vec<_>>>()?;
        if read_i32(owner, at + 0x20)? != MAGAZINE_LOGIC {
            if targets
                .iter()
                .any(|target| [CAPACITY, BULLETS_PER_SHOT].contains(target))
            {
                return Err(invalid(
                    "Stat logic of an undecoded type writes the Magazine count",
                ));
            }
            continue;
        }
        let (operands, start) = array(owner, at + 0x10, OPERAND_CLASS)?;
        if operands < 4 {
            return Err(invalid("Magazine logic has fewer than four operands"));
        }
        let operand = |slot: usize| operand(owner, start + slot * OPERAND_SIZE, translations);
        steps.push((
            read_u32(owner, at + 0x24)?,
            Step::Magazine {
                targets,
                operands: [operand(0)?, operand(1)?, operand(2)?, operand(3)?],
            },
        ));
    }
    Ok(steps)
}

/// Whether the HUD Content definition at `definition` counts shots for `content_group`: the mode
/// row holding that key at +0x10, or the default row where none does, sets byte +0x13D. Which row
/// the client selects is matched by that key in stock packages, not traced to its writer.
fn counts_shots(owner: &[u8], definition: usize, content_group: u32) -> AuthoringResult<bool> {
    let rows = ammo::property_offsets(owner, definition)?;
    let mut row = rows[0];
    for &mode in &rows[1..] {
        if read_u32(owner, mode + MODE_CONTENT_KEY)? == content_group {
            row = mode;
            break;
        }
    }
    Ok(read_u8(owner, row + COUNTS_SHOTS)? != 0)
}

/// The capacity and bullets-per-shot modifiers `perks` attach, by sandbox perk index. A perk
/// without a runtime action, such as a declaration-only row, attaches none.
pub(crate) fn perk_modifiers(
    manager: &PackageManager,
    perks: &[u16],
) -> Result<Vec<Modifier>, String> {
    if perks.is_empty() {
        return Ok(Vec::new());
    }
    let globals = resolve_live_named_tag(manager, "investment_globals", None)?;
    let globals = manager
        .read_tag(globals)
        .map_err(|error| error.to_string())?;
    let mut found = Vec::new();
    for &perk in perks {
        let Ok(action) = load_sandbox_perk_runtime_action(manager, &globals, usize::from(perk))
        else {
            continue;
        };
        for source in &action.graphs {
            let graph =
                load_weapon_runtime_graph_for_entity(manager, 0, 0, source.tag.0, &source.payload)?;
            for record in discover(&graph) {
                let (Ok(component), Ok(input)) = (
                    u32::try_from(record.component_number()),
                    u32::try_from(record.input_number()),
                ) else {
                    continue;
                };
                let target = (component, input);
                if [CAPACITY, BULLETS_PER_SHOT].contains(&target)
                    && (record.adds() || record.multiplies())
                {
                    found.push(Modifier {
                        perk,
                        target,
                        multiplies: record.multiplies(),
                        amount: record.stock(),
                    });
                }
            }
        }
    }
    Ok(found)
}
