//! A property: one established value of a graph, with its kind, reading, idle state and
//! the gates that hide it.
use super::*;

/// What a tile changes, on every graph its card stands for.
pub(super) enum Value {
    Duration(Vec<EffectLength>),
    /// A constant of a timer that scales an input before adding seconds: the seconds it adds,
    /// or the input's coefficient when `scaling`. Each constant row holds its value in all four
    /// lanes.
    Timer {
        fields: Vec<WeaponRuntimeField>,
        scaling: bool,
    },
    Flight(Vec<Parameter>),
    /// A setting of a projectile's speed and gravity curve, kept in its definition and its
    /// instance's curve state together.
    Curve(Vec<Curve>),
    /// A setting its native reader acts on, such as a region's recovery or a filter's flag.
    Setting(Vec<Setting>),
    /// A modifier's amount: a factor when it multiplies, else an amount it adds.
    Amount {
        fields: Vec<WeaponRuntimeField>,
        multiply: bool,
    },
    /// A named float of a component whose meaning is established.
    Field(Vec<WeaponRuntimeField>),
    /// A movement controller value inside an opaque field.
    Movement(Vec<MovementValue>),
}

/// One tile: its name, what its tooltip adds, the stock value and the values it changes.
pub(super) struct Property {
    pub(super) label: String,
    pub(super) hint: String,
    pub(super) stock: f32,
    pub(super) value: Value,
}

pub(super) type PropertySignature = (String, u32, u8, Option<(WeaponRuntimeFieldLocator, u32)>);

impl Property {
    /// What two properties share when one tile stands for both. Conditional damage rows stay
    /// apart by their row, since each row has a predicate of its own.
    pub(super) fn signature(&self) -> PropertySignature {
        let kind = match &self.value {
            Value::Duration(_) => 0,
            Value::Flight(_) => 1,
            Value::Amount {
                multiply: false, ..
            } => 2,
            Value::Amount { multiply: true, .. } => 3,
            Value::Field(_) => 4,
            Value::Movement(_) => 5,
            Value::Curve(_) => 6,
            Value::Setting(_) => 7,
            Value::Timer { scaling: false, .. } => 8,
            Value::Timer { scaling: true, .. } => 9,
        };
        let row = match &self.value {
            Value::Setting(settings)
                if matches!(
                    settings[0].kind,
                    SettingKind::ConditionalDamage | SettingKind::Native(_)
                ) =>
            {
                Some((settings[0].field.locator.clone(), settings[0].offset()))
            }
            _ => None,
        };
        (self.label.clone(), self.stock.to_bits(), kind, row)
    }

    /// Grouped controls must disclose differing saved values instead of showing only the first.
    pub(super) fn mixed(&self, values: &[WeaponRuntimeValueOverride]) -> bool {
        let readings = match &self.value {
            Value::Duration(items) => items
                .iter()
                .filter_map(|item| length_reading(values, item))
                .collect::<Vec<_>>(),
            Value::Timer { fields, .. } => fields
                .iter()
                .filter_map(|field| {
                    effect_length::seconds(
                        &own(values, field).unwrap_or_else(|| field.value.clone()),
                    )
                })
                .collect(),
            Value::Flight(items) => items
                .iter()
                .filter_map(|item| item.value(values).ok())
                .collect(),
            Value::Amount { fields, .. } | Value::Field(fields) => fields
                .iter()
                .filter_map(|field| {
                    float(&own(values, field).unwrap_or_else(|| field.value.clone()))
                })
                .collect(),
            Value::Movement(items) => items
                .iter()
                .map(|item| item.unit.value(item.bits(values)))
                .collect(),
            Value::Curve(items) => items
                .iter()
                .filter_map(|item| item.value(values).ok())
                .collect(),
            Value::Setting(items) => items.iter().map(|item| item.value(values)).collect(),
        };
        readings.first().is_some_and(|first| {
            readings
                .iter()
                .skip(1)
                .any(|value| value.to_bits() != first.to_bits())
        })
    }

    /// Takes in the values of `other`, which has this one's signature.
    pub(super) fn absorb(&mut self, other: Self) {
        match (&mut self.value, other.value) {
            (Value::Duration(own), Value::Duration(more)) => own.extend(more),
            (Value::Timer { fields: own, .. }, Value::Timer { fields: more, .. }) => {
                own.extend(more);
            }
            (Value::Flight(own), Value::Flight(more)) => own.extend(more),
            (Value::Amount { fields: own, .. }, Value::Amount { fields: more, .. })
            | (Value::Field(own), Value::Field(more)) => own.extend(more),
            (Value::Movement(own), Value::Movement(more)) => own.extend(more),
            (Value::Curve(own), Value::Curve(more)) => own.extend(more),
            (Value::Setting(own), Value::Setting(more)) => own.extend(more),
            _ => {}
        }
    }

