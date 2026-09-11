//! Moving-projectile properties traced through the Shadowkeep native consumer.
//! These mappings describe code behavior. Gameplay validation is separate.
use crate::weapon_runtime::{
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
            field.locator.value_offset <= offset
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
        if (locator.binding_hash, locator.resource_index)
            != (native.binding_hash, native.resource_index)
            && !self
                .aliases
                .contains(&(locator.binding_hash, locator.resource_index))
        {
            return false;
        }
        let mut normalized = locator.clone();
        normalized.binding_hash = native.binding_hash;
        normalized.resource_index = native.resource_index;
        normalized == *native
    }

    fn bits(&self, draft: &[WeaponRuntimeValueOverride]) -> Result<u32, String> {
        let mut entries = draft.iter().filter(|entry| self.matches(&entry.locator));
        let value = entries
            .next()
            .map_or(&self.field.value, |entry| &entry.value);
        if entries.next().is_some() {
            return Err("Two edits target the same projectile field.".into());
        }
        let WeaponRuntimeValue::Bytes(bytes) = value else {
            return Err("Projectile data has an incompatible value type.".into());
        };
        let bytes = bytes
            .get(self.offset..self.offset + 4)
            .ok_or("Projectile data is truncated.")?;
        Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
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
        self.instance.matches(locator) || self.definition.matches(locator)
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

/// Discover only the exact native moving-projectile schema and its paired roots.
/// Callers select a private action's owned graph before invoking this adapter.
pub fn discover(graph: &WeaponRuntimeGraph) -> Vec<Parameter> {
    graph
        .resources
        .iter()
        .flat_map(|resource| {
            let Some(definition) = &resource.definition else {
                return Vec::new();
            };
            if resource.instance.kind != WeaponRuntimeRootKind::ComponentInstance
                || resource.instance.schema != 0x8080_3B73
                || resource.instance.byte_size != 0x1E0
                || definition.kind != WeaponRuntimeRootKind::ComponentDefinition
                || definition.schema != 0x8080_388F
                || definition.byte_size != 0x5D0
            {
                return Vec::new();
            }
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
