//! Ability banks: the property rows an Ability Property perk action looks up by key, and the
//! edits that give a bank a row it lacks.
//!
//! A bank is a component resource of an ability entity. Every typed block in it starts with
//! the owning tag, its class and the absolute offset of its twin, blocks come in definition
//! and instance pairs, and the four bytes before a block hold its twin's class. The header
//! keeps the payload size at +0 and relative pointers to the bank definition block at +0x10
//! and its instance block at +0x18. The instance block's descriptor at +0x88, a count and a
//! relative pointer, is what the client's lookup walks: it lands on an array header, a count
//! and a class behind the `80809FBD` marker, followed by 56 byte definition rows with the key
//! at +0x10, a handler index at +0x1C and a pointer to the row's modifier at +0x30. Attack
//! rows keep a second pointer at +0x28 to the `8080454A` key reference twelve bytes before
//! their modifier, which attack selection follows and checks by class, so a moved row carries
//! it along or the client faults at world load with a sword equipped. The definition block's
//! descriptor at +0x170 lands on the matching 64 byte instance rows, which point back at the
//! bank and at the modifier's instance twin. The handler index is the slot
//! of the ability's component that consumes the row's modifier, numbered per bank: charges go
//! to slot 0 in the Hunter melee bank, 1 in the Titan and Warlock melee banks and 2 in the
//! Barricade bank, and a modifier handed to the wrong slot faults the client at world load,
//! so a new row copies the slot from the bank's rows with the same modifier class.
//!
//! Two modifier shapes are edited here. A charge is the pair `80804534` (definition, the
//! integer at +0x10) and `80804535`, which the base bank handler adds to the ability's charge
//! count. A script parameter is the pair `80804516` (definition, a first index at +0x20 and a
//! count at +0x24 into the bank's parameter table) and `80804517`: the table hangs off the
//! instance block's descriptor at +0x98, behind the same marker, as 16 byte `80804519` rows
//! holding the parameter's name hash, the value the script resets it to, the value the
//! property applies and whether that value is added to the running one or written over it.
//!
//! The client clones a bank in two regions, the definition block through to the instance block
//! and the instance block through to the end, and the clone an equipped ability walks holds
//! only the first, so every array a block's descriptor names must sit in that block's region.
//! Arrays are contiguous, so a bank with one more row gets its arrays rebuilt: the instance
//! rows and the new instance modifier go in at the end of the definition region, which shifts
//! the instance region and every absolute twin offset, header pointer and header table entry
//! that reaches across, and the definition rows, the parameter table and the new definition
//! modifier go at the end. Every stock instance modifier is pointed back at its moved row. The
//! old arrays stay as bytes nothing points at. The ability entity that owns the bank names its
//! blocks and fields by absolute offset (the bank tag, the class it expects there and the
//! offset, sixteen bytes), so every tag referencing the bank has those offsets moved too
//! (`retarget_references`). Bungie put the +1 charge row in all 21 grenade banks but in only one melee bank and the
//! Dodge bank, and each script parameter only where a stock exotic needed it, which is why a
//! key finds nothing on the other banks until the row is added.
use std::collections::{BTreeMap, BTreeSet};

use crate::package_payload::{i64_at, u32_at, u64_at};

mod census;
mod handler;
pub use crate::ability::AbilityTarget;
pub use handler::HandlerIndex;
mod names;
mod read;
mod registry;
mod scalar;
mod write;

pub use census::{SLOT_PARAMETERS, StockParameter, slot_parameters};
pub use names::{
    PARAMETER_NAMES, ParameterKind, ParameterName, parameter_kind, parameter_label,
    parameter_meaning, parameter_name,
};
pub use read::{
    bank_owner, block_count, handler_slot, instance_shift, parameter_rows, parameters,
    property_rows, retarget_references, row_modifiers, validate,
};
pub use registry::{bank_name, bank_names, parameter_abilities, register_bank_names};

use read::*;
use write::*;