    /// The value it shows: its first value's own, else the stock one.
    pub(super) fn current(&self, values: &[WeaponRuntimeValueOverride]) -> f32 {
        let edited = match &self.value {
            Value::Duration(lengths) => lengths
                .first()
                .and_then(|length| length_reading(values, length)),
            Value::Timer { fields, .. } => fields
                .first()
                .and_then(|field| own(values, field))
                .and_then(|value| effect_length::seconds(&value)),
            Value::Flight(parameters) => parameters
                .first()
                .and_then(|parameter| parameter.value(values).ok()),
            Value::Amount { fields, .. } | Value::Field(fields) => fields
                .first()
                .and_then(|field| own(values, field))
                .as_ref()
                .and_then(float),
            Value::Movement(movement) => movement
                .first()
                .map(|movement| movement_value(movement, movement.bits(values))),
            Value::Curve(curves) => curves.first().and_then(|curve| curve.value(values).ok()),
            Value::Setting(settings) => settings.first().map(|setting| setting.value(values)),
        };
        edited.unwrap_or(self.stock)
    }

    pub(super) fn is_modified(&self, values: &[WeaponRuntimeValueOverride]) -> bool {
        match &self.value {
            Value::Duration(lengths) => lengths
                .iter()
                .any(|length| length_reading(values, length) != length_reading(&[], length)),
            Value::Timer { fields, .. } => fields.iter().any(|field| own(values, field).is_some()),
            Value::Flight(parameters) => parameters
                .iter()
                .any(|parameter| parameter.is_modified(values)),
            Value::Amount { fields, .. } | Value::Field(fields) => {
                fields.iter().any(|field| own(values, field).is_some())
            }
            Value::Movement(movement) => {
                movement.iter().any(|movement| movement.is_modified(values))
            }
            Value::Curve(curves) => curves.iter().any(|curve| curve.is_modified(values)),
            Value::Setting(settings) => settings.iter().any(|setting| setting.is_modified(values)),
        }
    }

    /// Whether each length it stands for can be Unlimited.
    pub(super) fn can_be_unlimited(&self) -> bool {
        matches!(&self.value, Value::Duration(lengths)
            if lengths.iter().all(|length| length.unlimited.is_some()))
    }

    /// Sets the Unlimited flag of a length saved as Unlimited without it, so the length never
    /// ends, as it reads.
    pub(super) fn keep_unlimited(&self, values: &mut Vec<WeaponRuntimeValueOverride>) {
        let Value::Duration(lengths) = &self.value else {
            return;
        };
        for length in lengths {
            let Some(flag) = &length.unlimited else {
                continue;
            };
            let negative = own(values, &length.field)
                .as_ref()
                .and_then(effect_length::seconds)
                .is_some_and(|seconds| seconds < 0.0);
            let current = own(values, &flag.field).unwrap_or_else(|| flag.field.value.clone());
            if negative
                && !flag.is_set(&current)
                && let Some(set) = flag.with(&current, true)
            {
                put(values, &flag.field, set);
            }
        }
    }

    /// Sets every value it stands for to `value`.
    pub(super) fn set(&self, values: &mut Vec<WeaponRuntimeValueOverride>, value: f32) {
        match &self.value {
            // Unlimited is -1 with the timer's flag set, and a length takes back the stock flag.
            Value::Duration(lengths) => {
                let unlimited = value < 0.0;
                for length in lengths {
                    let seconds = if unlimited { UNLIMITED } else { value };
                    put(values, &length.field, EffectLength::encode(seconds));
                    if let Some(flag) = &length.unlimited {
                        let current =
                            own(values, &flag.field).unwrap_or_else(|| flag.field.value.clone());
                        if let Some(next) = flag.with(&current, unlimited) {
                            put(values, &flag.field, next);
                        }
                    }
                }
            }
            Value::Timer { fields, .. } => {
                for field in fields {
                    put(values, field, EffectLength::encode(value));
                }
            }
            Value::Flight(parameters) => {
                if parameters
                    .iter()
                    .all(|parameter| parameter.kind.validate(value).is_ok())
                {
                    for parameter in parameters {
                        let _ = parameter.set(values, value);
                    }
                }
            }
            Value::Amount { fields, .. } | Value::Field(fields) => {
                for field in fields {
                    put(
                        values,
                        field,
                        WeaponRuntimeValue::Float32Bits(value.to_bits()),
                    );
                }
            }
            Value::Movement(movement) => {
                for movement in movement {
                    let bits = movement.unit.bits(value);
                    movement.write(values, bits);
                }
            }
            Value::Curve(curves) => {
                if curves
                    .iter()
                    .all(|curve| curve.kind.validate(value).is_ok())
                {
                    for curve in curves {
                        let _ = curve.set(values, value);
                    }
                }
            }
            Value::Setting(settings) => {
                for setting in settings {
                    let _ = setting.set(values, value);
                }
            }
        }
    }

