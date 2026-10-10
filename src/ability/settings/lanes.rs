//! Settings a component definition keeps inside an opaque field: bytes the runtime graph does not
//! declare, of which a native reader takes one float, integer or byte at a fixed place. An edit
//! writes those bytes into the field's override and keeps every other byte as the draft has it,
//! so two settings of one field can both change.
//!
//! Each place below is the definition class, the offset in it and how the reader takes it, from
//! the verified ability property survey of build 86657.20.08.23.1800.d2_rc___release:
//! - `8080424E`, the Glide phase pair: +0x260 seconds the initial phase holds (`D15073` reads it
//!   at activation and converts it to ticks) and +0x3DC the fall speed that picks that phase and
//!   arms fall arrest (`D13B90`, `C54248` compare vertical velocity with its negation).
//! - `808044BF`, a jump, lift or glide preparation: +0x250 and +0x254 the seconds its impulse
//!   ramps in and fades out, each dividing the frame's seconds before a 0 to 1 weight clamp.
//! - `80804326`, Blink: +0x744 a number added to the travel direction's vertical part before it is
//!   normalized.
//! - `8080388F`, a projectile's settings: +0x5C and +0x60 the finish timer's endpoints, +0x64 the
//!   age limit for finishing travel, +0x68 the age it expires at, +0x6C a signed count of updates
//!   that expires it instead (-1 expires it by age), +0x70 what expiring does, +0x78 and +0x79 its
//!   pierce and bounce limits (zero lifts each), +0x7B how it looks for contacts and +0x7C its
//!   collision radius.
//! - `8080377D`, tracking: +0x1C0 the turn rate in radians per second, +0x1C4 and +0x1C8 the most
//!   it leads a target in seconds and distance, +0x1CC the steering axis threshold, +0x1D0 how much
//!   it leads, and +0x1D4 a proximity range (zero turns the branch off) with +0x1D8 and +0x1DC the
//!   timer endpoints it then sets.
//! - `80803F29`, a target query: +0x48 its delay, +0x4C its range and +0x50 the owner exception.
//! - An ability controller's energy settings, shared by every definition class that derives from
//!   `80803BF6`: +0xCC the energy activation spends, +0xD0 the rate active use spends, +0xD4 the
//!   energy ending spends, +0xD8 the seconds after it ends before recharge, +0xDC the energy a
//!   readiness check requires and +0xE0 the floor an energy synchronization keeps. Energy is a
//!   fraction of one charge, clamped to 0 through 1. The survey found them in the 25 classes of
//!   [`ENERGY_CONTROLLERS`].
use super::{BYTECODE_ROW, CONSTANT_ROW, ELEMENT, Kind, Setting, TRACKING, through};
use crate::runtime::{
    WeaponRuntimeFieldSource, WeaponRuntimeRoot, WeaponRuntimeValue, WeaponRuntimeValueKind,
};

const GLIDE_PHASE: u32 = 0x8080_424E;
const PREPARATION: u32 = 0x8080_44BF;
const BLINK: u32 = 0x8080_4326;
const PROJECTILE_SETTINGS: u32 = 0x8080_388F;
const TARGET_QUERY: u32 = 0x8080_3F29;
/// The ability controller definitions whose settings derive from `80803BF6`, each with the energy
/// settings at +0xCC to +0xE0 inside its opaque field at +0xC0.
const ENERGY_CONTROLLERS: [u32; 25] = [
    0x8080_2D43,
    0x8080_2D5D,
    0x8080_3FEE,
    0x8080_4148,
    0x8080_4161,
    0x8080_4166,
    0x8080_4191,
    0x8080_41A1,
    0x8080_41B0,
    0x8080_41B2,
    0x8080_41C2,
    0x8080_424C,
    0x8080_427D,
    0x8080_429C,
    0x8080_42BB,
    0x8080_42BD,
    0x8080_42BF,
    0x8080_4324,
    0x8080_4326,
    0x8080_4368,
    0x8080_43B3,
    0x8080_43C5,
    0x8080_4404,
    0x8080_44BC,
    0x8080_44E0,
];
/// The block at tracking +0x160 that holds its optional turn-rate program, which replaces the
/// fallback rate at +0x1C0 when present.
const TURN_PROGRAM: u32 = 0x8080_376F;
const TURN_PROGRAM_OFFSET: u32 = 0x160;

