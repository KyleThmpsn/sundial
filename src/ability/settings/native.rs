//! Exact native configuration lanes verified for Shadowkeep build 86657.20.08.23.1800.d2_rc___release.
//! Locators store the property and structural row path. Both discovery and compilation repeat
//! the same bounds, typed-array and reciprocal-pair validation. No absolute offset is trusted.
use super::{Kind, Lane, Setting, Unit, lanes::Codec};
use crate::package_payload::{
    bytes_at, i64_at, native_array_at, relative_offset, rows_fit, u32_at, u64_at,
};
use crate::package_runtime::reader::PackageManager;
use crate::package_runtime::references::schema::Registry;
use crate::runtime::*;
use tiger_pkg::TagHash;

mod contact;
mod controller;
mod creator;
mod expression;
pub(super) use expression::discover as expression_settings;

const ROOT: u32 = 0x5041_5200;
const ROW: u32 = 0x5041_4100;
const VALUE: u32 = 0x5041_5600;

/// A proven native configuration role. Numeric IDs are persisted locator identities.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum Property {
    MinimumActivationDelay = 1,
    MaximumActivationDelay = 2,
    MinimumCycleDuration = 3,
    MaximumCycleDuration = 4,
    RepeatCount = 5,
    LaunchSpeedWeight = 6,
    StartingHealth = 7,
    RechargeScale = 8,
    ActivationCostScale = 9,
    ActiveEnergyScale = 10,
    ActivationLockout = 11,
    TargetingRange = 12,
    CollisionLimit = 13,
    TrackingSpeedChange = 14,
    MinimumSpawnDelay = 15,
    MaximumSpawnDelay = 16,
    MinimumPartDuration = 17,
    MaximumPartDuration = 18,
    EffectPriority = 19,
    EffectGroup = 20,
    BounceCountIncrement = 21,
    BounceAngleVariation = 22,
    BounceSpeedVariation = 23,
    BounceSurfaceRadius = 24,
    ContactSpeed = 25,
    ContactFinalSpeed = 26,
    ContactFinalGravity = 27,
    ContactCurveStart = 28,
    ContactCurveEnd = 29,
    CleanupOnContact = 30,
    SurfacePlacement = 31,
    TriggeredDurationScaling = 32,
    QueryScaleReduction = 33,
    ContactCleanupDelay = 34,
    ContactCleanupBaseOffset = 35,
    ContactTriggerDelay = 36,
    ContactTriggerBaseOffset = 37,
    ContactCleanupOffset = 38,
    ContactTriggerOffset = 39,
    ContactCleanupScaling = 40,
    ContactTriggerScaling = 41,
    ContactCleanupVariation = 42,
    ContactCleanupScalingOffset = 43,
    ContactTriggerScalingOffset = 44,
}

