//! Settings of an ability's parts whose native readers establish what they do: a spawned
//! object's health regions, an incoming damage filter and its multipliers, and an invisibility
//! attachment's thresholds and cleanup.
//!
//! Each is a typed field the runtime graph reads as a native declaration, so an edit is an
//! ordinary field override the build writes into a private copy of its owner. A setting shows
//! only where its reader acts on it:
//!
//! - A health region row (`80804C5F`, in the `80804B7F` array at +0x368 of `80804B8A`) takes its
//!   capacity, delays and recovery time only when its recovery flags at +0x1C have neither 0x2
//!   nor 0x4 set (`test al, 6` at `B8B6F5`, `B8BA45` and `B8BEFD`). The flags are not a declared
//!   field, so they are read from the owner's payload.
//! - An incoming damage filter (`80803F8C`) inverts its source test only when it names a source
//!   property at +0xE0, since the empty hash `811C9DC5` skips the test (`F83F49`) and keeps the
//!   owner result. Source Filter turns that test off by writing the empty hash, and on again by
//!   restoring the stock key, so only a filter that names one offers it. Its base program at +0x80
//!   is a multiplier only when it is `34 00 3E 00`, push constant 0 and store, with one constant,
//!   whose first lane is the factor.
//! - An invisibility attachment (`808043EC`, settings `808043E2` at +0x48) waits Retirement Delay
//!   only when its Attachment Flags hold 0x20, which Retire on Removal sets.
//! - A component program a native reader evaluates for one result, such as a target query's
//!   scale or a spawn row's attempt count, is a setting only when it is `34 00 3E 00` with one
//!   constant no other program shares. Its first lane is then the whole result. [`ROLES`] lists
//!   each program by where it sits. An incoming damage filter's conditional rows (`80802A1C`)
//!   each hold such a program at +0x50, which the filter multiplies in when the row's predicate
//!   matches (`F840E3`, `F840FB`).
//! - A program that scales one input and adds a constant, `3C n 34 00 34 01 12 3E 00`, gives two
//!   settings: the input's coefficient and the offset. [`AFFINE_ROLES`] lists each by where it
//!   sits.
//! - An attachment spawner program that some parts compute from their inputs, rather than hold as
//!   one constant, gives the constant it adds last as an offset setting, in one of the
//!   [`OFFSET_SHAPES`] and only while its descriptor takes no fast path ([`OFFSET_ROLES`]).
//! - A value a definition keeps inside an opaque field, such as a projectile's expiration age or
//!   a Glide's initial phase, is a few bytes of that field at a fixed place ([`lanes`]).
use std::collections::{BTreeMap, BTreeSet};

mod lanes;
pub use lanes::{Codec, Lane};
pub(crate) mod native;
pub use native::Property as NativeProperty;
pub use native::{Creation, creations};
mod validation;
pub use validation::validate_values;

use crate::package_runtime::reader::PackageManager;
use crate::runtime::{
    WeaponRuntimeField, WeaponRuntimeFieldSource, WeaponRuntimeGraph, WeaponRuntimePathElement,
    WeaponRuntimeRoot, WeaponRuntimeValue, WeaponRuntimeValueOverride,
};

const HEALTH: u32 = 0x8080_4B8A;
const REGION_ROWS: u32 = 0x8080_4B7F;
const REGION_ROWS_OFFSET: u32 = 0x368;
const REGION_ROW: u32 = 0x8080_4C5F;
/// The region row's recovery flags, and the flags that bypass its own recovery values.
const REGION_FLAGS: usize = 0x1C;
const REGION_BYPASS: u8 = 0x6;
const DAMAGE_FILTER: u32 = 0x8080_3F8C;
const SOURCE_PROPERTY: u32 = 0xE0;
const NO_SOURCE_PROPERTY: u64 = 0x811C_9DC5;
const BASE_PROGRAM_OFFSET: u32 = 0x80;
const PROGRAM: u32 = 0x8080_89F6;
const INVISIBILITY: u32 = 0x8080_43EC;
const INVISIBILITY_SETTINGS: u32 = 0x8080_43E2;
const INVISIBILITY_SETTINGS_OFFSET: u32 = 0x48;
const ATTACHMENT_FLAGS: u32 = 0x298;
const RETIRE: u64 = 0x20;
const POINTER: u32 = 0x504E_5000;
const ELEMENT: u32 = 0x504E_4100;
const ARRAY: u32 = 0x8080_9FBD;
const BYTECODE_ROW: u32 = 0x8080_0009;
const CONSTANT_ROW: u32 = 0x8080_0090;
/// Push constant 0, store.
const PUSH_AND_STORE: [u8; 4] = [0x34, 0x00, 0x3E, 0x00];
/// A component that moves its graph as a projectile.
const MOVING_PROJECTILE: u32 = 0x8080_3B73;
/// The program bodies whose bytecode and constant arrays a literal reads. An incoming damage
/// filter's conditional row holds its program's arrays itself, and tracking's optional turn-rate
/// program sits in a `80803773` body.
const PROGRAM_BODIES: [u32; 5] = [
    0x8080_89F8,
    0x8080_3775,
    0x8080_89F4,
    CONDITIONAL_ROW,
    TURN_PROGRAM_BODY,
];
/// An incoming damage filter's conditional rows: the holder at +0x70 of the filter, the array at
/// +0x8 of it, and each 0xA0-byte row with its program at +0x50. The program's descriptor keeps
/// its fast-path index at +0x3C, which must be zero for the interpreter to read the constant.
const CONDITIONAL_HOLDER: u32 = 0x8080_2A1A;
const CONDITIONAL_HOLDER_OFFSET: u32 = 0x70;
const CONDITIONAL_ROW: u32 = 0x8080_2A1C;
const CONDITIONAL_ROW_SIZE: usize = 0xA0;
const CONDITIONAL_FAST_PATH: usize = 0x50 + 0x3C;
/// Tracking's optional turn-rate program: the block at +0x160 and the body it points to.
const TURN_PROGRAM_BLOCK: u32 = 0x8080_376F;
const TURN_PROGRAM_BODY: u32 = 0x8080_3773;
/// Where a tracking definition keeps the event source its proximity trigger fires, and the
/// classes an event row names for that source and for a destination that receives it.
const PROXIMITY_SOURCE: u32 = 0x1F8;
const EVENT_SOURCE: u32 = 0x8080_9BD9;
const EVENT_DESTINATION: u32 = 0x8080_3891;
/// An incoming damage filter's own branch, which takes precedence over Owner Damage Only at
/// +0xE8. It is not a declared field, so it is read from the owner's payload.
const FILTER_BRANCH: usize = 0xE9;
/// A target query emitter's definition, and the flags that send its accepted response to the
/// projectile timer: +0x1A8 on and the attachment branch at +0x14A off (`D0796D`, `D079CA`).
const QUERY_EMITTER: u32 = 0x8080_3813;
const QUERY_TIMER: u32 = 0x1A8;
const QUERY_ATTACHMENT: u32 = 0x14A;

/// How a setting's value reads.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Unit {
    Factor,
    Seconds,
    Flag,
    Number,
    /// A distance in the game's own units, which Travel Limit reads in too.
    Distance,
    /// A whole number of attempts, targets or generations.
    Count,
}

/// A component program whose evaluated result one native reader takes, by the classes and
/// offsets from the root to its program body. An offset of `None` takes its class wherever it
/// sits. Each reader was traced in build 86657.
struct Role {
    kind: Kind,
    steps: &'static [(u32, Option<u32>)],
    /// Whether the steps start at the root, or end the path wherever another class embeds them.
    from_root: bool,
    /// Whether only a graph moving as a projectile (`80803B73`) reads it this way.
    projectile: bool,
}

const fn role(kind: Kind, steps: &'static [(u32, Option<u32>)], projectile: bool) -> Role {
    Role {
        kind,
        steps,
        from_root: true,
        projectile,
    }
}

const QUERY_SHAPE: u32 = 0x8080_3816;
const TRACKING: u32 = 0x8080_377D;
/// The block at tracking +0x158 that holds its optional speed program.
const TRACKING_SPEED_SLOT: u32 = 0x8080_3770;
const TRACKING_SPEED: u32 = 0x8080_3775;
const ATTACHMENT_SPAWNER: u32 = 0x8080_37A9;
const ENERGY_GATE: u32 = 0x8080_4562;
const PROGRAM_SLOT: u32 = 0x8080_89F6;
const PROGRAM_BODY: u32 = 0x8080_89F8;
/// The body of an invisibility attachment's damage response programs.
const RESPONSE_BODY: u32 = 0x8080_89F4;

