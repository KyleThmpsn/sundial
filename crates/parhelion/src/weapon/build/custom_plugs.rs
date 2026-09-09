use super::*;

pub(super) fn plan(
    sources: &sources::ProjectSources,
    resolved: &[resolve::ResolvedWeapon],
    sandbox_perk_string_template: &[u8],
) -> AuthoringResult<Vec<ResolvedCustomPlug>> {
    let custom_plug_count = resolved.iter().try_fold(0usize, |count, donor| {
        count
            .checked_add(donor.weapon.overrides.socket_plug_variants.len())
            .ok_or_else(|| invalid("Private socket-plug count overflowed"))
    })?;
    let mut occupied_item_hashes = sources
        .stock_item_rows_by_hash
        .keys()
        .copied()
        .collect::<BTreeSet<_>>();
    occupied_item_hashes.extend(resolved.iter().map(|donor| donor.weapon.identity.item_hash));
    let mut occupied_perk_hashes = BTreeSet::new();
    for index in
        0..finished_sandbox_perk_count(&sources.stock_finished_sandbox_perks).map_err(invalid)?
    {
        occupied_perk_hashes.insert(
            finished_sandbox_perk_at(&sources.stock_finished_sandbox_perks, index)
                .map_err(invalid)?
                .perk_hash,
        );
    }
    let mut occupied_runtime_keys = BTreeSet::new();
    for index in 0..sandbox_perk_runtime_assignment_count(&sources.stock_entity_assignments)
        .map_err(invalid)?
    {
        occupied_runtime_keys.insert(
            sandbox_perk_runtime_assignment_at(&sources.stock_entity_assignments, index)
                .map_err(invalid)?
                .runtime_key,
        );
    }
    occupied_runtime_keys.extend(
        resolved
            .iter()
            .map(|donor| donor.weapon.identity.pattern_global_id_hash),
    );
    let mut occupied_localized_hashes = BTreeSet::from([
        LOCALIZATION_DONOR_STRING_HASHES[0],
        LOCALIZATION_DONOR_STRING_HASHES[1],
        SUNRISE_BADGE_DESCRIPTION_HASH,
        SUNRISE_BADGE_NAME_HASH,
    ]);
    for donor in resolved {
        let identity = donor.weapon.identity;
        occupied_localized_hashes.extend([
            identity.name_hash,
            identity.type_hash,
            identity.flavor_hash,
            identity.source_hash,
            identity.collection_name_hash,
            identity.collection_description_hash,
            identity.inventory_hint_hash,
            identity.collection_requirement_hash,
        ]);
    }
    let private_host_ordinal_base = resolved
        .len()
        .checked_mul(2)
        .ok_or_else(|| invalid("Authored weapon host-tag count overflowed"))?;
    let mut custom_plugs = Vec::with_capacity(custom_plug_count);
    for (weapon_ordinal, donor) in resolved.iter().enumerate() {
        let mut variants = donor.weapon.overrides.socket_plug_variants.clone();
        variants.sort_by_key(|variant| (variant.socket_index, variant.choice_index));
        for variant in variants {
            let source_rows = sources
                .stock_item_rows_by_hash
                .get(&variant.source_plug_hash)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            let [source_item_index] = source_rows else {
                return Err(invalid(format!(
                    "Private socket plug 0x{:08X} resolves to {} stock item rows",
                    variant.source_plug_hash,
                    source_rows.len()
                ))
                .context(format!(
                    "Weapon {:?} ({})",
                    donor.weapon.text.name, donor.weapon.namespace
                )));
            };
            if read_u32(
                &sources.stock_item_strings,
                sources.string_rows + source_item_index * ITEM_ROW_SIZE,
            )? != variant.source_plug_hash
            {
                return Err(validation(
                    "Private socket-plug item and item-string rows are not aligned",
                ));
            }
            let source_definition_tag = TagHash(read_u32(
                &sources.stock_item_table,
                sources.item_rows + source_item_index * ITEM_ROW_SIZE + 16,
            )?);
            let source_string_tag = TagHash(read_u32(
                &sources.stock_item_strings,
                sources.string_rows + source_item_index * ITEM_ROW_SIZE + 16,
            )?);
            let source_definition = read_tag(
                &sources.manager,
                source_definition_tag,
                "private socket-plug donor definition",
            )?;
            let source_strings = read_tag(
                &sources.manager,
                source_string_tag,
                "private socket-plug donor strings",
            )?;
            if matching_u32_offsets(&source_definition, variant.source_plug_hash)
                != [ITEM_DEFINITION_HASH_OFFSET]
                || !matching_u32_offsets(&source_strings, variant.source_plug_hash).is_empty()
            {
                return Err(invalid(format!(
                    "Private socket-plug donor 0x{:08X} embeds its identity at unsupported offsets",
                    variant.source_plug_hash
                )));
            }
            validate_weapon_sandbox_perk_parallelism(
                &source_definition,
                &source_strings,
                sandbox_perk_string_template,
            )?;
            let source_perk_indices = weapon_sandbox_perks(&source_definition)?;
            for &index in &variant.additional_sandbox_perks {
                if source_perk_indices.contains(&index) {
                    return Err(invalid(format!(
                        "Private plug already supplies additional perk {index}"
                    )));
                }
                load_sandbox_perk_runtime_action(
                    &sources.manager,
                    &sources.globals_data,
                    usize::from(index),
                )
                .map_err(invalid)?;
            }
            let source_icon_index = read_u16(&source_strings, ITEM_STRING_ICON_INDEX_OFFSET)?;
            validate_reused_stock_item_icon(
                &sources.stock_item_icons,
                &source_strings,
                source_icon_index,
            )?;
            let source_icon_container =
                stock_item_icon_container(&sources.stock_item_icons, source_icon_index)?;
            let mut classification_perk_index = None;
            let classification = variant.classification_donor_hash.map(|hash| {
                let rows = sources.stock_item_rows_by_hash.get(&hash).map(Vec::as_slice).unwrap_or(&[]);
                let [index] = rows else {
                    return Err(invalid(format!("Private plug classification source 0x{hash:08X} resolves to {} stock item rows", rows.len())));
                };
                if read_u32(&sources.stock_item_strings, sources.string_rows + index * ITEM_ROW_SIZE)? != hash {
                    return Err(validation("Classification source item/string indices are not aligned"));
                }
                let definition = read_tag(&sources.manager, TagHash(read_u32(&sources.stock_item_table,
                    sources.item_rows + index * ITEM_ROW_SIZE + 16)?), "classification source definition")?;
                let strings = read_tag(&sources.manager, TagHash(read_u32(&sources.stock_item_strings,
                    sources.string_rows + index * ITEM_ROW_SIZE + 16)?), "classification source strings")?;
                for perk in weapon_sandbox_perks(&definition)? {
                    let row = finished_sandbox_perk_at(&sources.stock_finished_sandbox_perks, usize::from(perk)).map_err(invalid)?;
                    if row.detail.as_ref().is_some_and(|detail| read_u16(detail, 0).ok() != Some(u16::MAX))
                        && classification_perk_index.replace(usize::from(perk)).is_some() {
                        return Err(invalid("Classification donor has more than one visible finished perk"));
                    }
                }
                crate::plug_classification::PlugClassification::from_template(&definition, &strings)
            }).transpose()?;
            let custom_ordinal = custom_plugs.len();
            let authored_item_index = u16::try_from(
                sources
                    .stock_item_count
                    .checked_add(resolved.len())
                    .and_then(|count| count.checked_add(custom_ordinal))
                    .ok_or_else(|| invalid("Private socket-plug item index overflowed"))?,
            )
            .map_err(|_| invalid("Private socket-plug item index does not fit 16 bits"))?;
            let tag_ordinal = private_host_ordinal_base
                .checked_add(
                    custom_ordinal
                        .checked_mul(2)
                        .ok_or_else(|| invalid("Private socket-plug tag index overflowed"))?,
                )
                .ok_or_else(|| invalid("Private socket-plug tag index overflowed"))?;
            let authored_definition_tag = TagHash::new(
                HOST_PACKAGE_ID,
                u16::try_from(HOST_EXPECTED_ENTRY_COUNT + tag_ordinal).map_err(|_| {
                    invalid("Private socket-plug definition tag does not fit 16 bits")
                })?,
            );
            let authored_string_tag = TagHash::new(
                HOST_PACKAGE_ID,
                u16::try_from(HOST_EXPECTED_ENTRY_COUNT + tag_ordinal + 1)
                    .map_err(|_| invalid("Private socket-plug string tag does not fit 16 bits"))?,
            );
            let item_role = format!(
                "socket/{}/choice/{}/private-plug",
                variant.socket_index, variant.choice_index
            );
            let authored_item_hash = allocate_identity_hash(
                &donor.weapon.namespace,
                &item_role,
                &mut occupied_item_hashes,
                None,
            )?;
            let authored_name = variant.name.clone();
            let authored_name_hash = authored_name
                .as_ref()
                .map(|_| {
                    allocate_identity_hash(
                        &donor.weapon.namespace,
                        &format!(
                            "socket/{}/choice/{}/private-plug/name",
                            variant.socket_index, variant.choice_index
                        ),
                        &mut occupied_localized_hashes,
                        Some(LOCALIZATION_DONOR_STRING_HASHES[1]),
                    )
                })
                .transpose()?;
            let authored_description_hash = variant
                .description
                .as_ref()
                .map(|_| {
                    allocate_identity_hash(
                        &donor.weapon.namespace,
                        &format!(
                            "socket/{}/choice/{}/private-plug/description",
                            variant.socket_index, variant.choice_index
                        ),
                        &mut occupied_localized_hashes,
                        Some(LOCALIZATION_DONOR_STRING_HASHES[1]),
                    )
                })
                .transpose()?;
            let mut private_perks = Vec::with_capacity(variant.sandbox_perks.len());
            let effects =
                variant
                    .sandbox_perks
                    .into_iter()
                    .map(|perk| (perk, false))
                    .chain(variant.additional_sandbox_perks.iter().copied().map(
                        |source_perk_index| {
                            (
                                WeaponSandboxPerkRuntimeOverride {
                                    source_perk_index,
                                    activation: None,
                                    runtime_values: Vec::new(),
                                    action_float_values: Vec::new(),
                                },
                                true,
                            )
                        },
                    ));
            for (perk, hidden) in effects {
                if !hidden && !source_perk_indices.contains(&perk.source_perk_index) {
                    return Err(invalid(format!(
                        "Private socket-plug donor 0x{:08X} does not contain finished sandbox-perk index {}",
                        variant.source_plug_hash, perk.source_perk_index
                    )));
                }
                let runtime_action = load_sandbox_perk_runtime_action(
                    &sources.manager,
                    &sources.globals_data,
                    usize::from(perk.source_perk_index),
                )
                .map_err(invalid)?;
                let perk_role = format!(
                    "socket/{}/choice/{}/perk/{}/definition",
                    variant.socket_index, variant.choice_index, perk.source_perk_index
                );
                let runtime_role = format!(
                    "socket/{}/choice/{}/perk/{}/runtime",
                    variant.socket_index, variant.choice_index, perk.source_perk_index
                );
                private_perks.push(ResolvedPrivateSandboxPerk {
                    source_index: usize::from(perk.source_perk_index),
                    activation: perk.activation,
                    hidden,
                    runtime_action,
                    authored_perk_hash: allocate_identity_hash(
                        &donor.weapon.namespace,
                        &perk_role,
                        &mut occupied_perk_hashes,
                        None,
                    )?,
                    authored_runtime_key: allocate_identity_hash(
                        &donor.weapon.namespace,
                        &runtime_role,
                        &mut occupied_runtime_keys,
                        None,
                    )?,
                    runtime_values: perk.runtime_values,
                    action_float_values: perk.action_float_values,
                });
            }
            custom_plugs.push(ResolvedCustomPlug {
                investment_stats: variant.investment_stats,
                weapon_ordinal,
                socket_index: usize::from(variant.socket_index),
                choice_index: usize::from(variant.choice_index),
                source_item_hash: variant.source_plug_hash,
                source_item_index: *source_item_index,
                source_definition_tag,
                source_string_tag,
                source_definition,
                source_strings,
                source_icon_container,
                authored_item_hash,
                authored_item_index,
                authored_definition_tag,
                authored_string_tag,
                authored_name_hash,
                authored_name,
                classification,
                classification_perk_index,
                classification_item_index: variant
                    .classification_donor_hash
                    .map(|hash| sources.stock_item_rows_by_hash[&hash][0]),
                authored_description_hash,
                authored_description: variant.description,
                additional_sandbox_perks: variant.additional_sandbox_perks,
                sandbox_perks: private_perks,
            });
        }
    }

    Ok(custom_plugs)
}

