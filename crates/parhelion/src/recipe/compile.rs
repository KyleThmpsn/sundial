//! Checked conversion from saved recipe fields to native compiler inputs.

use super::*;

impl From<RecipeAmmoType> for WeaponAmmoType {
    fn from(value: RecipeAmmoType) -> Self {
        match value {
            RecipeAmmoType::Primary => Self::Primary,
            RecipeAmmoType::Special => Self::Special,
            RecipeAmmoType::Heavy => Self::Heavy,
        }
    }
}

impl From<RecipeRarity> for AuthoredWeaponRarity {
    fn from(value: RecipeRarity) -> Self {
        match value {
            RecipeRarity::Common => Self::Common,
            RecipeRarity::Uncommon => Self::Uncommon,
            RecipeRarity::Rare => Self::Rare,
            RecipeRarity::Legendary => Self::Legendary,
            RecipeRarity::Exotic => Self::Exotic,
        }
    }
}

impl From<RecipeInventorySlot> for WeaponInventorySlot {
    fn from(value: RecipeInventorySlot) -> Self {
        match value {
            RecipeInventorySlot::Kinetic => Self::Kinetic,
            RecipeInventorySlot::Energy => Self::Energy,
            RecipeInventorySlot::Power => Self::Power,
        }
    }
}

impl From<RecipeBehaviorFiring> for crate::weapon::behavior::BehaviorFiring {
    fn from(value: RecipeBehaviorFiring) -> Self {
        match value {
            RecipeBehaviorFiring::Behavior => Self::Behavior,
            RecipeBehaviorFiring::Weapon => Self::Weapon,
        }
    }
}

impl From<RecipeDamageType> for ModernDamageType {
    fn from(value: RecipeDamageType) -> Self {
        match value {
            RecipeDamageType::Kinetic => Self::Kinetic,
            RecipeDamageType::Arc => Self::Arc,
            RecipeDamageType::Solar => Self::Solar,
            RecipeDamageType::Void => Self::Void,
        }
    }
}

impl From<ModernDamageType> for RecipeDamageType {
    fn from(value: ModernDamageType) -> Self {
        match value {
            ModernDamageType::Kinetic => Self::Kinetic,
            ModernDamageType::Arc => Self::Arc,
            ModernDamageType::Solar => Self::Solar,
            ModernDamageType::Void => Self::Void,
        }
    }
}

impl From<WeaponCloneIdentity> for WeaponIdentity {
    fn from(value: WeaponCloneIdentity) -> Self {
        Self {
            item_hash: value.item_hash.into(),
            collectible_hash: value.collectible_hash.into(),
            unlock_hash: value.unlock_hash.into(),
            pattern_global_id_hash: Some(value.pattern_global_id_hash.into()),
            name_hash: value.name_hash.into(),
            type_hash: Some(value.type_hash.into()),
            flavor_hash: value.flavor_hash.into(),
            source_hash: value.source_hash.into(),
            collection_name_hash: Some(value.collection_name_hash.into()),
            collection_description_hash: Some(value.collection_description_hash.into()),
            inventory_hint_hash: Some(value.inventory_hint_hash.into()),
            collection_requirement_hash: Some(value.collection_requirement_hash.into()),
        }
    }
}

impl WeaponIdentity {
    pub(super) fn to_compiler(&self, namespace: &str) -> Result<WeaponCloneIdentity, RecipeError> {
        let derived = WeaponCloneIdentity::from_namespace(namespace)?;
        Ok(WeaponCloneIdentity {
            item_hash: parse_recipe_hash("identity.item_hash", &self.item_hash)?,
            collectible_hash: parse_recipe_hash(
                "identity.collectible_hash",
                &self.collectible_hash,
            )?,
            unlock_hash: parse_recipe_hash("identity.unlock_hash", &self.unlock_hash)?,
            pattern_global_id_hash: self
                .pattern_global_id_hash
                .as_ref()
                .map_or(Ok(derived.pattern_global_id_hash), |hash| {
                    parse_recipe_hash("identity.pattern_global_id_hash", hash)
                })?,
            name_hash: parse_recipe_hash("identity.name_hash", &self.name_hash)?,
            type_hash: self
                .type_hash
                .as_ref()
                .map_or(Ok(derived.type_hash), |hash| {
                    parse_recipe_hash("identity.type_hash", hash)
                })?,
            flavor_hash: parse_recipe_hash("identity.flavor_hash", &self.flavor_hash)?,
            source_hash: parse_recipe_hash("identity.source_hash", &self.source_hash)?,
            collection_name_hash: self
                .collection_name_hash
                .as_ref()
                .map_or(Ok(derived.collection_name_hash), |hash| {
                    parse_recipe_hash("identity.collection_name_hash", hash)
                })?,
            collection_description_hash: self
                .collection_description_hash
                .as_ref()
                .map_or(Ok(derived.collection_description_hash), |hash| {
                    parse_recipe_hash("identity.collection_description_hash", hash)
                })?,
            inventory_hint_hash: self
                .inventory_hint_hash
                .as_ref()
                .map_or(Ok(derived.inventory_hint_hash), |hash| {
                    parse_recipe_hash("identity.inventory_hint_hash", hash)
                })?,
            collection_requirement_hash: self
                .collection_requirement_hash
                .as_ref()
                .map_or(Ok(derived.collection_requirement_hash), |hash| {
                    parse_recipe_hash("identity.collection_requirement_hash", hash)
                })?,
        })
    }

