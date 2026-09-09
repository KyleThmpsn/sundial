use super::*;

mod custom_plugs;
mod metadata;
mod preparation;
pub(super) use preparation::{METADATA_LAYOUT, SANDBOX_PATTERN_LAYOUT};

pub(super) struct WeaponTables {
    pub item_hash_index: Vec<u8>,
    pub item_table: Vec<u8>,
    pub item_strings: Vec<u8>,
    pub item_metadata: Vec<u8>,
    pub item_metadata_index: Vec<u8>,
    pub sandbox_patterns: Vec<u8>,
    pub sandbox_pattern_index: Vec<u8>,
    pub dense: Vec<u8>,
    pub collectibles: Vec<u8>,
    pub collectible_displays: Vec<u8>,
    pub unlocks: Vec<u8>,
    pub unlock_banks: Vec<u8>,
    pub unlock_displays: Vec<u8>,
    pub definitions: Vec<NewTagSpec>,
    pub authored_strings: Vec<NewTagSpec>,
    pub plans: Vec<NewWeaponPlan>,
    pub project_rows: Vec<ProjectAuthoredRow>,
    pub any_sandbox_pattern: bool,
}

pub(super) struct WeaponBuildContext<'a> {
    pub stock_item_count: usize,
    pub stock_collectible_count: usize,
    pub authored_weapon_icon_indices: &'a [u16],
    pub authored_weapon_icon_containers: &'a [TagHash],
    pub authored_item_icons: &'a [u8],
    pub sandbox_perk_definition_template: &'a [u8; ITEM_SANDBOX_PERK_ROW_SIZE],
    pub sandbox_perk_string_template: &'a [u8],
    pub custom_plugs: &'a [ResolvedCustomPlug],
    pub sandbox_pattern_layout: KeyedAuxiliaryLayout,
    pub authored_pattern_global_ids: &'a [Option<u32>],
}

