use super::*;

/// Stock finished sandbox-perk row whose live action serves as the tag-placement template when a
/// private program is authored over a declaration-only source row. 464 is The Fundamentals' Void
/// effect: a plain, always-present sandbox-perk action with no entity assets.
const DECLARATION_ONLY_TEMPLATE_PERK_INDEX: usize = 464;
mod damage_markers;
mod sharing;

/// Validated stock rows and decoded tables behind one private socket plug donor.
struct PrivatePlugSource {
    cosmetic: bool,
    item_index: usize,
    definition_tag: TagHash,
    string_tag: TagHash,
    definition: Vec<u8>,
    strings: Vec<u8>,
    icon_container: TagHash,
    perk_indices: Vec<u16>,
    classification: Option<crate::plug_classification::PlugClassification>,
    classification_perk_index: Option<usize>,
}

/// Reads the classification template from a donor, recording its one visible finished perk.
fn read_plug_classification(
    sources: &sources::ProjectSources,
    hash: u32,
    classification_perk_index: &mut Option<usize>,
) -> AuthoringResult<crate::plug_classification::PlugClassification> {
    let rows = sources
        .stock_item_rows_by_hash
        .get(&hash)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let [index] = rows else {
        return Err(invalid(format!(
            "Private plug classification source 0x{hash:08X} resolves to {} stock item rows",
            rows.len()
        )));
    };
    if read_u32(
        &sources.stock_item_strings,
        sources.string_rows + index * ITEM_ROW_SIZE,
    )? != hash
    {
        return Err(validation(
            "Classification source item/string indices are not aligned",
        ));
    }
    let definition = read_tag(
        &sources.manager,
        TagHash(read_u32(
            &sources.stock_item_table,
            sources.item_rows + index * ITEM_ROW_SIZE + 16,
        )?),
        "classification source definition",
    )?;
    let strings = read_tag(
        &sources.manager,
        TagHash(read_u32(
            &sources.stock_item_strings,
            sources.string_rows + index * ITEM_ROW_SIZE + 16,
        )?),
        "classification source strings",
    )?;
    for perk in weapon_sandbox_perks(&definition)? {
        let row =
            finished_sandbox_perk_at(&sources.stock_finished_sandbox_perks, usize::from(perk))
                .map_err(invalid)?;
        if row
            .detail
            .as_ref()
            .is_some_and(|detail| read_u16(detail, 0).ok() != Some(u16::MAX))
            && classification_perk_index
                .replace(usize::from(perk))
                .is_some()
        {
            return Err(invalid(
                "Classification donor has more than one visible finished perk",
            ));
        }
    }
    crate::plug_classification::PlugClassification::from_template(&definition, &strings)
}