impl Property {
    pub(super) fn validate(self, value: f32) -> Result<(), String> {
        let (minimum, maximum) = match self {
            Self::StartingHealth | Self::BounceSpeedVariation => (0.0, 100.0),
            Self::LaunchSpeedWeight | Self::CleanupOnContact | Self::SurfacePlacement => (0.0, 1.0),
            Self::EffectGroup => (0.0, 15.0),
            Self::RepeatCount => (0.0, 255.0),
            Self::CollisionLimit => (0.0, 127.0),
            Self::EffectPriority
            | Self::ContactCleanupBaseOffset
            | Self::ContactTriggerBaseOffset
            | Self::ContactCleanupOffset
            | Self::ContactTriggerOffset
            | Self::ContactCleanupScalingOffset
            | Self::ContactTriggerScalingOffset => (f32::MIN, f32::MAX),
            _ => (0.0, f32::MAX),
        };
        if value.is_finite() && (minimum..=maximum).contains(&value) {
            Ok(())
        } else {
            Err(format!(
                "{} must be between {minimum} and {maximum}.",
                self.label()
            ))
        }
    }
    fn from_id(id: u32) -> Option<Self> {
        Some(match id {
            1 => Self::MinimumActivationDelay,
            2 => Self::MaximumActivationDelay,
            3 => Self::MinimumCycleDuration,
            4 => Self::MaximumCycleDuration,
            5 => Self::RepeatCount,
            6 => Self::LaunchSpeedWeight,
            7 => Self::StartingHealth,
            8 => Self::RechargeScale,
            9 => Self::ActivationCostScale,
            10 => Self::ActiveEnergyScale,
            11 => Self::ActivationLockout,
            12 => Self::TargetingRange,
            13 => Self::CollisionLimit,
            14 => Self::TrackingSpeedChange,
            15 => Self::MinimumSpawnDelay,
            16 => Self::MaximumSpawnDelay,
            17 => Self::MinimumPartDuration,
            18 => Self::MaximumPartDuration,
            19 => Self::EffectPriority,
            20 => Self::EffectGroup,
            21 => Self::BounceCountIncrement,
            22 => Self::BounceAngleVariation,
            23 => Self::BounceSpeedVariation,
            24 => Self::BounceSurfaceRadius,
            25 => Self::ContactSpeed,
            26 => Self::ContactFinalSpeed,
            27 => Self::ContactFinalGravity,
            28 => Self::ContactCurveStart,
            29 => Self::ContactCurveEnd,
            30 => Self::CleanupOnContact,
            31 => Self::SurfacePlacement,
            32 => Self::TriggeredDurationScaling,
            33 => Self::QueryScaleReduction,
            34 => Self::ContactCleanupDelay,
            35 => Self::ContactCleanupBaseOffset,
            36 => Self::ContactTriggerDelay,
            37 => Self::ContactTriggerBaseOffset,
            38 => Self::ContactCleanupOffset,
            39 => Self::ContactTriggerOffset,
            40 => Self::ContactCleanupScaling,
            41 => Self::ContactTriggerScaling,
            42 => Self::ContactCleanupVariation,
            43 => Self::ContactCleanupScalingOffset,
            44 => Self::ContactTriggerScalingOffset,
            _ => return None,
        })
    }
    pub const fn label(self) -> &'static str {
        match self {
            Self::MinimumActivationDelay => "Minimum Creation Delay",
            Self::MaximumActivationDelay => "Maximum Creation Delay",
            Self::MinimumCycleDuration => "Minimum Cycle Duration",
            Self::MaximumCycleDuration => "Maximum Cycle Duration",
            Self::RepeatCount => "Repeat Count",
            Self::LaunchSpeedWeight => "Launch Speed Weight",
            Self::StartingHealth => "Starting Health",
            Self::RechargeScale => "Recharge Scale",
            Self::ActivationCostScale => "Activation Cost Scale",
            Self::ActiveEnergyScale => "Active Energy Scale",
            Self::ActivationLockout => "Activation Lockout",
            Self::TargetingRange => "Targeting Range",
            Self::CollisionLimit => "Collision Limit",
            Self::TrackingSpeedChange => "Tracking Speed Change",
            Self::MinimumSpawnDelay => "Minimum Spawn Delay",
            Self::MaximumSpawnDelay => "Maximum Spawn Delay",
            Self::MinimumPartDuration => "Minimum Part Duration",
            Self::MaximumPartDuration => "Maximum Part Duration",
            Self::EffectPriority => "Effect Priority",
            Self::EffectGroup => "Effect Group",
            Self::BounceCountIncrement => "Bounce Count Increment",
            Self::BounceAngleVariation => "Bounce Angle Variation",
            Self::BounceSpeedVariation => "Bounce Speed Variation",
            Self::BounceSurfaceRadius => "Bounce Surface Radius",
            Self::ContactSpeed => "Contact Speed",
            Self::ContactFinalSpeed => "Contact Final Speed",
            Self::ContactFinalGravity => "Contact Final Gravity",
            Self::ContactCurveStart => "Contact Curve Start",
            Self::ContactCurveEnd => "Contact Curve End",
            Self::CleanupOnContact => "Cleanup on Contact",
            Self::SurfacePlacement => "Surface Placement",
            Self::TriggeredDurationScaling => "Triggered Duration Scaling",
            Self::QueryScaleReduction => "Query Scale Reduction",
            Self::ContactCleanupDelay => "Contact Cleanup Delay",
            Self::ContactCleanupBaseOffset => "Contact Cleanup Base Offset",
            Self::ContactTriggerDelay => "Contact Trigger Delay",
            Self::ContactTriggerBaseOffset => "Contact Trigger Base Offset",
            Self::ContactCleanupOffset => "Contact Cleanup Offset",
            Self::ContactTriggerOffset => "Contact Trigger Offset",
            Self::ContactCleanupScaling => "Contact Cleanup Scaling",
            Self::ContactTriggerScaling => "Contact Trigger Scaling",
            Self::ContactCleanupVariation => "Contact Cleanup Variation",
            Self::ContactCleanupScalingOffset => "Contact Cleanup Scaling Offset",
            Self::ContactTriggerScalingOffset => "Contact Trigger Scaling Offset",
        }
    }
    pub const fn hint(self) -> &'static str {
        match self {
            Self::MinimumActivationDelay => "Shortest wait before the part is created",
            Self::MaximumActivationDelay => "Longest wait before the part is created",
            Self::MinimumCycleDuration => "Shortest countdown holding a cycle",
            Self::MaximumCycleDuration => "Longest countdown holding a cycle",
            Self::RepeatCount => {
                "Cycles this group can run. Zero and one both allow one cycle. This does not count spawned objects."
            }
            Self::LaunchSpeedWeight => {
                "Weight of an optional launch speed input. Stock identity inputs make this setting inactive."
            }
            Self::StartingHealth => {
                "Starting fraction of this object region capacity. Other removal and recovery rules still apply."
            }
            Self::RechargeScale => {
                "Scales this controller's recharge input. Combines with applied modifiers and keyed Recharge Rate changes."
            }
            Self::ActivationCostScale => {
                "Scales activation energy use. A zero cost does not remove readiness requirements."
            }
            Self::ActiveEnergyScale => {
                "Scales active energy use. A selected movement bank can also replace the base energy rate."
            }
            Self::ActivationLockout => {
                "Seconds of activation lockout for this controller. Other cooldown and input requirements still apply."
            }
            Self::TargetingRange => {
                "Scales this controller target queries. Other targeting rules still apply."
            }
            Self::CollisionLimit => {
                "Stored contact query allowance. Zero allows one entry, and each added step allows one more."
            }
            Self::TrackingSpeedChange => {
                "Maximum tracking speed change per simulation update. This is not acceleration per second."
            }
            Self::MinimumSpawnDelay => "Earliest deferred attachment attempt",
            Self::MaximumSpawnDelay => "Latest sampled attachment deadline",
            Self::MinimumPartDuration => {
                "Shortest time before the part is cleaned up. Other events can remove it sooner."
            }
            Self::MaximumPartDuration => {
                "Longest time before the part is cleaned up. This may control a visual effect."
            }
            Self::EffectPriority => {
                "Priority within a conflicting effect group. Used only when Effect Group is not zero."
            }
            Self::EffectGroup => {
                "Conflict group bits 1, 2, 4 and 8. Native group names are unknown. Some groups also affect effect lifecycle."
            }
            Self::BounceCountIncrement => "Weight this bounce response",
            Self::BounceAngleVariation => "Direction variation after contact",
            Self::BounceSpeedVariation => "Speed variation after contact",
            Self::BounceSurfaceRadius => "Surface query for bounce normal",
            Self::ContactSpeed => "Speed after this contact",
            Self::ContactFinalSpeed => "Final speed after contact",
            Self::ContactFinalGravity => "Final gravity after contact",
            Self::ContactCurveStart => {
                "Distance travelled after this contact before the new speed and gravity curve starts."
            }
            Self::ContactCurveEnd => {
                "Distance travelled after this contact where the new speed and gravity curve ends."
            }
            Self::CleanupOnContact => "Start the cleanup response",
            Self::SurfacePlacement => "Offset attachment against the surface",
            Self::TriggeredDurationScaling => {
                "Multiplies the supplied input to set the timer after a query accepts a target. The input has no assigned physical unit."
            }
            Self::QueryScaleReduction => {
                "Subtracts this value from the supplied query multiplier. Nonpositive results do not perform the query."
            }
            Self::ContactCleanupDelay => {
                "Cleanup threshold after this contact. It can affect every projectile in the pool, while only the current clock resets."
            }
            Self::ContactCleanupBaseOffset => {
                "Seconds added before multiplying the contact cleanup timer by its second input and scaling offset."
            }
            Self::ContactTriggerDelay => {
                "Age threshold after this contact. Used only when Expiration Updates is By Age. This does not promise a detonation."
            }
            Self::ContactTriggerBaseOffset => {
                "Seconds added before multiplying the contact trigger timer by its second input and scaling offset. Used only with By Age expiration."
            }
            Self::ContactCleanupOffset => {
                "Seconds added to the computed cleanup timer after contact. Other finish paths can end the projectile sooner."
            }
            Self::ContactTriggerOffset => {
                "Seconds added to the computed trigger timer after contact. Used only with By Age expiration."
            }
            Self::ContactCleanupScaling => {
                "Multiplies the supplied input to compute the cleanup timer. The coefficient has no assigned physical unit."
            }
            Self::ContactTriggerScaling => {
                "Multiplies the supplied input to compute the trigger timer. Used only with By Age expiration."
            }
            Self::ContactCleanupVariation => {
                "Seconds of variation from the verified zero-to-one seed input, added to the cleanup offset."
            }
            Self::ContactCleanupScalingOffset => {
                "Added to the second input before it multiplies the cleanup timer. This is a multiplier offset, not seconds."
            }
            Self::ContactTriggerScalingOffset => {
                "Added to the second input before it multiplies the trigger timer. This is a multiplier offset, not seconds."
            }
        }
    }
    pub const fn unit(self) -> Unit {
        match self {
            Self::MinimumActivationDelay => Unit::Seconds,
            Self::MaximumActivationDelay => Unit::Seconds,
            Self::MinimumCycleDuration => Unit::Seconds,
            Self::MaximumCycleDuration => Unit::Seconds,
            Self::RepeatCount => Unit::Count,
            Self::LaunchSpeedWeight => Unit::Number,
            Self::StartingHealth => Unit::Number,
            Self::RechargeScale => Unit::Factor,
            Self::ActivationCostScale => Unit::Factor,
            Self::ActiveEnergyScale => Unit::Factor,
            Self::ActivationLockout => Unit::Seconds,
            Self::TargetingRange => Unit::Factor,
            Self::CollisionLimit => Unit::Count,
            Self::TrackingSpeedChange => Unit::Number,
            Self::MinimumSpawnDelay => Unit::Seconds,
            Self::MaximumSpawnDelay => Unit::Seconds,
            Self::MinimumPartDuration => Unit::Seconds,
            Self::MaximumPartDuration => Unit::Seconds,
            Self::EffectPriority => Unit::Number,
            Self::EffectGroup => Unit::Count,
            Self::BounceCountIncrement => Unit::Count,
            Self::BounceAngleVariation => Unit::Number,
            Self::BounceSpeedVariation => Unit::Number,
            Self::BounceSurfaceRadius => Unit::Distance,
            Self::ContactSpeed => Unit::Number,
            Self::ContactFinalSpeed => Unit::Number,
            Self::ContactFinalGravity => Unit::Factor,
            Self::ContactCurveStart => Unit::Distance,
            Self::ContactCurveEnd => Unit::Distance,
            Self::CleanupOnContact => Unit::Flag,
            Self::SurfacePlacement => Unit::Flag,
            Self::TriggeredDurationScaling => Unit::Number,
            Self::QueryScaleReduction => Unit::Factor,
            Self::ContactCleanupDelay => Unit::Seconds,
            Self::ContactCleanupBaseOffset => Unit::Seconds,
            Self::ContactTriggerDelay => Unit::Seconds,
            Self::ContactTriggerBaseOffset => Unit::Seconds,
            Self::ContactCleanupOffset => Unit::Seconds,
            Self::ContactTriggerOffset => Unit::Seconds,
            Self::ContactCleanupScaling => Unit::Number,
            Self::ContactTriggerScaling => Unit::Number,
            Self::ContactCleanupVariation => Unit::Seconds,
            Self::ContactCleanupScalingOffset => Unit::Number,
            Self::ContactTriggerScalingOffset => Unit::Number,
        }
    }
    pub const fn range(self) -> (f32, f32) {
        match self {
            Self::MinimumActivationDelay => (0.0, 10.0),
            Self::MaximumActivationDelay => (0.0, 10.0),
            Self::MinimumCycleDuration => (0.0, 10.0),
            Self::MaximumCycleDuration => (0.0, 10.0),
            Self::RepeatCount => (0.0, 255.0),
            Self::LaunchSpeedWeight => (0.0, 1.0),
            Self::StartingHealth => (0.0, 100.0),
            Self::RechargeScale => (0.0, 3.0),
            Self::ActivationCostScale => (0.0, 3.0),
            Self::ActiveEnergyScale => (0.0, 3.0),
            Self::ActivationLockout => (0.0, 10.0),
            Self::TargetingRange => (0.0, 3.0),
            Self::CollisionLimit => (0.0, 127.0),
            Self::TrackingSpeedChange => (0.0, 200.0),
            Self::MinimumSpawnDelay => (0.0, 10.0),
            Self::MaximumSpawnDelay => (0.0, 10.0),
            Self::MinimumPartDuration => (0.001, 60.0),
            Self::MaximumPartDuration => (0.001, 60.0),
            Self::EffectPriority => (-1.0, 100.0),
            Self::EffectGroup => (0.0, 15.0),
            Self::BounceCountIncrement => (0.0, 16.0),
            Self::BounceAngleVariation => (0.0, 30.0),
            Self::BounceSpeedVariation => (0.0, 100.0),
            Self::BounceSurfaceRadius => (0.0, 2.0),
            Self::ContactSpeed => (0.0, 50.0),
            Self::ContactFinalSpeed => (0.0, 80.0),
            Self::ContactFinalGravity => (0.0, 10.0),
            Self::ContactCurveStart => (0.0, 30.0),
            Self::ContactCurveEnd => (0.0, 30.0),
            Self::CleanupOnContact => (0.0, 1.0),
            Self::SurfacePlacement => (0.0, 1.0),
            Self::TriggeredDurationScaling => (0.0, 8.0),
            Self::QueryScaleReduction => (0.0, 5.0),
            Self::ContactCleanupDelay => (0.0, 60.0),
            Self::ContactCleanupBaseOffset => (-60.0, 60.0),
            Self::ContactTriggerDelay => (0.0, 60.0),
            Self::ContactTriggerBaseOffset => (-60.0, 60.0),
            Self::ContactCleanupOffset => (-60.0, 60.0),
            Self::ContactTriggerOffset => (-60.0, 60.0),
            Self::ContactCleanupScaling => (0.0, 8.0),
            Self::ContactTriggerScaling => (0.0, 8.0),
            Self::ContactCleanupVariation => (0.0, 60.0),
            Self::ContactCleanupScalingOffset => (-8.0, 8.0),
            Self::ContactTriggerScalingOffset => (-8.0, 8.0),
        }
    }
    pub const fn scale(self) -> f32 {
        match self {
            Self::StartingHealth | Self::BounceSpeedVariation => 100.0,
            Self::BounceAngleVariation => 180.0 / std::f32::consts::PI,
            _ => 1.0,
        }
    }
}