/// How a lane's bytes read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Codec {
    /// A little-endian 32-bit float.
    Float,
    /// A little-endian signed 32-bit integer.
    Integer,
    /// One signed byte.
    Byte,
    /// One unsigned byte, independent of adjacent flags and padding.
    UnsignedByte,
}

impl Codec {
    pub(super) const fn width(self) -> usize {
        match self {
            Self::Float | Self::Integer => 4,
            Self::Byte | Self::UnsignedByte => 1,
        }
    }
}

/// Where a setting's bytes start in its opaque field, counted from the field's first byte, and
/// how they read.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Lane {
    pub at: usize,
    pub codec: Codec,
}

impl Lane {
    /// The lane's bytes in `value`, an opaque field's bytes.
    pub(super) fn bytes(self, value: &WeaponRuntimeValue) -> Option<&[u8]> {
        let WeaponRuntimeValue::Bytes(bytes) = value else {
            return None;
        };
        bytes.get(self.at..self.at + self.codec.width())
    }

    /// The number the lane holds in `value`.
    pub(super) fn read(self, value: &WeaponRuntimeValue) -> Option<f32> {
        let bytes = self.bytes(value)?;
        Some(match self.codec {
            Codec::Float => f32::from_le_bytes(bytes.try_into().ok()?),
            Codec::Integer => i32::from_le_bytes(bytes.try_into().ok()?) as f32,
            Codec::Byte => f32::from(i8::from_le_bytes([*bytes.first()?])),
            Codec::UnsignedByte => f32::from(*bytes.first()?),
        })
    }

    /// `value` as the lane's bytes: the float itself, or a whole number kept in the codec's range.
    pub(super) fn encode(self, value: f32) -> Vec<u8> {
        match self.codec {
            Codec::Float => value.to_le_bytes().to_vec(),
            Codec::Integer => (value.round() as i32).to_le_bytes().to_vec(),
            Codec::Byte => (value.round().clamp(-128.0, 127.0) as i8)
                .to_le_bytes()
                .to_vec(),
            Codec::UnsignedByte => vec![value.round().clamp(0.0, 255.0) as u8],
        }
    }

    /// `value` with the lane's bytes replaced by `bytes`. `None` when `value` is not an opaque
    /// field that holds the lane.
    pub(super) fn with(
        self,
        value: &WeaponRuntimeValue,
        bytes: &[u8],
    ) -> Option<WeaponRuntimeValue> {
        let mut next = value.clone();
        let WeaponRuntimeValue::Bytes(all) = &mut next else {
            return None;
        };
        all.get_mut(self.at..self.at + bytes.len())?
            .copy_from_slice(bytes);
        Some(next)
    }
}