const SIZE: usize = 0x00;
const DEFINITION_POINTER: usize = 0x10;
const INSTANCE_POINTER: usize = 0x18;
const BLOCK_CLASS: usize = 0x04;
const BLOCK_TWIN: usize = 0x08;
/// The definition block's descriptor of the instance rows.
const INSTANCE_ROWS_DESCRIPTOR: usize = 0x170;
/// The instance block's descriptor of the definition rows, the array the client walks.
const DEFINITION_ROWS_DESCRIPTOR: usize = 0x88;
/// The instance block's descriptor of the script parameter table.
const PARAMETERS_DESCRIPTOR: usize = 0x98;
const ARRAY_MARKER: u32 = 0x8080_9FBD;
pub const DEFINITION_ROW_CLASS: u32 = 0x8080_453A;
pub const INSTANCE_ROW_CLASS: u32 = 0x8080_453B;
pub const CHARGE_DEFINITION_CLASS: u32 = 0x8080_4534;
pub const CHARGE_INSTANCE_CLASS: u32 = 0x8080_4535;
pub const SCRIPT_DEFINITION_CLASS: u32 = 0x8080_4516;
pub const SCRIPT_INSTANCE_CLASS: u32 = 0x8080_4517;
pub const PARAMETER_ROW_CLASS: u32 = 0x8080_4519;
pub const ATTACK_DEFINITION_CLASS: u32 = 0x8080_4427;
const ATTACK_INSTANCE_CLASS: u32 = 0x8080_4428;
/// The key reference block attack selection expects an attack row's +0x28 pointer to land on.
const ATTACK_KEY_CLASS: u32 = 0x8080_454A;
/// The native unpowered contact profile used by the direct melee attack template.
pub const MELEE_DAMAGE_PROFILE: u32 = 0x81A6_B618;
const DEFINITION_ROW_SIZE: usize = 56;
const INSTANCE_ROW_SIZE: usize = 64;
const PARAMETER_ROW_SIZE: usize = 16;
const ROW_KEY: usize = 0x10;
const ROW_NAME: usize = 0x14;
const ROW_FLAGS: usize = 0x18;
const ROW_HANDLER: usize = 0x1C;
const ROW_MODE: usize = 0x20;
/// Attack rows' pointer to their key reference, zero on every other row.
const ROW_ATTACK_KEY: usize = 0x28;
const ROW_MODIFIER: usize = 0x30;
const INSTANCE_ROW_BANK: usize = 0x10;
const INSTANCE_ROW_MODIFIER: usize = 0x30;
const CHARGE_VALUE: usize = 0x10;
const SCRIPT_FIRST: usize = 0x20;
const SCRIPT_COUNT: usize = 0x24;
const INSTANCE_MODIFIER_ROW: usize = 0x10;

/// Extra Class Ability Charge: the Dodge bank's Double Dodge row, applied on the class
/// ability slot.
pub const CLASS_ABILITY_CHARGE_KEY: u32 = 0x8354_9A10;
/// Extra Melee Charge: the Warlock melee bank's The Whispers row, applied on the melee slot.
pub const MELEE_CHARGE_KEY: u32 = 0xD80D_2810;
/// Ability Property's grenade slot.
pub const GRENADE_SLOT: AbilityTarget = AbilityTarget::Grenade;
/// Ability Property's Super slot.
pub const SUPER_SLOT: AbilityTarget = AbilityTarget::Super;
/// Ability Property's melee slot.
pub const MELEE_SLOT: AbilityTarget = AbilityTarget::Melee;
/// Ability Property's jump slot.
pub const JUMP_SLOT: AbilityTarget = AbilityTarget::Jump;
/// Ability Property's movement slot: sprinting, sliding and turning.
pub const MOVEMENT_SLOT: AbilityTarget = AbilityTarget::Movement;
/// Ability Property's class ability slot.
pub const CLASS_ABILITY_SLOT: AbilityTarget = AbilityTarget::ClassAbility;