    pub(crate) fn parsed_hashes(&self, namespace: &str) -> Result<[u32; 12], RecipeError> {
        let derived = WeaponCloneIdentity::from_namespace(namespace)?;
        Ok([
            parse_recipe_hash("identity.item_hash", &self.item_hash)?,
            parse_recipe_hash("identity.collectible_hash", &self.collectible_hash)?,
            parse_recipe_hash("identity.unlock_hash", &self.unlock_hash)?,
            self.pattern_global_id_hash
                .as_ref()
                .map_or(Ok(derived.pattern_global_id_hash), |hash| {
                    parse_recipe_hash("identity.pattern_global_id_hash", hash)
                })?,
            parse_recipe_hash("identity.name_hash", &self.name_hash)?,
            self.type_hash
                .as_ref()
                .map_or(Ok(derived.type_hash), |hash| {
                    parse_recipe_hash("identity.type_hash", hash)
                })?,
            parse_recipe_hash("identity.flavor_hash", &self.flavor_hash)?,
            parse_recipe_hash("identity.source_hash", &self.source_hash)?,
            self.collection_name_hash
                .as_ref()
                .map_or(Ok(derived.collection_name_hash), |hash| {
                    parse_recipe_hash("identity.collection_name_hash", hash)
                })?,
            self.collection_description_hash
                .as_ref()
                .map_or(Ok(derived.collection_description_hash), |hash| {
                    parse_recipe_hash("identity.collection_description_hash", hash)
                })?,
            self.inventory_hint_hash
                .as_ref()
                .map_or(Ok(derived.inventory_hint_hash), |hash| {
                    parse_recipe_hash("identity.inventory_hint_hash", hash)
                })?,
            self.collection_requirement_hash
                .as_ref()
                .map_or(Ok(derived.collection_requirement_hash), |hash| {
                    parse_recipe_hash("identity.collection_requirement_hash", hash)
                })?,
        ])
    }
}

impl WeaponLocaleTextRecipe {
    pub(super) fn to_compiler(&self) -> WeaponLocaleTextOverride {
        WeaponLocaleTextOverride {
            locale_index: self.locale_index,
            name: self.name.clone(),
            type_name: self.type_name.clone(),
            flavor: self.flavor.clone(),
            source: self.source.clone(),
            collection_name: self.collection_name.clone(),
            collection_description: self.collection_description.clone(),
            inventory_hint: self.inventory_hint.clone(),
            collection_requirement: self.collection_requirement.clone(),
        }
    }
}