    pub(super) fn reset(&self, values: &mut Vec<WeaponRuntimeValueOverride>) {
        match &self.value {
            Value::Duration(lengths) => {
                values.retain(|each| {
                    lengths
                        .iter()
                        .all(|length| each.locator != length.field.locator)
                });
                for flag in lengths
                    .iter()
                    .filter_map(|length| length.unlimited.as_ref())
                {
                    if let Some(current) = own(values, &flag.field)
                        && let Some(stock) = flag.with(&current, flag.stock())
                    {
                        put(values, &flag.field, stock);
                    }
                }
            }
            Value::Timer { fields, .. } => {
                values.retain(|each| fields.iter().all(|field| each.locator != field.locator));
            }
            Value::Flight(parameters) => {
                for parameter in parameters {
                    let _ = parameter.reset(values);
                }
            }
            Value::Amount { fields, .. } | Value::Field(fields) => {
                values.retain(|each| fields.iter().all(|field| each.locator != field.locator));
            }
            Value::Movement(movement) => {
                for movement in movement {
                    movement.reset(values);
                }
            }
            Value::Curve(curves) => {
                for curve in curves {
                    let _ = curve.reset(values);
                }
            }
            Value::Setting(settings) => {
                for setting in settings {
                    setting.reset(values);
                }
            }
        }
    }

    /// `value` as this property reads: seconds, a factor, a signed amount or a distance.
    pub(super) fn reading(&self, value: f64) -> String {
        match &self.value {
            Value::Duration(_) => duration_text(value),
            Value::Timer { scaling: false, .. } => format!("{} s", number(value)),
            Value::Timer { scaling: true, .. } => number(value),
            Value::Flight(parameters) if parameters[0].kind == Kind::TravelDistance => {
                travel_text(value)
            }
            Value::Flight(parameters) if parameters[0].kind == Kind::Speed => speed_text(value),
            Value::Flight(_) | Value::Amount { multiply: true, .. } => {
                format!("×{}", number(value))
            }
            Value::Amount {
                multiply: false, ..
            } => signed(value),
            Value::Movement(movement) => movement_text(movement[0].unit, value),
            Value::Field(_) => number(value),
            Value::Curve(curves) => match curves[0].kind {
                CurveKind::FinalSpeed => number(value),
                CurveKind::FinalGravity => format!("×{}", number(value)),
                CurveKind::Start | CurveKind::End => format!("{} Units", number(value)),
            },
            Value::Setting(settings) => setting_text(settings[0].kind, value),
        }
    }
}

/// The Recovery Time at or below which a region's reader stops its regeneration update.
pub(super) const RECOVERY_OFF: f64 = 0.0001;