/// A charge key, the slot a perk applies it on, and the stock banks of that slot without a
/// row under it, which is where the key finds nothing until the row is added.
#[derive(Clone, Copy, Debug)]
pub struct ChargeRow {
    pub slot: AbilityTarget,
    pub key: u32,
    pub banks: &'static [u32],
}

/// The two keys whose rows stock leaves out of most banks.
/// The Dodge bank `80BC2C8A` and the Warlock melee bank `80BC3439` carry theirs and are not
/// listed. The Hunter melee bank's own charge row is the throwing knife's, under a key of its
/// own, so it is listed for the melee key. The Rift bank and the second Titan melee bank
/// `80BC4036` have no charge row of their own. Each shares its native class with a bank that
/// has one, so `handler_slot` numbers a charge handler after their others.
pub const CHARGE_ROWS: [ChargeRow; 2] = [
    ChargeRow {
        slot: CLASS_ABILITY_SLOT,
        key: CLASS_ABILITY_CHARGE_KEY,
        // Barricade, then Rift.
        banks: &[0x80BC_2BCC, 0x80BC_2CA3],
    },
    ChargeRow {
        slot: MELEE_SLOT,
        key: MELEE_CHARGE_KEY,
        // The Hunter bank, then the two other melee banks without the row.
        banks: &[0x80BC_3204, 0x80BC_32F8, 0x80BC_4036],
    },
];

/// The stock banks each Ability Property slot resolves to, by the slots the stock perks
/// apply their keys on, matched against the banks that define those keys. A tuning for a slot
/// goes to every bank of it that lists the parameter. Nine of the 54 root banks are touched
/// by no stock key and sit in `UNPLACED_BANKS`.
pub const SLOT_BANKS: [(AbilityTarget, &[u32]); 6] = [
    (
        GRENADE_SLOT,
        &[
            0x80B8_075C,
            0x80B8_0B7B,
            0x80B8_0CC5,
            0x80B8_0EF1,
            0x80B8_0EF4,
            0x80BB_D547,
            0x80BB_D65C,
            0x80BB_D727,
            0x80BB_D7D4,
            0x80BB_D9D1,
            0x80BB_D9EB,
            0x80BB_DB5B,
            0x80BC_0173,
            0x80BC_024E,
            0x80BC_04DB,
            0x80BC_051B,
            0x80BC_0559,
            0x80BC_2D77,
            0x80BC_2F4B,
            0x80BC_2FD9,
            0x80BF_A18C,
        ],
    ),
    (
        SUPER_SLOT,
        &[
            0x80BC_3424,
            0x80BC_3DA2,
            0x80BC_3F6C,
            0x80BC_3FBD,
            0x80BC_4993,
            0x80BC_4FF1,
            0x80BC_5602,
            0x80BC_82A3,
            0x80BC_8404,
            0x80BC_9189,
            0x80BC_9C02,
        ],
    ),
    (
        MELEE_SLOT,
        &[0x80BC_3204, 0x80BC_32F8, 0x80BC_3439, 0x80BC_4036],
    ),
    (
        JUMP_SLOT,
        &[
            0x80B8_1B7F,
            0x80BC_1D56,
            0x80BC_1D70,
            0x80BC_1D8D,
            0x80BC_34F2,
        ],
    ),
    (MOVEMENT_SLOT, &[0x80BC_3571]),
    (CLASS_ABILITY_SLOT, &[0x80BC_2BCC, 0x80BC_2C8A, 0x80BC_2CA3]),
];

/// Root banks no stock key touches, so no slot is established for them.
pub const UNPLACED_BANKS: [u32; 9] = [
    0x80B8_16B3,
    0x80B8_1C0F,
    0x80B8_1CBD,
    0x80BA_A293,
    0x80BC_3CB0,
    0x80BC_41DF,
    0x80BC_45C5,
    0x80BC_967E,
    0x80F2_E34B,
];

/// The banks of one slot.
#[must_use]
pub fn slot_banks(slot: AbilityTarget) -> &'static [u32] {
    SLOT_BANKS
        .iter()
        .find(|(candidate, _)| *candidate == slot)
        .map_or(&[], |(_, banks)| banks)
}

