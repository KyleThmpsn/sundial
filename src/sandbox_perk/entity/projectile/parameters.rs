//! Moving-projectile properties traced through the Shadowkeep native consumer.
//! These mappings describe code behavior. Gameplay validation is separate.
use crate::runtime::{
    WeaponRuntimeField, WeaponRuntimeFieldLocator, WeaponRuntimeGraph, WeaponRuntimeResource,
    WeaponRuntimeRoot, WeaponRuntimeRootKind, WeaponRuntimeValue, WeaponRuntimeValueOverride,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Speed,
    Gravity,
    TravelDistance,
}

impl Kind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Speed => "Projectile Speed Multiplier",
            Self::Gravity => "Gravity Multiplier",
            Self::TravelDistance => "Travel Distance Limit",
        }
    }

    pub const fn suffix(self) -> &'static str {
        match self {
            Self::Speed | Self::Gravity => " ×",
            Self::TravelDistance => " units",
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Speed => {
                "Scales the launch speed. The weapon and any later speed curve still contribute to the final velocity."
            }
            Self::Gravity => {
                "Scales the initial downward acceleration. Zero removes this gravity contribution. A later gravity curve can still change it. The native range is 0 to 10."
            }
            Self::TravelDistance => {
                "Limits accumulated travel distance in native world units. Zero disables this limit. The launch source can apply an additional multiplier."
            }
        }
    }

    fn offsets(self) -> (u32, u32) {
        // CEC3C0 copies definition -> instance during reset. CF2380 applies
        // launch multipliers. CE43B0, CE4290 and CFC000 consume these fields.
        match self {
            Self::Speed => (0x144, 0x88),
            Self::Gravity => (0x148, 0xD8),
            Self::TravelDistance => (0x168, 0x74),
        }
    }

    pub fn validate(self, value: f32) -> Result<(), String> {
        let valid = value.is_finite()
            && match self {
                Self::Speed => value > 0.0,
                Self::Gravity => (0.0..=10.0).contains(&value),
                Self::TravelDistance => value >= 0.0,
            };
        if valid {
            Ok(())
        } else {
            let range = match self {
                Self::Speed => "greater than zero",
                Self::Gravity => "from 0 to 10",
                Self::TravelDistance => "zero or greater",
            };
            Err(format!("{} must be a finite value {range}.", self.label()))
        }
    }
}

/// The paired initial and reset values in one native Projectile Movement owner.
/// Symbolic imported owners may use their declared placeholder as `owner_tag`.
pub struct Stored {
    kind: Kind,
    offsets: [usize; 2],
    original: f32,
}

impl Stored {
    pub fn read(kind: Kind, owner: &[u8], owner_tag: u32, instance: usize) -> Result<Self, String> {
        use crate::package_payload::{u32_at, u64_at};
        if instance < 4
            || u32_at(owner, instance - 4)? != 0x8080_3B73
            || u32_at(owner, instance)? != owner_tag
            || u32_at(owner, instance + 4)? != 0x8080_388F
        {
            return Err("Projectile Movement has an unsupported stored instance".into());
        }
        let definition = usize::try_from(u64_at(owner, instance + 8)?)
            .map_err(|_| "Projectile definition offset overflow")?;
        if definition < 4 || u32_at(owner, definition - 4)? != 0x8080_388F {
            return Err("Projectile Movement has an unsupported stored definition".into());
        }
        let (initial, reset) = kind.offsets();
        let offsets = [
            instance.checked_add(initial as usize),
            definition.checked_add(reset as usize),
        ];
        let offsets = [
            offsets[0].ok_or("Projectile initial offset overflow")?,
            offsets[1].ok_or("Projectile reset offset overflow")?,
        ];
        let original = f32::from_bits(u32_at(owner, offsets[0])?);
        kind.validate(original)?;
        if original.to_bits() != u32_at(owner, offsets[1])? {
            return Err("Projectile initial and reset values disagree".into());
        }
        Ok(Self {
            kind,
            offsets,
            original,
        })
    }

    pub fn original(&self) -> f32 {
        self.original
    }

