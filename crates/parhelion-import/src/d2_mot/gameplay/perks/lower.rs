//! Checked native nodes translated from supported modern effect layouts.
//!
//! A lowered node is a relocatable native allocation. It is not a complete perk or proof
//! that the source controller, other effects and dependencies can be installed.
use anyhow::{Context, Result, ensure};

use crate::d2_mot::payload::Payload;

mod energy;
pub use energy::component_value;

pub struct Node {
    pub class: u32,
    pub kind: u8,
    /// NativeNode-compatible data with its root at zero and every pointer relocated locally.
    pub bytes: Vec<u8>,
}

/// The source Draw Event header carried unchanged by two independent
/// modern/native package pairs. The rest of the 112-byte native node comes
/// from a checked native template in the authoring crate.
pub fn draw_condition_prefix(source: &Payload, at: usize) -> Result<[u8; 24]> {
    ensure!(
        at >= 4 && source.u32(at - 4)? == 0x8080_30AE,
        "unsupported source draw condition class"
    );
    let bytes = source.bytes::<24>(at)?;
    let probability = f32::from_le_bytes(bytes[..4].try_into()?);
    ensure!(
        probability.is_finite() && (0.0..=1.0).contains(&probability),
        "unsupported source draw probability"
    );
    ensure!(
        bytes[4..9] == [0xFF, 16, 0, 0, 1] && bytes[9..].iter().all(|byte| *byte == 0),
        "unsupported source draw filter or dispatch fields"
    );
    Ok(bytes)
}

/// Dispatch a checked source condition by its format class. An unsupported
/// condition remains a visible import gap instead of borrowing a native kind
/// with the same number and different semantics.
pub fn condition(source: &Payload, at: usize) -> Result<Node> {
    ensure!(at >= 4, "source condition has no class word");
    match source.u32(at - 4)? {
        0x8080_3060 => timer_condition(source, at),
        0x8080_3086 => weapon_swap_condition(source, at),
        0x8080_30BE => ability_condition(source, at),
        0x8080_3061 => state_value_condition(source, at),
        0x8080_BDCF => object_slot_condition(source, at),
        class => anyhow::bail!("source condition class {class:08X} has no native translation"),
    }
}

/// Lower the four-channel object event predicate shared by the modern and native
/// controllers. Both predicates test byte +8 against event slot +4, then evaluate
/// the inline object matcher at +0x10 against event handle +0. The supported
/// modern matcher is empty, so its source-only header value is normalized to the
/// empty native matcher. A populated matcher needs a separate schema translation.
pub fn object_slot_condition(source: &Payload, at: usize) -> Result<Node> {
    ensure!(
        at >= 4 && source.u32(at - 4)? == 0x8080BDCF,
        "unsupported source object-slot condition class"
    );
    let mut bytes = source.bytes::<40>(at)?.to_vec();
    let probability = f32::from_le_bytes(bytes[..4].try_into()?);
    ensure!(
        probability.is_finite() && (0.0..=1.0).contains(&probability),
        "unsupported source object-slot probability"
    );
    ensure!(
        bytes[4] == 0xFF && bytes[5] == 49 && bytes[6] <= 1,
        "unsupported source object-slot dispatch or flags"
    );
    ensure!(
        bytes[8] & !0x0F == 0 && bytes[9..16].iter().all(|byte| *byte == 0),
        "unsupported source object-slot channel mask"
    );
    ensure!(
        source.u64(at + 16)? == 0x100 && bytes[24..40].iter().all(|byte| *byte == 0),
        "source object-slot matcher is populated or has an unknown layout"
    );
    bytes[5] = 41;
    bytes[16..24].fill(0);
    Ok(Node {
        class: 0x808029E1,
        kind: 41,
        bytes,
    })
}