/// The name of an Ability Property slot, as the stock perks that apply keys on it establish
/// it (`fields::values` names the same choices).
#[must_use]
pub fn slot_name(slot: AbilityTarget) -> Option<&'static str> {
    slot.label()
}

/// The key an authored tuning is applied and defined under. It is a hash of the tuning
/// itself, so equal tunings on any perk share one row. It is 32 bits, so different tunings or a
/// stock row can share a key, and the build refuses that rather than reuse the other row.
#[must_use]
pub fn tuning_key(slot: AbilityTarget, parameter: u32, value_bits: u32, add: bool) -> u32 {
    crate::hash::fnv1_name_hash(&format!(
        "parhelion.ability.tuning.{slot}.{parameter:08x}.{value_bits:08x}.{}",
        u8::from(add)
    ))
}

/// One script parameter of a bank's table.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Parameter {
    pub name: u32,
    /// The value the script resets the variable to.
    pub reset: f32,
    /// The value a property row applies.
    pub applied: f32,
    /// Whether the applied value is added to the running value rather than written over it.
    pub add: bool,
}

/// One property row as the client's lookup reads it.
#[derive(Clone, Debug, PartialEq)]
pub struct PropertyRow {
    pub key: u32,
    pub name: u32,
    /// The handler slot that consumes the modifier, numbered per bank.
    pub handler: HandlerIndex,
    pub modifier_class: u32,
    /// The integer of a charge modifier, absent for any other modifier class.
    pub charge: Option<i64>,
    /// The table rows a script parameter modifier selects, empty for any other class.
    pub parameters: Vec<Parameter>,
}

/// What a new row does.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Modifier {
    /// Adds to the ability's charge count.
    Charges(i64),
    /// Sets a script parameter the bank already lists to `applied`, added to the running value
    /// or written over it. The reset value stays the bank's own.
    Parameter { name: u32, applied: f32, add: bool },
    /// A direct native melee attack with a privately authored contact profile and, when set,
    /// its own per-surface impact response table.
    Melee { damage: u32, responses: Option<u32> },
    /// A base ability input, added to its value or multiplied while the property is active.
    Scalar {
        input: u8,
        value: f32,
        multiply: bool,
    },
}

struct Rows {
    count: usize,
    first: usize,
}

struct Layout {
    owner: u32,
    definition: usize,
    instance: usize,
    instance_rows: Rows,
    definition_rows: Rows,
    parameters: Option<Rows>,
}

/// The bank with one more property row: `key` with a +1 charge modifier, shaped as the Dodge
/// bank's Double Dodge row is. Refuses a bank that already has the key.
pub fn with_charge_row(payload: &[u8], key: u32) -> Result<Vec<u8>, String> {
    with_property_row(payload, key, Modifier::Charges(1))
}