    pub fn write(&self, owner: &mut [u8], value: f32) -> Result<(), String> {
        self.kind.validate(value)?;
        for offset in self.offsets {
            if crate::package_payload::u32_at(owner, offset)? != self.original.to_bits() {
                return Err("Stored projectile value changed before writing".into());
            }
        }
        for offset in self.offsets {
            owner[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
struct Lane {
    field: WeaponRuntimeField,
    offset: usize,
    aliases: Vec<(u32, u16)>,
}

impl Lane {
    fn resolve(
        root: &WeaponRuntimeRoot,
        resource: &WeaponRuntimeResource,
        offset: u32,
    ) -> Option<Self> {
        let mut fields = root.fields.iter().filter(|field| {
            field.source != crate::runtime::WeaponRuntimeFieldSource::NativeDeclaration
                && field.locator.value_offset <= offset
                && field
                    .locator
                    .value_offset
                    .checked_add(field.locator.byte_size)
                    .is_some_and(|end| end >= offset + 4)
        });
        let field = fields.next()?.clone();
        if fields.next().is_some() || !matches!(field.value, WeaponRuntimeValue::Bytes(_)) {
            return None;
        }
        let lane = Self {
            offset: (offset - field.locator.value_offset) as usize,
            field,
            aliases: resource.alias_bindings.clone(),
        };
        lane.bits(&[]).ok()?;
        Some(lane)
    }

    fn matches(&self, locator: &WeaponRuntimeFieldLocator) -> bool {
        let native = &self.field.locator;
        if matches!((locator.graph_tag, native.graph_tag), (Some(a), Some(b)) if a != b) {
            return false;
        }
        if (locator.binding_hash.get(), locator.resource_index)
            != (native.binding_hash.get(), native.resource_index)
            && !self
                .aliases
                .contains(&(locator.binding_hash.get(), locator.resource_index))
        {
            return false;
        }
        let mut normalized = locator.clone();
        normalized.graph_tag = native.graph_tag;
        normalized.binding_hash = native.binding_hash;
        normalized.resource_index = native.resource_index;
        normalized == *native
    }

    fn bits(&self, draft: &[WeaponRuntimeValueOverride]) -> Result<u32, String> {
        let mut entries = draft
            .iter()
            .filter(|entry| self.matches(&entry.locator) || self.matches_scalar(&entry.locator));
        let entry = entries.next();
        let value = entry.map_or(&self.field.value, |entry| &entry.value);
        if entries.next().is_some() {
            return Err("Two edits target the same projectile field.".into());
        }
        if entry.is_some_and(|entry| self.matches_scalar(&entry.locator)) {
            return match value {
                WeaponRuntimeValue::Float32Bits(bits) => Ok(*bits),
                _ => Err("Projectile scalar has an incompatible value type.".into()),
            };
        }
        let WeaponRuntimeValue::Bytes(bytes) = value else {
            return Err("Projectile data has an incompatible value type.".into());
        };
        let bytes = bytes
            .get(self.offset..self.offset + 4)
            .ok_or("Projectile data is truncated.")?;
        Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn matches_scalar(&self, locator: &WeaponRuntimeFieldLocator) -> bool {
        let source = &self.field.locator;
        if matches!((locator.graph_tag, source.graph_tag), (Some(a), Some(b)) if a != b) {
            return false;
        }
        let pair = (locator.binding_hash.get(), locator.resource_index);
        if pair != (source.binding_hash.get(), source.resource_index)
            && !self.aliases.contains(&pair)
        {
            return false;
        }
        locator.root == source.root
            && locator.root_schema == source.root_schema
            && locator.byte_size == 4
            && locator
                .path
                .first()
                .is_some_and(|step| step.name_hash == 0x504E_5200)
            && locator
                .path
                .last()
                .is_some_and(|step| step.name_hash == 0x504E_5600)
            && locator.path[1..locator.path.len().saturating_sub(1)]
                .iter()
                .all(|step| step.name_hash == 0x504E_4900)
            && locator
                .path
                .iter()
                .skip(1)
                .map(|step| u64::from(step.byte_offset))
                .sum::<u64>()
                == u64::from(source.value_offset) + self.offset as u64
    }

    fn write(&self, draft: &mut Vec<WeaponRuntimeValueOverride>, bits: u32) -> Result<(), String> {
        self.bits(draft)?;
        let existing = draft.iter().position(|entry| self.matches(&entry.locator));
        let mut entry = existing
            .map(|index| draft[index].clone())
            .unwrap_or_else(|| WeaponRuntimeValueOverride {
                locator: self.field.locator.clone(),
                value: self.field.value.clone(),
            });
        let WeaponRuntimeValue::Bytes(bytes) = &mut entry.value else {
            unreachable!()
        };
        bytes[self.offset..self.offset + 4].copy_from_slice(&bits.to_le_bytes());
        if let Some(index) = existing {
            draft.remove(index);
        }
        draft.retain(|entry| !self.matches_scalar(&entry.locator));
        if entry.value != self.field.value {
            draft.push(entry);
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct Parameter {
    pub kind: Kind,
    pub owner_tag: u32,
    instance: Lane,
    definition: Lane,
}

impl Parameter {
    pub fn original(&self) -> f32 {
        f32::from_bits(self.definition.bits(&[]).expect("resolved native field"))
    }

    pub fn contains(&self, locator: &WeaponRuntimeFieldLocator) -> bool {
        self.instance.matches(locator)
            || self.definition.matches(locator)
            || self.instance.matches_scalar(locator)
            || self.definition.matches_scalar(locator)
    }

    /// Identifies either paired native scalar by its resolved position in this owner.
    pub fn targets_field(&self, owner: u32, field: &WeaponRuntimeField) -> bool {
        self.owner_tag == owner
            && field.locator.byte_size == 4
            && [&self.instance, &self.definition].into_iter().any(|lane| {
                lane.field.owner_offset.checked_add(lane.offset as u32) == Some(field.owner_offset)
            })
    }

    pub fn is_modified(&self, draft: &[WeaponRuntimeValueOverride]) -> bool {
        [&self.instance, &self.definition]
            .into_iter()
            .any(|lane| lane.bits(draft) != lane.bits(&[]))
    }

    pub fn value(&self, draft: &[WeaponRuntimeValueOverride]) -> Result<f32, String> {
        let bits = self.definition.bits(draft)?;
        let value = f32::from_bits(bits);
        self.kind.validate(value)?;
        // An uninitialized package instance can differ from the reset definition.
        // Once this property is edited, both copies must agree. Changes to other
        // lanes in the same opaque field do not make this property modified.
        if self.is_modified(draft) && self.instance.bits(draft)? != bits {
            return Err(format!(
                "{} differs between the instance and definition. Set it again or reset it.",
                self.kind.label()
            ));
        }
        Ok(value)
    }

    pub fn set(
        &self,
        draft: &mut Vec<WeaponRuntimeValueOverride>,
        value: f32,
    ) -> Result<(), String> {
        self.kind.validate(value)?;
        let mut next = draft.clone();
        for lane in [&self.instance, &self.definition] {
            lane.write(&mut next, value.to_bits())?;
        }
        *draft = next;
        Ok(())
    }

    pub fn reset(&self, draft: &mut Vec<WeaponRuntimeValueOverride>) -> Result<(), String> {
        let mut next = draft.clone();
        for lane in [&self.instance, &self.definition] {
            lane.write(&mut next, lane.bits(&[])?)?;
        }
        *draft = next;
        Ok(())
    }
}

/// The moving projectile's definition, when `resource` is one with paired roots of the native
/// sizes.
fn moving_projectile(resource: &WeaponRuntimeResource) -> Option<&WeaponRuntimeRoot> {
    let definition = resource.definition.as_ref()?;
    (resource.instance.kind == WeaponRuntimeRootKind::ComponentInstance
        && resource.instance.schema == 0x8080_3B73
        && resource.instance.byte_size == 0x1E0
        && definition.kind == WeaponRuntimeRootKind::ComponentDefinition
        && definition.schema == 0x8080_388F
        && definition.byte_size == 0x5D0)
        .then_some(definition)
}

/// Discover only the exact native moving-projectile schema and its paired roots.
/// Callers select a private action's owned graph before invoking this adapter.
pub fn discover(graph: &WeaponRuntimeGraph) -> Vec<Parameter> {
    graph
        .resources
        .iter()
        .flat_map(|resource| {
            let Some(definition) = moving_projectile(resource) else {
                return Vec::new();
            };
            [Kind::Speed, Kind::Gravity, Kind::TravelDistance]
                .into_iter()
                .filter_map(|kind| {
                    let (instance_offset, definition_offset) = kind.offsets();
                    let parameter = Parameter {
                        kind,
                        owner_tag: resource.owner_tag,
                        instance: Lane::resolve(&resource.instance, resource, instance_offset)?,
                        definition: Lane::resolve(definition, resource, definition_offset)?,
                    };
                    parameter.value(&[]).ok()?;
                    Some(parameter)
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// One setting of a moving projectile's speed and gravity curve. Over the distance it travels
/// from Curve Start to Curve End, its speed and gravity move from their launch values to Final
/// Speed and Final Gravity, clamped at both ends.
///
/// The definition keeps the four settings as typed fields of `80803803` at +0xC8. Reset copies
/// them into the instance's curve state, where they sit at +0x14C, followed at +0x15C by the
/// distance scale: one over the interval, or one when its absolute value is under 0.0001
/// (`487FAD`), clamped to 0 through 10000 (`D00D31..D00D58`).
/// The package instance holds that state already, so an edit writes the setting, its copy in the
/// state, and, for either end, the scale.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CurveKind {
    FinalSpeed,
    FinalGravity,
    Start,
    End,
}

impl CurveKind {
    const ALL: [Self; 4] = [Self::FinalSpeed, Self::FinalGravity, Self::Start, Self::End];

    const fn index(self) -> usize {
        match self {
            Self::FinalSpeed => 0,
            Self::FinalGravity => 1,
            Self::Start => 2,
            Self::End => 3,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::FinalSpeed => "Final Speed",
            Self::FinalGravity => "Final Gravity",
            Self::Start => "Curve Start",
            Self::End => "Curve End",
        }
    }

    pub fn validate(self, value: f32) -> Result<(), String> {
        let valid = value.is_finite()
            && match self {
                Self::FinalGravity => (0.0..=10.0).contains(&value),
                Self::FinalSpeed | Self::Start | Self::End => value >= 0.0,
            };
        if valid {
            Ok(())
        } else {
            let range = match self {
                Self::FinalGravity => "from 0 to 10",
                Self::FinalSpeed | Self::Start | Self::End => "zero or greater",
            };
            Err(format!("{} must be a finite value {range}.", self.label()))
        }
    }
}

/// Where the definition's curve settings sit, and the member that holds them.
const CURVE_SETTINGS: u32 = 0x8080_3803;
const CURVE_MEMBER: u32 = 0x8080_37B3;
const CURVE_MEMBER_OFFSET: u32 = 0xC8;
/// Where the instance's curve state keeps its copy of the settings, and then the scale.
const CURVE_STATE: u32 = 0x14C;
const CURVE_SCALE: u32 = 0x15C;
/// An interval narrower than this takes a scale of one.
const NARROW_INTERVAL: f32 = 0.0001;

/// The scale the native reset derives from a curve's ends.
fn curve_scale(start: f32, end: f32) -> f32 {
    let interval = end - start;
    if interval.abs() < NARROW_INTERVAL {
        1.0
    } else {
        (1.0 / interval).clamp(0.0, 10000.0)
    }
}

#[derive(Clone, Debug)]
pub struct Curve {
    pub kind: CurveKind,
    pub owner_tag: u32,
    /// The definition's four settings, in [`CurveKind`] order.
    settings: [WeaponRuntimeField; 4],
    /// The instance's copy of each, in the same order.
    state: [Lane; 4],
    scale: Lane,
}

impl Curve {
    fn setting(&self, kind: CurveKind, draft: &[WeaponRuntimeValueOverride]) -> f32 {
        let field = &self.settings[kind.index()];
        let value = draft
            .iter()
            .find(|entry| entry.locator == field.locator)
            .map_or(&field.value, |entry| &entry.value);
        match value {
            WeaponRuntimeValue::Float32Bits(bits) => f32::from_bits(*bits),
            _ => f32::NAN,
        }
    }

    pub fn original(&self) -> f32 {
        self.setting(self.kind, &[])
    }

    pub fn is_modified(&self, draft: &[WeaponRuntimeValueOverride]) -> bool {
        let state = &self.state[self.kind.index()];
        self.setting(self.kind, draft).to_bits() != self.original().to_bits()
            || state.bits(draft) != state.bits(&[])
    }

    pub fn value(&self, draft: &[WeaponRuntimeValueOverride]) -> Result<f32, String> {
        let value = self.setting(self.kind, draft);
        self.kind.validate(value)?;
        if self.is_modified(draft) && self.state[self.kind.index()].bits(draft)? != value.to_bits()
        {
            return Err(format!(
                "{} differs between the setting and the curve state. Set it again or reset it.",
                self.kind.label()
            ));
        }
        Ok(value)
    }

    pub fn set(
        &self,
        draft: &mut Vec<WeaponRuntimeValueOverride>,
        value: f32,
    ) -> Result<(), String> {
        self.kind.validate(value)?;
        self.write(draft, value)
    }

    pub fn reset(&self, draft: &mut Vec<WeaponRuntimeValueOverride>) -> Result<(), String> {
        self.write(draft, self.original())
    }

    fn write(&self, draft: &mut Vec<WeaponRuntimeValueOverride>, value: f32) -> Result<(), String> {
        let mut next = draft.clone();
        let field = &self.settings[self.kind.index()];
        next.retain(|entry| entry.locator != field.locator);
        if value.to_bits() != self.original().to_bits() {
            next.push(WeaponRuntimeValueOverride {
                locator: field.locator.clone(),
                value: WeaponRuntimeValue::Float32Bits(value.to_bits()),
            });
        }
        self.state[self.kind.index()].write(&mut next, value.to_bits())?;
        if matches!(self.kind, CurveKind::Start | CurveKind::End) {
            let (start, end) = (
                self.setting(CurveKind::Start, &next),
                self.setting(CurveKind::End, &next),
            );
            // Stock ends keep the stock scale bits, so a reset leaves the state as it shipped.
            let stock = start.to_bits() == self.setting(CurveKind::Start, &[]).to_bits()
                && end.to_bits() == self.setting(CurveKind::End, &[]).to_bits();
            let bits = if stock {
                self.scale.bits(&[])?
            } else {
                curve_scale(start, end).to_bits()
            };
            self.scale.write(&mut next, bits)?;
        }
        *draft = next;
        Ok(())
    }
}

/// The definition's curve setting `kind`, a typed field of the member at +0xC8.
fn curve_setting(definition: &WeaponRuntimeRoot, kind: CurveKind) -> Option<WeaponRuntimeField> {
    let offset = 4 * kind.index() as u32;
    let mut fields = definition.fields.iter().filter(|field| {
        field.source == crate::runtime::WeaponRuntimeFieldSource::NativeDeclaration
            && field.locator.type_handle.get() == CURVE_SETTINGS
            && field.locator.value_offset == offset
            && field.locator.byte_size == 4
            && matches!(field.value, WeaponRuntimeValue::Float32Bits(_))
            && field.locator.path.iter().any(|step| {
                step.type_handle.get() == CURVE_MEMBER && step.byte_offset == CURVE_MEMBER_OFFSET
            })
    });
    let field = fields.next()?.clone();
    fields.next().is_none().then_some(field)
}

/// The speed and gravity curve settings of each moving projectile whose instance state already
/// holds the definition's settings and the scale they derive. Any other shape offers none.
pub fn curves(graph: &WeaponRuntimeGraph) -> Vec<Curve> {
    let mut found = Vec::new();
    for resource in &graph.resources {
        let Some(definition) = moving_projectile(resource) else {
            continue;
        };
        let Some(settings) = CurveKind::ALL
            .iter()
            .map(|kind| curve_setting(definition, *kind))
            .collect::<Option<Vec<_>>>()
            .and_then(|settings| <[WeaponRuntimeField; 4]>::try_from(settings).ok())
        else {
            continue;
        };
        let Some(state) = CurveKind::ALL
            .iter()
            .map(|kind| {
                Lane::resolve(
                    &resource.instance,
                    resource,
                    CURVE_STATE + 4 * kind.index() as u32,
                )
            })
            .collect::<Option<Vec<_>>>()
            .and_then(|state| <[Lane; 4]>::try_from(state).ok())
        else {
            continue;
        };
        let Some(scale) = Lane::resolve(&resource.instance, resource, CURVE_SCALE) else {
            continue;
        };
        let template = Curve {
            kind: CurveKind::FinalSpeed,
            owner_tag: resource.owner_tag,
            settings,
            state,
            scale,
        };
        // The state must hold each setting, and the scale its ends derive.
        let held = CurveKind::ALL.iter().all(|kind| {
            let setting = template.setting(*kind, &[]);
            kind.validate(setting).is_ok()
                && template.state[kind.index()].bits(&[]).ok() == Some(setting.to_bits())
        });
        let expected = curve_scale(
            template.setting(CurveKind::Start, &[]),
            template.setting(CurveKind::End, &[]),
        );
        let scaled = template
            .scale
            .bits(&[])
            .is_ok_and(|bits| (f32::from_bits(bits) - expected).abs() <= expected.abs() * 1e-5);
        if !held || !scaled {
            continue;
        }
        found.extend(CurveKind::ALL.map(|kind| Curve {
            kind,
            ..template.clone()
        }));
    }
    found
}