impl WeaponTables {
    pub(super) fn author_weapon(
        &mut self,
        context: &WeaponBuildContext<'_>,
        ordinal: usize,
        donor: &resolve::ResolvedWeapon,
    ) -> AuthoringResult<()> {
        let identity = donor.weapon.identity;
        let authored_icon_index = *context
            .authored_weapon_icon_indices
            .get(ordinal)
            .ok_or_else(|| validation("Authored weapon icon index is missing"))?;
        let authored_icon_container = *context
            .authored_weapon_icon_containers
            .get(ordinal)
            .ok_or_else(|| validation("Authored weapon icon container is missing"))?;
        let definition_tag = TagHash::new(
            HOST_PACKAGE_ID,
            u16::try_from(HOST_EXPECTED_ENTRY_COUNT + ordinal * 2)
                .map_err(|_| invalid("Definition tag index does not fit 16 bits"))?,
        );
        let string_tag = TagHash::new(
            HOST_PACKAGE_ID,
            u16::try_from(HOST_EXPECTED_ENTRY_COUNT + ordinal * 2 + 1)
                .map_err(|_| invalid("String tag index does not fit 16 bits"))?,
        );
        let item_index = u16::try_from(context.stock_item_count + ordinal)
            .map_err(|_| invalid("Authored item index does not fit 16 bits"))?;
        let collectible_index = u16::try_from(context.stock_collectible_count + ordinal)
            .map_err(|_| invalid("Authored collectible index does not fit 16 bits"))?;
        let (current_unlock_count, _, current_unlock_rows, _) = array_at(&self.unlocks, 8)?;
        let unlock_definition_index = u16::try_from(current_unlock_count)
            .map_err(|_| invalid("Authored unlock index does not fit 16 bits"))?;
        let unlock_slot = first_free_unlock_slot(
            &self.unlocks,
            current_unlock_rows,
            current_unlock_count,
            ACCOUNT_UNLOCK_BANK,
            SHADOWKEEP_ACCOUNT_FLAG_REGION_CAPACITY,
        )?;

        let mut definition = donor.definition.clone();
        let mut strings = donor.strings.clone();
        let donor_inventory_slot = weapon_inventory_slot(&definition)?;
        let authored_pattern_index = donor
            .gear_art_pattern_source
            .as_ref()
            .map(|source| {
                let current_source =
                    sandbox_pattern_source_at(&self.sandbox_patterns, source.row_index)?;
                if current_source != *source {
                    return Err(validation(
                        "Weapon gear-art/runtime row source moved while compiling the project",
                    ));
                }
                u16::try_from(
                    validate_keyed_auxiliary_alignment(
                        &self.sandbox_patterns,
                        &self.sandbox_pattern_index,
                        context.sandbox_pattern_layout,
                    )?
                    .count,
                )
                .map_err(|_| invalid("Authored weapon-pattern index does not fit 16 bits"))
            })
            .transpose()?;
        write_u32(
            &mut definition,
            ITEM_DEFINITION_HASH_OFFSET,
            identity.item_hash,
        )?;
        if let Some(index) = authored_pattern_index {
            set_weapon_pattern_index(&mut definition, index)?;
        }
        apply_weapon_slot_and_damage_overrides(
            &mut definition,
            &mut strings,
            &donor.weapon.overrides,
            donor.damage_carrier_source.as_ref(),
            context.sandbox_perk_definition_template,
            context.sandbox_perk_string_template,
        )?;
        definition::apply_scalar_overrides(&mut definition, &donor.weapon.overrides)?;
        let expected_socket_columns = definition::apply_socket_overrides(
            &mut definition,
            donor,
            context.custom_plugs,
            ordinal,
        )?;
        let collection_material_set = definition::apply_presentation(
            &mut definition,
            &mut strings,
            donor,
            donor_inventory_slot,
            authored_icon_index,
        )?;
        validate_authored_payloads(
            &definition,
            &strings,
            (!expected_socket_columns.is_empty()).then_some(expected_socket_columns.as_slice()),
            authored_pattern_index,
            &donor.weapon,
        )?;
        validate_weapon_raw_payload_patches(
            &definition,
            &strings,
            &donor.weapon.overrides.raw_payload_patches,
        )?;

        validate_authored_item_icon(
            context.authored_item_icons,
            &strings,
            identity.item_hash,
            authored_icon_index,
            authored_icon_container,
        )?;

        let row = WeaponRow {
            current_unlock_count,
            identity,
            definition_tag,
            string_tag,
            item_index,
            collectible_index,
            unlock_definition_index,
            unlock_slot,
            authored_icon_index,
            authored_icon_container,
            authored_pattern_index,
            collection_material_set,
        };
        self.append_presentation(donor, &row, context, ordinal)?;
        self.append_pattern(donor, &row, context, ordinal)?;
        self.append_collection(donor, &row)?;
        self.definitions.push(NewTagSpec {
            template_tag: donor.definition_tag,
            payload: definition,
            storage: crate::NewTagStorageMode::InheritTemplate,
        });
        self.authored_strings.push(NewTagSpec {
            template_tag: donor.string_tag,
            payload: strings,
            storage: crate::NewTagStorageMode::InheritTemplate,
        });
        self.plans.push(NewWeaponPlan {
            item_hash: identity.item_hash,
            definition_tag,
            string_tag,
            icon_definition_tag: authored_icon_container,
            item_index,
            collectible_hash: identity.collectible_hash,
            collectible_index,
            unlock_hash: identity.unlock_hash,
            unlock_definition_index,
            unlock_bank: ACCOUNT_UNLOCK_BANK,
            unlock_slot,
            template_item_hash: donor.weapon.donor_item_hash,
            template_definition_tag: donor.definition_tag,
            template_string_tag: donor.string_tag,
        });
        self.project_rows.push(ProjectAuthoredRow {
            donor_collectible_index: donor.collection_donor_index,
            authored_collectible_index: usize::from(collectible_index),
            weapon_page: donor.weapon_page,
            source_acquired_flag: donor.source_acquired_flag,
            authored_unlock_index: unlock_definition_index,
            count_selection: donor.count_selection.clone(),
        });
        Ok(())
    }