/// Reads and validates the stock donor one private socket plug is built from.
///
/// This performs every check that does not allocate authored identity, so that
/// allocation and its occupancy bookkeeping stay together in the caller.
fn read_private_plug_source(
    sources: &sources::ProjectSources,
    variant: &WeaponSocketPlugVariantOverride,
    sandbox_perk_string_template: &[u8],
) -> AuthoringResult<PrivatePlugSource> {
    let source_rows = sources
        .stock_item_rows_by_hash
        .get(&variant.source_plug_hash)
        .map(Vec::as_slice)
        .unwrap_or_default();
    let [item_index] = source_rows else {
        return Err(invalid(format!(
            "Private socket plug 0x{:08X} resolves to {} stock item rows",
            variant.source_plug_hash,
            source_rows.len()
        )));
    };
    let item_index = *item_index;
    if read_u32(
        &sources.stock_item_strings,
        sources.string_rows + item_index * ITEM_ROW_SIZE,
    )? != variant.source_plug_hash
    {
        return Err(validation(
            "Private socket-plug item and item-string rows are not aligned",
        ));
    }
    let definition_tag = TagHash(read_u32(
        &sources.stock_item_table,
        sources.item_rows + item_index * ITEM_ROW_SIZE + 16,
    )?);
    let string_tag = TagHash(read_u32(
        &sources.stock_item_strings,
        sources.string_rows + item_index * ITEM_ROW_SIZE + 16,
    )?);
    let definition = read_tag(
        &sources.manager,
        definition_tag,
        "private socket-plug donor definition",
    )?;
    let strings = read_tag(
        &sources.manager,
        string_tag,
        "private socket-plug donor strings",
    )?;
    if matching_u32_offsets(&definition, variant.source_plug_hash) != [ITEM_DEFINITION_HASH_OFFSET]
        || !matching_u32_offsets(&strings, variant.source_plug_hash).is_empty()
    {
        return Err(invalid(format!(
            "Private socket-plug donor 0x{:08X} embeds its identity at unsupported offsets",
            variant.source_plug_hash
        )));
    }
    // A native cosmetic plug has no investment resource or finished-perk strings.
    // Keep that absence when cloning presentation-only choices.
    let cosmetic = variant.replace_effects
        && variant.sandbox_perks.is_empty()
        && variant.additional_sandbox_perks.is_empty()
        && variant.investment_stats.is_empty()
        && variant.classification_donor_hash.is_none()
        && read_u64(&definition, ITEM_INVESTMENT_STAT_POINTER_OFFSET)? == 0
        && read_u64(&strings, ITEM_STRING_SANDBOX_PERK_RESOURCE_POINTER_OFFSET)? == 0;
    let perk_indices = if cosmetic {
        Vec::new()
    } else {
        validate_weapon_sandbox_perk_parallelism(
            &definition,
            &strings,
            sandbox_perk_string_template,
        )?;
        weapon_sandbox_perks(&definition)?
    };
    for &index in &variant.additional_sandbox_perks {
        if !variant.replace_effects && perk_indices.contains(&index) {
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
    let icon_index = read_u16(&strings, ITEM_STRING_ICON_INDEX_OFFSET)?;
    validate_reused_stock_item_icon(&sources.stock_item_icons, &strings, icon_index)?;
    let icon_container = stock_item_icon_container(&sources.stock_item_icons, icon_index)?;
    let mut classification_perk_index = None;
    let classification = variant
        .classification_donor_hash
        .map(|hash| read_plug_classification(sources, hash, &mut classification_perk_index))
        .transpose()?;
    Ok(PrivatePlugSource {
        cosmetic,
        item_index,
        definition_tag,
        string_tag,
        definition,
        strings,
        icon_container,
        perk_indices,
        classification,
        classification_perk_index,
    })
}

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
        if donor.weapon.overrides.lore.is_some() {
            occupied_localized_hashes.insert(crate::presentation::text_hash(
                &donor.weapon.namespace,
                "lore",
            ));
        }
        if let Some(badge) = &donor.weapon.overrides.badge {
            occupied_localized_hashes
                .insert(crate::presentation::text_hash(&badge.name, "badge-name"));
            occupied_localized_hashes.insert(crate::presentation::text_hash(
                &badge.name,
                "badge-description",
            ));
        }
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
    let mut custom_plugs: Vec<ResolvedCustomPlug> = Vec::with_capacity(custom_plug_count);
    let mut shared_keys = Vec::with_capacity(custom_plug_count);
    for (weapon_ordinal, donor) in resolved.iter().enumerate() {
        let mut variants = donor.weapon.overrides.socket_plug_variants.clone();
        variants.sort_by_key(|variant| (variant.socket_index, variant.choice_index));
        for variant in variants {
            let context = format!(
                "{}\nPrivate Perk: {:?}\nSource Item: 0x{:08X}\nSocket: {}\nChoice: {}",
                donor.weapon.error_context(),
                variant.name.as_deref().unwrap_or("Unnamed Perk"),
                variant.source_plug_hash,
                usize::from(variant.socket_index) + 1,
                usize::from(variant.choice_index) + 1
            );
            (|| -> AuthoringResult<()> {
                    let source =
                        read_private_plug_source(sources, &variant, sandbox_perk_string_template)?;
                    let usage = CustomPlugUse {
                        weapon_ordinal,
                        socket_index: usize::from(variant.socket_index),
                        choice_index: usize::from(variant.choice_index),
                    };
                    let shared_key =
                        sharing::Key::new(sources, &variant, source.classification, source.classification_perk_index)?;
                    if let Some(index) = shared_keys.iter().position(|key| *key == shared_key) {
                        custom_plugs[index].uses.push(usage);
                        return Ok(());
                    }
                    shared_keys.push(shared_key);
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
                    let mut effect_indices = Vec::new();
                    let edited = variant
                        .sandbox_perks
                        .iter()
                        .map(|perk| perk.source_perk_index)
                        .collect::<BTreeSet<_>>();
                    let effects = variant
                        .sandbox_perks
                        .into_iter()
                        .enumerate()
                        .map(|(position, perk)| {
                            let hidden = if variant.replace_effects {
                                position > 0
                            } else {
                                variant
                                    .additional_sandbox_perks
                                    .contains(&perk.source_perk_index)
                            };
                            (perk, hidden)
                        })
                        .chain(
                            variant
                                .additional_sandbox_perks
                                .iter()
                                .copied()
                                .filter(|index| !edited.contains(index))
                                .map(|source_perk_index| {
                                    (
                                        WeaponSandboxPerkRuntimeOverride {
                                            program: None,
                                            projectiles: Vec::new(),
                                            source_perk_index,
                                            activation: None,
                                            runtime_values: Vec::new(),
                                            action_float_values: Vec::new(),
                                        },
                                        true,
                                    )
                                }),
                        );
                    for (perk, hidden) in effects {
                        effect_indices.push(perk.source_perk_index);
                        if !variant.replace_effects
                            && !hidden
                            && !source.perk_indices.contains(&perk.source_perk_index)
                        {
                            return Err(invalid(format!(
                                "Private socket-plug donor 0x{:08X} does not contain finished sandbox-perk index {}",
                                variant.source_plug_hash, perk.source_perk_index
                            )));
                        }
                        let runtime_action = match load_sandbox_perk_runtime_action(
                            &sources.manager,
                            &sources.globals_data,
                            usize::from(perk.source_perk_index),
                        ) {
                            Ok(action) => action,
                            // A declaration-only row has no runtime action: its runtime key is
                            // the no-hash sentinel. Stock ships such rows on real plugs (479,
                            // Volatile Light, Hard Light's alternate-fire marker), and the client
                            // docs record that the native perk-bank producer simply skips them.
                            // With an authored program the source contributes only its
                            // finished-perk identity and a placement template for the new tag,
                            // so borrow a live stock action as the template and let the program
                            // supply the whole payload.
                            Err(error)
                                if perk.program.is_some() && error.contains("is not assigned") =>
                            {
                                let finished_perk = finished_sandbox_perk_at(
                                    &sources.stock_finished_sandbox_perks,
                                    usize::from(perk.source_perk_index),
                                )
                                .map_err(invalid)?;
                                let template = load_sandbox_perk_runtime_action(
                                    &sources.manager,
                                    &sources.globals_data,
                                    DECLARATION_ONLY_TEMPLATE_PERK_INDEX,
                                )
                                .map_err(invalid)?;
                                sundial::package_authoring::sandbox_perk::SandboxPerkRuntimeAction {
                                    finished_perk,
                                    action_tag: template.action_tag,
                                    action_payload: Vec::new(),
                                    graphs: Vec::new(),
                                }
                            }
                            Err(error) => return Err(invalid(error)),
                        };
                        if damage_markers::keeps_stock_identity(&perk) {
                            // A stock elemental marker is both an identity and a runtime action.
                            // Retain that identity on this private plug without changing the
                            // shared marker's payload or presentation. Edited effects still clone.
                            continue;
                        }
                        let perk_role = format!(
                            "socket/{}/choice/{}/perk/{}/definition",
                            variant.socket_index, variant.choice_index, perk.source_perk_index
                        );
                        let runtime_role = format!(
                            "socket/{}/choice/{}/perk/{}/runtime",
                            variant.socket_index, variant.choice_index, perk.source_perk_index
                        );
                        private_perks.push(ResolvedPrivateSandboxPerk {
                            program: perk.program,
                            source_index: usize::from(perk.source_perk_index),
                            projectiles: perk.projectiles,
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
                        cosmetic: source.cosmetic,
                        replace_effects: variant.replace_effects,
                        investment_stats: variant.investment_stats,
                        uses: vec![usage],
                        source_item_hash: variant.source_plug_hash,
                        source_item_index: source.item_index,
                        source_definition_tag: source.definition_tag,
                        source_string_tag: source.string_tag,
                        source_definition: source.definition,
                        source_strings: source.strings,
                        source_icon_container: source.icon_container,
                        authored_icon_container: None,
                        icon: variant.icon.clone(),
                        authored_item_hash,
                        authored_item_index,
                        authored_definition_tag,
                        authored_string_tag,
                        authored_name_hash,
                        authored_name,
                        classification: source.classification,
                        classification_perk_index: source.classification_perk_index,
                        classification_item_index: variant
                            .classification_donor_hash
                            .map(|hash| sources.stock_item_rows_by_hash[&hash][0]),
                        authored_description_hash,
                        authored_description: variant.description,
                        additional_sandbox_perks: variant.additional_sandbox_perks,
                        effect_indices,
                        sandbox_perks: private_perks,
                    });
                    Ok(())
                })().map_err(|error| error.context(context))?;
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
    resolved: &[resolve::ResolvedWeapon],
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
        (|| -> AuthoringResult<()> {
            let mut definition = custom_plug.source_definition.clone();
            if custom_plug.cosmetic {
                // Cosmetics retain their absent investment block.
            } else if custom_plug.replace_effects {
                replace_custom_plug_stats(&mut definition, &custom_plug.investment_stats)?;
            } else {
                apply_custom_plug_stats(&mut definition, &custom_plug.investment_stats)?;
            }
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
            if !custom_plug.cosmetic && (custom_plug.replace_effects || !custom_plug.additional_sandbox_perks.is_empty()) {
                let mut perks = if custom_plug.replace_effects {
                    custom_plug.effect_indices.clone()
                } else {
                    weapon_sandbox_perks(&definition)?
                };
                if !custom_plug.replace_effects {
                    perks.extend_from_slice(&custom_plug.additional_sandbox_perks);
                }
                if custom_plug.replace_effects {
                    // Independent effects must not inherit their presentation donor's conditions.
                    set_item_string_sandbox_perk_count(&mut strings, 0, sandbox_perk_string_template)?;
                    set_weapon_base_sandbox_perks(&mut definition, &[], sandbox_perk_definition_template)?;
                }
                set_weapon_base_sandbox_perks_with_strings(
                    &mut definition,
                    &mut strings,
                    &perks,
                    sandbox_perk_definition_template,
                    sandbox_perk_string_template,
                )?;

            }
            for perk in &custom_plug.sandbox_perks {
                let authored_runtime_tag = clone_private_sandbox_perk_runtime(
                    manager,
                    &perk.runtime_action,
                    custom_runtime::PrivateRuntimeEdits {
                        program: perk.program.as_ref(),
                        values: &perk.runtime_values,
                        action_float_values: &perk.action_float_values,
                        projectiles: &perk.projectiles,
                        activation: perk.activation,
                    },
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
                            icon_index: custom_plug
                                .authored_icon_container
                                .map(|_| read_u16(&strings, ITEM_STRING_ICON_INDEX_OFFSET))
                                .transpose()?,
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
            if !custom_plug.cosmetic {
            validate_weapon_sandbox_perk_parallelism(
                &definition,
                &strings,
                sandbox_perk_string_template,
            )?;
            }
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
            Ok(())
        })().map_err(|error| error.context(plug_context(custom_plug, resolved)))?;
    }
    Ok(CustomPlugPayloads {
        definitions: custom_plug_definitions,
        strings: custom_plug_strings,
    })
}

fn plug_context(plug: &ResolvedCustomPlug, resolved: &[resolve::ResolvedWeapon]) -> String {
    let owners = plug
        .uses
        .iter()
        .map(|usage| {
            format!(
                "{}\nSocket: {}\nChoice: {}",
                resolved[usage.weapon_ordinal].weapon.error_context(),
                usage.socket_index + 1,
                usage.choice_index + 1
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n");
    format!(
        "Private Perk: {:?}\nSource Item: 0x{:08X}\nUsed By:\n{owners}",
        plug.authored_name.as_deref().unwrap_or("Unnamed Perk"),
        plug.source_item_hash
    )
}
