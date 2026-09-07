use super::*;

pub(super) fn fields_for<'a>(
    loaded: &'a PrivatePerkRuntimeGraph,
    locator: &WeaponRuntimeFieldLocator,
) -> Vec<&'a WeaponRuntimeField> {
    loaded
        .graphs
        .iter()
        .flat_map(|(_, graph)| graph.fields())
        .filter(|field| guided::equivalent(loaded, &field.locator, locator))
        .collect()
}

fn finite(value: &WeaponRuntimeValue) -> bool {
    match value {
        WeaponRuntimeValue::Float32Bits(bits) => f32::from_bits(*bits).is_finite(),
        WeaponRuntimeValue::Vector4Float32Bits(bits) => {
            bits.iter().all(|bits| f32::from_bits(*bits).is_finite())
        }
        _ => true,
    }
}

impl PerkEditor {
    pub(super) fn validation_errors(&self) -> Vec<String> {
        let Some(loaded) = &self.graph else {
            return vec!["Wait for the perk data to load.".into()];
        };
        let mut errors = Vec::new();
        if let Some(error) = &self.parameter_error {
            errors.push(error.clone());
        }
        if !loaded.warnings.is_empty() && !self.draft.is_empty() {
            errors.push("Some perk graphs could not be decoded. Runtime edits cannot be applied until the complete graph can be checked.".into());
        }
        for (index, value) in self.draft.iter().enumerate() {
            let fields = fields_for(loaded, &value.locator);
            let valid = fields.len() == 1
                && runtime_field_is_editable(fields[0])
                && encode_weapon_runtime_value(&fields[0].kind, &value.value).is_ok()
                && finite(&value.value);
            if !valid {
                errors.push(format!("Saved field {} is missing, ambiguous, non-finite, or has the wrong type. Reset or remove it.", index + 1));
            }
            if self.draft[..index]
                .iter()
                .any(|other| guided::equivalent(loaded, &other.locator, &value.locator))
            {
                errors.push(format!(
                    "Saved field {} edits the same parameter twice.",
                    index + 1
                ));
            }
        }
        for ((locator, _), text) in &self.value_text {
            let fields = fields_for(loaded, locator);
            if fields.len() != 1 {
                continue;
            }
            let valid = match fields[0].kind {
                WeaponRuntimeValueKind::FixedBytes { size } => {
                    parse_runtime_hex_bytes(text, size as usize).is_some()
                }
                WeaponRuntimeValueKind::Float32 | WeaponRuntimeValueKind::Vector4Float32 => {
                    parse_runtime_hex_u64(text)
                        .and_then(|bits| u32::try_from(bits).ok())
                        .is_some_and(|bits| f32::from_bits(bits).is_finite())
                }
                WeaponRuntimeValueKind::HexIdentifier { .. } => parse_runtime_hex_u64(text)
                    .is_some_and(|value| {
                        value <= fields[0].kind.unsigned_maximum().unwrap_or(u64::MAX)
                    }),
                _ => true,
            };
            if !valid {
                errors.push("An unfinished field entry is invalid. Correct it or reset the field before applying.".into());
            }
        }
        let mut offsets = BTreeSet::new();
        for (index, value) in self.action_draft.iter().enumerate() {
            let valid = actions::source_bits(&loaded.action_payload, value).ok()
                == Some(value.expected_bits)
                && f32::from_bits(value.value_bits).is_finite()
                && f32::from_bits(value.expected_bits).is_finite()
                && value.value_bits != value.expected_bits;
            if !valid {
                errors.push(format!(
                    "Action value {} is stale or invalid. Reset it or enter a finite value.",
                    index + 1
                ));
            }
            if let Ok(offset) = actions::action_offset(&loaded.action_payload, value)
                && !offsets.insert(offset)
            {
                errors.push("Two action edits target the same scalar.".into());
            }
        }
        for parameter in guided::ProjectileSpeed::discover_all(loaded) {
            if let Err(error) = parameter.value(loaded, &self.draft) {
                errors.push(error);
            }
        }
        errors.sort();
        errors.dedup();
        errors
    }
}