/// Keep the 32-byte ability-event condition shared by both package eras. Three
/// independently shipped pairs have identical bytes and the same kind number.
/// This subset accepts an empty inline filter, as Eager Edge stores.
pub fn ability_condition(source: &Payload, at: usize) -> Result<Node> {
    ensure!(
        at >= 4 && source.u32(at - 4)? == 0x808030BE,
        "unsupported source ability condition class"
    );
    let bytes = source.bytes::<32>(at)?;
    let probability = f32::from_le_bytes(bytes[..4].try_into()?);
    ensure!(
        probability.is_finite() && (0.0..=1.0).contains(&probability),
        "unsupported source ability condition probability"
    );
    ensure!(
        bytes[4] == 0xFF && bytes[5] == 8 && bytes[6] <= 1,
        "unsupported source ability condition dispatch or flags"
    );
    ensure!(
        bytes[8] & !0x0F == 0
            && bytes[9..16].iter().all(|byte| *byte == 0)
            && bytes[16..32].iter().all(|byte| *byte == 0),
        "unsupported source ability condition filter"
    );
    Ok(Node {
        class: 0x80803E01,
        kind: 8,
        bytes: bytes.to_vec(),
    })
}

fn scalar_rows(
    source: &Payload,
    at: usize,
    stride: usize,
    class: u32,
    count: usize,
) -> Result<Vec<usize>> {
    ensure!(
        source.u64(at)? == count as u64,
        "source scalar array count differs"
    );
    let header = source.pointer(at + 8)?;
    let marker = header
        .checked_sub(4)
        .context("source scalar array has no marker")?;
    ensure!(
        source.u32(marker)? == 0x80809FB8,
        "source scalar array marker differs"
    );
    let rows = source.array(at, stride, Some(class))?;
    ensure!(rows.len() == count, "source scalar array rows differ");
    Ok(rows)
}