/// A setting's value as its field reads it. A value its native reader gives a meaning of its own
/// reads as that meaning: a region whose Recovery Time is zero never recovers, -1 updates expire a
/// projectile by age, a proximity range of zero and a negative proximity time set nothing, and a
/// pierce or bounce limit of zero lifts the limit.
pub(super) fn setting_text(kind: SettingKind, value: f64) -> String {
    if let SettingKind::Native(property) = kind {
        use sundial::package_authoring::ability_settings::NativeProperty;
        let suffix = match property {
            NativeProperty::StartingHealth | NativeProperty::BounceSpeedVariation => Some("%"),
            NativeProperty::BounceAngleVariation => Some("°"),
            NativeProperty::ContactSpeed | NativeProperty::ContactFinalSpeed => Some(" Units/s"),
            NativeProperty::TrackingSpeedChange => Some(" Units/Update"),
            _ => None,
        };
        if let Some(suffix) = suffix {
            return format!("{}{suffix}", number(value));
        }
    }
    let word = match kind {
        SettingKind::RecoveryTime if value <= RECOVERY_OFF => Some("Off"),
        SettingKind::ExpirationUpdates if value < 0.0 => Some("By Age"),
        SettingKind::ProximityRange if value <= 0.0 => Some("Off"),
        SettingKind::DamageBreakThreshold if value == 0.0 => Some("Never"),
        SettingKind::MinimumProximityTime | SettingKind::MaximumProximityTime if value < 0.0 => {
            Some("Off")
        }
        SettingKind::PierceLimit | SettingKind::BounceLimit if value.round() == 0.0 => {
            Some("No Limit")
        }
        SettingKind::ExpirationResponse => setting_choice(&EXPIRATION_RESPONSES, value),
        SettingKind::CollisionMode => setting_choice(&COLLISION_MODES, value),
        _ => None,
    };
    if let Some(word) = word {
        return word.to_owned();
    }
    if matches!(
        kind,
        SettingKind::TurnRate | SettingKind::FastThrowTracking | SettingKind::TurnRateOffset
    ) {
        return format!("{}°/s", number(value));
    }
    match kind.unit() {
        SettingUnit::Factor => format!("×{}", number(value)),
        SettingUnit::Seconds => format!("{} s", number(value)),
        SettingUnit::Flag if value >= 0.5 => "On".to_owned(),
        SettingUnit::Flag => "Off".to_owned(),
        SettingUnit::Number => number(value),
        SettingUnit::Distance => format!("{} Units", number(value)),
        SettingUnit::Count => format!("{value:.0}"),
    }
}

/// What a projectile does when it expires, by the stored value: retire unless already finishing,
/// start finishing travel, or trigger its components' finish events.
pub(super) const EXPIRATION_RESPONSES: [&str; 3] = ["Retire", "Finish Travel", "Finish Events"];
/// How a projectile looks for contacts, by the stored value: a deferred contact query, the same
/// with a supplemental sphere, or a direct sphere query.
pub(super) const COLLISION_MODES: [&str; 3] = ["Contact", "Contact and Sphere", "Sphere"];

pub(super) fn parse_setting(kind: SettingKind, text: &str) -> Option<f64> {
    match (kind, text.trim().to_ascii_lowercase().as_str()) {
        (SettingKind::RecoveryTime | SettingKind::ProximityRange, "off")
        | (SettingKind::DamageBreakThreshold, "never")
        | (SettingKind::PierceLimit | SettingKind::BounceLimit, "no limit") => Some(0.0),
        (SettingKind::ExpirationUpdates, "by age")
        | (SettingKind::MinimumProximityTime | SettingKind::MaximumProximityTime, "off") => {
            Some(-1.0)
        }
        _ => parse_number(text),
    }
}