/// Where each program setting sits. The target query scales its shape's uniform lane
/// (`CE3D70`). The query's duration is the projectile timer limit after an accepted response
/// (`D000E0`). The energy gate's cost and requirement are read at +0x30 and +0x90 wherever a
/// `80804562` block sits. Tracking multiplies its strength programs at runtime and limits its
/// speed by 3775 +0x78. The attachment spawner's six programs are its acquisition scale,
/// per-target limit, chance threshold, attempts per row, generation limit and total limit. An
/// invisibility attachment's two damage responses add to its break accumulator and its strength
/// loss for each qualifying hit. An incoming damage filter's conditional row multiplies its
/// constant into the incoming factor when its predicate matches.
const ROLES: [Role; 18] = [
    role(
        Kind::DamageBreakResponse,
        &[
            (INVISIBILITY, Some(0)),
            (INVISIBILITY_SETTINGS, Some(INVISIBILITY_SETTINGS_OFFSET)),
            (RESPONSE_BODY, Some(0x18)),
        ],
        false,
    ),
    role(
        Kind::StrengthLoss,
        &[
            (INVISIBILITY, Some(0)),
            (INVISIBILITY_SETTINGS, Some(INVISIBILITY_SETTINGS_OFFSET)),
            (RESPONSE_BODY, Some(0xC8)),
        ],
        false,
    ),
    role(
        Kind::QueryScale,
        &[
            (QUERY_EMITTER, Some(0)),
            (QUERY_SHAPE, Some(0x48)),
            (PROGRAM_BODY, Some(0x18)),
        ],
        false,
    ),
    role(
        Kind::TriggeredDuration,
        &[(QUERY_EMITTER, Some(0)), (PROGRAM_BODY, Some(0x158))],
        true,
    ),
    Role {
        kind: Kind::EnergyCost,
        steps: &[
            (ENERGY_GATE, None),
            (PROGRAM_SLOT, Some(0x30)),
            (PROGRAM_BODY, Some(0)),
        ],
        from_root: false,
        projectile: false,
    },
    Role {
        kind: Kind::RequiredEnergy,
        steps: &[
            (ENERGY_GATE, None),
            (PROGRAM_SLOT, Some(0x90)),
            (PROGRAM_BODY, Some(0)),
        ],
        from_root: false,
        projectile: false,
    },
    role(
        Kind::TrackingStrength,
        &[(TRACKING, Some(0)), (PROGRAM_BODY, Some(0x58))],
        true,
    ),
    role(
        Kind::DistanceTracking,
        &[(TRACKING, Some(0)), (PROGRAM_BODY, Some(0xB0))],
        true,
    ),
    role(
        Kind::BounceTracking,
        &[(TRACKING, Some(0)), (PROGRAM_BODY, Some(0x108))],
        true,
    ),
    role(
        Kind::TrackingVariation,
        &[(TRACKING, Some(0)), (PROGRAM_BODY, Some(0x170))],
        true,
    ),
    role(
        Kind::TrackingSpeed,
        &[
            (TRACKING, Some(0)),
            (TRACKING_SPEED_SLOT, Some(0x158)),
            (TRACKING_SPEED, Some(0)),
        ],
        true,
    ),
    role(
        Kind::AcquisitionScale,
        &[
            (ATTACHMENT_SPAWNER, Some(0)),
            (PROGRAM_SLOT, Some(0x168)),
            (PROGRAM_BODY, Some(0)),
        ],
        false,
    ),
    role(
        Kind::TargetLimit,
        &[
            (ATTACHMENT_SPAWNER, Some(0)),
            (PROGRAM_SLOT, Some(0x1C8)),
            (PROGRAM_BODY, Some(0)),
        ],
        false,
    ),
    role(
        Kind::SpawnChance,
        &[
            (ATTACHMENT_SPAWNER, Some(0)),
            (PROGRAM_SLOT, Some(0x228)),
            (PROGRAM_BODY, Some(0)),
        ],
        false,
    ),
    role(
        Kind::SpawnCount,
        &[
            (ATTACHMENT_SPAWNER, Some(0)),
            (PROGRAM_SLOT, Some(0x288)),
            (PROGRAM_BODY, Some(0)),
        ],
        false,
    ),
    role(
        Kind::GenerationLimit,
        &[
            (ATTACHMENT_SPAWNER, Some(0)),
            (PROGRAM_SLOT, Some(0x2E8)),
            (PROGRAM_BODY, Some(0)),
        ],
        false,
    ),
    role(
        Kind::SpawnLimit,
        &[
            (ATTACHMENT_SPAWNER, Some(0)),
            (PROGRAM_SLOT, Some(0x348)),
            (PROGRAM_BODY, Some(0)),
        ],
        false,
    ),
    role(
        Kind::ConditionalDamage,
        &[
            (DAMAGE_FILTER, Some(0)),
            (CONDITIONAL_HOLDER, Some(CONDITIONAL_HOLDER_OFFSET)),
            (ARRAY, Some(0x8)),
            (CONDITIONAL_ROW, None),
        ],
        false,
    ),
];

/// A program that scales input `input` by its first constant and adds its second, by where it
/// sits from its root, with the setting each constant is.
struct AffineRole {
    gain: Kind,
    offset: Kind,
    input: u8,
    steps: &'static [(u32, Option<u32>)],
    /// Whether only a graph moving as a projectile (`80803B73`) reads it this way.
    projectile: bool,
}

/// Tracking's optional turn-rate program, which replaces the fallback Turn Rate when present.
/// `BBE34F` evaluates it, and `BBE358` converts its degrees to radians. Input 5 is the named
/// `fast_throw_tracking` parameter (`99E042AE`), which a bank parameter resets to zero and an
/// applied key sets to one.
const AFFINE_ROLES: [AffineRole; 1] = [AffineRole {
    gain: Kind::FastThrowTracking,
    offset: Kind::TurnRateOffset,
    input: 5,
    steps: &[
        (TRACKING, Some(0)),
        (TURN_PROGRAM_BLOCK, Some(0x160)),
        (TURN_PROGRAM_BODY, Some(0)),
    ],
    projectile: true,
}];

/// A program whose constant `added` is the last term it adds, so editing that constant moves
/// the result by the same amount whatever its inputs give: its bytecode, its constant count and
/// the input count its descriptor declares. Opcode 01 adds the two vectors on top of the stack
/// (`3B7CB7`), and 12 multiplies the two below the top and adds the top (`3B839A`, `3B83C9`).
struct Shape {
    code: &'static [u8],
    constants: usize,
    inputs: u32,
    added: usize,
}

/// `C0 + input 1`, `input 1 + C0`, `C0 × input 2 + (C1 + input 1)` and `C0 × input 1 + C1`.
const OFFSET_SHAPES: [Shape; 4] = [
    Shape {
        code: &[0x34, 0x00, 0x3C, 0x01, 0x01, 0x3E, 0x00],
        constants: 1,
        inputs: 2,
        added: 0,
    },
    Shape {
        code: &[0x3C, 0x01, 0x34, 0x00, 0x01, 0x3E, 0x00],
        constants: 1,
        inputs: 2,
        added: 0,
    },
    Shape {
        code: &[
            0x34, 0x00, 0x3C, 0x02, 0x34, 0x01, 0x3C, 0x01, 0x01, 0x12, 0x3E, 0x00,
        ],
        constants: 2,
        inputs: 3,
        added: 1,
    },
    Shape {
        code: &[0x34, 0x00, 0x3C, 0x01, 0x34, 0x01, 0x12, 0x3E, 0x00],
        constants: 2,
        inputs: 2,
        added: 1,
    },
];

/// The attachment spawner's programs that some stock parts compute from their inputs instead of
/// holding as one constant, by their slot: the acquisition scale, the attempts per row, the
/// generation limit and the total limit. Their readers are the literal settings' readers, which
/// take the result's X lane (`578DF4`).
const OFFSET_ROLES: [(Kind, u32); 4] = [
    (Kind::AcquisitionScaleOffset, 0x168),
    (Kind::SpawnCountOffset, 0x288),
    (Kind::GenerationLimitOffset, 0x2E8),
    (Kind::SpawnLimitOffset, 0x348),
];