pub(crate) fn recognizes(locator: &WeaponRuntimeFieldLocator) -> bool {
    locator.path.first().is_some_and(|step| {
        step.name_hash == ROOT && step.type_handle == locator.root_schema && step.byte_offset == 0
    })
}

pub(super) fn supports(schema: u32) -> bool {
    matches!(
        schema,
        0x8080_84E9 | 0x8080_388F | 0x8080_4B8A | 0x8080_377D | 0x8080_37A9 | 0x8080_4205
    ) || crate::runtime::native_type_inherits(schema, 0x8080_3BF6)
}

pub(super) fn setting(
    owner: u32,
    root: &WeaponRuntimeRoot,
    field: &WeaponRuntimeField,
    payload: Option<&[u8]>,
) -> Option<Setting> {
    if !recognizes(&field.locator) {
        return None;
    }
    let last = field.locator.path.last()?;
    let property = Property::from_id(last.name_hash.checked_sub(VALUE)?)?;
    let requires_positive_minimum = property == Property::MaximumActivationDelay
        && payload
            .and_then(|payload| u32_at(payload, field.owner_offset as usize - 8).ok())
            .is_none_or(|flags| flags & 2 == 0);
    let codec = match field.kind {
        WeaponRuntimeValueKind::Float32 => Codec::Float,
        WeaponRuntimeValueKind::SignedInteger { bits: 32 } => Codec::Integer,
        WeaponRuntimeValueKind::SignedInteger { bits: 8 } => Codec::Byte,
        WeaponRuntimeValueKind::UnsignedInteger { bits: 8 } => Codec::UnsignedByte,
        _ => return None,
    };
    // Reuse a compatible opaque field so sibling controls compose with saved overrides.
    let end = field.owner_offset.checked_add(field.locator.byte_size)?;
    let mut covers = root.fields.iter().filter(|candidate| {
        candidate.source == WeaponRuntimeFieldSource::OpaqueNativeType
            && matches!(candidate.value, WeaponRuntimeValue::Bytes(_))
            && candidate.owner_offset <= field.owner_offset
            && candidate
                .owner_offset
                .checked_add(candidate.locator.byte_size)
                .is_some_and(|limit| limit >= end)
    });
    let (candidate, extra) = (covers.next(), covers.next());
    let (field, lane) = if let (Some(candidate), None) = (candidate, extra) {
        (
            candidate.clone(),
            Some(Lane {
                at: (field.owner_offset - candidate.owner_offset) as usize,
                codec,
            }),
        )
    } else {
        (field.clone(), None)
    };
    Some(Setting {
        kind: Kind::Native(property),
        owner_tag: owner,
        field,
        lane,
        requires_positive_minimum,
    })
}

