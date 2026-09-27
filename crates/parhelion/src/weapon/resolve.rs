//! Resolve all weapon dependencies against a validated, read-only source generation.
use super::sources::ProjectSources;
use super::*;

#[derive(Clone)]
pub(super) struct ResolvedWeapon {
    pub(super) weapon: WeaponCloneSpec,
    pub(super) donor_item_index: usize,
    /// Where the item lands in Collections. A subclass has no Collections entry.
    pub(super) collection: Option<ResolvedCollection>,
    pub(super) definition_tag: TagHash,
    pub(super) string_tag: TagHash,
    pub(super) definition: Vec<u8>,
    pub(super) strings: Vec<u8>,
    pub(super) icon_template_item_index: usize,
    /// The icon container the template item's own presentation row resolves to. The dense table
    /// is checked against this, not against the icon donor's container, which differs from it
    /// exactly when an icon donor is set.
    pub(super) icon_template_container: TagHash,
    pub(super) donor_icon_index: u16,
    pub(super) donor_icon_container: TagHash,
    pub(super) presentation_donor: Option<ResolvedPresentationDonor>,
    pub(super) damage_carrier_source: Option<ResolvedDamageCarrierSource>,
    pub(super) render_gear_donor: Option<ResolvedRenderGearDonor>,
    pub(super) runtime_component_donors: Vec<ResolvedRuntimeComponentDonor>,
    pub(super) runtime_pattern_source: Option<ResolvedSandboxPatternSource>,
    pub(super) gear_art_pattern_source: Option<ResolvedSandboxPatternSource>,
    /// The appearance whose rig and animations are promoted onto the gameplay runtime, set
    /// only when the two belong to different weapon families and the promotion applies.
    pub(super) appearance_rig_donor: Option<u32>,
    pub(super) socket_column_indices: Vec<Option<ResolvedSocketColumn>>,
    pub(super) has_authored_shader: bool,
    /// A subclass's own socket-entry list, when its abilities come from other subclasses.
    pub(super) subclass_list: Option<super::subclass::ResolvedSubclassList>,
    /// A shader's dye rows once custom dyes replace the ones it edits.
    pub(super) dye_rows: Option<[Vec<WeaponDyeReferenceOverride>; 3]>,
}

/// The stock rows an authored item's collectible copies, and the page it lands on.
#[derive(Clone)]
pub(super) struct ResolvedCollection {
    pub(super) collectible_display_template_index: usize,
    pub(super) collectible_template_index: usize,
    pub(super) collection_donor_index: usize,
    pub(super) source_unlock_index: usize,
    pub(super) source_acquired_flag: u16,
    pub(super) weapon_page: u16,
    pub(super) count_selection: SunriseAcquiredPoolSelection,
}

#[cfg(test)]
pub(super) fn resolve_project_weapons(
    sources: &ProjectSources,
    weapons: &[WeaponCloneSpec],
) -> AuthoringResult<Vec<ResolvedWeapon>> {
    let placements = placements::Plan::new(sources, weapons)?;
    resolve_project_weapons_with_placements(sources, weapons, &placements)
}

#[cfg(test)]
pub(super) fn resolve_project_weapons_with_placements(
    sources: &ProjectSources,
    weapons: &[WeaponCloneSpec],
    placements: &placements::Plan,
) -> AuthoringResult<Vec<ResolvedWeapon>> {
    let mut report = |_: build::Phase, _: &str, _: usize, _: usize| {};
    let mut progress = build::Progress::new(weapons.len(), &mut report);
    resolve_project_weapons_with_progress(sources, weapons, placements, &mut progress)
}