pub(super) struct PerkCatalog<'a> {
    pub entity_assignments: &'a mut Vec<u8>,
    pub finished_sandbox_perks: &'a mut Vec<u8>,
    pub sandbox_perk_indices: &'a mut Vec<u8>,
    pub private_perk_runtime_new_tags: &'a mut Vec<NewTagSpec>,
    pub private_perk_runtime_tag_allocator: AppendedTagAllocator,
}

pub(super) struct CustomPlugPayloads {
    pub definitions: Vec<NewTagSpec>,
    pub strings: Vec<NewTagSpec>,
}

pub(super) fn author_payloads(
    manager: &PackageManager,
    custom_plugs: &[ResolvedCustomPlug],
    sandbox_perk_definition_template: &[u8; ITEM_SANDBOX_PERK_ROW_SIZE],
    sandbox_perk_string_template: &[u8],
    catalog: PerkCatalog<'_>,
) -> AuthoringResult<CustomPlugPayloads> {
    let PerkCatalog {
        entity_assignments,
        finished_sandbox_perks,
        sandbox_perk_indices,
        private_perk_runtime_new_tags,
        private_perk_runtime_tag_allocator,
    } = catalog;
    let mut custom_plug_definitions = Vec::with_capacity(custom_plugs.len());
    let mut custom_plug_strings = Vec::with_capacity(custom_plugs.len());
    for custom_plug in custom_plugs {
        let mut definition = custom_plug.source_definition.clone();
        apply_custom_plug_stats(&mut definition, &custom_plug.investment_stats)?;
        let mut strings = custom_plug.source_strings.clone();
        if let Some(classification) = custom_plug.classification {
            classification.apply(&mut definition, &mut strings)?;
        }
        write_u32(
            &mut definition,
            ITEM_DEFINITION_HASH_OFFSET,
            custom_plug.authored_item_hash,
        )?;
        if let Some(name_hash) = custom_plug.authored_name_hash {
            write_localized_reference(
                &mut strings,
                ITEM_NAME_REFERENCE_OFFSET,
                LOCALIZATION_DONOR_TABLE_INDEX as u32,
                name_hash,
            )?;
        }
        if let Some(description_hash) = custom_plug.authored_description_hash {
            write_localized_reference(
                &mut strings,
                ITEM_DESCRIPTION_REFERENCE_OFFSET,
                LOCALIZATION_DONOR_TABLE_INDEX as u32,
                description_hash,
            )?;
        }
        if !custom_plug.additional_sandbox_perks.is_empty() {
            let mut perks = weapon_sandbox_perks(&definition)?;
            perks.extend_from_slice(&custom_plug.additional_sandbox_perks);
            let original_rows = weapon_sandbox_perk_rows(&definition)?
                .into_iter()
                .map(<[u8]>::to_vec)
                .collect::<Vec<_>>();
            set_weapon_base_sandbox_perks_with_strings(
                &mut definition,
                &mut strings,
                &perks,
                sandbox_perk_definition_template,
                sandbox_perk_string_template,
            )?;
            let resource = relative_target(&definition, ITEM_INVESTMENT_STAT_POINTER_OFFSET)?;
            let (_, _, rows, _) =
                array_at(&definition, resource + ITEM_SANDBOX_PERK_DESCRIPTOR_OFFSET)?;
            for (index, row) in original_rows.iter().enumerate() {
                write_bytes(
                    &mut definition,
                    rows + index * ITEM_SANDBOX_PERK_ROW_SIZE,
                    row,
                )?;
            }
        }
        for perk in &custom_plug.sandbox_perks {
            let authored_runtime_tag = clone_private_sandbox_perk_runtime(
                manager,
                &perk.runtime_action,
                &perk.runtime_values,
                &perk.action_float_values,
                perk.activation,
                private_perk_runtime_tag_allocator,
                private_perk_runtime_new_tags,
            )?;
            *entity_assignments = insert_sandbox_perk_runtime_assignment(
                entity_assignments,
                perk.authored_runtime_key,
                authored_runtime_tag.0,
            )
            .map_err(invalid)?;
            let source_metadata_hash =
                sandbox_perk_index_hash_at(sandbox_perk_indices, perk.source_index)
                    .map_err(invalid)?;
            if source_metadata_hash != perk.runtime_action.finished_perk.perk_hash {
                return Err(validation(format!(
                    "Finished sandbox-perk row {} has hash 0x{:08X}, but its indexed metadata companion has 0x{source_metadata_hash:08X}",
                    perk.source_index, perk.runtime_action.finished_perk.perk_hash
                )));
            }
            let authored_name =
                custom_plug
                    .authored_name_hash
                    .map(|string_hash| FinishedSandboxPerkName {
                        bank_index: u16::try_from(LOCALIZATION_DONOR_TABLE_INDEX)
                            .expect("localization donor table index fits 16 bits"),
                        string_hash,
                    });
            let (authored_catalog, authored_perk_index) =
                clone_and_append_presented_finished_sandbox_perk(
                    finished_sandbox_perks,
                    perk.source_index,
                    perk.authored_perk_hash,
                    perk.authored_runtime_key,
                    FinishedSandboxPerkPresentation {
                        name: authored_name,
                        description: custom_plug.authored_description_hash.map(|string_hash| {
                            FinishedSandboxPerkName {
                                bank_index: LOCALIZATION_DONOR_TABLE_INDEX as u16,
                                string_hash,
                            }
                        }),
                        category_source_index: custom_plug.classification_perk_index,
                        hidden: perk.hidden,
                    },
                )
                .map_err(invalid)?;
            *finished_sandbox_perks = authored_catalog;
            let (authored_indices, authored_metadata_index) = clone_and_append_sandbox_perk_index(
                sandbox_perk_indices,
                perk.source_index,
                perk.authored_perk_hash,
            )
            .map_err(invalid)?;
            *sandbox_perk_indices = authored_indices;
            if authored_metadata_index != authored_perk_index {
                return Err(validation(format!(
                    "Authored finished sandbox-perk index {authored_perk_index} does not match metadata index {authored_metadata_index}"
                )));
            }
            let authored_perk_index = u16::try_from(authored_perk_index)
                .map_err(|_| invalid("Private finished sandbox-perk index does not fit 16 bits"))?;
            replace_weapon_sandbox_perk_index(
                &mut definition,
                u16::try_from(perk.source_index).map_err(|_| {
                    invalid("Source finished sandbox-perk index does not fit 16 bits")
                })?,
                authored_perk_index,
            )?;
        }
        validate_weapon_sandbox_perk_parallelism(
            &definition,
            &strings,
            sandbox_perk_string_template,
        )?;
        custom_plug_definitions.push(NewTagSpec {
            template_tag: custom_plug.source_definition_tag,
            payload: definition,
            storage: crate::NewTagStorageMode::InheritTemplate,
        });
        custom_plug_strings.push(NewTagSpec {
            template_tag: custom_plug.source_string_tag,
            payload: strings,
            storage: crate::NewTagStorageMode::InheritTemplate,
        });
    }
    Ok(CustomPlugPayloads {
        definitions: custom_plug_definitions,
        strings: custom_plug_strings,
    })
}