/// Lower the single-key scalar form of modern condition 21 to native condition 20.
/// The modern record holds a typed 808042D5 program, while Shadowkeep stores its
/// checked scalar comparison in the flat 256-byte 80803DCE condition. The source
/// opcode and neutral-field shape is shared by Eager Edge, Whirlwind Blade,
/// Backup Plan and Explosive Light. Other condition-21 programs are rejected.
pub fn state_value_condition(source: &Payload, at: usize) -> Result<Node> {
    ensure!(
        at >= 4 && source.u32(at - 4)? == 0x80803061,
        "unsupported source state-value condition class"
    );
    let root = source.bytes::<0x44>(at)?;
    let probability = f32::from_le_bytes(root[..4].try_into()?);
    let hold = source.f32(at + 0xC)?;
    ensure!(
        probability.is_finite() && (0.0..=1.0).contains(&probability),
        "unsupported source state-value probability"
    );
    ensure!(
        root[4] == 0xFF
            && root[5] == 21
            && root[6] == 0
            && root[8] <= 1
            && root[9..12] == [0; 3]
            && hold.is_finite()
            && (0.0..=7.0).contains(&hold)
            && source.u64(at + 0x10)? == 0x20
            && source.u64(at + 0x30)? == 0
            && source.u64(at + 0x38)? == 0
            && source.u32(at + 0x40)? == 0,
        "unsupported source state-value dispatch or root fields"
    );

    let rows = scalar_rows(source, at + 0x20, 16, 0x808091B7, 1)?;
    let row = rows[0];
    ensure!(source.u64(row)? == 0, "source scalar row flags differ");
    let program = source.pointer(row + 8)?;
    ensure!(
        program >= 4 && source.u32(program - 4)? == 0x808042D5,
        "unsupported source state-value program class"
    );
    source.bytes::<0x90>(program)?;
    for (offset, expected) in [
        (0, 0),
        (0x28, 0),
        (0x30, 0),
        (0x38, 1),
        (0x40, 1),
        (0x48, 0),
        (0x50, 0),
        (0x78, 0),
        (0x80, 1),
        (0x88, 0),
    ] {
        ensure!(
            source.u64(program + offset)? == expected,
            "unsupported source state-value program field at {offset:X}"
        );
    }
    let binding = scalar_rows(source, program + 8, 8, 0x8080920F, 1)?[0];
    ensure!(
        source.pointer(at + 0x18)? == binding && source.u32(binding)? == 1,
        "source state-value binding differs"
    );
    let key = source.u32(binding + 4)?;
    ensure!(key != 0, "source state-value key is absent");
    let first_code = scalar_rows(source, program + 0x18, 1, 0x80800009, 4)?;
    let second_code = scalar_rows(source, program + 0x58, 1, 0x80800009, 4)?;
    ensure!(
        first_code
            .iter()
            .map(|row| source.u8(*row))
            .collect::<Result<Vec<_>>>()?
            == [0x4A, 0, 0x4C, 0]
            && second_code
                .iter()
                .map(|row| source.u8(*row))
                .collect::<Result<Vec<_>>>()?
                == [0x42, 0, 0x4C, 0],
        "unsupported source state-value opcodes"
    );
    let vector = scalar_rows(source, program + 0x68, 16, 0x80800090, 1)?[0];
    let constants = source.bytes::<16>(vector)?;
    let minimum = f32::from_le_bytes(constants[..4].try_into()?);
    let maximum = f32::from_le_bytes(constants[12..].try_into()?);
    ensure!(
        constants[4..12] == [0; 8]
            && minimum.is_finite()
            && maximum.is_finite()
            && minimum <= maximum,
        "unsupported source state-value range"
    );

    let mut bytes = vec![0; 256];
    bytes[..4].copy_from_slice(&root[..4]);
    bytes[4..8].copy_from_slice(&[0xFF, 20, 1, root[7]]);
    for (offset, value) in [
        (0x0C, 1.0_f32),
        (0x10, -1.0),
        (0x14, -1.0),
        (0x1C, 1.0),
        (0x24, 1.0),
        (0x2C, 1.0),
        (0x34, 32.0),
        (0x88, 1.0),
        (0x90, 1.0),
        (0x98, 1.0),
        (0xA0, 1.0),
        (0xA8, 1.0),
        (0xB0, 1.0),
        (0xB8, 1.0),
    ] {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
    bytes[0x40..0x44].copy_from_slice(&0x20_u32.to_le_bytes());
    bytes[0xC8..0xCC].copy_from_slice(&u32::MAX.to_le_bytes());
    bytes[0xD4..0xD8].copy_from_slice(&key.to_le_bytes());
    bytes[0xD8..0xDC].copy_from_slice(&minimum.to_le_bytes());
    bytes[0xDC..0xE0].copy_from_slice(&maximum.to_le_bytes());
    bytes[0xE0..0xE4].copy_from_slice(&hold.to_le_bytes());
    bytes[0xF8] = root[8];
    Ok(Node {
        class: 0x80803DCE,
        kind: 20,
        bytes,
    })
}

/// A fixed source timer and native timer share their complete 12-byte record.
/// En Garde supplies an independently shipped byte-identical package pair.
pub fn timer_condition(source: &Payload, at: usize) -> Result<Node> {
    ensure!(
        at >= 4 && source.u32(at - 4)? == 0x80803060,
        "unsupported source timer condition class"
    );
    let bytes = source.bytes::<12>(at)?;
    let probability = f32::from_le_bytes(bytes[..4].try_into()?);
    let seconds = f32::from_le_bytes(bytes[8..12].try_into()?);
    ensure!(
        probability.is_finite() && (0.0..=1.0).contains(&probability),
        "unsupported source timer probability"
    );
    ensure!(
        bytes[4] == 0xFF && bytes[5] == 1 && bytes[6] <= 1,
        "unsupported source timer dispatch or flags"
    );
    ensure!(
        seconds.is_finite() && (0.0..=3600.0).contains(&seconds),
        "unsupported source timer duration"
    );
    Ok(Node {
        class: 0x80803DCD,
        kind: 1,
        bytes: bytes.to_vec(),
    })
}

/// Translate the simple weapon-swap condition and its shared label registry.
/// Sprint Grip has the same scalar condition bytes in both package eras. The
/// source registry tag is build-specific, while both records name the same path.
pub fn weapon_swap_condition(source: &Payload, at: usize) -> Result<Node> {
    const LABEL_PATH: &str = "content/common/native/sandbox/label_globals.label_globals.tft";
    const NATIVE_LABEL_TAG: u32 = 0x80C70CA1;
    ensure!(
        at >= 4 && source.u32(at - 4)? == 0x80803086,
        "unsupported source weapon-swap condition class"
    );
    let source_bytes = source.bytes::<112>(at)?;
    let probability = f32::from_le_bytes(source_bytes[..4].try_into()?);
    ensure!(
        probability.is_finite() && (0.0..=1.0).contains(&probability),
        "unsupported source weapon-swap probability"
    );
    ensure!(
        source_bytes[4] == 0xFF
            && source_bytes[5] == 18
            && source_bytes[6] <= 1
            && source_bytes[8] <= 1
            && source_bytes[9..0x50].iter().all(|byte| *byte == 0),
        "unsupported source weapon-swap dispatch or filter"
    );
    ensure!(
        source.u64(at + 0x50)? != 0
            && (0x80800000..0x82000000).contains(&source.u32(at + 0x58)?)
            && source_bytes[0x60..0x68].iter().all(|byte| *byte == 0)
            && source_bytes[0x68] == 0xFF
            && source_bytes[0x69..0x70].iter().all(|byte| *byte == 0),
        "unsupported source weapon-swap label binding"
    );
    let path = source.pointer(at + 0x50)?;
    let tail = source
        .0
        .get(path..)
        .context("weapon-swap label path outside source")?;
    let end = tail
        .iter()
        .position(|byte| *byte == 0)
        .context("unterminated weapon-swap label path")?;
    ensure!(
        &tail[..end] == LABEL_PATH.as_bytes(),
        "unsupported source weapon-swap label path"
    );
    let mut bytes = vec![0; 112];
    bytes[..0x50].copy_from_slice(&source_bytes[..0x50]);
    bytes[0x50..0x58].copy_from_slice(&32i64.to_le_bytes());
    bytes[0x58..0x5C].copy_from_slice(&NATIVE_LABEL_TAG.to_le_bytes());
    bytes[0x60] = 0xFF;
    bytes.extend_from_slice(LABEL_PATH.as_bytes());
    bytes.push(0);
    Ok(Node {
        class: 0x80803DDD,
        kind: 18,
        bytes,
    })
}

/// Map only modifier destinations whose native input contract is established.
/// Component and input numbering are independent between the two eras.
pub fn modifier_input(component: u8, input: u16) -> Result<(u8, u16)> {
    let component = match (component, input) {
        (3, 1 | 2) => 3,
        (15, 5 | 9) => 14,
        // EF8797 consumes category 11's input 3 as melee reach. The modern
        // lunge input has this role. Native melee has no input 4 or 5.
        (12, 3) => 11,
        (component, _) => anyhow::bail!(
            "source modifier component {component} input {input} has no validated native consumer"
        ),
    };
    Ok((component, input))
}

/// Lower the values of one modifier into an existing native settings envelope.
/// The caller owns private cloning and metadata dependency validation. This does
/// not translate an attached entity or allocate its runtime object. Rapid Hit's
/// independently shipped rows establish the definition layout and the reload
/// and weapon-stat selector contracts. The melee consumer has four inputs, so
/// the modern angular modifiers cannot be carried by this native record.
pub fn modifier_settings(
    source: &Payload,
    at: usize,
    native: &Payload,
    to: usize,
) -> Result<[u8; 88]> {
    let settings = source.pointer(24)?;
    ensure!(
        settings >= 4
            && source.u32(settings - 4)? == 0x80802D2B
            && source
                .array(settings + 88, 112, Some(0x80802D33))?
                .contains(&at)
            && source.u32(at + 4)? == 0x80802D32,
        "unsupported source modifier-settings class"
    );
    source.bytes::<112>(at)?;
    let instance = usize::try_from(source.u64(at + 8)?)?;
    ensure!(
        source.u32(instance)? == source.u32(at)?
            && source.u32(instance + 4)? == 0x80802D33
            && source.u64(instance + 8)? == at as u64,
        "source modifier pair differs"
    );
    ensure!(
        source.u8(at + 20)? <= 1
            && source.bytes::<3>(at + 21)? == [0; 3]
            && i64::from_le_bytes(source.bytes(at + 24)?) == -24
            && source.u32(at + 36)? == 0
            && source.u64(at + 40)? == 0
            && source.u64(at + 48)? == 0
            && i64::from_le_bytes(source.bytes(at + 56)?) == -56
            && source.u32(at + 68)? == 0
            && source.u64(at + 72)? == 1
            && source.u64(at + 80)? == 0
            && source.i16(at + 88)? == -1
            && source.u32(at + 92)? == 0x811C9DC5
            && source.bytes::<3>(at + 97)? == [0; 3]
            && source.u32(at + 100)? == 0
            && source.u32(at + 104)? == u32::MAX
            && source.u32(at + 108)? == 0,
        "unsupported source modifier metadata or flags"
    );
    for offset in [32, 64] {
        ensure!(
            (0x80800000..0x82000000).contains(&source.u32(at + offset)?),
            "source modifier metadata reference differs"
        );
    }
    let (component, input) = modifier_input(source.u8(at + 96)?, source.u16(at + 90)?)?;
    let settings = native.pointer(24)?;
    ensure!(
        settings >= 4
            && native.u32(settings - 4)? == 0x80803AFE
            && native
                .array(settings + 88, 88, Some(0x80803B06))?
                .contains(&to)
            && native.u32(to + 4)? == 0x80803B05,
        "unsupported native modifier-settings envelope"
    );
    let mut bytes = native.bytes::<88>(to)?;
    let instance = usize::try_from(native.u64(to + 8)?)?;
    ensure!(
        native.u32(instance)? == native.u32(to)?
            && native.u32(instance + 4)? == 0x80803B06
            && native.u64(instance + 8)? == to as u64
            && i64::from_le_bytes(native.bytes(to + 16)?) == -16
            && native.u32(to + 28)? == 0
            && native.u64(to + 32)? == 0
            && i64::from_le_bytes(native.bytes(to + 48)?) == -48
            && native.u32(to + 60)? == 0
            && native.u64(to + 64)? == 1
            && native.bytes::<3>(to + 77)? == [0; 3]
            && native.u32(to + 80)? == 0
            && native.u32(to + 84)? == u32::MAX,
        "native modifier pair or metadata differs"
    );
    for offset in [24, 56] {
        ensure!(
            (0x80800000..0x82000000).contains(&native.u32(to + offset)?),
            "native modifier metadata reference differs"
        );
    }
    bytes[40..44].copy_from_slice(&source.bytes::<4>(at + 16)?);
    bytes[44] = source.u8(at + 20)?;
    bytes[72..74].copy_from_slice(&source.bytes::<2>(at + 88)?);
    bytes[74..76].copy_from_slice(&input.to_le_bytes());
    bytes[76] = component;
    Ok(bytes)
}

/// Modern effect 31 registers a typed movement record. The shared Tome of Dawn package
/// pair establishes the 72-byte 639C to 692C record contract. Shadowkeep effect 36 accepts
/// this concrete class through D6F7A0 and removes the action-owned record on cleanup.
/// Preserve the complete record, including identity, flags, scalar bits and padding.
pub fn host_record(source: &Payload, at: usize) -> Result<Node> {
    ensure!(
        at >= 4 && source.u32(at - 4)? == 0x808030F3,
        "unsupported source host-record class"
    );
    ensure!(
        source.bytes::<8>(at)? == [31, 0, 0, 0, 0, 0, 0, 0],
        "unsupported source host-record dispatch or flags"
    );
    ensure!(source.u64(at + 8)? != 0, "source host record is absent");
    let record = source.pointer(at + 8)?;
    ensure!(
        record >= 4 && source.u32(record - 4)? == 0x8080639C,
        "unsupported source movement-record class"
    );
    let record = source.bytes::<72>(record)?;
    let mut bytes = vec![0; 104];
    bytes[0] = 36;
    bytes[8..16].copy_from_slice(&24i64.to_le_bytes());
    bytes[28..32].copy_from_slice(&0x8080692Cu32.to_le_bytes());
    bytes[32..].copy_from_slice(&record);
    Ok(Node {
        class: 0x80803E19,
        kind: 36,
        bytes,
    })
}

/// Translate an ordinary dynamic attachment and its four-lane value program. The caller
/// supplies an already translated native entity. A modern entity identifier is not a native
/// dependency. The shared Harmonic Laser packages independently establish this layout.
pub fn dynamic_entity(source: &Payload, at: usize, native_entity: u32) -> Result<Node> {
    ensure!(
        at >= 4 && source.u32(at - 4)? == 0x80803130,
        "unsupported source dynamic-attachment class"
    );
    let header = source.bytes::<8>(at)?;
    ensure!(
        header[0] == 2 && header[1] == 1 && header[2] <= 1 && header[3..] == [0; 5],
        "unsupported source dynamic-attachment dispatch or flags"
    );
    ensure!(
        (0x80800000..0x82000000).contains(&native_entity),
        "dynamic attachment requires a translated native entity"
    );
    source.bytes::<116>(at)?;
    ensure!(
        source.u64(at + 32)? == 0
            && source.u64(at + 72)? == 1
            && source.u64(at + 80)? == 1
            && source.u64(at + 88)? == 0
            && source.u64(at + 96)? == 0
            && source.u16(at + 106)? == 0
            && source.u32(at + 108)? == 0x811C9DC5
            && source.u32(at + 112)? == 0,
        "unsupported source dynamic-attachment input metadata"
    );
    let selector = source.u8(at + 104)?;
    let normalized = source.u8(at + 105)?;
    ensure!(
        matches!(selector, 0 | 1 | 255) && normalized <= 1,
        "unsupported source dynamic-attachment input selector"
    );
    ensure!(
        source.u64(at + 40)? <= 4096 && source.u64(at + 56)? <= 256,
        "dynamic-attachment program exceeds native limits"
    );
    let code_range = source.array_range(at + 40, 1, Some(0x80800009))?;
    let constant_range = source.array_range(at + 56, 16, Some(0x80800090))?;
    let constants = &source.0[constant_range];
    let code = crate::d2_mot::native::effects::lower_program(
        &source.0[code_range],
        constants.len() / 16,
        1,
    )?;
    let mut bytes = vec![0; 88];
    bytes[..8].copy_from_slice(&header);
    bytes[16..20].copy_from_slice(&native_entity.to_le_bytes());
    bytes[64..68].copy_from_slice(&1u32.to_le_bytes());
    bytes[72..76].copy_from_slice(&1u32.to_le_bytes());
    bytes[80] = selector;
    bytes[81] = normalized;
    append_array(&mut bytes, 32, 0x80800009, &code, 1)?;
    append_array(&mut bytes, 48, 0x80800090, constants, 16)?;
    Ok(Node {
        class: 0x80803E44,
        kind: 2,
        bytes,
    })
}

fn append_array(
    bytes: &mut Vec<u8>,
    at: usize,
    class: u32,
    rows: &[u8],
    stride: usize,
) -> Result<()> {
    ensure!(
        rows.len().is_multiple_of(stride),
        "native array stride differs"
    );
    if rows.is_empty() {
        return Ok(());
    }
    let header = bytes
        .len()
        .checked_add(19)
        .context("native array overflow")?
        & !15;
    bytes.resize(header - 4, 0);
    bytes.extend(0x80809FBDu32.to_le_bytes());
    let count = rows.len() as u64 / stride as u64;
    bytes.extend(count.to_le_bytes());
    bytes.extend((class as u64).to_le_bytes());
    bytes.extend(rows);
    bytes[at..at + 8].copy_from_slice(&count.to_le_bytes());
    let pointer_at = at + 8;
    let delta = i64::try_from(header)? - i64::try_from(pointer_at)?;
    bytes[pointer_at..pointer_at + 8].copy_from_slice(&delta.to_le_bytes());
    Ok(())
}