pub(crate) fn fields(
    manager: &PackageManager,
    data: &[u8],
    root: &WeaponRuntimeRoot,
    binding: u32,
    index: u16,
) -> Result<Vec<WeaponRuntimeField>, String> {
    if !supports(root.schema) {
        return Ok(Vec::new());
    }
    let mut reader = Reader {
        manager,
        data,
        root,
        binding,
        index,
        registry: Registry::new()?,
        fields: Vec::new(),
    };
    let run = match root.schema {
        0x8080_84E9 => reader.creator(),
        0x8080_388F => reader.projectile(),
        0x8080_4B8A => reader.health(),
        0x8080_377D => reader.tracking(),
        0x8080_37A9 => reader.spawner(),
        0x8080_4205 => reader.status(),
        _ if crate::runtime::native_type_inherits(root.schema, 0x8080_3BF6) => reader.controller(),
        _ => Ok(()),
    };
    run.map_err(|error| {
        format!(
            "Ability settings 0x{:08X} at +0x{:X}: {error}",
            root.schema, root.owner_offset
        )
    })?;
    Ok(reader.fields)
}

/// What a creating group or node of an effect flow makes. Its settings sit in `body`. A group
/// makes every graph its kind-15 nodes name, and a node the one it names.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Creation {
    pub owner_tag: u32,
    pub body: u32,
    pub graphs: Vec<u32>,
}