impl WeaponRecipeOverrides {
    pub(crate) fn to_compiler(&self) -> Result<WeaponCloneOverrides, RecipeError> {
        let socket_columns = self
            .socket_columns
            .iter()
            .enumerate()
            .map(|(socket_index, column)| {
                column
                    .as_ref()
                    .map(|column| {
                        if (column.choices.is_empty() && column.socket_type != Some(u16::MAX))
                            || column.choices.len() > MAX_AUTHORED_EMBEDDED_SOCKET_CHOICES
                        {
                            return Err(RecipeError::Validation(format!(
                                "Socket {socket_index} must contain between 1 and {MAX_AUTHORED_EMBEDDED_SOCKET_CHOICES} ordered choices"
                            )));
                        }
                        let choices = column
                            .choices
                            .iter()
                            .enumerate()
                            .map(|(choice_index, hash)| {
                                parse_recipe_hash(
                                    &format!("socket {socket_index} choice {choice_index}"),
                                    hash,
                                )
                            })
                            .collect::<Result<Vec<_>, _>>()?;
                        if choices.contains(&0) {
                            return Err(RecipeError::Validation(format!(
                                "Socket {socket_index} cannot contain plug hash zero"
                            )));
                        }
                        // The compiler validates duplicates using each choice's private
                        // definition. Different custom perks may share a stock template.
                        Ok(WeaponSocketColumnOverride {
                            choices,
                            socket_type: column.socket_type,
                            choice_weight_bits: column.choice_weight_bits.clone(),
                            choice_conditions: column
                                .choice_conditions
                                .iter()
                                .map(|program| {
                                    program
                                        .iter()
                                        .map(|instruction| WeaponNumericInstruction {
                                            opcode: instruction.opcode,
                                            operand: instruction.operand,
                                        })
                                        .collect()
                                })
                                .collect(),
                            reusable_plug_set_index: column.reusable_plug_set_index,
                            randomized_plug_set_index: column.randomized_plug_set_index,
                            randomized_selection_program: column
                                .randomized_selection_program
                                .iter()
                                .map(|instruction| WeaponNumericInstruction {
                                    opcode: instruction.opcode,
                                    operand: instruction.operand,
                                })
                                .collect(),
                        })
                    })
                    .transpose()
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut investment_stats = self
            .investment_stats
            .iter()
            .map(|stat| (stat.definition_index, stat.value))
            .collect::<Vec<_>>();
        investment_stats.sort_unstable_by_key(|(definition_index, _)| *definition_index);
        let mut removed_investment_stats = self.removed_investment_stats.clone();
        removed_investment_stats.sort_unstable();
        if let Some(hash) = &self.weapon_pattern_donor_hash {
            parse_recipe_hash("weapon-pattern donor", hash)?;
        }
        if let Some(hash) = &self.stat_group_donor_hash {
            parse_recipe_hash("stat-group donor", hash)?;
        }
        if let Some(group) = &self.custom_stat_group {
            if self.stat_group_index.is_some() {
                return Err(RecipeError::Validation(
                    "Choose either a stock stat group or a custom one".into(),
                ));
            }
            group.validate().map_err(RecipeError::Validation)?;
        }
        if self.remove_lore && self.lore.is_some() {
            return Err(RecipeError::Validation(
                "Choose either source lore text or no lore tab".into(),
            ));
        }
        Ok(WeaponCloneOverrides {
            remove_lore: self.remove_lore,
            #[cfg(feature = "d2-model-importer")]
            imported_graph: self.imported_graph.clone(),
            icon_edit: self.icon_edit.clone(),
            hud_icon: self.hud_icon.clone(),
            badge: self.badge.clone(),
            exclude_from_sunrise_badge: self.exclude_from_sunrise_badge,
            collection_destination: self.collection_destination,
            corner_icon: self.corner_icon.clone(),
            lore: self.lore.clone(),
            investment_stats,
            removed_investment_stats,
            base_sandbox_perks: self.base_sandbox_perks.clone(),
            trait_indices: self.trait_indices.clone(),
            max_stack_size: self.max_stack_size,
            socket_entry_list_index: self.socket_entry_list_index,
            plug_category_hash: self
                .plug_category_hash
                .as_ref()
                .map(|hash| parse_recipe_hash("plug category", hash))
                .transpose()?,
            roll_set_index: self.roll_set_index,
            linked_plug_index: self.linked_plug_index,
            inventory_slot: self.inventory_slot.map(WeaponInventorySlot::from),
            ammo_type: self.ammo_type.map(WeaponAmmoType::from),
            modern_damage_type: self.modern_damage_type.map(ModernDamageType::from),
            variable_damage: self
                .variable_damage
                .as_ref()
                .map(VariableDamageRecipe::to_compiler),
            additional_behaviors: self
                .additional_behaviors
                .iter()
                .map(|entry| entry.behavior.clone())
                .collect(),
            skip_behavior_perks: self.skip_behavior_perks,
            behavior_firing: self.behavior_firing.map(Into::into).unwrap_or_default(),
            behavior_projectile_speed: self.behavior_projectile_speed_bits.map(f32::from_bits),
            fired_graph: self.fired_graph.clone(),
            projectile: self
                .projectile
                .clone()
                .filter(|projectile| !projectile.values.is_empty()),
            barrel: self.barrel.clone().filter(|barrel| !barrel.is_empty()),
            animation_donor: self
                .animation_donor
                .as_ref()
                .map(|donor| parse_recipe_hash("animation donor", &donor.item_hash))
                .transpose()?,
            animation_actions: self
                .animation_actions
                .iter()
                .map(|(action, donor)| {
                    Ok((
                        *action,
                        parse_recipe_hash("animation action donor", &donor.item_hash)?,
                    ))
                })
                .collect::<Result<Vec<_>, RecipeError>>()?,
            type_marker_donor: self
                .type_marker_donor
                .as_ref()
                .map(|donor| parse_recipe_hash("type marker donor", &donor.item_hash))
                .transpose()?,
            marker_offsets: self
                .marker_offsets
                .iter()
                .map(|offset| {
                    if offset
                        .offset_um
                        .iter()
                        .any(|um| um.abs() > MARKER_OFFSET_LIMIT_UM)
                    {
                        return Err(RecipeError::Validation(
                            "A marker can move at most 50 cm".into(),
                        ));
                    }
                    Ok((
                        parse_recipe_hash("marker", &offset.marker)?,
                        offset.offset_um.map(|um| um as f32 / 1_000_000.0),
                    ))
                })
                .collect::<Result<Vec<_>, RecipeError>>()?,
            held_offset: if self
                .held_offset_um
                .iter()
                .any(|um| um.abs() > HELD_OFFSET_LIMIT_UM)
            {
                return Err(RecipeError::Validation(
                    "The weapon can move at most 20 cm in the hand".into(),
                ));
            } else {
                (!is_zero_offset(&self.held_offset_um))
                    .then(|| self.held_offset_um.map(|um| um as f32 / 1_000_000.0))
            },
            component_splices: self
                .component_splices
                .iter()
                .map(|splice| {
                    Ok((
                        parse_recipe_hash("component binding", &splice.binding_hash)?,
                        parse_recipe_hash("component donor", &splice.donor.item_hash)?,
                    ))
                })
                .collect::<Result<Vec<_>, RecipeError>>()?,
            power_cap_group: self.power_cap_group,
            power_cap_groups: self.power_cap_groups.clone(),
            rarity: self.rarity.map(AuthoredWeaponRarity::from),
            weapon_pattern_index: self.weapon_pattern_index,
            stat_group_index: self.stat_group_index,
            custom_stat_group: self.custom_stat_group.clone(),
            art_arrangements: self.art_arrangements.as_ref().map(|rows| {
                rows.iter()
                    .map(|row| WeaponArtArrangementOverride {
                        character_class: row.character_class,
                        arrangement: row.arrangement,
                    })
                    .collect()
            }),
            render_dye_rows: self.render_dye_rows.as_ref().map(|arrays| {
                std::array::from_fn(|array| {
                    arrays[array]
                        .iter()
                        .map(|row| WeaponDyeReferenceOverride {
                            channel_index: row.channel_index,
                            dye_reference_index: row.dye_reference_index,
                        })
                        .collect()
                })
            }),
            subclass_abilities: self
                .subclass_abilities
                .as_ref()
                .map(|abilities| {
                    abilities
                        .validate()
                        .map(|()| abilities.clone())
                        .map_err(RecipeError::Validation)
                })
                .transpose()?,
            subclass_every_class: self.subclass_every_class,
            subclass_class: self.subclass_class,
            subclass_damage_type: self.subclass_damage_type,
            armor_class: self.armor_class,
            sparrow: self.sparrow.clone(),
            shader_glow: self.shader_glow,
            base_type: self.base_type,
            dye_edits: {
                crate::dye::validate_edits(&self.dye_edits).map_err(RecipeError::Validation)?;
                self.dye_edits.clone()
            },
            dye_texture_edits: {
                crate::dye::validate_texture_edits(&self.dye_texture_edits)
                    .map_err(RecipeError::Validation)?;
                self.dye_texture_edits.clone()
            },
            stat_trackers: self
                .stat_trackers
                .as_ref()
                .map(|trackers| {
                    trackers
                        .validate()
                        .map(|()| trackers.clone())
                        .map_err(RecipeError::Validation)
                })
                .transpose()?,
            nameplate: self
                .nameplate
                .as_ref()
                .filter(|nameplate| !nameplate.is_empty())
                .map(|nameplate| {
                    nameplate
                        .validate()
                        .map(|()| nameplate.clone())
                        .map_err(RecipeError::Validation)
                })
                .transpose()?,
            screen_art: self
                .screen_art
                .as_ref()
                .filter(|art| !art.is_empty())
                .map(|art| {
                    art.validate()
                        .map(|()| art.clone())
                        .map_err(RecipeError::Validation)
                })
                .transpose()?,
            subclass_icon: self
                .subclass_icon
                .as_ref()
                .map(|icon| {
                    icon.validate()
                        .map(|()| icon.clone())
                        .map_err(RecipeError::Validation)
                })
                .transpose()?,
            socket_columns,
            socket_plug_variants: self
                .socket_plug_variants
                .iter()
                .enumerate()
                .map(|(index, variant)| variant.to_compiler(index))
                .collect::<Result<Vec<_>, RecipeError>>()?,
            runtime_values: self.runtime_values.clone(),
            sword_profile: self
                .sword_profile
                .as_ref()
                .map(|profile| {
                    Ok::<_, RecipeError>(SwordProfileOverride {
                        key: parse_recipe_hash("sword profile key", &profile.key)?,
                        near_scale_bits: profile.near_scale_bits,
                        far_scale_bits: profile.far_scale_bits,
                    })
                })
                .transpose()?,
            runtime_resource_patches: self
                .runtime_resource_patches
                .iter()
                .enumerate()
                .map(|(index, patch)| {
                    Ok(crate::WeaponRuntimeResourcePatch {
                        binding_hash: parse_recipe_hash(
                            &format!("runtime resource patch {index} binding"),
                            &patch.binding_hash,
                        )?,
                        resource_index: patch.resource_index,
                        offset: patch.offset,
                        bytes: parse_raw_patch_bytes(&patch.bytes, index)?,
                        graph_values: patch.graph_values.clone(),
                        graph_removals: Vec::new(),
                        graph_trajectories: None,
                    })
                })
                .collect::<Result<Vec<_>, RecipeError>>()?,
            raw_payload_patches: self
                .raw_payload_patches
                .iter()
                .enumerate()
                .map(|(index, patch)| {
                    Ok(WeaponRawPayloadPatch {
                        target: patch.target,
                        offset: patch.offset,
                        bytes: parse_raw_patch_bytes(&patch.bytes, index)?,
                    })
                })
                .collect::<Result<Vec<_>, RecipeError>>()?,
        })
    }
}

pub(super) fn parse_raw_patch_bytes(value: &str, index: usize) -> Result<Vec<u8>, RecipeError> {
    let compact = value
        .chars()
        .filter(|character| !character.is_ascii_whitespace() && *character != '_')
        .collect::<String>();
    let compact = compact
        .strip_prefix("0x")
        .or_else(|| compact.strip_prefix("0X"))
        .unwrap_or(&compact);
    if compact.is_empty()
        || compact.len() % 2 != 0
        || !compact.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(RecipeError::Validation(format!(
            "Raw payload patch {index} bytes must contain a non-empty even number of hexadecimal digits"
        )));
    }
    compact
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            std::str::from_utf8(pair)
                .ok()
                .and_then(|pair| u8::from_str_radix(pair, 16).ok())
                .ok_or_else(|| {
                    RecipeError::Validation(format!(
                        "Raw payload patch {index} contains malformed hexadecimal bytes"
                    ))
                })
        })
        .collect()
}