/// The bank with one more property row under `key`, doing what `modifier` says. A charge row
/// takes the Dodge bank's shape (no name); a parameter row takes the stock parameter rows'
/// shape and appends its parameter to the table with the bank's own reset value. Either
/// goes to the handler slot the bank's own rows of that modifier class use. Refuses a bank
/// that already has the key, that does not list the parameter, since only a listed
/// parameter is known to be read by the bank's script, or that shows no slot for the
/// modifier.
pub fn with_property_row(payload: &[u8], key: u32, modifier: Modifier) -> Result<Vec<u8>, String> {
    if let Modifier::Scalar {
        input,
        value,
        multiply,
    } = modifier
    {
        return scalar::with_row(payload, key, input, value, multiply);
    }
    let before = property_rows(payload)?;
    if before.iter().any(|row| row.key == key) {
        return Err(format!(
            "The bank already has a property row for key {key:08X}"
        ));
    }
    let layout = layout(payload)?;
    let table = table_parameters(payload, layout.parameters.as_ref())?;
    let parameter = listed_parameter(&table, modifier)?;
    let handler =
        handler_slot_in(&before, read::bank_class(payload)?, modifier)?.ok_or_else(|| {
            format!(
                "The bank hands no {} modifier to any handler slot",
                modifier_kind(modifier)
            )
        })?;
    let count = layout.definition_rows.count;
    let links = blocks(payload, layout.owner);
    let classes = modifier_classes(modifier);

    let mut region = open_definition_region(payload, &layout, count, classes.definition);
    let shift = Shift {
        instance: layout.instance,
        delta: region.delta,
    };
    follow_shift(payload, &mut region.out, &links, shift)?;

    // The definition rows array again at the end, one row longer, then the parameter table when
    // the row needs one, then the new definition modifier.
    let out = &mut region.out;
    let definition_header = open_block(out, ARRAY_MARKER);
    out.resize(
        definition_header + 16 + (count + 1) * DEFINITION_ROW_SIZE,
        0,
    );
    put_u64(out, definition_header, count as u64 + 1);
    put_u32(out, definition_header + 8, INSTANCE_ROW_CLASS);
    let parameter_header =
        parameter.map(|parameter| append_parameter_table(out, &layout, &table, parameter, shift));
    let modifier_definition = open_block(out, classes.instance);
    out.resize(modifier_definition + classes.definition_size, 0);
    // A melee row carries two copies of its attack, at modifier +0x18 and +0x20, as every stock
    // Warlock attack modifier does. Attack selection resolves applied keys through +0x20
    // 105F870, so a row with only +0x18 left the airborne melee on the class's own attack.
    // Both rank above every attack the bank holds, or a subclass melee key wins the tie.
    let attack = if let Modifier::Melee { damage, responses } = modifier {
        let priority = weapon_attack_priority(payload, &layout)?;
        let first = append_melee(out, damage, responses, priority)?;
        let second = append_melee(out, damage, responses, priority)?;
        Some((first, second))
    } else {
        None
    };
    out.resize(out.len().next_multiple_of(16), 0);

    let placement = Placement {
        instance_first: region.instance_first,
        definition_first: definition_header + 16,
        modifier_instance: region.modifier_instance,
        modifier_definition,
    };
    write_rows(
        payload, out, &layout, count, &placement, shift, key, handler,
    )?;
    write_modifier_pair(
        out,
        layout.owner,
        classes,
        &placement,
        modifier,
        table.len(),
        attack,
    )?;
    write_descriptors(
        out,
        &layout,
        shift,
        count,
        region.instance_header,
        definition_header,
        parameter_header.map(|header| (header, table.len() + 1)),
    )?;
    let size = out.len() as u64;
    put_u64(out, SIZE, size);

    // Every stock block still links to its twin, plus the rows and the modifier pair added.
    if blocks(out, layout.owner).len() != links.len() + 2 * (count + 1) + 2 {
        return Err("The edited bank lost a block's link to its twin".into());
    }
    check_read_back(
        out,
        &before,
        key,
        handler,
        classes.definition,
        modifier,
        parameter,
    )?;
    Ok(region.out)
}

/// The classes of a modifier's definition and instance blocks, and the definition's size.
#[derive(Clone, Copy)]
struct ModifierClasses {
    definition: u32,
    instance: u32,
    definition_size: usize,
}

/// Where the instance region moved: every offset at or past the instance block moves by
/// `delta`, a multiple of 16.
#[derive(Clone, Copy)]
struct Shift {
    instance: usize,
    delta: usize,
}

impl Shift {
    fn at(self, at: usize) -> usize {
        if at >= self.instance {
            at + self.delta
        } else {
            at
        }
    }
}

/// The payload with the definition region grown by the new instance rows array and the new
/// instance modifier, and the instance region after them.
struct Region {
    out: Vec<u8>,
    instance_header: usize,
    instance_first: usize,
    modifier_instance: usize,
    delta: usize,
}

/// Where the new arrays and the new modifier pair sit in the edited payload.
struct Placement {
    instance_first: usize,
    definition_first: usize,
    modifier_instance: usize,
    modifier_definition: usize,
}