    fn append_presentation(
        &mut self,
        donor: &resolve::ResolvedWeapon,
        row: &WeaponRow,
        context: &WeaponBuildContext<'_>,
        ordinal: usize,
    ) -> AuthoringResult<()> {
        let WeaponRow {
            identity,
            authored_icon_container,
            ..
        } = *row;
        self.dense = append_dense_item_presentation(
            std::mem::take(&mut self.dense),
            donor.icon_template_item_index,
            context.stock_item_count + ordinal,
            donor.donor_icon_container,
            authored_icon_container,
        )?;
        let dense_arrays = dense_item_presentation_arrays(&self.dense)?;
        apply_array_row_raw_payload_patches(
            &mut self.dense,
            ITEM_DENSE_ICON_TAG_DESCRIPTOR,
            dense_arrays[0].count - 1,
            ITEM_DENSE_ICON_TAG_ROW_SIZE,
            ITEM_DENSE_ICON_TAG_ROW_CLASS,
            WeaponRawPayloadTarget::DenseIconTagRow,
            &donor.weapon.overrides.raw_payload_patches,
        )?;
        apply_array_row_raw_payload_patches(
            &mut self.dense,
            ITEM_DENSE_ICON_SELECTOR_DESCRIPTOR,
            dense_arrays[2].count - 1,
            ITEM_DENSE_ICON_SELECTOR_ROW_SIZE,
            ITEM_DENSE_ICON_SELECTOR_ROW_CLASS,
            WeaponRawPayloadTarget::DenseIconSelectorRow,
            &donor.weapon.overrides.raw_payload_patches,
        )?;
        apply_array_row_raw_payload_patches(
            &mut self.dense,
            ITEM_DENSE_PRESENTATION_DESCRIPTOR,
            dense_arrays[3].count - 1,
            ITEM_DENSE_PRESENTATION_ROW_SIZE,
            ITEM_DENSE_PRESENTATION_ROW_CLASS,
            WeaponRawPayloadTarget::DensePresentationRow,
            &donor.weapon.overrides.raw_payload_patches,
        )?;
        validate_dense_item_presentation(
            &self.dense,
            donor.icon_template_item_index,
            context.stock_item_count + ordinal + 1,
            donor.donor_icon_container,
            authored_icon_container,
        )?;
        metadata::append(
            &mut self.item_metadata,
            &mut self.item_metadata_index,
            donor.weapon.donor_item_hash,
            identity.item_hash,
            &donor.weapon.overrides.raw_payload_patches,
        )?;
        Ok(())
    }