/// Each lane: the definition classes that hold it, the offset in them, the setting it holds and
/// how it reads.
const LANES: [(&[u32], u32, Kind, Codec); 32] = [
    (&[GLIDE_PHASE], 0x260, Kind::ImpulseHoldTime, Codec::Float),
    (
        &[GLIDE_PHASE],
        0x3DC,
        Kind::FallSpeedThreshold,
        Codec::Float,
    ),
    (&[PREPARATION], 0x250, Kind::ImpulseRampTime, Codec::Float),
    (&[PREPARATION], 0x254, Kind::ImpulseFadeTime, Codec::Float),
    (&[BLINK], 0x744, Kind::VerticalBias, Codec::Float),
    (
        &[PROJECTILE_SETTINGS],
        0x5C,
        Kind::MinimumFinishTime,
        Codec::Float,
    ),
    (
        &[PROJECTILE_SETTINGS],
        0x60,
        Kind::MaximumFinishTime,
        Codec::Float,
    ),
    (
        &[PROJECTILE_SETTINGS],
        0x64,
        Kind::FinishTravelTime,
        Codec::Float,
    ),
    (
        &[PROJECTILE_SETTINGS],
        0x68,
        Kind::ExpirationTime,
        Codec::Float,
    ),
    (
        &[PROJECTILE_SETTINGS],
        0x6C,
        Kind::ExpirationUpdates,
        Codec::Integer,
    ),
    (
        &[PROJECTILE_SETTINGS],
        0x70,
        Kind::ExpirationResponse,
        Codec::Byte,
    ),
    (&[PROJECTILE_SETTINGS], 0x78, Kind::PierceLimit, Codec::Byte),
    (&[PROJECTILE_SETTINGS], 0x79, Kind::BounceLimit, Codec::Byte),
    (
        &[PROJECTILE_SETTINGS],
        0x7B,
        Kind::CollisionMode,
        Codec::Byte,
    ),
    (
        &[PROJECTILE_SETTINGS],
        0x7C,
        Kind::CollisionRadius,
        Codec::Float,
    ),
    (&[TRACKING], 0x1C0, Kind::TurnRate, Codec::Float),
    (&[TRACKING], 0x1C4, Kind::LeadTimeLimit, Codec::Float),
    (&[TRACKING], 0x1C8, Kind::LeadDistanceLimit, Codec::Float),
    (
        &[TRACKING],
        0x1CC,
        Kind::SteeringAxisThreshold,
        Codec::Float,
    ),
    (&[TRACKING], 0x1D0, Kind::TargetLead, Codec::Float),
    (&[TRACKING], 0x1D4, Kind::ProximityRange, Codec::Float),
    (&[TRACKING], 0x1D8, Kind::MinimumProximityTime, Codec::Float),
    (&[TRACKING], 0x1DC, Kind::MaximumProximityTime, Codec::Float),
    (&[TARGET_QUERY], 0x48, Kind::SearchDelay, Codec::Float),
    (&[TARGET_QUERY], 0x4C, Kind::SearchRange, Codec::Float),
    (&[TARGET_QUERY], 0x50, Kind::IncludeSelf, Codec::Byte),
    (
        &ENERGY_CONTROLLERS,
        0xCC,
        Kind::ActivationEnergy,
        Codec::Float,
    ),
    (
        &ENERGY_CONTROLLERS,
        0xD0,
        Kind::ActiveEnergyRate,
        Codec::Float,
    ),
    (
        &ENERGY_CONTROLLERS,
        0xD4,
        Kind::EndingEnergyCost,
        Codec::Float,
    ),
    (&ENERGY_CONTROLLERS, 0xD8, Kind::RechargeDelay, Codec::Float),
    (
        &ENERGY_CONTROLLERS,
        0xDC,
        Kind::MinimumActivationEnergy,
        Codec::Float,
    ),
    (&ENERGY_CONTROLLERS, 0xE0, Kind::EnergyFloor, Codec::Float),
];

/// The lane settings `root`, a component definition `owner` holds, carries: each lane of its
/// class whose bytes lie inside exactly one opaque field of the root itself. `timed` says a target
/// query in the same graph sets its projectile's finish timer, which then ignores its own
/// endpoints (survey: every withheld finish-time occurrence sits in such a graph, and none of the
/// others does). `proximity` says an event row connects a tracking definition's proximity
/// trigger to a receiver, without which its range and timers do nothing.
pub(super) fn find(
    owner: u32,
    root: &WeaponRuntimeRoot,
    (timed, proximity): (bool, bool),
) -> Vec<Setting> {
    let mut found = Vec::new();
    for &(classes, offset, kind, codec) in &LANES {
        let class = root.schema;
        if !classes.contains(&class) || withheld(root, kind, (timed, proximity)) {
            continue;
        }
        let end = offset + codec.width() as u32;
        let mut covering = root.fields.iter().filter(|field| {
            field.source != WeaponRuntimeFieldSource::NativeDeclaration
                && field.locator.type_handle.get() == class
                && matches!(field.value, WeaponRuntimeValue::Bytes(_))
                && field.locator.value_offset <= offset
                && field
                    .locator
                    .value_offset
                    .checked_add(field.locator.byte_size)
                    .is_some_and(|field_end| field_end >= end)
        });
        let (Some(field), None) = (covering.next(), covering.next()) else {
            continue;
        };
        let lane = Lane {
            at: (offset - field.locator.value_offset) as usize,
            codec,
        };
        if lane.read(&field.value).is_some() {
            found.push(Setting {
                kind,
                owner_tag: owner,
                field: field.clone(),
                lane: Some(lane),
                requires_positive_minimum: false,
            });
        }
    }
    found
}