/// Where a program's descriptor keeps its input count, a second count, its output count and its
/// fast-path index, as four words after its bytecode and constant arrays. The interpreter reads
/// the arrays only while the fast-path index is zero (`3B7AA2`, `3B7AB4`).
const PROGRAM_WORDS: usize = 0x30;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Kind {
    Native(NativeProperty),
    HealthScale,
    RecoveryDelay,
    DepletedRecoveryDelay,
    RecoveryTime,
    OwnerDamageOnly,
    SourceFilter,
    InvertSourceFilter,
    IncomingDamage,
    ConditionalDamage,
    DamageBreakThreshold,
    SuppressionTime,
    MovementThreshold,
    IgnoreMovement,
    RetireOnRemoval,
    RetirementDelay,
    DisruptionGracePeriod,
    DamageBreakResponse,
    StrengthLoss,
    QueryScale,
    TriggeredDuration,
    EnergyCost,
    RequiredEnergy,
    TrackingStrength,
    DistanceTracking,
    BounceTracking,
    TrackingVariation,
    TrackingSpeed,
    FastThrowTracking,
    TurnRateOffset,
    AcquisitionScale,
    TargetLimit,
    SpawnChance,
    SpawnCount,
    GenerationLimit,
    SpawnLimit,
    AcquisitionScaleOffset,
    SpawnCountOffset,
    GenerationLimitOffset,
    SpawnLimitOffset,
    ImpulseHoldTime,
    FallSpeedThreshold,
    ImpulseRampTime,
    ImpulseFadeTime,
    VerticalBias,
    MinimumFinishTime,
    MaximumFinishTime,
    FinishTravelTime,
    ExpirationTime,
    ExpirationUpdates,
    ExpirationResponse,
    PierceLimit,
    BounceLimit,
    CollisionMode,
    CollisionRadius,
    TurnRate,
    LeadTimeLimit,
    LeadDistanceLimit,
    SteeringAxisThreshold,
    TargetLead,
    ProximityRange,
    MinimumProximityTime,
    MaximumProximityTime,
    SearchDelay,
    SearchRange,
    IncludeSelf,
    ActivationEnergy,
    ActiveEnergyRate,
    EndingEnergyCost,
    RechargeDelay,
    MinimumActivationEnergy,
    EnergyFloor,
}

/// Tracking stores its turn rate in radians per second and the tile shows degrees.
const DEGREES_PER_RADIAN: f32 = 180.0 / std::f32::consts::PI;