/// The creations of `graph`'s effect flows (`808084E9`). A flow that does not read has none.
pub fn creations(manager: &PackageManager, graph: &WeaponRuntimeGraph) -> Vec<Creation> {
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
        );
    let mut seen = std::collections::BTreeSet::new();
    let mut found = Vec::new();
    for (owner, root) in roots {
        if root.schema != 0x8080_84E9 || !seen.insert((owner, root.owner_offset)) {
            continue;
        }
        let (Ok(data), Ok(registry)) = (manager.read_tag(TagHash(owner)), Registry::new()) else {
            continue;
        };
        let mut reader = Reader {
            manager,
            data: &data,
            root,
            binding: 0,
            index: 0,
            registry,
            fields: Vec::new(),
        };
        let Ok(made) = reader.creations() else {
            continue;
        };
        found.extend(made.into_iter().filter_map(|(body, graphs)| {
            Some(Creation {
                owner_tag: owner,
                body: u32::try_from(body).ok()?,
                graphs,
            })
        }));
    }
    found
}

struct Reader<'a> {
    manager: &'a PackageManager,
    data: &'a [u8],
    root: &'a WeaponRuntimeRoot,
    binding: u32,
    index: u16,
    registry: Registry,
    fields: Vec<WeaponRuntimeField>,
}