/// The name of a choice a setting stores as a small whole number.
pub(super) fn setting_choice(names: &[&'static str], value: f64) -> Option<&'static str> {
    let index = value.round();
    (index >= 0.0)
        .then(|| names.get(index as usize).copied())
        .flatten()
}

/// Whether a setting does nothing while another setting of its part turns its reader off, so its
/// tile waits until that one is set:
/// - A region's delays wait on its own Recovery Time. A time of zero skips the region's update.
///   The delays sit 8 and 4 bytes before the time in the region's row.
/// - Expiration Time waits on Expiration Updates of the same field, read only while that is
///   exactly -1. Any other count bypasses the age read.
/// - The proximity timer endpoints wait on Proximity Range of the same field, whose branch a
///   range of zero turns off.
/// - Invert Source Filter waits on Source Filter, the key 4 bytes before it, since the empty key
///   skips the inversion.
pub(super) fn idle(
    property: &Property,
    siblings: &[Property],
    values: &[WeaponRuntimeValueOverride],
) -> bool {
    let Value::Setting(settings) = &property.value else {
        return false;
    };
    let gate = Gate { siblings, values };
    settings.iter().all(|setting| {
        let same_field = |other: &Setting| {
            other.owner_tag == setting.owner_tag && other.field.locator == setting.field.locator
        };
        match setting.kind {
            SettingKind::RetirementDelay => gate.off(
                SettingKind::RetireOnRemoval,
                |other| {
                    other.owner_tag == setting.owner_tag
                        && other.field.owner_offset == setting.field.owner_offset + 4
                },
                |on| on < 0.5,
            ),
            SettingKind::MovementThreshold => gate.off(
                SettingKind::IgnoreMovement,
                |other| {
                    other.owner_tag == setting.owner_tag
                        && other.field.owner_offset == setting.field.owner_offset + 0x60
                },
                |on| on >= 0.5,
            ),
            SettingKind::Native(native) => {
                use sundial::package_authoring::ability_settings::NativeProperty;
                match native {
                    NativeProperty::MaximumActivationDelay if setting.requires_positive_minimum => {
                        gate.off(
                            SettingKind::Native(NativeProperty::MinimumActivationDelay),
                            |other| {
                                other.owner_tag == setting.owner_tag
                                    && other.offset() + 4 == setting.offset()
                            },
                            |minimum| minimum <= 0.0,
                        )
                    }
                    NativeProperty::LaunchSpeedWeight => true,
                    NativeProperty::ContactTriggerDelay
                    | NativeProperty::ContactTriggerBaseOffset
                    | NativeProperty::ContactTriggerOffset
                    | NativeProperty::ContactTriggerScaling
                    | NativeProperty::ContactTriggerScalingOffset => gate.off(
                        SettingKind::ExpirationUpdates,
                        |other| {
                            other.owner_tag == setting.owner_tag
                                && other.field.locator.binding_hash
                                    == setting.field.locator.binding_hash
                                && other.field.locator.resource_index
                                    == setting.field.locator.resource_index
                        },
                        |count| count != -1.0,
                    ),
                    NativeProperty::EffectPriority => gate.off(
                        SettingKind::Native(NativeProperty::EffectGroup),
                        |other| {
                            other.owner_tag == setting.owner_tag
                                && (same_field(other)
                                    || other.field.owner_offset + 4 == setting.field.owner_offset)
                        },
                        |group| group == 0.0,
                    ),
                    _ => false,
                }
            }
            SettingKind::RecoveryDelay | SettingKind::DepletedRecoveryDelay => {
                let gap = if setting.kind == SettingKind::RecoveryDelay {
                    0x8
                } else {
                    0x4
                };
                let row = |other: &Setting| {
                    other.owner_tag == setting.owner_tag
                        && other.field.owner_offset == setting.field.owner_offset + gap
                };
                gate.off(SettingKind::RecoveryTime, row, |time| time <= RECOVERY_OFF)
            }
            SettingKind::ExpirationTime => {
                gate.off(SettingKind::ExpirationUpdates, same_field, |count| {
                    count.round() != -1.0
                })
            }
            SettingKind::MinimumProximityTime | SettingKind::MaximumProximityTime => {
                gate.off(SettingKind::ProximityRange, same_field, |range| {
                    range <= 0.0
                })
            }
            SettingKind::InvertSourceFilter => {
                let key = |other: &Setting| {
                    other.owner_tag == setting.owner_tag
                        && other.field.owner_offset + 4 == setting.field.owner_offset
                };
                gate.off(SettingKind::SourceFilter, key, |on| on < 0.5)
            }
            _ => false,
        }
    })
}

/// Editing an endpoint carries its partner only when necessary to keep their native range
/// ordered. Every comparison uses the actual owner and scalar position, not matching labels.
pub(super) fn keep_ordered(
    property: &Property,
    before: f32,
    siblings: &[Property],
    values: &mut Vec<WeaponRuntimeValueOverride>,
) {
    let after = property.current(values);
    if after.to_bits() == before.to_bits() {
        return;
    }
    let Value::Setting(settings) = &property.value else {
        return;
    };
    use sundial::package_authoring::ability_settings::NativeProperty as N;
    for setting in settings {
        let (partner, minimum) = match setting.kind {
            SettingKind::Native(N::MinimumActivationDelay) => (N::MaximumActivationDelay, true),
            SettingKind::Native(N::MaximumActivationDelay) => (N::MinimumActivationDelay, false),
            SettingKind::Native(N::MinimumCycleDuration) => (N::MaximumCycleDuration, true),
            SettingKind::Native(N::MaximumCycleDuration) => (N::MinimumCycleDuration, false),
            SettingKind::Native(N::MinimumSpawnDelay) => (N::MaximumSpawnDelay, true),
            SettingKind::Native(N::MaximumSpawnDelay) => (N::MinimumSpawnDelay, false),
            SettingKind::Native(N::MinimumPartDuration) => (N::MaximumPartDuration, true),
            SettingKind::Native(N::MaximumPartDuration) => (N::MinimumPartDuration, false),
            SettingKind::Native(N::ContactCurveStart) => (N::ContactCurveEnd, true),
            SettingKind::Native(N::ContactCurveEnd) => (N::ContactCurveStart, false),
            _ => continue,
        };
        let own = setting.value(values);
        for sibling in siblings {
            let Value::Setting(others) = &sibling.value else {
                continue;
            };
            for other in others {
                let adjacent = if minimum {
                    setting.offset() + 4 == other.offset()
                } else {
                    other.offset() + 4 == setting.offset()
                };
                let reversed = if minimum {
                    own > other.value(values)
                } else {
                    own < other.value(values)
                };
                if other.kind == SettingKind::Native(partner)
                    && other.owner_tag == setting.owner_tag
                    && adjacent
                    && reversed
                {
                    let _ = other.set(values, own);
                }
            }
        }
    }
}

/// A card's settings, read with the draft's values, as the settings that can turn another off.
pub(super) struct Gate<'a> {
    pub(super) siblings: &'a [Property],
    pub(super) values: &'a [WeaponRuntimeValueOverride],
}

impl Gate<'_> {
    /// Whether a setting of `kind` that `place` accepts reads a value `off` takes.
    pub(super) fn off(
        &self,
        kind: SettingKind,
        place: impl Fn(&Setting) -> bool,
        off: impl Fn(f64) -> bool,
    ) -> bool {
        self.siblings.iter().any(|sibling| match &sibling.value {
            Value::Setting(others) => others.iter().any(|other| {
                other.kind == kind && place(other) && off(f64::from(other.value(self.values)))
            }),
            _ => false,
        })
    }
}