impl Kind {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Native(property) => property.label(),
            Self::HealthScale => "Health Scale",
            Self::RecoveryDelay => "Recovery Delay",
            Self::DepletedRecoveryDelay => "Depleted Recovery Delay",
            Self::RecoveryTime => "Recovery Time",
            Self::OwnerDamageOnly => "Owner Damage Only",
            Self::SourceFilter => "Source Filter",
            Self::InvertSourceFilter => "Invert Source Filter",
            Self::IncomingDamage => "Incoming Damage",
            Self::ConditionalDamage => "Conditional Damage",
            Self::DamageBreakThreshold => "Damage Break Threshold",
            Self::SuppressionTime => "Suppression Time",
            Self::MovementThreshold => "Movement Threshold",
            Self::IgnoreMovement => "Ignore Movement",
            Self::RetireOnRemoval => "Retire on Removal",
            Self::RetirementDelay => "Retirement Delay",
            Self::DisruptionGracePeriod => "Grace Period",
            Self::DamageBreakResponse => "Hit Disruption",
            Self::StrengthLoss => "Hit Strength Loss",
            Self::QueryScale => "Target Search Size",
            Self::TriggeredDuration => "Life After Trigger",
            Self::EnergyCost => "Energy Cost",
            Self::RequiredEnergy => "Required Energy",
            Self::TrackingStrength => "Tracking Strength",
            Self::DistanceTracking => "Distance Tracking",
            Self::BounceTracking => "Bounce Tracking",
            Self::TrackingVariation => "Tracking Variation",
            Self::TrackingSpeed => "Tracking Speed",
            Self::FastThrowTracking => "Fast-Throw Tracking",
            Self::TurnRateOffset => "Turn Rate Offset",
            Self::AcquisitionScale => "Spawn Search Size",
            Self::TargetLimit => "Picks per Target",
            Self::SpawnChance => "Spawn Chance",
            Self::SpawnCount => "Spawn Attempts",
            Self::GenerationLimit => "Chain Depth",
            Self::SpawnLimit => "Max Spawns",
            Self::AcquisitionScaleOffset => "Spawn Search Size Offset",
            Self::SpawnCountOffset => "Spawn Attempts Offset",
            Self::GenerationLimitOffset => "Chain Depth Offset",
            Self::SpawnLimitOffset => "Max Spawns Offset",
            Self::ImpulseHoldTime => "Impulse Hold Time",
            Self::FallSpeedThreshold => "Fall Speed Threshold",
            Self::ImpulseRampTime => "Impulse Ramp Time",
            Self::ImpulseFadeTime => "Impulse Fade Time",
            Self::VerticalBias => "Vertical Bias",
            Self::MinimumFinishTime => "Minimum Finish Time",
            Self::MaximumFinishTime => "Maximum Finish Time",
            Self::FinishTravelTime => "Finish Travel Time",
            Self::ExpirationTime => "Expiration Time",
            Self::ExpirationUpdates => "Expiration Updates",
            Self::ExpirationResponse => "Expiration Response",
            Self::PierceLimit => "Pierce Limit",
            Self::BounceLimit => "Bounce Limit",
            Self::CollisionMode => "Collision Mode",
            Self::CollisionRadius => "Collision Radius",
            Self::TurnRate => "Turn Rate",
            Self::LeadTimeLimit => "Lead Time Limit",
            Self::LeadDistanceLimit => "Lead Distance Limit",
            Self::SteeringAxisThreshold => "Steering Axis Threshold",
            Self::TargetLead => "Target Lead",
            Self::ProximityRange => "Proximity Range",
            Self::MinimumProximityTime => "Minimum Proximity Time",
            Self::MaximumProximityTime => "Maximum Proximity Time",
            Self::SearchDelay => "Search Delay",
            Self::SearchRange => "Search Range",
            Self::IncludeSelf => "Include Self",
            Self::ActivationEnergy => "Activation Energy",
            Self::ActiveEnergyRate => "Active Energy Rate",
            Self::EndingEnergyCost => "Ending Energy Cost",
            Self::RechargeDelay => "Recharge Delay",
            Self::MinimumActivationEnergy => "Minimum Activation Energy",
            Self::EnergyFloor => "Energy Floor",
        }
    }

    #[must_use]
    pub const fn hint(self) -> &'static str {
        match self {
            Self::Native(property) => property.hint(),
            Self::HealthScale => "Health of this region of the object",
            Self::RecoveryDelay => "Wait before the region recovers after damage",
            Self::DepletedRecoveryDelay => "Wait before a depleted region recovers",
            Self::RecoveryTime => "Time the region takes to recover fully. Zero turns recovery off",
            Self::OwnerDamageOnly => "Only damage from the same owner counts",
            Self::SourceFilter => "Tests damage for the required source property",
            Self::InvertSourceFilter => "Counts damage without the required source property",
            Self::IncomingDamage => "Scales the damage it takes while active",
            Self::ConditionalDamage => "Scales matching damage it takes",
            Self::DamageBreakThreshold => "Damage that breaks invisibility. Zero never breaks",
            Self::SuppressionTime => "How long a break holds invisibility off",
            Self::MovementThreshold => "Movement that disrupts invisibility",
            Self::IgnoreMovement => "Movement does not disrupt invisibility",
            Self::RetireOnRemoval => "Removes the effect after Retirement Delay",
            Self::RetirementDelay => "Wait before the effect is removed",
            Self::DisruptionGracePeriod => "Wait before damage or movement can disrupt it",
            Self::DamageBreakResponse => {
                "Disruption each qualifying hit adds. Breaks only past a Damage Break Threshold"
            }
            Self::StrengthLoss => "Invisibility strength each qualifying hit removes",
            Self::QueryScale => "Size of the shape it finds targets with",
            Self::TriggeredDuration => "Seconds it lasts once it finds a target",
            Self::EnergyCost => "Energy this spends. 1 is a full charge",
            Self::RequiredEnergy => "Energy needed to use it. 1 is a full charge",
            Self::TrackingStrength => "Steering strength over time",
            Self::DistanceTracking => "Steering strength by target distance",
            Self::BounceTracking => "Turn strength after bounces",
            Self::TrackingVariation => "Variation in its steering",
            Self::TrackingSpeed => "Speed while tracking",
            Self::FastThrowTracking => "Turn rate added by its fast-throw input",
            Self::TurnRateOffset => "Turn rate added to the computed rate",
            Self::AcquisitionScale => "Size of the shape it finds spawn targets with",
            Self::TargetLimit => "Times one target can be picked",
            Self::SpawnChance => "Chance each attempt passes, 0 to 1",
            Self::SpawnCount => "Attempts for each spawn",
            Self::GenerationLimit => "Generations of attachments allowed",
            Self::SpawnLimit => "Accepted attempts in each batch",
            Self::AcquisitionScaleOffset => "Search size added to what its inputs give",
            Self::SpawnCountOffset => "Attempts added to what its inputs give",
            Self::GenerationLimitOffset => "Generations added to what its inputs give",
            Self::SpawnLimitOffset => "Attempts added to what its inputs give",
            Self::ImpulseHoldTime => "How long Glide's first phase holds",
            Self::FallSpeedThreshold => "Fall speed that changes how Glide starts",
            Self::ImpulseRampTime => "Time to the full preparation impulse",
            Self::ImpulseFadeTime => "Time for the preparation impulse to fade",
            Self::VerticalBias => "Tilts the Blink direction upward",
            Self::MinimumFinishTime => "Shortest finish timer it draws",
            Self::MaximumFinishTime => "Longest finish timer it draws",
            Self::FinishTravelTime => "Age limit for finishing travel",
            Self::ExpirationTime => "Age it expires at",
            Self::ExpirationUpdates => "Updates before it expires. By Age uses Expiration Time",
            Self::ExpirationResponse => "What it does when it expires",
            Self::PierceLimit => "Targets it passes through. 0 has no limit",
            Self::BounceLimit => "Bounces it makes. 0 has no limit",
            Self::CollisionMode => "How it finds contacts",
            Self::CollisionRadius => "Contact size",
            Self::TurnRate => "Base steering speed in degrees per second",
            Self::LeadTimeLimit => "Most it leads a moving target",
            Self::LeadDistanceLimit => "Farthest it leads a moving target",
            Self::SteeringAxisThreshold => "When it turns about a fallback axis",
            Self::TargetLead => "How much it leads a moving target",
            Self::ProximityRange => "Target distance that triggers it. 0 is off",
            Self::MinimumProximityTime => "Shortest timer a trigger sets",
            Self::MaximumProximityTime => "Longest timer a trigger sets",
            Self::SearchDelay => "Wait before it searches for targets",
            Self::SearchRange => "Farthest target it accepts",
            Self::IncludeSelf => "Lets its owner pass the target check",
            Self::ActivationEnergy => "Energy activating spends. 1 is a full charge",
            Self::ActiveEnergyRate => "Energy spent each second while active",
            Self::EndingEnergyCost => "Energy spent when it ends. 1 is a full charge",
            Self::RechargeDelay => "Wait after it ends before recharging",
            Self::MinimumActivationEnergy => "Energy it needs to be ready. 1 is a full charge",
            Self::EnergyFloor => "Lowest energy kept when energy syncs",
        }
    }

    #[must_use]
    pub const fn unit(self) -> Unit {
        match self {
            Self::Native(property) => property.unit(),
            Self::HealthScale
            | Self::IncomingDamage
            | Self::ConditionalDamage
            | Self::QueryScale
            | Self::TrackingStrength
            | Self::DistanceTracking
            | Self::BounceTracking
            | Self::TrackingVariation
            | Self::AcquisitionScale
            | Self::TargetLead => Unit::Factor,
            Self::RecoveryDelay
            | Self::DepletedRecoveryDelay
            | Self::RecoveryTime
            | Self::SuppressionTime
            | Self::RetirementDelay
            | Self::DisruptionGracePeriod
            | Self::TriggeredDuration
            | Self::ImpulseHoldTime
            | Self::ImpulseRampTime
            | Self::ImpulseFadeTime
            | Self::MinimumFinishTime
            | Self::MaximumFinishTime
            | Self::FinishTravelTime
            | Self::ExpirationTime
            | Self::LeadTimeLimit
            | Self::MinimumProximityTime
            | Self::MaximumProximityTime
            | Self::SearchDelay
            | Self::RechargeDelay => Unit::Seconds,
            Self::OwnerDamageOnly
            | Self::SourceFilter
            | Self::InvertSourceFilter
            | Self::IgnoreMovement
            | Self::RetireOnRemoval
            | Self::IncludeSelf => Unit::Flag,
            Self::DamageBreakThreshold
            | Self::MovementThreshold
            | Self::EnergyCost
            | Self::RequiredEnergy
            | Self::TrackingSpeed
            | Self::FastThrowTracking
            | Self::TurnRateOffset
            | Self::SpawnChance
            | Self::AcquisitionScaleOffset
            | Self::DamageBreakResponse
            | Self::StrengthLoss
            | Self::FallSpeedThreshold
            | Self::VerticalBias
            | Self::TurnRate
            | Self::SteeringAxisThreshold
            | Self::ActivationEnergy
            | Self::ActiveEnergyRate
            | Self::EndingEnergyCost
            | Self::MinimumActivationEnergy
            | Self::EnergyFloor => Unit::Number,
            Self::CollisionRadius
            | Self::LeadDistanceLimit
            | Self::ProximityRange
            | Self::SearchRange => Unit::Distance,
            Self::TargetLimit
            | Self::SpawnCount
            | Self::GenerationLimit
            | Self::SpawnLimit
            | Self::SpawnCountOffset
            | Self::GenerationLimitOffset
            | Self::SpawnLimitOffset
            | Self::ExpirationUpdates
            | Self::ExpirationResponse
            | Self::PierceLimit
            | Self::BounceLimit
            | Self::CollisionMode => Unit::Count,
        }
    }

    /// What a stored value is multiplied by to read as the tile shows it.
    #[must_use]
    pub const fn scale(self) -> f32 {
        match self {
            Self::Native(property) => property.scale(),
            Self::TurnRate => DEGREES_PER_RADIAN,
            _ => 1.0,
        }
    }

    /// The range its field offers. Stock values outside it stay as they are until changed.
    #[must_use]
    pub const fn range(self) -> (f32, f32) {
        match self {
            Self::Native(property) => property.range(),
            Self::HealthScale => (0.1, 5.0),
            Self::RecoveryDelay | Self::DepletedRecoveryDelay => (0.0, 30.0),
            Self::RecoveryTime => (0.0, 30.0),
            Self::IncomingDamage | Self::ConditionalDamage => (0.1, 3.0),
            Self::DamageBreakThreshold => (0.0, 0.99),
            Self::SuppressionTime | Self::RetirementDelay => (0.0, 10.0),
            Self::MovementThreshold => (0.0, 20.0),
            Self::OwnerDamageOnly
            | Self::SourceFilter
            | Self::InvertSourceFilter
            | Self::IgnoreMovement
            | Self::RetireOnRemoval
            | Self::SpawnChance
            | Self::DamageBreakResponse
            | Self::StrengthLoss => (0.0, 1.0),
            Self::QueryScale => (0.0, 5.0),
            Self::TriggeredDuration | Self::DisruptionGracePeriod => (0.0, 10.0),
            Self::EnergyCost | Self::RequiredEnergy => (0.0, 2.0),
            Self::TrackingStrength
            | Self::DistanceTracking
            | Self::BounceTracking
            | Self::TrackingVariation => (0.0, 3.0),
            Self::TrackingSpeed => (0.0, 200.0),
            Self::FastThrowTracking => (0.0, 4000.0),
            Self::TurnRateOffset => (0.0, 720.0),
            Self::AcquisitionScale => (0.01, 4.0),
            Self::AcquisitionScaleOffset => (0.0, 4.0),
            Self::TargetLimit => (1.0, 8.0),
            Self::SpawnCount
            | Self::SpawnLimit
            | Self::SpawnCountOffset
            | Self::SpawnLimitOffset => (0.0, 16.0),
            Self::GenerationLimit | Self::GenerationLimitOffset => (0.0, 6.0),
            Self::ImpulseHoldTime => (0.0, 3.0),
            Self::FallSpeedThreshold | Self::MinimumFinishTime | Self::MaximumFinishTime => {
                (0.0, 30.0)
            }
            Self::FinishTravelTime | Self::ExpirationTime => (0.0, 30.0),
            Self::ImpulseRampTime | Self::ImpulseFadeTime => (0.0, 5.0),
            Self::VerticalBias => (0.0, 4.0),
            // -1 expires by age, as most stock projectiles do.
            Self::ExpirationUpdates => (-1.0, 127.0),
            Self::ExpirationResponse | Self::CollisionMode => (0.0, 2.0),
            Self::PierceLimit => (0.0, 31.0),
            Self::BounceLimit => (0.0, 127.0),
            Self::CollisionRadius
            | Self::SteeringAxisThreshold
            | Self::TargetLead
            | Self::IncludeSelf
            | Self::ActivationEnergy
            | Self::ActiveEnergyRate
            | Self::EndingEnergyCost
            | Self::MinimumActivationEnergy
            | Self::EnergyFloor => (0.0, 1.0),
            Self::RechargeDelay => (0.0, 30.0),
            Self::TurnRate => (0.0, 20000.0),
            Self::LeadTimeLimit | Self::SearchDelay => (0.0, 10.0),
            Self::LeadDistanceLimit => (0.0, 200.0),
            Self::ProximityRange => (0.0, 20.0),
            // Below zero sets no timer, as every stock trigger does.
            Self::MinimumProximityTime | Self::MaximumProximityTime => (-1.0, 10.0),
            Self::SearchRange => (0.0, 50.0),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Setting {
    pub kind: Kind,
    pub owner_tag: u32,
    pub field: WeaponRuntimeField,
    /// Where in an opaque field the setting's bytes sit, for a setting kept that way. A declared
    /// field's setting has none.
    pub lane: Option<Lane>,
    /// The group does not force a delay, so its maximum is read only with a positive minimum.
    pub requires_positive_minimum: bool,
}

impl Setting {
    /// The one graph this setting's creating group or node makes, when it makes exactly one. Its
    /// timing then belongs with that graph, as a delay before it appears or its lifetime.
    #[must_use]
    pub fn created_graph(&self, creations: &[Creation]) -> Option<u32> {
        if !matches!(
            self.kind,
            Kind::Native(
                NativeProperty::MinimumActivationDelay
                    | NativeProperty::MaximumActivationDelay
                    | NativeProperty::MinimumCycleDuration
                    | NativeProperty::MaximumCycleDuration
                    | NativeProperty::RepeatCount
                    | NativeProperty::MinimumPartDuration
                    | NativeProperty::MaximumPartDuration
            )
        ) {
            return None;
        }
        let body = self
            .field
            .owner_offset
            .checked_sub(self.field.locator.value_offset)?;
        let creation = creations
            .iter()
            .find(|creation| creation.owner_tag == self.owner_tag && creation.body == body)?;
        match creation.graphs.as_slice() {
            [graph] => Some(*graph),
            _ => None,
        }
    }

    /// Absolute start of the individual scalar, even when it shares an opaque field.
    #[must_use]
    pub fn offset(&self) -> u32 {
        self.field.owner_offset + self.lane.map_or(0, |lane| lane.at as u32)
    }
    /// `value` as this setting reads it: a number, one or zero for a flag, or a factor's lane.
    fn read(&self, value: &WeaponRuntimeValue) -> Option<f32> {
        if let Some(lane) = self.lane {
            let number = lane.read(value)?;
            return Some(if self.kind.unit() == Unit::Flag {
                if number == 0.0 { 0.0 } else { 1.0 }
            } else {
                number * self.kind.scale()
            });
        }
        match (self.kind, value) {
            (Kind::RetireOnRemoval, WeaponRuntimeValue::Unsigned(flags)) => {
                Some(if flags & RETIRE == 0 { 0.0 } else { 1.0 })
            }
            (Kind::SourceFilter, WeaponRuntimeValue::Unsigned(key)) => {
                Some(if *key == NO_SOURCE_PROPERTY { 0.0 } else { 1.0 })
            }
            (_, WeaponRuntimeValue::Boolean(on)) => Some(if *on { 1.0 } else { 0.0 }),
            (Kind::Native(_), WeaponRuntimeValue::Signed(value)) => {
                Some(*value as f32 * self.kind.scale())
            }
            (Kind::Native(_), WeaponRuntimeValue::Unsigned(value)) => {
                Some(*value as f32 * self.kind.scale())
            }
            (_, WeaponRuntimeValue::Float32Bits(bits)) => {
                Some(f32::from_bits(*bits) * self.kind.scale())
            }
            // A program's constant, whose first lane is its whole result.
            (_, WeaponRuntimeValue::Vector4Float32Bits(lanes)) => Some(f32::from_bits(lanes[0])),
            _ => None,
        }
    }

    fn current<'a>(&'a self, draft: &'a [WeaponRuntimeValueOverride]) -> &'a WeaponRuntimeValue {
        draft
            .iter()
            .find(|entry| entry.locator == self.field.locator)
            .map_or(&self.field.value, |entry| &entry.value)
    }

    #[must_use]
    pub fn stock(&self) -> f32 {
        self.read(&self.field.value).unwrap_or(f32::NAN)
    }

    #[must_use]
    pub fn value(&self, draft: &[WeaponRuntimeValueOverride]) -> f32 {
        self.read(self.current(draft))
            .unwrap_or_else(|| self.stock())
    }

    /// Whether `draft` changes it. A lane compares only its own bytes, so a change to another
    /// setting of the same field leaves it unchanged.
    #[must_use]
    pub fn is_modified(&self, draft: &[WeaponRuntimeValueOverride]) -> bool {
        match self.lane {
            Some(lane) => lane.bytes(self.current(draft)) != lane.bytes(&self.field.value),
            None => self.value(draft).to_bits() != self.stock().to_bits(),
        }
    }

    /// Sets it to `value`, keeping every other bit, lane and flag of its field as they are.
    pub fn set(
        &self,
        draft: &mut Vec<WeaponRuntimeValueOverride>,
        value: f32,
    ) -> Result<(), String> {
        // A few settings keep a negative stock value that means none, such as -1 updates.
        let lowest = self.kind.range().0.min(0.0);
        if !value.is_finite() || value < lowest {
            return Err(if lowest == 0.0 {
                format!("{} must be zero or greater.", self.kind.label())
            } else {
                format!("{} must be {lowest} or greater.", self.kind.label())
            });
        }
        if let Kind::Native(property) = self.kind {
            property.validate(value)?;
        }
        let value = if self.kind.unit() == Unit::Count {
            value.round()
        } else {
            value
        };
        let on = value >= 0.5;
        let next = if let Some(lane) = self.lane {
            let stored = if self.kind.unit() == Unit::Flag {
                f32::from(u8::from(on))
            } else {
                value / self.kind.scale()
            };
            lane.with(self.current(draft), &lane.encode(stored))
                .ok_or_else(|| {
                    format!("{} has a value of an unexpected kind.", self.kind.label())
                })?
        } else {
            self.declared_value(draft, value)?
        };
        draft.retain(|entry| entry.locator != self.field.locator);
        if next != self.field.value {
            draft.push(WeaponRuntimeValueOverride {
                locator: self.field.locator.clone(),
                value: next,
            });
        }
        Ok(())
    }

    /// The value its declared field takes for `value`.
    fn declared_value(
        &self,
        draft: &[WeaponRuntimeValueOverride],
        value: f32,
    ) -> Result<WeaponRuntimeValue, String> {
        let on = value >= 0.5;
        Ok(match (self.kind, self.current(draft)) {
            (Kind::RetireOnRemoval, WeaponRuntimeValue::Unsigned(flags)) => {
                WeaponRuntimeValue::Unsigned(if on { flags | RETIRE } else { flags & !RETIRE })
            }
            // On restores the stock key, never another ability's or an edited one.
            (Kind::SourceFilter, WeaponRuntimeValue::Unsigned(_)) => {
                if on {
                    self.field.value.clone()
                } else {
                    WeaponRuntimeValue::Unsigned(NO_SOURCE_PROPERTY)
                }
            }
            (_, WeaponRuntimeValue::Boolean(_)) => WeaponRuntimeValue::Boolean(on),
            (_, WeaponRuntimeValue::Float32Bits(_)) => {
                WeaponRuntimeValue::Float32Bits((value / self.kind.scale()).to_bits())
            }
            (Kind::Native(_), WeaponRuntimeValue::Signed(_)) => {
                WeaponRuntimeValue::Signed((value / self.kind.scale()).round() as i64)
            }
            (Kind::Native(_), WeaponRuntimeValue::Unsigned(_)) => {
                WeaponRuntimeValue::Unsigned((value / self.kind.scale()).round() as u64)
            }
            (_, WeaponRuntimeValue::Vector4Float32Bits(lanes)) => {
                let mut lanes = *lanes;
                lanes[0] = value.to_bits();
                WeaponRuntimeValue::Vector4Float32Bits(lanes)
            }
            _ => {
                return Err(format!(
                    "{} has a value of an unexpected kind.",
                    self.kind.label()
                ));
            }
        })
    }

    /// Restores its stock value. A lane restores only its own bytes, so the field's other settings
    /// keep their edits.
    pub fn reset(&self, draft: &mut Vec<WeaponRuntimeValueOverride>) {
        let next = match self.lane {
            Some(lane) => lane
                .bytes(&self.field.value)
                .and_then(|stock| lane.with(self.current(draft), stock)),
            None => self.declared_value(draft, self.stock()).ok(),
        };
        draft.retain(|entry| entry.locator != self.field.locator);
        if let Some(next) = next.filter(|next| *next != self.field.value) {
            draft.push(WeaponRuntimeValueOverride {
                locator: self.field.locator.clone(),
                value: next,
            });
        }
    }
}

/// A declared field of `class` at `offset` with a value of the expected kind.
fn declared(field: &WeaponRuntimeField, class: u32, offset: u32) -> bool {
    field.source == WeaponRuntimeFieldSource::NativeDeclaration
        && field.locator.type_handle.get() == class
        && field.locator.value_offset == offset
}

fn through(field: &WeaponRuntimeField, class: u32, offset: u32) -> bool {
    field
        .locator
        .path
        .iter()
        .any(|step| step.type_handle.get() == class && step.byte_offset == offset)
}

fn is_float(field: &WeaponRuntimeField) -> bool {
    matches!(field.value, WeaponRuntimeValue::Float32Bits(_))
}

fn is_flag(field: &WeaponRuntimeField) -> bool {
    matches!(field.value, WeaponRuntimeValue::Boolean(_))
}

/// Every setting of the graph's parts its native readers act on.
pub fn discover(manager: &PackageManager, graph: &WeaponRuntimeGraph) -> Vec<Setting> {
    let roots = graph
        .resources
        .iter()
        .flat_map(|resource| {
            std::iter::once(&resource.instance)
                .chain(resource.definition.iter())
                .map(move |root| (resource.owner_tag, root))
        })
        .chain(
            graph
                .owners
                .iter()
                .flat_map(|owner| owner.roots.iter().map(move |root| (owner.owner_tag, root))),
        )
        .collect::<Vec<_>>();
    let projectile = graph
        .resources
        .iter()
        .any(|resource| resource.instance.schema == MOVING_PROJECTILE);
    // Flags these readers take that are not declared fields come from the owner's payload. Only
    // the health, damage filter, target query and attachment spawner readers use it.
    let mut payloads = BTreeMap::<u32, Option<Vec<u8>>>::new();
    for &(owner, root) in &roots {
        if matches!(
            root.schema,
            HEALTH | DAMAGE_FILTER | QUERY_EMITTER | ATTACHMENT_SPAWNER | 0x8080_84E9
        ) || native::supports(root.schema)
        {
            payloads
                .entry(owner)
                .or_insert_with(|| manager.read_tag(owner).ok());
        }
    }
    let payload = |owner: u32| payloads.get(&owner).and_then(Option::as_deref);
    // A target query that sends its response to its projectile's timer sets that timer itself,
    // so the projectile's own finish timer goes unread.
    let timed = roots
        .iter()
        .any(|&(owner, root)| root.schema == QUERY_EMITTER && query_timed(root, payload(owner)));
    let connected = connected_sources(manager, graph);
    let mut found = Vec::new();
    let mut programs = Programs::default();
    for &(owner, root) in &roots {
        let payload = payload(owner);
        match root.schema {
            HEALTH => found.extend(regions(owner, root, payload)),
            DAMAGE_FILTER => found.extend(damage_filter(owner, root, payload)),
            INVISIBILITY => found.extend(invisibility(owner, root)),
            _ => {}
        }
        // A tracking definition's proximity trigger fires through its event source, which only
        // does something when an event row connects it to a receiver. A graph whose rows cannot
        // be read keeps its proximity settings, as before the gate.
        let source = (
            owner,
            u64::from(root.owner_offset) + u64::from(PROXIMITY_SOURCE),
        );
        let proximity = connected
            .as_ref()
            .is_none_or(|connected| connected.contains(&source));
        found.extend(lanes::find(owner, root, (timed, proximity)));
        // A generic reflected declaration may already occupy a native lane's bytes. Generate
        // its semantic setting independently, while reusing a compatible saved field below.
        if let Some(payload) = payload {
            let binding = root.fields.first().map(|field| {
                (
                    field.locator.binding_hash.get(),
                    field.locator.resource_index,
                )
            });
            if let Some((binding, index)) = binding
                && let Ok(mut fields) = native::fields(manager, payload, root, binding, index)
            {
                let scope = root.fields.iter().find_map(|field| field.locator.graph_tag);
                for field in &mut fields {
                    field.locator.graph_tag = scope;
                }
                found.extend(
                    fields
                        .iter()
                        .filter_map(|field| native::setting(owner, root, field, Some(payload))),
                );
            }
        }
        let interpreted = if root.schema == DAMAGE_FILTER {
            interpreted_rows(root, payload)
        } else {
            Vec::new()
        };
        let gates = Gates {
            projectile,
            timed: root.schema == QUERY_EMITTER && query_timed(root, payload),
            interpreted: &interpreted,
            payload,
        };
        programs.read(owner, root, gates);
    }
    found.extend(programs.settings());
    found.extend(native::expression_settings(manager, graph));
    // A root a resource and an owner both list is found twice. Lanes of one field are distinct.
    let mut unique: Vec<Setting> = Vec::new();
    for setting in lanes::separate(found) {
        if !unique.iter().any(|each| {
            each.owner_tag == setting.owner_tag
                && each.kind == setting.kind
                && each.offset() == setting.offset()
        }) {
            unique.push(setting);
        }
    }
    unique
}

/// The event sources the graph's entity connects to a receiver: each event row whose source is an
/// event source object and whose destination is a receiving interface rather than the null
/// owner, by the source's owner and offset (survey: the two stock tracking rows with a null
/// destination skip building their callback). `None` when the entity's rows cannot be read.
fn connected_sources(
    manager: &PackageManager,
    graph: &WeaponRuntimeGraph,
) -> Option<BTreeSet<(u32, u64)>> {
    let entity = manager.read_tag(graph.entity_tag).ok()?;
    let rows = crate::entity::owner::event_ends(&entity).ok()?;
    Some(
        rows.into_iter()
            .filter(|(source, destination)| {
                source.class == EVENT_SOURCE
                    && destination.owner != u32::MAX
                    && destination.class == EVENT_DESTINATION
            })
            .map(|(source, _)| (source.owner, source.offset))
            .collect(),
    )
}

/// Whether each conditional row of an incoming damage filter runs its program in the
/// interpreter, which reads its constant: its descriptor's fast-path index is zero. The holder
/// at +0x70 keeps the row count, then a relative pointer to the array, whose rows follow a
/// 16-byte header. A row past the payload counts as not read.
fn interpreted_rows(root: &WeaponRuntimeRoot, payload: Option<&[u8]>) -> Vec<bool> {
    let Some(payload) = payload else {
        return Vec::new();
    };
    let word = |at: usize| {
        payload
            .get(at..at + 8)
            .and_then(|bytes| bytes.try_into().ok())
            .map(u64::from_le_bytes)
    };
    let holder = root.owner_offset as usize + CONDITIONAL_HOLDER_OFFSET as usize;
    let (Some(count), Some(relative)) = (word(holder), word(holder + 8)) else {
        return Vec::new();
    };
    let Some(rows) = (holder + 8)
        .checked_add_signed(relative as i64 as isize)
        .and_then(|header| header.checked_add(16))
    else {
        return Vec::new();
    };
    (0..count.min(256) as usize)
        .map(|row| {
            let at = rows + row * CONDITIONAL_ROW_SIZE + CONDITIONAL_FAST_PATH;
            payload.get(at..at + 4).is_some_and(|fast| fast == [0; 4])
        })
        .collect()
}

/// The health region rows whose own recovery values the reader takes.
fn regions(owner: u32, root: &WeaponRuntimeRoot, payload: Option<&[u8]>) -> Vec<Setting> {
    let Some(payload) = payload else {
        return Vec::new();
    };
    const FIELDS: [(u32, Kind); 4] = [
        (0x14, Kind::HealthScale),
        (0x38, Kind::RecoveryDelay),
        (0x3C, Kind::DepletedRecoveryDelay),
        (0x40, Kind::RecoveryTime),
    ];
    let mut rows =
        BTreeMap::<Vec<WeaponRuntimePathElement>, Vec<(Kind, &WeaponRuntimeField)>>::new();
    for field in &root.fields {
        let Some(&(_, kind)) = FIELDS
            .iter()
            .find(|(offset, _)| declared(field, REGION_ROW, *offset))
        else {
            continue;
        };
        if !is_float(field) || !through(field, REGION_ROWS, REGION_ROWS_OFFSET) {
            continue;
        }
        let row = field.locator.path[..field.locator.path.len().saturating_sub(1)].to_vec();
        rows.entry(row).or_default().push((kind, field));
    }
    let mut found = Vec::new();
    for fields in rows.into_values() {
        // The row starts where its capacity sits less its offset.
        let Some(start) = fields
            .iter()
            .find(|(kind, _)| *kind == Kind::HealthScale)
            .and_then(|(_, field)| (field.owner_offset as usize).checked_sub(0x14))
        else {
            continue;
        };
        let Some(&flags) = payload.get(start + REGION_FLAGS) else {
            continue;
        };
        if flags & REGION_BYPASS != 0 {
            continue;
        }
        found.extend(fields.into_iter().map(|(kind, field)| Setting {
            kind,
            owner_tag: owner,
            field: field.clone(),
            lane: None,
            requires_positive_minimum: false,
        }));
    }
    found
}

/// An incoming damage filter's owner and source tests, and its base multiplier. Owner Damage
/// Only is read only while the filter's own branch at +0xE9, which takes precedence, is off.
fn damage_filter(owner: u32, root: &WeaponRuntimeRoot, payload: Option<&[u8]>) -> Vec<Setting> {
    let direct = |offset: u32| {
        root.fields
            .iter()
            .find(|field| declared(field, DAMAGE_FILTER, offset) && field.locator.path.len() == 2)
    };
    let branch = payload.and_then(|payload| {
        payload
            .get(root.owner_offset as usize + FILTER_BRANCH)
            .copied()
    });
    let mut found = Vec::new();
    if branch == Some(0)
        && let Some(field) = direct(0xE8).filter(|field| is_flag(field))
    {
        found.push(Setting {
            kind: Kind::OwnerDamageOnly,
            owner_tag: owner,
            field: field.clone(),
            lane: None,
            requires_positive_minimum: false,
        });
    }
    let source = direct(SOURCE_PROPERTY).filter(|field| {
        matches!(field.value, WeaponRuntimeValue::Unsigned(key) if key != NO_SOURCE_PROPERTY)
    });
    if let Some(field) = source {
        found.push(Setting {
            kind: Kind::SourceFilter,
            owner_tag: owner,
            field: field.clone(),
            lane: None,
            requires_positive_minimum: false,
        });
    }
    if source.is_some()
        && let Some(field) = direct(0xE4).filter(|field| is_flag(field))
    {
        found.push(Setting {
            kind: Kind::InvertSourceFilter,
            owner_tag: owner,
            field: field.clone(),
            lane: None,
            requires_positive_minimum: false,
        });
    }
    if let Some(field) = base_multiplier(root) {
        found.push(Setting {
            kind: Kind::IncomingDamage,
            owner_tag: owner,
            field,
            lane: None,
            requires_positive_minimum: false,
        });
    }
    found
}

/// The constant row of the filter's base program, when the program pushes it and stores it.
fn base_multiplier(root: &WeaponRuntimeRoot) -> Option<WeaponRuntimeField> {
    let mut bytecode = Vec::<(u32, u8)>::new();
    let mut constants = Vec::<&WeaponRuntimeField>::new();
    for field in &root.fields {
        if field.source != WeaponRuntimeFieldSource::NativeDeclaration
            || field.locator.value_offset != 0
        {
            continue;
        }
        let path = &field.locator.path;
        // The program at +0x80 of the filter itself, not one a conditional row holds.
        if path.get(1).is_none_or(|step| {
            step.type_handle.get() != PROGRAM || step.byte_offset != BASE_PROGRAM_OFFSET
        }) {
            continue;
        }
        let Some(pointer) = path
            .iter()
            .rposition(|step| step.name_hash == POINTER && step.type_handle.get() == ARRAY)
        else {
            continue;
        };
        let Some(element) = path
            .get(pointer + 1)
            .filter(|step| step.name_hash == ELEMENT)
        else {
            continue;
        };
        match (element.type_handle.get(), &field.value) {
            (BYTECODE_ROW, WeaponRuntimeValue::Unsigned(byte)) => {
                bytecode.push((element.byte_offset, u8::try_from(*byte).unwrap_or(u8::MAX)));
            }
            (CONSTANT_ROW, WeaponRuntimeValue::Vector4Float32Bits(_)) => constants.push(field),
            _ => {}
        }
    }
    bytecode.sort_by_key(|(row, _)| *row);
    let code = bytecode.iter().map(|(_, byte)| *byte).collect::<Vec<_>>();
    match constants.as_slice() {
        [constant] if code == PUSH_AND_STORE => Some((*constant).clone()),
        _ => None,
    }
}

/// A program's place: the classes and offsets from its root to its body.
type Place = Vec<(u32, u32)>;

/// What a part's program readers depend on beyond a program's place.
#[derive(Clone, Copy)]
struct Gates<'a> {
    /// The graph moves as a projectile (`80803B73`).
    projectile: bool,
    /// The root is a target query emitter whose accepted response sets the projectile timer.
    timed: bool,
    /// For an incoming damage filter, whether each conditional row's program runs in the
    /// interpreter, which reads its constant, rather than a fast path.
    interpreted: &'a [bool],
    /// The owner's payload, for the words of a program's descriptor no field declares.
    payload: Option<&'a [u8]>,
}

impl Gates<'_> {
    /// The four words of the descriptor of the program at `slot` of `root`: its input count, a
    /// second count, its output count and its fast-path index.
    fn descriptor(self, root: &WeaponRuntimeRoot, slot: u32) -> Option<[u32; 4]> {
        let at = root.owner_offset as usize + slot as usize + PROGRAM_WORDS;
        let bytes = self.payload?.get(at..at + 16)?;
        let mut words = [0; 4];
        for (word, chunk) in words.iter_mut().zip(bytes.chunks_exact(4)) {
            *word = u32::from_le_bytes(chunk.try_into().ok()?);
        }
        Some(words)
    }

    /// Whether the reader of a `kind` program at `place` reads it in this part: a projectile's
    /// program only in a moving projectile, Triggered Duration only where the query's response
    /// sets the projectile timer, and a conditional row only when the interpreter runs it.
    fn reads(self, kind: Kind, projectile: bool, place: &[(u32, u32)]) -> bool {
        if projectile && !self.projectile {
            return false;
        }
        match kind {
            Kind::TriggeredDuration => self.timed,
            Kind::ConditionalDamage => place
                .last()
                .is_some_and(|&(_, row)| self.interpreted.get(row as usize) == Some(&true)),
            _ => true,
        }
    }
}