    fn append_pattern(
        &mut self,
        donor: &resolve::ResolvedWeapon,
        row: &WeaponRow,
        context: &WeaponBuildContext<'_>,
        ordinal: usize,
    ) -> AuthoringResult<()> {
        let WeaponRow {
            identity,
            authored_pattern_index,
            ..
        } = *row;
        if let Some(pattern_source) = &donor.gear_art_pattern_source {
            if !matches!(
                classify_keyed_auxiliary_donor(
                    &self.sandbox_patterns,
                    &self.sandbox_pattern_index,
                    pattern_source.item_hash,
                    identity.item_hash,
                    context.sandbox_pattern_layout,
                )?,
                KeyedAuxiliaryDonorPresence::Present(_)
            ) {
                return Err(validation(
                    "Resolved gear-art/runtime row source disappeared while compiling the project",
                ));
            }
            (self.sandbox_patterns, self.sandbox_pattern_index) = append_keyed_auxiliary_pair(
                std::mem::take(&mut self.sandbox_patterns),
                std::mem::take(&mut self.sandbox_pattern_index),
                pattern_source.item_hash,
                identity.item_hash,
                context.sandbox_pattern_layout,
            )?;
            let sandbox_pattern_row_index = validate_keyed_auxiliary_alignment(
                &self.sandbox_patterns,
                &self.sandbox_pattern_index,
                context.sandbox_pattern_layout,
            )?
            .count
                - 1;
            if Some(sandbox_pattern_row_index) != authored_pattern_index.map(usize::from) {
                return Err(validation(
                    "Authored weapon definition and sandbox-pattern row indices diverged",
                ));
            }
            let target_pattern_global_id_hash = context
                .authored_pattern_global_ids
                .get(ordinal)
                .copied()
                .flatten()
                .ok_or_else(|| {
                    validation(
                        "Authored sandbox-pattern row has no private runtime identity assignment",
                    )
                })?;
            let row_offset = array_at(&self.sandbox_patterns, 8)?.2
                + sandbox_pattern_row_index * SANDBOX_PATTERN_ROW_SIZE;
            write_u32(
                &mut self.sandbox_patterns,
                row_offset + SANDBOX_PATTERN_GLOBAL_ID_OFFSET,
                target_pattern_global_id_hash,
            )?;
            apply_array_row_raw_payload_patches(
                &mut self.sandbox_patterns,
                8,
                sandbox_pattern_row_index,
                SANDBOX_PATTERN_ROW_SIZE,
                SANDBOX_PATTERN_ROW_CLASS,
                WeaponRawPayloadTarget::SandboxPatternRow,
                &donor.weapon.overrides.raw_payload_patches,
            )?;
            let authored_pattern =
                sandbox_pattern_identity(&self.sandbox_patterns, identity.item_hash)
                    .map_err(invalid)?
                    .ok_or_else(|| validation("Authored gear-art/runtime row is missing"))?;
            if authored_pattern.pattern_global_id_hash != target_pattern_global_id_hash {
                return Err(invalid(
                    "Raw sandbox-pattern patches changed the structured runtime entity identity",
                ));
            }
            if authored_pattern.weapon_content_group_hash
                != pattern_source.weapon_content_group_hash
                || authored_pattern.weapon_translation_group_hash
                    != pattern_source.weapon_translation_group_hash
            {
                return Err(invalid(
                    "Raw sandbox-pattern patches changed the structured gear-art identity",
                ));
            }
            apply_array_row_raw_payload_patches(
                &mut self.sandbox_pattern_index,
                8,
                sandbox_pattern_row_index,
                SANDBOX_PATTERN_INDEX_ROW_SIZE,
                SANDBOX_PATTERN_INDEX_ROW_CLASS,
                WeaponRawPayloadTarget::SandboxPatternIndexRow,
                &donor.weapon.overrides.raw_payload_patches,
            )?;
            validate_keyed_auxiliary_structure(
                &self.sandbox_patterns,
                &self.sandbox_pattern_index,
                context.sandbox_pattern_layout,
            )?;
            self.any_sandbox_pattern = true;
        } else if donor
            .weapon
            .overrides
            .raw_payload_patches
            .iter()
            .any(|patch| {
                matches!(
                    patch.target,
                    WeaponRawPayloadTarget::SandboxPatternRow
                        | WeaponRawPayloadTarget::SandboxPatternIndexRow
                )
            })
        {
            return Err(invalid(
                "Raw sandbox-pattern patch requested for a donor with no active weapon pattern",
            ));
        }
        Ok(())
    }