/// Whether a value's role is established. A movement value whose native consumer is not traced
/// sits in a card's closed Unconfirmed section, apart from the values whose meaning is known.
pub(super) fn confirmed(property: &Property) -> bool {
    !matches!(&property.value, Value::Movement(movement) if !movement[0].traced)
}

/// Settings most authors leave alone: finer clocks, contact and steering details, and the energy
/// values beside a controller's activation cost and recharge delay. A card keeps them in its
/// closed More section, so it leads with the values that change how an ability plays.
pub(super) const MORE: [SettingKind; 19] = [
    SettingKind::FallSpeedThreshold,
    SettingKind::MinimumFinishTime,
    SettingKind::MaximumFinishTime,
    SettingKind::FinishTravelTime,
    SettingKind::ExpirationUpdates,
    SettingKind::ExpirationResponse,
    SettingKind::CollisionMode,
    SettingKind::LeadTimeLimit,
    SettingKind::LeadDistanceLimit,
    SettingKind::SteeringAxisThreshold,
    SettingKind::TargetLead,
    SettingKind::MinimumProximityTime,
    SettingKind::MaximumProximityTime,
    SettingKind::SearchDelay,
    SettingKind::IncludeSelf,
    SettingKind::ActiveEnergyRate,
    SettingKind::EndingEnergyCost,
    SettingKind::MinimumActivationEnergy,
    SettingKind::EnergyFloor,
];

/// Whether a value sits in its card's closed More section.
pub(super) fn more(property: &Property) -> bool {
    use sundial::package_authoring::ability_settings::NativeProperty;

    matches!(&property.value, Value::Setting(settings) if MORE.contains(&settings[0].kind))
        || matches!(&property.value, Value::Setting(settings) if matches!(settings[0].kind,
            SettingKind::Native(native) if !matches!(native,
                NativeProperty::StartingHealth
                    | NativeProperty::RechargeScale
                    | NativeProperty::ActivationCostScale
                    | NativeProperty::ActiveEnergyScale
                    | NativeProperty::ActivationLockout
                    | NativeProperty::TargetingRange)))
        || matches!(&property.value, Value::Duration(_) if property.stock <= 0.0)
}

/// Whether a card's tile shows: a value whose role is established, which a bank lane does not
/// stand for, which no other setting of its card turns off, and which is not kept under More.
pub(super) fn shown(
    property: &Property,
    siblings: &[Property],
    lanes: &[&RowLane],
    values: &[WeaponRuntimeValueOverride],
) -> bool {
    confirmed(property)
        && !more(property)
        && !shadowed(lanes, property)
        && !idle(property, siblings, values)
}