pub(super) fn parse_recipe_hash(description: &str, hash: &HexHash) -> Result<u32, RecipeError> {
    hash.parse_u32()
        .map_err(|error| RecipeError::Validation(format!("Invalid {description}: {error}")))
}

impl VariableDamageRecipe {
    pub(super) fn to_compiler(&self) -> WeaponVariableDamage {
        WeaponVariableDamage {
            elements: self
                .elements
                .iter()
                .copied()
                .map(ModernDamageType::from)
                .collect(),
        }
    }
}

impl WeaponRecipe {
    pub fn validate(&self) -> Result<(), RecipeError> {
        self.to_spec().map(|_| ())
    }
}

impl WeaponRecipe {
    pub fn to_spec(&self) -> Result<WeaponCloneSpec, RecipeError> {
        if self.schema != RECIPE_SCHEMA {
            return Err(RecipeError::Validation(format!(
                "Unsupported recipe schema {}; expected {RECIPE_SCHEMA}",
                self.schema
            )));
        }
        if self.overrides.subclass_every_class && self.kind != ItemKind::Subclass {
            return Err(RecipeError::Validation(
                "Only a subclass can be given to every class".to_owned(),
            ));
        }
        if self.overrides.subclass_class.is_some() && self.kind != ItemKind::Subclass {
            return Err(RecipeError::Validation(
                "Only a subclass can be given another class".to_owned(),
            ));
        }
        if self.overrides.subclass_damage_type.is_some() && self.kind != ItemKind::Subclass {
            return Err(RecipeError::Validation(
                "Only a subclass can show another damage type".to_owned(),
            ));
        }
        if self.overrides.armor_class.is_some() && self.kind != ItemKind::Armor {
            return Err(RecipeError::Validation(
                "Only armor can select an armor class".into(),
            ));
        }
        if self.overrides.nameplate.is_some() && self.kind != ItemKind::Emblem {
            return Err(RecipeError::Validation(
                "Only an emblem has a nameplate".to_owned(),
            ));
        }
        if self.overrides.screen_art.is_some() && self.kind != ItemKind::Subclass {
            return Err(RecipeError::Validation(
                "Only a subclass has screen art".to_owned(),
            ));
        }
        if self.overrides.subclass_icon.is_some() && self.kind != ItemKind::Subclass {
            return Err(RecipeError::Validation(
                "Only a subclass has a generated icon".to_owned(),
            ));
        }
        if self.overrides.stat_trackers.is_some() && self.kind != ItemKind::Emblem {
            return Err(RecipeError::Validation(
                "Only an emblem can select stat tracker categories".into(),
            ));
        }
        let spec = WeaponCloneSpec {
            kind: self.kind,
            namespace: self.namespace.clone(),
            donor_item_hash: parse_recipe_hash("donor.item_hash", &self.donor.item_hash)?,
            expected_donor_name: self.donor.expected_name.clone(),
            presentation_donor: self
                .presentation_donor
                .as_ref()
                .map(|donor| {
                    Ok::<_, RecipeError>(crate::WeaponPresentationDonorReference {
                        item_hash: parse_recipe_hash(
                            "presentation_donor.item_hash",
                            &donor.item_hash,
                        )?,
                        expected_name: donor.expected_name.clone(),
                    })
                })
                .transpose()?,
            render_gear_donor: self
                .render_gear_donor
                .as_ref()
                .map(|donor| {
                    Ok::<_, RecipeError>(WeaponRenderGearDonorReference {
                        item_hash: parse_recipe_hash(
                            "render_gear_donor.item_hash",
                            &donor.item_hash,
                        )?,
                        expected_name: donor.expected_name.clone(),
                    })
                })
                .transpose()?,
            icon_donor: self
                .icon_donor
                .as_ref()
                .map(|donor| {
                    Ok::<_, RecipeError>(crate::WeaponIconDonorReference {
                        item_hash: parse_recipe_hash("icon_donor.item_hash", &donor.item_hash)?,
                        expected_name: donor.expected_name.clone(),
                    })
                })
                .transpose()?,
            runtime_component_donors: self
                .runtime_component_donors
                .iter()
                .map(|component| {
                    Ok::<_, RecipeError>(crate::WeaponRuntimeComponentDonorReference {
                        binding_hash: parse_recipe_hash(
                            "runtime_component_donors.binding_hash",
                            &component.binding_hash,
                        )?,
                        item_hash: parse_recipe_hash(
                            "runtime_component_donors.donor.item_hash",
                            &component.donor.item_hash,
                        )?,
                        expected_name: component.donor.expected_name.clone(),
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
            identity: self.identity.to_compiler(&self.namespace)?,
            text: WeaponCloneText {
                name: self.name.clone(),
                type_name: self.type_name.clone(),
                flavor: self.flavor.clone(),
                source: self.source.clone(),
                collection_name: self.collection_name.clone(),
                collection_description: self.collection_description.clone(),
                inventory_hint: self.inventory_hint.clone(),
                collection_requirement: self.collection_requirement.clone(),
                locale_overrides: self
                    .locale_overrides
                    .iter()
                    .map(WeaponLocaleTextRecipe::to_compiler)
                    .collect(),
            },
            overrides: self.overrides.to_compiler()?,
        };
        spec.validate()?;
        Ok(spec)
    }
}