impl Reader<'_> {
    fn size(&mut self, class: u32) -> Result<usize, String> {
        self.registry
            .record(class, |_| {
                Err("An ability lane requires a native schema".into())
            })
            .map(|record| record.size)
    }
    fn extent(&mut self, at: usize, class: u32) -> Result<(), String> {
        let size = self.size(class)?;
        rows_fit(self.data, at, 1, size)
    }
    fn pair(&mut self, at: usize, definition: u32, source: u32) -> Result<usize, String> {
        self.extent(at, definition)?;
        let twin = usize::try_from(u64_at(self.data, at + 8)?)
            .map_err(|_| "Ability pair pointer overflows")?;
        self.extent(twin, source)?;
        if u32_at(self.data, at + 4)? != source
            || u32_at(self.data, twin + 4)? != definition
            || u32_at(self.data, at)? != u32_at(self.data, twin)?
            || u32_at(self.data, at)? != u32_at(self.data, self.root.owner_offset as usize)?
            || u64_at(self.data, twin + 8)? != at as u64
        {
            return Err("Ability definition and source are not a complete reciprocal pair".into());
        }
        Ok(twin)
    }
    fn root_pair(&mut self) -> Result<usize, String> {
        let at = self.root.owner_offset as usize;
        self.pair(at, self.root.schema, u32_at(self.data, at + 4)?)
    }
    fn array(&mut self, at: usize, class: u32) -> Result<Vec<usize>, String> {
        // Native empty descriptors may have a null pointer and no array header.
        if u64_at(self.data, at)? == 0 {
            bytes_at::<16>(self.data, at)?;
            return Ok(Vec::new());
        }
        let (count, header, rows, actual) = native_array_at(self.data, at)?;
        if actual != class || u32_at(self.data, header + 12)? != 0 {
            return Err("Ability array has an incompatible row type".into());
        }
        let stride = self.size(class)?;
        rows_fit(self.data, rows, count, stride)?;
        if stride == 0 || count > 8192 {
            return Err("Ability array exceeds its structural bound".into());
        }
        Ok((0..count).map(|index| rows + index * stride).collect())
    }
    fn optional(&mut self, at: usize, class: u32) -> Result<Option<usize>, String> {
        let relative = i64_at(self.data, at)?;
        if relative == 0 {
            return Ok(None);
        }
        let target = relative_offset(at, 0, relative)?;
        if target < 4 || u32_at(self.data, target - 4)? != class {
            return Err("Ability reference has an incompatible native type".into());
        }
        self.extent(target, class)?;
        Ok(Some(target))
    }
    fn float(&self, at: usize) -> Result<f32, String> {
        let value = f32::from_bits(u32_at(self.data, at)?);
        if !value.is_finite() {
            return Err("Ability setting is not finite".into());
        }
        Ok(value)
    }
    fn seconds(&self, at: usize) -> Result<bool, String> {
        let low = self.float(at)?;
        let high = self.float(at + 4)?;
        Ok(low >= 0.0 && high >= low)
    }
    fn put(
        &mut self,
        property: Property,
        class: u32,
        body: usize,
        offset: usize,
        route: &[u32],
        codec: Codec,
    ) -> Result<(), String> {
        let at = body
            .checked_add(offset)
            .ok_or("Ability lane offset overflows")?;
        if self.fields.iter().any(|field| {
            field.owner_offset as usize == at
                && field
                    .locator
                    .path
                    .last()
                    .is_some_and(|step| step.name_hash == VALUE + property as u32)
        }) {
            return Ok(());
        }
        let (kind, value) = match codec {
            Codec::Float => (
                WeaponRuntimeValueKind::Float32,
                WeaponRuntimeValue::Float32Bits(self.float(at)?.to_bits()),
            ),
            Codec::Integer => (
                WeaponRuntimeValueKind::SignedInteger { bits: 32 },
                WeaponRuntimeValue::Signed(i64::from(i32::from_le_bytes(bytes_at(self.data, at)?))),
            ),
            Codec::Byte => (
                WeaponRuntimeValueKind::SignedInteger { bits: 8 },
                WeaponRuntimeValue::Signed(i64::from(bytes_at::<1>(self.data, at)?[0] as i8)),
            ),
            Codec::UnsignedByte => (
                WeaponRuntimeValueKind::UnsignedInteger { bits: 8 },
                WeaponRuntimeValue::Unsigned(u64::from(bytes_at::<1>(self.data, at)?[0])),
            ),
        };
        let step = |name_hash, type_handle, byte_offset| WeaponRuntimePathElement {
            name_hash,
            type_handle: SchemaHandle::from(type_handle),
            byte_offset,
        };
        let mut path = vec![step(ROOT, self.root.schema, 0)];
        path.extend(route.iter().map(|index| step(ROW, class, *index)));
        path.push(step(VALUE + property as u32, class, offset as u32));
        self.fields.push(WeaponRuntimeField {
            locator: WeaponRuntimeFieldLocator {
                graph_tag: None,
                binding_hash: self.binding.into(),
                resource_index: self.index,
                root: self.root.kind,
                root_schema: self.root.schema.into(),
                path,
                type_handle: class.into(),
                value_offset: offset as u32,
                byte_size: kind.byte_size(),
            },
            owner_offset: u32::try_from(at).map_err(|_| "Ability field exceeds 32-bit offsets")?,
            name: property.label().into(),
            path_label: format!("{} / {}", self.root.kind.label(), property.label()),
            kind,
            value,
            source: WeaponRuntimeFieldSource::NativeDeclaration,
            generated_kind: None,
            name_inferred: false,
        });
        Ok(())
    }
}