    fn append_collection(
        &mut self,
        donor: &resolve::ResolvedWeapon,
        row: &WeaponRow,
    ) -> AuthoringResult<()> {
        let WeaponRow {
            current_unlock_count,
            identity,
            definition_tag,
            string_tag,
            item_index,
            collectible_index,
            unlock_definition_index,
            unlock_slot,
            authored_icon_index,
            collection_material_set,
            ..
        } = *row;
        self.item_table = append_index_row(
            std::mem::take(&mut self.item_table),
            donor.donor_item_index,
            identity.item_hash,
            definition_tag,
            ITEM_DEFINITION_INDEX_ROW_CLASS,
            "item-definition index",
        )?;
        self.item_strings = append_index_row(
            std::mem::take(&mut self.item_strings),
            donor.donor_item_index,
            identity.item_hash,
            string_tag,
            ITEM_STRING_INDEX_ROW_CLASS,
            "item-string index",
        )?;
        apply_array_row_raw_payload_patches(
            &mut self.item_table,
            8,
            usize::from(item_index),
            ITEM_ROW_SIZE,
            ITEM_DEFINITION_INDEX_ROW_CLASS,
            WeaponRawPayloadTarget::ItemDefinitionIndexRow,
            &donor.weapon.overrides.raw_payload_patches,
        )?;
        apply_array_row_raw_payload_patches(
            &mut self.item_strings,
            8,
            usize::from(item_index),
            ITEM_ROW_SIZE,
            ITEM_STRING_INDEX_ROW_CLASS,
            WeaponRawPayloadTarget::ItemStringIndexRow,
            &donor.weapon.overrides.raw_payload_patches,
        )?;
        let parents = sunrise_badge_collectible_parents(donor.weapon_page);
        self.collectibles = append_collectible(
            std::mem::take(&mut self.collectibles),
            donor.donor_collectible_index,
            AuthoredCollectibleSpec {
                collectible_hash: identity.collectible_hash,
                item_index,
                unlock: CollectibleUnlockClone {
                    source_index: donor.source_unlock_index,
                    authored_index: unlock_definition_index,
                },
                material_set_index: collection_material_set,
                presentation_parents: &parents,
                require_donor_parent_subset: false,
            },
        )?;
        apply_array_row_raw_payload_patches(
            &mut self.collectibles,
            8,
            usize::from(collectible_index),
            COLLECTIBLE_ROW_SIZE,
            COLLECTIBLE_DEFINITION_ROW_CLASS,
            WeaponRawPayloadTarget::CollectibleDefinitionRow,
            &donor.weapon.overrides.raw_payload_patches,
        )?;
        validate_authored_collectible_nested_isolation(
            &self.collectibles,
            usize::from(collectible_index),
            donor.source_unlock_index,
            unlock_definition_index,
        )?;
        let mut collectible_identity = identity;
        if donor.weapon.text.collection_name.is_some() {
            collectible_identity.name_hash = identity.collection_name_hash;
        }
        if donor.weapon.text.collection_description.is_some() {
            collectible_identity.flavor_hash = identity.collection_description_hash;
        }
        self.collectible_displays = append_collectible_display(
            std::mem::take(&mut self.collectible_displays),
            donor.donor_collectible_index,
            collectible_identity,
            authored_icon_index,
            LOCALIZATION_DONOR_TABLE_INDEX as u32,
        )?;
        if donor.weapon.text.collection_requirement.is_some() {
            let (_, _, display_rows, display_class) = array_at(&self.collectible_displays, 8)?;
            if display_class != COLLECTIBLE_DISPLAY_ROW_CLASS {
                return Err(validation(
                    "Authored collectible display table has the wrong row class",
                ));
            }
            write_localized_reference(
                &mut self.collectible_displays,
                display_rows
                    + usize::from(collectible_index) * COLLECTIBLE_DISPLAY_ROW_SIZE
                    + COLLECTIBLE_DISPLAY_REQUIREMENT_REFERENCE_OFFSET,
                LOCALIZATION_DONOR_TABLE_INDEX as u32,
                identity.collection_requirement_hash,
            )?;
        }
        apply_array_row_raw_payload_patches(
            &mut self.collectible_displays,
            8,
            usize::from(collectible_index),
            COLLECTIBLE_DISPLAY_ROW_SIZE,
            COLLECTIBLE_DISPLAY_ROW_CLASS,
            WeaponRawPayloadTarget::CollectibleDisplayRow,
            &donor.weapon.overrides.raw_payload_patches,
        )?;
        validate_authored_collectible_display_row(&self.collectible_displays, collectible_index)?;
        self.unlocks = append_unlock(
            std::mem::take(&mut self.unlocks),
            identity.unlock_hash,
            ACCOUNT_UNLOCK_BANK,
            unlock_slot,
        )?;
        apply_array_row_raw_payload_patches(
            &mut self.unlocks,
            8,
            usize::from(unlock_definition_index),
            UNLOCK_ROW_SIZE,
            UNLOCK_FLAG_DEFINITION_ROW_CLASS,
            WeaponRawPayloadTarget::UnlockDefinitionRow,
            &donor.weapon.overrides.raw_payload_patches,
        )?;
        let unlock_sorted_position =
            unlock_sorted_index_position(&self.unlocks, unlock_definition_index)?;
        apply_array_row_raw_payload_patches(
            &mut self.unlocks,
            0x18,
            unlock_sorted_position,
            UNLOCK_FLAG_SORTED_INDEX_ROW_SIZE,
            UNLOCK_FLAG_SORTED_INDEX_ROW_CLASS,
            WeaponRawPayloadTarget::UnlockSortedIndexRow,
            &donor.weapon.overrides.raw_payload_patches,
        )?;
        unlock_sorted_index_position(&self.unlocks, unlock_definition_index)?;
        self.unlock_banks = append_unlock_flag_bank_row(
            std::mem::take(&mut self.unlock_banks),
            ACCOUNT_UNLOCK_BANK,
            unlock_slot,
            identity.unlock_hash,
            unlock_definition_index,
        )?;
        apply_array_row_raw_payload_patches(
            &mut self.unlock_banks,
            unlock_flag_bank_descriptor(ACCOUNT_UNLOCK_BANK)?,
            usize::from(unlock_slot),
            crate::progression::UNLOCK_FLAG_BANK_ROW_SIZE,
            crate::progression::UNLOCK_FLAG_BANK_ROW_CLASS,
            WeaponRawPayloadTarget::UnlockBankRow,
            &donor.weapon.overrides.raw_payload_patches,
        )?;
        validate_authored_unlock_flag_bank_row(
            &self.unlock_banks,
            ACCOUNT_UNLOCK_BANK,
            unlock_slot,
            identity.unlock_hash,
            unlock_definition_index,
        )?;
        self.unlock_displays = append_unlock_display(
            std::mem::take(&mut self.unlock_displays),
            donor.source_unlock_index,
            identity.unlock_hash,
            current_unlock_count,
        )?;
        apply_array_row_raw_payload_patches(
            &mut self.unlock_displays,
            8,
            usize::from(unlock_definition_index),
            UNLOCK_DISPLAY_ROW_SIZE,
            UNLOCK_FLAG_DISPLAY_ROW_CLASS,
            WeaponRawPayloadTarget::UnlockDisplayRow,
            &donor.weapon.overrides.raw_payload_patches,
        )?;
        validate_authored_unlock_display_row(
            &self.unlock_displays,
            unlock_definition_index,
            identity.unlock_hash,
        )?;
        validate_authored_tables(
            &self.item_table,
            &self.item_strings,
            &self.collectibles,
            &self.collectible_displays,
            &self.unlocks,
            &self.unlock_displays,
            collectible_identity,
            definition_tag,
            string_tag,
            item_index,
            collectible_index,
            unlock_definition_index,
            unlock_slot,
            authored_icon_index,
            LOCALIZATION_DONOR_TABLE_INDEX as u32,
            collection_material_set,
            &parents,
            donor
                .weapon
                .text
                .collection_requirement
                .as_ref()
                .map(|_| identity.collection_requirement_hash),
        )?;
        Ok(())
    }
}

#[derive(Clone, Copy)]
struct WeaponRow {
    current_unlock_count: usize,
    identity: WeaponCloneIdentity,
    definition_tag: TagHash,
    string_tag: TagHash,
    item_index: u16,
    collectible_index: u16,
    unlock_definition_index: u16,
    unlock_slot: u16,
    authored_icon_index: u16,
    authored_icon_container: TagHash,
    authored_pattern_index: Option<u16>,
    collection_material_set: u16,
}