/// Compose disjoint lanes in one opaque field, and retain the first verified role when two
/// meanings address the same bytes. Definitions can overlap in their owner's payload. Survey
/// graph `80B81666` keeps Impulse Hold Time and Impulse Ramp Time in the same four bytes, even
/// after their fields have been rebased to one shared override container.
pub(super) fn separate(mut settings: Vec<Setting>) -> Vec<Setting> {
    let rank = |setting: &Setting| {
        LANES
            .iter()
            .position(|&(_, _, kind, _)| kind == setting.kind)
            .unwrap_or(LANES.len())
    };
    let span = |setting: &Setting| {
        let start = setting.field.owner_offset;
        (start, start.saturating_add(setting.field.locator.byte_size))
    };
    // A scalar reached through two embedded roots can use one shared opaque field. Compose
    // their disjoint lanes in that field instead of hiding one simply because its old container
    // overlapped. Exact native declarations can join an existing opaque override the same way.
    let original = settings.clone();
    for setting in &mut settings {
        let codec = match setting.lane {
            Some(lane) => lane.codec,
            None if matches!(setting.kind, Kind::Native(_)) => match setting.field.kind {
                WeaponRuntimeValueKind::Float32 => Codec::Float,
                WeaponRuntimeValueKind::SignedInteger { bits: 32 } => Codec::Integer,
                WeaponRuntimeValueKind::SignedInteger { bits: 8 } => Codec::Byte,
                WeaponRuntimeValueKind::UnsignedInteger { bits: 8 } => Codec::UnsignedByte,
                _ => continue,
            },
            None => continue,
        };
        let start = setting.offset();
        let end = start + codec.width() as u32;
        let container = original
            .iter()
            .filter(|candidate| {
                candidate.owner_tag == setting.owner_tag
                    && matches!(candidate.field.value, WeaponRuntimeValue::Bytes(_))
                    && candidate.field.owner_offset <= start
                    && candidate
                        .field
                        .owner_offset
                        .checked_add(candidate.field.locator.byte_size)
                        .is_some_and(|limit| limit >= end)
            })
            .min_by_key(|candidate| {
                (
                    std::cmp::Reverse(candidate.field.locator.byte_size),
                    rank(candidate),
                    &candidate.field.locator,
                )
            });
        if let Some(container) = container {
            setting.field = container.field.clone();
            setting.lane = Some(Lane {
                at: (start - setting.field.owner_offset) as usize,
                codec,
            });
        }
    }
    let mut order = (0..settings.len())
        .filter(|&index| settings[index].lane.is_some())
        .collect::<Vec<_>>();
    order.sort_by_key(|&index| rank(&settings[index]));
    let mut kept = Vec::<&Setting>::new();
    let mut withheld = Vec::new();
    for index in order {
        let setting = &settings[index];
        let (start, end) = span(setting);
        let shares = kept.iter().any(|each| {
            let (each_start, each_end) = span(each);
            let overlaps_value = setting.kind != each.kind
                && setting.offset()
                    < each.offset() + each.lane.expect("ordered lanes").codec.width() as u32
                && each.offset()
                    < setting.offset() + setting.lane.expect("ordered lanes").codec.width() as u32;
            each.owner_tag == setting.owner_tag
                && (overlaps_value
                    || (each.field.locator != setting.field.locator
                        && start < each_end
                        && each_start < end))
        });
        if shares {
            withheld.push(index);
        } else {
            kept.push(setting);
        }
    }
    settings
        .into_iter()
        .enumerate()
        .filter(|(index, _)| !withheld.contains(index))
        .map(|(_, setting)| setting)
        .collect()
}

/// Whether another reader takes over a lane's role in this graph, or nothing reads it, so editing
/// it would do nothing.
fn withheld(root: &WeaponRuntimeRoot, kind: Kind, (timed, proximity): (bool, bool)) -> bool {
    match kind {
        Kind::MinimumFinishTime | Kind::MaximumFinishTime => timed,
        Kind::TurnRate => turn_program(root),
        Kind::ProximityRange | Kind::MinimumProximityTime | Kind::MaximumProximityTime => {
            !proximity
        }
        _ => false,
    }
}

/// Whether tracking holds the optional turn-rate program at +0x160: any bytecode or constant row
/// of a program in that block.
fn turn_program(root: &WeaponRuntimeRoot) -> bool {
    root.fields.iter().any(|field| {
        through(field, TURN_PROGRAM, TURN_PROGRAM_OFFSET)
            && field.locator.path.iter().any(|step| {
                step.name_hash == ELEMENT
                    && [BYTECODE_ROW, CONSTANT_ROW].contains(&step.type_handle.get())
            })
    })
}