/// Whether `place`, a program's classes and offsets from its root, ends in `steps`, and starts
/// with them when `from_root`. An offset of `None` takes its class wherever it sits.
fn matches(steps: &[(u32, Option<u32>)], from_root: bool, place: &[(u32, u32)]) -> bool {
    let fits = |(class, offset): &(u32, u32), (wanted, at): &(u32, Option<u32>)| {
        class == wanted && at.is_none_or(|at| at == *offset)
    };
    let Some(start) = place.len().checked_sub(steps.len()) else {
        return false;
    };
    (!from_root || start == 0)
        && place[start..]
            .iter()
            .zip(steps)
            .all(|(step, wanted)| fits(step, wanted))
}

impl Role {
    fn matches(&self, place: &[(u32, u32)]) -> bool {
        matches(self.steps, self.from_root, place)
    }
}

/// The programs of a graph's parts: every program's constants by owner, which tell a shared
/// constant buffer apart, and the program settings found so far.
#[derive(Default)]
struct Programs {
    constants: BTreeMap<(u32, u32), BTreeSet<Place>>,
    found: Vec<(Place, Setting)>,
}

impl Programs {
    /// Reads the programs of `root`, which `owner` holds, with what its readers depend on beyond
    /// a program's place.
    fn read(&mut self, owner: u32, root: &WeaponRuntimeRoot, gates: Gates<'_>) {
        // Each body's bytecode and constants, by their row.
        type Rows<'a> = (Vec<(u32, u8)>, Vec<(u32, &'a WeaponRuntimeField)>);
        let mut bodies = BTreeMap::<Place, Rows<'_>>::new();
        for field in &root.fields {
            if field.source != WeaponRuntimeFieldSource::NativeDeclaration
                || field.locator.value_offset != 0
            {
                continue;
            }
            let path = &field.locator.path;
            let Some(pointer) = path
                .iter()
                .rposition(|step| step.name_hash == POINTER && step.type_handle.get() == ARRAY)
            else {
                continue;
            };
            let Some(element) = path
                .get(pointer + 1)
                .filter(|step| step.name_hash == ELEMENT)
            else {
                continue;
            };
            let Some(body) = path[..pointer]
                .iter()
                .rposition(|step| PROGRAM_BODIES.contains(&step.type_handle.get()))
            else {
                continue;
            };
            let place = path[..=body]
                .iter()
                .map(|step| (step.type_handle.get(), step.byte_offset))
                .collect::<Place>();
            let (bytecode, constants) = bodies.entry(place).or_default();
            match (element.type_handle.get(), &field.value) {
                (BYTECODE_ROW, WeaponRuntimeValue::Unsigned(byte)) => {
                    bytecode.push((element.byte_offset, u8::try_from(*byte).unwrap_or(u8::MAX)));
                }
                (CONSTANT_ROW, WeaponRuntimeValue::Vector4Float32Bits(_)) => {
                    constants.push((element.byte_offset, field));
                }
                _ => {}
            }
        }
        for (place, (mut bytecode, mut constants)) in bodies {
            for (_, constant) in &constants {
                self.constants
                    .entry((owner, constant.owner_offset))
                    .or_default()
                    .insert(place.clone());
            }
            bytecode.sort_by_key(|(row, _)| *row);
            constants.sort_by_key(|(row, _)| *row);
            let code = bytecode.iter().map(|(_, byte)| *byte).collect::<Vec<_>>();
            let setting = |kind: Kind, constant: &WeaponRuntimeField| Setting {
                kind,
                owner_tag: owner,
                field: constant.clone(),
                lane: None,
                requires_positive_minimum: false,
            };
            if let Some(role) = ROLES.iter().find(|role| role.matches(&place))
                && let [(_, constant)] = constants.as_slice()
                && code == PUSH_AND_STORE
                && gates.reads(role.kind, role.projectile, &place)
            {
                self.found
                    .push((place.clone(), setting(role.kind, constant)));
            }
            if let Some(role) = AFFINE_ROLES
                .iter()
                .find(|role| matches(role.steps, true, &place))
                && let [(_, gain), (_, offset)] = constants.as_slice()
                && code == [0x3C, role.input, 0x34, 0x00, 0x34, 0x01, 0x12, 0x3E, 0x00]
                && (!role.projectile || gates.projectile)
            {
                self.found.push((place.clone(), setting(role.gain, gain)));
                self.found
                    .push((place.clone(), setting(role.offset, offset)));
            }
            // A spawner program computed from its inputs offers the constant it adds last, when
            // its descriptor declares the shape's inputs and one output and takes no fast path.
            if let Some(&(kind, slot)) = OFFSET_ROLES.iter().find(|(_, slot)| {
                place.as_slice()
                    == [
                        (ATTACHMENT_SPAWNER, 0),
                        (PROGRAM_SLOT, *slot),
                        (PROGRAM_BODY, 0),
                    ]
            }) && let Some(shape) = OFFSET_SHAPES
                .iter()
                .find(|shape| code == shape.code && constants.len() == shape.constants)
                && gates.descriptor(root, slot) == Some([shape.inputs, 0, 1, 0])
            {
                self.found
                    .push((place, setting(kind, constants[shape.added].1)));
            }
        }
    }

    /// The settings whose constant no other program of their owner reads, since editing a
    /// shared one would change that program too.
    fn settings(self) -> Vec<Setting> {
        let Self { constants, found } = self;
        found
            .into_iter()
            .filter(|(_, setting)| {
                constants
                    .get(&(setting.owner_tag, setting.field.owner_offset))
                    .is_none_or(|places| places.len() == 1)
            })
            .map(|(_, setting)| setting)
            .collect()
    }
}