pub(super) fn resolve_project_weapons_with_progress(
    sources: &ProjectSources,
    weapons: &[WeaponCloneSpec],
    placements: &placements::Plan,
    progress: &mut build::Progress<'_>,
) -> AuthoringResult<Vec<ResolvedWeapon>> {
    let manager = &sources.manager;
    let stock_item_table = &sources.stock_item_table;
    let stock_item_strings = &sources.stock_item_strings;
    let stock_sandbox_patterns = &sources.stock_sandbox_patterns;
    let stock_item_icons = &sources.stock_item_icons;
    let stock_collectibles = &sources.stock_collectibles;
    let stock_objectives = &sources.stock_objectives;
    let stock_nodes = &sources.stock_nodes;
    let stock_pools = &sources.stock_pools;
    let stock_unlocks = &sources.stock_unlocks;
    let stock_item_count = sources.stock_item_count;
    let item_rows = sources.item_rows;
    let stock_item_rows_by_hash = &sources.stock_item_rows_by_hash;
    let string_rows = sources.string_rows;
    let stock_collectible_count = sources.stock_collectible_count;
    let collectible_rows = sources.collectible_rows;
    let stock_unlock_count = sources.stock_unlock_count;
    let unlock_rows = sources.unlock_rows;
    let mut resolved = Vec::with_capacity(weapons.len());
    let mut exemplar_cache = CollectionExemplarCache::new(stock_collectible_count);
    for (weapon_ordinal, weapon) in weapons.iter().enumerate() {
        let operation = format!("Resolving {}", weapon.text.name);
        progress.start(&operation);
        let resolved_weapon = (|| -> AuthoringResult<ResolvedWeapon> {
            let identity = weapon.identity;
        // The same answer the whole-table scan gave, from the index the sources already carry.
        if sources
            .stock_item_rows_by_hash
            .contains_key(&identity.item_hash)
        {
            return Err(AuthoringError::InvalidInput(format!(
                "Item hash 0x{:08X} already exists in stock",
                identity.item_hash
            )));
        }
        if contains_u32_at_offset(
            stock_collectibles,
            collectible_rows,
            stock_collectible_count,
            COLLECTIBLE_ROW_SIZE,
            COLLECTIBLE_HASH_OFFSET,
            identity.collectible_hash,
        )? {
            return Err(AuthoringError::InvalidInput(format!(
                "Collectible hash 0x{:08X} already exists in stock",
                identity.collectible_hash
            )));
        }
        if contains_u32_row_key(
            stock_unlocks,
            unlock_rows,
            stock_unlock_count,
            UNLOCK_ROW_SIZE,
            identity.unlock_hash,
        )? {
            return Err(AuthoringError::InvalidInput(format!(
                "Unlock hash 0x{:08X} already exists in stock",
                identity.unlock_hash
            )));
        }
        let DonorItem { item_index: donor_item_index, definition_tag, string_tag } =
            resolve_donor_item(sources, weapon.donor_item_hash, "Donor")?;
        validate_donor_name(sources, string_tag, weapon.expected_donor_name.as_deref(), "Donor")?;
        let donor_item_index_u16 = u16::try_from(donor_item_index)
            .map_err(|_| invalid("Donor item index does not fit 16 bits"))?;
        let donor_collectibles = (0..stock_collectible_count)
            .filter(|index| {
                read_u16(
                    stock_collectibles,
                    collectible_rows
                        + *index * COLLECTIBLE_ROW_SIZE
                        + COLLECTIBLE_ITEM_INDEX_OFFSET,
                )
                .ok()
                    == Some(donor_item_index_u16)
            })
            .collect::<Vec<_>>();
        let donor_collectible_index = match donor_collectibles.as_slice() {
            [] => None,
            [index] => Some(*index),
            _ => return Err(invalid(format!(
                "Donor resolves to {} collectible rows, so its Collections template is ambiguous",
                donor_collectibles.len()
            ))),
        };
        let definition = read_tag(manager, definition_tag, "donor weapon")?;
        let strings = read_tag(manager, string_tag, "donor item-string")?;
        if weapon.kind == ItemKind::Subclass {
            return subclass::resolve(
                sources,
                weapon,
                DonorItem {
                    item_index: donor_item_index,
                    definition_tag,
                    string_tag,
                },
                definition,
                strings,
            );
        }
        if matching_u32_offsets(&definition, weapon.donor_item_hash)
            != [ITEM_DEFINITION_HASH_OFFSET]
            || !matching_u32_offsets(&strings, weapon.donor_item_hash).is_empty()
        {
            return Err(invalid(
                "Donor embeds its item identity at unsupported offsets",
            ));
        }
        if !weapon.kind.is_weapon() {
            return gear::resolve(
                sources,
                weapon,
                placements.gear_page_for(weapon_ordinal),
                DonorItem {
                    item_index: donor_item_index,
                    definition_tag,
                    string_tag,
                },
                donor_collectible_index,
                definition,
                strings,
            );
        }
        if read_i64(&definition, ITEM_ORDINARY_SOCKET_POINTER_OFFSET)? == 0 {
            return Err(invalid(
                "This weapon definition has no socket block for gameplay authoring. Choose another base or use it as an appearance donor.",
            ));
        }
        // Reload-hold element switching is an ordinary socket column plus, for a subset of
        // elements, a private plug variant. Expand it here so every later stage sees plain
        // socket overrides.
        let expanded = variable_damage::expand_spec(weapon, &definition)?;
        let weapon = expanded.as_ref().unwrap_or(weapon);
        // A grafted behavior brings its source weapon's own plugs, because several exotics keep
        // half of the behavior in a perk. Expanded here so later stages see plain socket columns.
        let with_behavior_perks = if weapon.overrides.additional_behaviors.is_empty() {
            None
        } else {
            let mut expanded = weapon.clone();
            expanded.overrides = crate::weapon_behavior::expand_socket_columns(
                &weapon.overrides,
                &weapon_socket_types(&definition)?,
            )?;
            // A borrowed plug's burst change was written for its own weapon type, so each plug
            // that changes the barrel becomes this weapon's own copy, carrying the firing pattern
            // the recipe chose. The types come from the item-type text both weapons carry.
            let item_type = |string_tag| resolve_item_type_name(manager, string_tag).ok();
            expanded.overrides = crate::weapon_behavior::firing_variants(
                &expanded.overrides,
                item_type(string_tag).as_deref(),
                &|entry| {
                    resolve_donor_item(sources, entry.source_item_hash, "Behavior source")
                        .ok()
                        .and_then(|item| item_type(item.string_tag))
                },
                &mut |plug| behavior_plug_firing(sources, plug),
            )?;
            Some(expanded)
        };
        let weapon = with_behavior_perks.as_ref().unwrap_or(weapon);
        let donor_icon_index = read_u16(&strings, ITEM_STRING_ICON_INDEX_OFFSET)?;
        validate_reused_stock_item_icon(stock_item_icons, &strings, donor_icon_index)?;
        let donor_icon_container =
            stock_item_icon_container(stock_item_icons, donor_icon_index)?;
        read_tag(manager, donor_icon_container, "gameplay donor icon container")?;
        // The bucket is the donor's slot. Authoring rewrites a disagreeing equipment slot to match.
        let gameplay_inventory_slot = weapon_inventory_slot(&definition)?;
        weapon_equipment_slot(&definition)?;
        let gameplay_pattern_index = weapon_pattern_index(&definition)?;
        let selected_pattern_index = weapon
            .overrides
            .weapon_pattern_index
            .or(gameplay_pattern_index);
        let runtime_pattern_source = selected_pattern_index
            .map(|row_index| sandbox_pattern_source_at(stock_sandbox_patterns, row_index))
            .transpose()?;
        if weapon.overrides.ammo_type.is_some() && runtime_pattern_source.is_none() {
            return Err(invalid("Native ammo authoring requires an active weapon sandbox pattern"));
        }
        if weapon.overrides.hud_icon.is_some() && runtime_pattern_source.is_none() {
            return Err(invalid("HUD icon authoring requires an active weapon sandbox pattern"));
        }
        let authored_inventory_slot = weapon
            .overrides
            .inventory_slot
            .unwrap_or(gameplay_inventory_slot);
        // Collections classification is independent of gameplay and appearance. In stock,
        // Exotic weapons are grouped by slot; other weapons are grouped by weapon family.
        // Use a real child of the target page as the placement/count exemplar, never move
        // a gameplay donor's count terms into an unrelated hierarchy.
        let authored_rarity = weapon.overrides.rarity.unwrap_or(weapon_rarity(&definition)?);
        let (collection_donor_index, weapon_page, source_acquired_flag, count_selection) = if let Some(page) = placements.page_for_weapon(weapon_ordinal) {
            let flag = collection_unlock_index(stock_collectibles, collectible_rows + page.donor * COLLECTIBLE_ROW_SIZE)?;
            (page.donor, page.index, u16::try_from(flag).map_err(|_| invalid("Collections acquired flag exceeds capacity"))?, SunriseAcquiredPoolSelection::default())
        } else {
        let collection_candidates = resolve_weapon_collection_donor(
            manager, stock_item_table, item_rows, stock_item_count,
            stock_item_strings, string_rows, stock_collectibles, collectible_rows,
            stock_collectible_count, stock_sandbox_patterns, &mut exemplar_cache, donor_collectible_index, &definition, &strings,
            authored_rarity, authored_inventory_slot,
        )?;
        let mut placement_error = None;
        let placement = collection_candidates.into_iter().find_map(|index| {
            let attempt = (|| -> AuthoringResult<_> {
                let parents = template_presentation_parents(stock_nodes, stock_collectibles, index)?;
                let page = donor_weapon_collection_page(stock_nodes, &parents)?;
                let unlock = collection_unlock_index(stock_collectibles, collectible_rows + index * COLLECTIBLE_ROW_SIZE)?;
                if unlock >= stock_unlock_count { return Err(invalid("Collection exemplar unlock is outside the table")); }
                let flag = u16::try_from(unlock).map_err(|_| invalid("Collection acquired flag does not fit 16 bits"))?;
                let counts = classify_sunrise_count_pools(stock_pools, stock_nodes, stock_collectibles,
                    stock_objectives, &parents, page, flag)?;
                Ok((index, page, flag, counts))
            })();
            match attempt {
                Ok(placement) => Some(placement),
                Err(error) => { placement_error = Some(error); None }
            }
        });
        placement
            .ok_or_else(|| placement_error.unwrap_or_else(|| invalid("No compatible Collections placement exemplar")))?
        };
        let (collectible_template_index, source_unlock_index) =
            crate::progression::collectible_clone_template(
                stock_collectibles,
                donor_collectible_index,
                collection_donor_index,
                stock_unlock_count,
            )?;
        let presentation_donor = weapon
            .presentation_donor
            .as_ref()
            .map(|reference| resolve_presentation_donor(sources, reference))
            .transpose()?;
        let damage_carrier_source = if weapon_damage_carrier(&definition)?.family().is_none()
            && weapon
                .overrides
                .modern_damage_type
                .is_some_and(|damage| damage != ModernDamageType::Kinetic)
        {
            resolve_added_damage_carrier_source(
                sources,
                runtime_pattern_source,
                &definition,
                // Kinetic-slot elements use the same proven carrier source as Energy conversion.
                if authored_inventory_slot == WeaponInventorySlot::Kinetic {
                    WeaponInventorySlot::Energy
                } else {
                    authored_inventory_slot
                },
                presentation_donor
                    .as_ref()
                    .map(|presentation| presentation.definition.as_slice()),
            )?
        } else {
            None
        };
        let gear_art_pattern_index = if let Some(presentation) = &presentation_donor {
            Some(
                weapon_pattern_index(&presentation.definition)?.ok_or_else(|| {
                    invalid("Geometry donor has no active gear-art/runtime row")
                })?,
            )
        } else {
            gameplay_pattern_index
        };
        let gear_art_pattern_source = gear_art_pattern_index
            .map(|row_index| sandbox_pattern_source_at(stock_sandbox_patterns, row_index))
            .transpose()?;
        if gear_art_pattern_source.is_none() && runtime_pattern_source.is_some() {
            return Err(invalid(
                "Runtime baseline cannot be attached because the appearance baseline has no gear-art/runtime row",
            ));
        }
        if gear_art_pattern_source.is_some() && runtime_pattern_source.is_none() {
            return Err(invalid(
                "Appearance baseline has a gear-art/runtime row, but the gameplay runtime baseline is disabled",
            ));
        }
        // Geometry from another weapon family is handled one of two ways, and which one
        // decides what the authored pattern row has to describe. Promoting the appearance's
        // rig and animations onto the gameplay runtime keeps the model's own bones, so the
        // row keeps describing the appearance's family. Failing that, the parts are pinned to
        // the gameplay rig at emission (`emission::reskin`) and the row has to describe that
        // family instead, or the client would look up animations the entity no longer has.
        let (gear_art_pattern_source, appearance_rig_donor) =
            match (gear_art_pattern_source, runtime_pattern_source) {
                (Some(gear_art), Some(runtime))
                    if gear_art.weapon_translation_group_hash
                        != runtime.weapon_translation_group_hash =>
                {
                    let entity = |item_hash, description| {
                        resolve_runtime_weapon_entity(
                            manager,
                            stock_sandbox_patterns,
                            &sources.stock_entity_assignments,
                            item_hash,
                            description,
                        )
                        .map(|(_, payload)| payload)
                    };
                    let gameplay = entity(runtime.item_hash, "gameplay runtime entity")?;
                    // Some appearances have gear art but no readable runtime entity. They can
                    // still use the existing pinned-model path.
                    if entity(gear_art.item_hash, "appearance runtime entity")
                        .is_ok_and(|appearance| {
                            rig::presentation_graft_applies(&gameplay, &appearance)
                        })
                    {
                        (Some(gear_art), Some(gear_art.item_hash))
                    } else {
                        (Some(runtime), None)
                    }
                }
                (gear_art, _) => (gear_art, None),
            };
        let icon_donor = weapon
            .icon_donor
            .as_ref()
            .map(|reference| resolve_icon_donor(sources, reference))
            .transpose()?;
        let render_gear_donor = weapon
            .render_gear_donor
            .as_ref()
            .map(|reference| resolve_render_gear_donor(sources, reference))
            .transpose()?;
        let runtime_component_donors = weapon
            .runtime_component_donors
            .iter()
            .map(|reference| resolve_runtime_component_donor(sources, reference))
            .collect::<AuthoringResult<Vec<_>>>()?;
        if let Some(presentation) = &presentation_donor {
            // Family is checked against the catalog, and the native translation group above
            // must match. Collections placement follows authored rarity independently.
            if presentation.inventory_slot != authored_inventory_slot
                && presentation.inventory_slot != gameplay_inventory_slot
                && !matches!(
                    (presentation.inventory_slot, authored_inventory_slot),
                    (WeaponInventorySlot::Kinetic, WeaponInventorySlot::Energy)
                        | (WeaponInventorySlot::Energy, WeaponInventorySlot::Kinetic)
                )
            {
                return Err(invalid(format!(
                    "Geometry donor uses {:?}, but the authored weapon targets {authored_inventory_slot:?}",
                    presentation.inventory_slot
                )));
            }
        }
        let socket_column_indices = resolve_socket_column_indices(
            stock_item_rows_by_hash,
            &definition,
            &weapon.overrides.socket_columns,
        )?;
        let has_authored_shader = selects_authored_shader(sources, &socket_column_indices)?;
        let inherited_icon = presentation_donor.as_ref().map_or(
            ResolvedIconDonor {
                item_index: donor_item_index,
                icon_index: donor_icon_index,
                icon_container: donor_icon_container,
            },
            |presentation| ResolvedIconDonor {
                item_index: presentation.item_index,
                icon_index: presentation.icon_index,
                icon_container: presentation.icon_container,
            },
        );
        let selected_icon = icon_donor.unwrap_or(inherited_icon);
            Ok(ResolvedWeapon {
                weapon: weapon.clone(),
                donor_item_index,
                collection: Some(ResolvedCollection {
                    collectible_display_template_index: donor_collectible_index
                        .unwrap_or(collectible_template_index),
                    collectible_template_index,
                    collection_donor_index,
                    source_unlock_index,
                    source_acquired_flag,
                    weapon_page,
                    count_selection,
                }),
                definition_tag,
                string_tag,
                definition,
                strings,
                // The icon donor supplies the icon, not the item. The dense presentation row is
                // templated from the item this weapon actually is, because that row carries the
                // client's UI cache entry: its presentation type and classification. Templating it
                // from the icon donor made a weapon whose icon came from an ornament present as
                // that ornament, which the inventory grid drew as an empty tile.
                icon_template_item_index: inherited_icon.item_index,
                icon_template_container: inherited_icon.icon_container,
                donor_icon_index: selected_icon.icon_index,
                donor_icon_container: selected_icon.icon_container,
                presentation_donor,
                damage_carrier_source,
                render_gear_donor,
                runtime_component_donors,
                runtime_pattern_source,
                gear_art_pattern_source,
                appearance_rig_donor,
                socket_column_indices,
                has_authored_shader,
                subclass_list: None,
                dye_rows: None,
            })
        })()
        .map_err(|error| weapon.in_recipe(error))?;
        resolved.push(resolved_weapon);
        progress.finish(&operation);
    }

    Ok(resolved)
}