/// Whether a target query emitter's accepted response sets its projectile's timer, which only
/// then reads Triggered Duration. Neither flag is a declared field: +0x1A8 opens an opaque block
/// and +0x14A has no field, so both are read from the owner's payload.
fn query_timed(root: &WeaponRuntimeRoot, payload: Option<&[u8]>) -> bool {
    let Some(payload) = payload else {
        return false;
    };
    let flag = |offset: u32| {
        payload
            .get(root.owner_offset as usize + offset as usize)
            .copied()
    };
    flag(QUERY_TIMER).is_some_and(|on| on != 0) && flag(QUERY_ATTACHMENT) == Some(0)
}

/// An invisibility attachment's break and movement thresholds, and its timed retirement.
fn invisibility(owner: u32, root: &WeaponRuntimeRoot) -> Vec<Setting> {
    const SETTINGS: [(u32, Kind, bool); 5] = [
        (0x10, Kind::DamageBreakThreshold, false),
        (0x170, Kind::SuppressionTime, false),
        (0x174, Kind::DisruptionGracePeriod, false),
        (0x178, Kind::MovementThreshold, false),
        (0x1D8, Kind::IgnoreMovement, true),
    ];
    let mut found = Vec::new();
    for (offset, kind, flag) in SETTINGS {
        if let Some(field) = root.fields.iter().find(|field| {
            declared(field, INVISIBILITY_SETTINGS, offset)
                && through(field, INVISIBILITY_SETTINGS, INVISIBILITY_SETTINGS_OFFSET)
                && if flag {
                    is_flag(field)
                } else {
                    is_float(field)
                }
        }) {
            found.push(Setting {
                kind,
                owner_tag: owner,
                field: field.clone(),
                lane: None,
                requires_positive_minimum: false,
            });
        }
    }
    let direct = |offset: u32| {
        root.fields
            .iter()
            .find(|field| declared(field, INVISIBILITY, offset) && field.locator.path.len() == 2)
    };
    if let Some(flags) = direct(ATTACHMENT_FLAGS) {
        let WeaponRuntimeValue::Unsigned(_) = flags.value else {
            return found;
        };
        found.push(Setting {
            kind: Kind::RetireOnRemoval,
            owner_tag: owner,
            field: flags.clone(),
            lane: None,
            requires_positive_minimum: false,
        });
        // A draft can enable timed retirement even when the stock flag is clear.
        if let Some(delay) = direct(0x294).filter(|field| is_float(field)) {
            found.push(Setting {
                kind: Kind::RetirementDelay,
                owner_tag: owner,
                field: delay.clone(),
                lane: None,
                requires_positive_minimum: false,
            });
        }
    }
    found
}