/// Whether any authored socket choice is a real shader. The empty Default Shader alone does not
/// request shader support.
pub(super) fn selects_authored_shader(
    sources: &ProjectSources,
    socket_column_indices: &[Option<ResolvedSocketColumn>],
) -> AuthoringResult<bool> {
    for item in socket_column_indices
        .iter()
        .flatten()
        .flat_map(|column| &column.choices)
        .copied()
        .collect::<BTreeSet<_>>()
    {
        let row = sources.item_rows + usize::from(item) * ITEM_ROW_SIZE;
        if read_u32(&sources.stock_item_table, row)? == 0xFD36_8D30 {
            continue;
        }
        let plug = read_tag(
            &sources.manager,
            TagHash(read_u32(&sources.stock_item_table, row + 16)?),
            "authored shader choice",
        )?;
        if crate::plug_classification::PlugClassification::category(&plug).ok()
            == Some(2_973_005_342)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// The barrel firing changes each stock perk on a borrowed behavior plug makes.
fn behavior_plug_firing(
    sources: &ProjectSources,
    plug: u32,
) -> AuthoringResult<Vec<crate::weapon_behavior::PerkFiring>> {
    let item = resolve_donor_item(sources, plug, "Behavior plug")?;
    let definition = read_tag(
        &sources.manager,
        item.definition_tag,
        "behavior plug definition",
    )?;
    let mut firing = Vec::new();
    for perk in weapon_sandbox_perks(&definition)? {
        if let Some(perk) =
            crate::weapon_behavior::perk_firing(&sources.manager, &sources.globals_data, perk)?
        {
            firing.push(perk);
        }
    }
    Ok(firing)
}
