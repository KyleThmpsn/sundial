//! Authoring for equippable items that are not weapons: armor, Sparrows, Ships, Ghost Shells and
//! emblems.
//!
//! A gear recipe keeps its base item's slot and runtime, with an optional armor class. Imported recipes supply their
//! own art variants, geometry and materials. Ordinary recipes keep the base appearance. Both change identity,
//! text, rarity, investment stats, sockets, custom perks and icon, which are native item fields
//! every equippable definition shares with weapons, and it lands on a runtime-branded Collections
//! page. Sockets are added, removed and given roles the way a weapon's are. A custom perk is a
//! private plug like a weapon's, and its runtime is its own, so it needs no item runtime. An emblem
//! has no stats or sockets, and its nameplate images are `crate::emblem`'s.
use super::resolve::ResolvedWeapon;
use super::sources::ProjectSources;
use super::*;
mod collections;
pub(super) mod runtime;

fn imported(spec: &WeaponCloneSpec) -> bool {
    #[cfg(feature = "d2-model-importer")]
    {
        spec.overrides.imported_graph.is_some()
    }
    #[cfg(not(feature = "d2-model-importer"))]
    {
        let _ = spec;
        false
    }
}

/// Exotic armor's unique-equip label and companion hash, read from stock exotic armor
/// (Celestial Nighthawk, Wormgod Caress, Ophidian Aspect). One exotic piece equips at a time.
const EXOTIC_ARMOR_EQUIPMENT_LABEL: (u32, u32) = (0x1ED9_4273, 0x2D5D_6C45);
/// FNV-1 of the empty label: no unique-equip group. Legendary armor and every Ghost Shell,
/// Sparrow and Ship carry it, exotic ones included.
const NO_EQUIPMENT_LABEL: (u32, u32) = (0x811C_9DC5, 0);
const EQUIPMENT_UNIQUE_LABEL_OFFSET: usize = 0x10;
/// Armor Energy Upgrade sockets. Their plug sets the energy type and capacity, so they keep their
/// role: the Energy Type and Energy Capacity controls own them, and a piece carries one of each.
pub(crate) const ENERGY_SOCKET_TYPES: [u16; 2] = [678, 679];

/// Rejects every override the gear path does not compile, so a recipe never carries a setting
/// that silently does nothing.
pub(super) fn validate_spec(spec: &WeaponCloneSpec) -> AuthoringResult<()> {
    match unsupported_setting(spec) {
        Some(setting) => Err(invalid(format!(
            "{} recipes cannot set {setting}",
            spec.kind.label()
        ))),
        None => Ok(()),
    }
}

fn unsupported_setting(spec: &WeaponCloneSpec) -> Option<&'static str> {
    let overrides = &spec.overrides;
    if spec.kind == ItemKind::Mod
        && let Some(setting) = unsupported_mod_setting(spec)
    {
        return Some(setting);
    }
    if spec.kind == ItemKind::Subclass {
        if let Some(setting) = crate::subclass::authoring::unsupported_setting(overrides) {
            return Some(setting);
        }
    } else if overrides.subclass_abilities.is_some() {
        return Some("subclass abilities");
    }
    if spec.kind != ItemKind::Shader
        && !(overrides.dye_edits.is_empty() && overrides.dye_texture_edits.is_empty())
    {
        return Some("dyes");
    }
    #[cfg(feature = "d2-model-importer")]
    if overrides.imported_graph.is_some() && crate::imported::kind(spec.kind).is_none() {
        return Some("an imported model");
    }
    [
        (spec.presentation_donor.is_some(), "an appearance donor"),
        (overrides.animation_donor.is_some(), "an animation donor"),
        (overrides.type_marker_donor.is_some(), "type markers"),
        (!overrides.marker_offsets.is_empty(), "moved markers"),
        (
            !overrides.component_splices.is_empty(),
            "components from other weapons",
        ),
        (overrides.held_offset.is_some(), "a moved model"),
        (spec.render_gear_donor.is_some(), "a render donor"),
        (
            !spec.runtime_component_donors.is_empty(),
            "runtime components",
        ),
        (
            overrides.collection_destination.is_some(),
            "a weapon Collections page",
        ),
        (overrides.hud_icon.is_some(), "an ammo HUD icon"),
        (overrides.base_sandbox_perks.is_some(), "base perks"),
        (overrides.trait_indices.is_some(), "item traits"),
        (overrides.max_stack_size.is_some(), "a stack size"),
        (
            overrides.socket_entry_list_index.is_some(),
            "a socket-entry list",
        ),
        (overrides.plug_category_hash.is_some(), "a plug category"),
        (overrides.roll_set_index.is_some(), "a roll set"),
        (overrides.linked_plug_index.is_some(), "a linked plug"),
        (overrides.inventory_slot.is_some(), "a weapon slot"),
        (overrides.ammo_type.is_some(), "an ammo type"),
        (overrides.modern_damage_type.is_some(), "a damage type"),
        (overrides.variable_damage.is_some(), "variable damage"),
        (
            !overrides.additional_behaviors.is_empty(),
            "weapon behaviors",
        ),
        (
            overrides.behavior_projectile_speed.is_some(),
            "a projectile speed",
        ),
        (overrides.projectile.is_some(), "projectile values"),
        (overrides.barrel.is_some(), "Barrel settings"),
        (
            overrides.behavior_firing != crate::weapon::behavior::BehaviorFiring::default(),
            "a firing pattern",
        ),
        (
            overrides.power_cap_group.is_some() || overrides.power_cap_groups.is_some(),
            "a power cap",
        ),
        (overrides.weapon_pattern_index.is_some(), "a weapon pattern"),
        (
            overrides.stat_group_index.is_some() || overrides.custom_stat_group.is_some(),
            "a stat display group",
        ),
        (overrides.art_arrangements.is_some(), "geometry rows"),
        (
            overrides.render_dye_rows.is_some() && spec.kind != ItemKind::Shader,
            "dye rows",
        ),
        // A shader is a plug with no lore tab, stats or sockets of its own. An emblem has none of
        // them either.
        (
            !spec.kind.has_lore_tab() && (overrides.lore.is_some() || overrides.remove_lore),
            "a lore tab",
        ),
        (
            matches!(spec.kind, ItemKind::Shader | ItemKind::Emblem)
                && (!overrides.investment_stats.is_empty()
                    || !overrides.removed_investment_stats.is_empty()),
            "stats",
        ),
        (
            matches!(spec.kind, ItemKind::Shader | ItemKind::Emblem)
                && (!overrides.socket_columns.is_empty()
                    || !overrides.socket_plug_variants.is_empty()),
            "sockets or custom perks",
        ),
        (!overrides.runtime_values.is_empty(), "runtime values"),
        (
            !overrides.runtime_resource_patches.is_empty(),
            "runtime patches",
        ),
        (!overrides.raw_payload_patches.is_empty(), "raw patches"),
    ]
    .into_iter()
    .find_map(|(set, setting)| set.then_some(setting))
}

/// A mod is its one custom perk, built on the perk's template plug and offered in every socket of
/// its type. Its text, icon, type and stats are the perk's, so the item carries none of its own.
fn unsupported_mod_setting(spec: &WeaponCloneSpec) -> Option<&'static str> {
    let overrides = &spec.overrides;
    let [perk] = overrides.socket_plug_variants.as_slice() else {
        return Some(if overrides.socket_plug_variants.is_empty() {
            "no custom perk"
        } else {
            "more than one custom perk"
        });
    };
    [
        (
            perk.socket_index != 0 || perk.choice_index != 0,
            "a socket position",
        ),
        (
            perk.source_plug_hash != spec.donor_item_hash,
            "a base other than its perk's template",
        ),
        (!perk.offer_everywhere, "a perk not offered everywhere"),
        (!overrides.socket_columns.is_empty(), "sockets"),
        (
            !overrides.investment_stats.is_empty()
                || !overrides.removed_investment_stats.is_empty(),
            "item stats",
        ),
        (overrides.rarity.is_some(), "a rarity"),
        (spec.icon_donor.is_some(), "an icon donor"),
    ]
    .into_iter()
    .find_map(|(set, setting)| set.then_some(setting))
}

/// A mod's perk stats follow the custom perk rule, and its type must be one some stock socket
/// offers, or the mod would join no plug set.
fn validate_mod_against_catalog(
    catalog: &InvestmentCatalog,
    spec: &WeaponCloneSpec,
) -> AuthoringResult<()> {
    let [perk] = spec.overrides.socket_plug_variants.as_slice() else {
        return Err(invalid("A mod holds exactly one custom perk"));
    };
    let perk_stats = catalog.perk_stat_choices();
    let source_stats = catalog.item_stat_contributions(perk.source_plug_hash);
    for &(index, _) in &perk.investment_stats {
        let declared = (perk.replace_effects
            && perk_stats.iter().any(|stat| stat.definition_index == index))
            || source_stats
                .iter()
                .any(|stat| stat.definition_index == index);
        if !declared {
            return Err(invalid(format!(
                "Mod stat {index} is not declared by its template plug"
            )));
        }
    }
    let offered = perk
        .classification_donor_hash
        .unwrap_or(perk.source_plug_hash);
    if catalog
        .reusable_set_counts()
        .get(&offered)
        .is_none_or(|&sets| sets == 0)
    {
        return Err(invalid(format!(
            "No stock socket offers {}, so the mod would appear nowhere. Choose another type.",
            catalog.plug_label(offered, true)
        )));
    }
    Ok(())
}

/// Checks a gear recipe's stats, sockets and custom perks against the installed catalog.
pub(super) fn validate_against_catalog(
    catalog: &InvestmentCatalog,
    spec: &WeaponCloneSpec,
    reusable_plug_set_count: usize,
) -> AuthoringResult<()> {
    if spec.kind == ItemKind::Subclass {
        return crate::subclass::authoring::validate_sources(
            catalog,
            spec.donor_item_hash,
            spec.overrides.subclass_abilities.as_ref(),
        );
    }
    if spec.kind == ItemKind::Mod {
        return validate_mod_against_catalog(catalog, spec);
    }
    if spec.kind == ItemKind::Shader {
        if !catalog.is_shader(spec.donor_item_hash) {
            return Err(invalid(format!(
                "Base item 0x{:08X} is not an installed shader",
                spec.donor_item_hash
            )));
        }
        #[cfg(feature = "d2-model-importer")]
        if let Some(reference) = &spec.overrides.imported_graph {
            reference
                .validate_shader()
                .map_err(|e| invalid(e.to_string()))?;
        }
        return spec
            .overrides
            .render_dye_rows
            .as_ref()
            .map_or(Ok(()), |arrays| {
                validate_shader_dye_rows(catalog, spec, arrays)
            });
    }
    let donor = catalog
        .gear_donor(spec.donor_item_hash)
        .filter(|donor| ItemKind::from_bucket_hash(donor.summary.bucket_hash) == Some(spec.kind))
        .ok_or_else(|| {
            invalid(format!(
                "Base item 0x{:08X} is not an installed {}",
                spec.donor_item_hash,
                spec.kind.label()
            ))
        })?;
    #[cfg(feature = "d2-model-importer")]
    if let Some(reference) = &spec.overrides.imported_graph {
        reference
            .validate_reusable(
                crate::imported::kind(spec.kind)
                    .ok_or_else(|| invalid("This gear kind cannot use imported models"))?,
            )
            .map_err(|error| invalid(error.to_string()))?;
    }
    for (part, hash) in spec
        .overrides
        .nameplate
        .iter()
        .flat_map(crate::emblem::Nameplate::emblems)
    {
        let hash = hash
            .parse_u32()
            .map_err(|error| invalid(error.to_string()))?;
        if catalog.gear_donor(hash).is_none_or(|emblem| {
            ItemKind::from_bucket_hash(emblem.summary.bucket_hash) != Some(ItemKind::Emblem)
        }) {
            return Err(invalid(format!(
                "{} item 0x{hash:08X} is not an installed emblem",
                part.label()
            )));
        }
    }
    let mut diagnostics = validate_stat_overrides(&donor, &spec.overrides.investment_stats);
    // A custom perk that replaces its template's effects may carry any installed stat, the same
    // rule as a weapon's. Otherwise its stats must be ones the plug or the base item declares.
    let perk_stats = catalog.perk_stat_choices();
    for variant in &spec.overrides.socket_plug_variants {
        let source_stats = catalog.item_stat_contributions(variant.source_plug_hash);
        for &(index, _) in &variant.investment_stats {
            let declared = (variant.replace_effects
                && perk_stats.iter().any(|stat| stat.definition_index == index))
                || source_stats
                    .iter()
                    .chain(&donor.investment_stats)
                    .any(|stat| stat.definition_index == index);
            if !declared {
                return Err(invalid(format!(
                    "Custom perk stat {index} is not declared by its source plug or the base item"
                )));
            }
        }
    }
    for (lane, column) in spec.overrides.socket_columns.iter().enumerate() {
        let Some(column) = column else {
            continue;
        };
        for (set, index) in [
            ("reusable", column.reusable_plug_set_index),
            ("randomized", column.randomized_plug_set_index),
        ] {
            if let Some(index) = index
                && usize::from(index) >= reusable_plug_set_count
            {
                return Err(invalid(format!(
                    "{} {:?} socket {lane} selects {set} plug set {index}, but the installed table has {reusable_plug_set_count} rows",
                    spec.kind.label(),
                    spec.text.name
                )));
            }
        }
    }
    for definition_index in &spec.overrides.removed_investment_stats {
        if !donor
            .investment_stats
            .iter()
            .any(|stat| stat.definition_index == *definition_index)
        {
            return Err(invalid(format!(
                "The base item has no stat {definition_index} to remove"
            )));
        }
    }
    if !spec.overrides.socket_columns.is_empty() || !spec.overrides.socket_plug_variants.is_empty()
    {
        let overrides = spec
            .overrides
            .socket_columns
            .iter()
            .map(|column| column.as_ref().map(|column| column.choices.clone()))
            .collect::<Vec<_>>();
        let socket_types = spec
            .overrides
            .socket_columns
            .iter()
            .map(|column| column.as_ref().and_then(|column| column.socket_type))
            .collect::<Vec<_>>();
        validate_energy_sockets(spec, &donor, &socket_types)?;
        super::validation::validate_known_socket_types(catalog, spec, &socket_types)?;
        let supported = catalog
            .supported_plug_sets(spec.donor_item_hash, &socket_types)
            .map_err(|error| invalid(format!("Could not decode compatible plugs: {error}")))?
            .into_iter()
            .map(|set| SupportedPlugSet {
                socket_index: set.socket_index,
                plug_hashes: set.plug_hashes,
            })
            .collect::<Vec<_>>();
        diagnostics.extend(validate_socket_column_overrides_with_variants(
            &donor,
            &overrides,
            &socket_types,
            &supported,
            &spec.overrides.socket_plug_variants,
        ));
    }
    diagnostics.retain(crate::capabilities::AuthoringDiagnostic::is_build_blocking);
    if diagnostics.is_empty() {
        return Ok(());
    }
    Err(invalid(format!(
        "{} {:?} has settings its base item does not support: {}",
        spec.kind.label(),
        spec.text.name,
        diagnostics
            .into_iter()
            .map(|diagnostic| diagnostic.message)
            .collect::<Vec<_>>()
            .join("; ")
    )))
}

/// The energy sockets keep their role, and no other socket takes an energy role.
fn validate_energy_sockets(
    spec: &WeaponCloneSpec,
    donor: &sundial::investment::WeaponDonor,
    socket_types: &[Option<u16>],
) -> AuthoringResult<()> {
    for (lane, socket_type) in socket_types.iter().copied().enumerate() {
        let Some(socket_type) = socket_type else {
            continue;
        };
        let base = donor.sockets.get(lane).map(|socket| socket.socket_type);
        let name = || {
            format!(
                "{} {:?} ({}) socket {lane}",
                spec.kind.label(),
                spec.text.name,
                spec.namespace
            )
        };
        if base.is_some_and(|base| ENERGY_SOCKET_TYPES.contains(&base)) && base != Some(socket_type)
        {
            return Err(invalid(format!(
                "{} holds the armor energy, which keeps its socket",
                name()
            )));
        }
        if ENERGY_SOCKET_TYPES.contains(&socket_type) && base != Some(socket_type) {
            return Err(invalid(format!(
                "{} cannot take an armor energy role",
                name()
            )));
        }
    }
    Ok(())
}

/// The native inventory-bucket byte and equipment-slot value of a gear definition, checked
/// against the recipe's kind.
pub(super) fn native_slot(definition: &[u8], kind: ItemKind) -> AuthoringResult<(u8, u16)> {
    let bucket = read_u8(definition, ITEM_INVENTORY_SLOT_OFFSET)?;
    if kind == ItemKind::Shader {
        // A shader is a profile plug with no equipment block.
        if !kind.native_slots().iter().any(|(slot, _)| *slot == bucket)
            || read_i64(definition, ITEM_EQUIPMENT_BLOCK_POINTER_OFFSET)? != 0
        {
            return Err(invalid(format!(
                "The base item uses inventory bucket {bucket}, which is not a shader"
            )));
        }
        return Ok((bucket, 0));
    }
    let block = relative_target(definition, ITEM_EQUIPMENT_BLOCK_POINTER_OFFSET)?;
    if block < 4 || read_u32(definition, block - 4)? != ITEM_EQUIPMENT_BLOCK_CLASS {
        return Err(invalid(format!(
            "The base {} has no recognized equipment block",
            kind.label()
        )));
    }
    let equipment = read_u16(definition, block + ITEM_EQUIPMENT_SLOT_OFFSET)?;
    if read_u16(definition, block + ITEM_EQUIPMENT_SLOT_SENTINEL_OFFSET)? != u16::MAX {
        return Err(invalid(
            "The base item's equipment slot is not followed by the native FFFF sentinel",
        ));
    }
    if kind.is_weapon() || !kind.native_slots().contains(&(bucket, equipment)) {
        return Err(invalid(format!(
            "The base item uses inventory bucket {bucket} and equipment slot {equipment}, which is not {}",
            kind.label()
        )));
    }
    Ok((bucket, equipment))
}

pub(super) fn rarity(definition: &[u8]) -> AuthoringResult<AuthoredWeaponRarity> {
    AuthoredWeaponRarity::from_package_value(read_u8(definition, ITEM_RARITY_OFFSET)?)
}

fn equipment_label(definition: &[u8]) -> AuthoringResult<(u32, u32)> {
    let block = relative_target(definition, ITEM_EQUIPMENT_BLOCK_POINTER_OFFSET)?;
    Ok((
        read_u32(definition, block + EQUIPMENT_UNIQUE_LABEL_OFFSET)?,
        read_u32(definition, block + EQUIPMENT_UNIQUE_LABEL_OFFSET + 4)?,
    ))
}

/// Writes the rarity byte and the unique-equip label that goes with it. Exotic armor joins the
/// one-exotic-piece group; nothing else of these kinds carries a group in stock.
fn set_rarity(
    definition: &mut [u8],
    kind: ItemKind,
    authored: AuthoredWeaponRarity,
) -> AuthoringResult<()> {
    let before = definition.to_vec();
    write_bytes(definition, ITEM_RARITY_OFFSET, &[authored.package_value()])?;
    if kind == ItemKind::Shader {
        // No equipment block, so no unique-equip label goes with a shader's rarity.
        return if rarity(definition)? == authored {
            Ok(())
        } else {
            Err(validation("Authored shader did not retain its rarity"))
        };
    }
    let original = equipment_label(definition)?;
    let label = if kind == ItemKind::Armor && authored == AuthoredWeaponRarity::Exotic {
        EXOTIC_ARMOR_EQUIPMENT_LABEL
    } else if original.0 == EXOTIC_ARMOR_EQUIPMENT_LABEL.0 || original.0 == NO_EQUIPMENT_LABEL.0 {
        NO_EQUIPMENT_LABEL
    } else {
        // Keep an unrelated stock unique-equip group rather than removing every restriction.
        original
    };
    let block = relative_target(definition, ITEM_EQUIPMENT_BLOCK_POINTER_OFFSET)?;
    write_u32(definition, block + EQUIPMENT_UNIQUE_LABEL_OFFSET, label.0)?;
    write_u32(
        definition,
        block + EQUIPMENT_UNIQUE_LABEL_OFFSET + 4,
        label.1,
    )?;
    if rarity(definition)? != authored || equipment_label(definition)? != label {
        return Err(validation(
            "Authored gear did not retain its rarity and label",
        ));
    }
    let mut normalized = definition.to_vec();
    write_bytes(
        &mut normalized,
        ITEM_RARITY_OFFSET,
        &before[ITEM_RARITY_OFFSET..=ITEM_RARITY_OFFSET],
    )?;
    write_u32(
        &mut normalized,
        block + EQUIPMENT_UNIQUE_LABEL_OFFSET,
        original.0,
    )?;
    write_u32(
        &mut normalized,
        block + EQUIPMENT_UNIQUE_LABEL_OFFSET + 4,
        original.1,
    )?;
    if normalized != before {
        return Err(validation(
            "Gear rarity authoring changed bytes outside the rarity byte and equipment label",
        ));
    }
    Ok(())
}

fn client_classification(
    strings: &[u8],
) -> AuthoringResult<[u8; ITEM_STRING_CLIENT_CLASSIFICATION_SIZE]> {
    crate::tag_payload::read_array(strings, ITEM_STRING_CLIENT_CLASSIFICATION_OFFSET)
}

/// A gear recipe's base as the build read it: its item rows, its Collections row when it has
/// one, and its definition and string payloads.
pub(super) struct Base {
    pub(super) donor: DonorItem,
    pub(super) collectible_index: Option<usize>,
    pub(super) definition: Vec<u8>,
    pub(super) strings: Vec<u8>,
}

/// Resolve gear onto branded pages, ordinary armor sets or native Exotic class pages.
pub(super) fn resolve(
    sources: &ProjectSources,
    spec: &WeaponCloneSpec,
    choice_rows: &BTreeMap<u32, Vec<usize>>,
    gear_pages: &[&super::placements::GearPagePlan],
    classes: crate::collection::Classes,
    base: Base,
) -> AuthoringResult<ResolvedWeapon> {
    let Base {
        donor,
        collectible_index: donor_collectible_index,
        definition,
        strings,
    } = base;
    let expanded = crate::vehicle::perks::expand(
        &sources.manager,
        spec,
        &definition,
        &sources.stock_item_table,
        sources.item_rows,
        &sources.stock_item_rows_by_hash,
    )?;
    let spec = expanded.as_ref().unwrap_or(spec);
    let (bucket, _) = native_slot(&definition, spec.kind)?;
    let base_rarity = rarity(&definition)?;
    // Ordinary armor keeps its base's placement, which Exotic decides. Imported items select a
    // compatible Collections exemplar independently of the native runtime template. Other gear
    // goes on the runtime's own page whatever its rarity, so any rarity suits any base.
    if let Some(authored) = spec.overrides.rarity
        && spec.kind == ItemKind::Armor
        && !imported(spec)
        && (authored == AuthoredWeaponRarity::Exotic)
            != (base_rarity == AuthoredWeaponRarity::Exotic)
    {
        return Err(invalid(
            "Armor can be Exotic only on an Exotic base, which places it under Exotics.",
        ));
    }
    client_classification(&strings)?;
    if (!spec.overrides.socket_columns.is_empty()
        || !spec.overrides.socket_plug_variants.is_empty())
        && read_i64(&definition, ITEM_ORDINARY_SOCKET_POINTER_OFFSET)? == 0
    {
        return Err(invalid(
            "This base item has no sockets to change. Choose another base.",
        ));
    }
    let base_collectible_index = match donor_collectible_index {
        Some(index) => index,
        None => stand_in_collectible(sources, &strings)?.ok_or_else(|| {
            invalid(
                "Neither this base item nor another version of it is in Collections. Choose a base from Collections.",
            )
        })?,
    };
    let collection_donor_index =
        collections::select(sources, spec, &definition, &strings, base_collectible_index)?;
    let parents = template_presentation_parents(
        &sources.stock_nodes,
        &sources.stock_collectibles,
        collection_donor_index,
    )?;
    let base_page = crate::progression::gear_collection_page(&sources.stock_nodes, &parents)?;
    let exemplar = unlock_exemplar(sources, collection_donor_index, &parents, base_page, bucket)?;
    let source_acquired_flag = exemplar.flag;
    // Ordinary armor joins a set in every supported class. Exotic armor keeps only its native page.
    let page = gear_pages.first().map_or(base_page, |page| page.index);
    let exotic_armor = spec.kind == ItemKind::Armor
        && spec.overrides.rarity.unwrap_or(base_rarity) == AuthoredWeaponRarity::Exotic;
    let pages = if exotic_armor {
        super::armor::exotic_pages(sources, classes)?
    } else {
        crate::collection::CollectionPages::new(
            std::iter::once(page).chain(gear_pages.iter().skip(1).map(|page| page.index)),
        )?
    };
    // A page whose progress counts cannot be traced keeps the direct count, which still
    // acquires the item and only leaves the page total unchanged. So does an unlock borrowed
    // from another page, since this page's counts never read it.
    let count_selection =
        if exemplar.on_page && gear_pages.is_empty() && pages.iter().eq([base_page]) {
            classify_sunrise_count_pools(
                &sources.stock_pools,
                &sources.stock_nodes,
                &sources.stock_collectibles,
                &sources.stock_objectives,
                &parents,
                page,
                source_acquired_flag,
            )
            .unwrap_or_default()
        } else {
            SunriseAcquiredPoolSelection::default()
        };
    let (collectible_template_index, source_unlock_index) =
        crate::progression::collectible_clone_template(
            &sources.stock_collectibles,
            Some(collection_donor_index),
            exemplar.index,
            sources.stock_unlock_count,
        )?;
    let donor_icon_index = read_u16(&strings, ITEM_STRING_ICON_INDEX_OFFSET)?;
    validate_reused_stock_item_icon(&sources.stock_item_icons, &strings, donor_icon_index)?;
    let donor_icon_container =
        stock_item_icon_container(&sources.stock_item_icons, donor_icon_index)?;
    read_tag(
        &sources.manager,
        donor_icon_container,
        "base item icon container",
    )?;
    let icon_donor = spec
        .icon_donor
        .as_ref()
        .map(|reference| resolve_icon_donor(sources, reference))
        .transpose()?;
    let nameplate = spec
        .overrides
        .nameplate
        .as_ref()
        .map(|nameplate| {
            let row = read_u16(&strings, ITEM_STRING_SECONDARY_ICON_INDEX_OFFSET)?;
            let base = (
                row,
                stock_item_icon_container(&sources.stock_item_icons, row)?,
            );
            crate::emblem::resolve(&sources.manager, base, nameplate, &|hash| {
                emblem_nameplate(sources, hash)
            })
        })
        .transpose()?;
    let socket_column_indices = if spec.overrides.socket_columns.is_empty() {
        Vec::new()
    } else {
        let mut columns = resolve_socket_column_indices(
            choice_rows,
            &definition,
            &spec.overrides.socket_columns,
        )?;
        keep_base_plug_sets(&definition, &mut columns)?;
        columns
    };
    let has_authored_shader =
        super::resolve::selects_authored_shader(sources, &socket_column_indices)?;
    let selected_icon = icon_donor.unwrap_or(ResolvedIconDonor {
        item_index: donor.item_index,
        icon_index: donor_icon_index,
        icon_container: donor_icon_container,
    });
    let runtime_pattern_source = runtime::source(sources, spec, &definition)?;
    Ok(ResolvedWeapon {
        weapon: spec.clone(),
        donor_item_index: donor.item_index,
        collection: Some(super::resolve::ResolvedCollection {
            collectible_display_template_index: collection_donor_index,
            collectible_template_index,
            collection_donor_index,
            source_unlock_index,
            source_acquired_flag,
            pages,
            classes,
            count_selection,
        }),
        definition_tag: donor.definition_tag,
        string_tag: donor.string_tag,
        definition,
        strings,
        icon_template_item_index: donor.item_index,
        icon_template_container: donor_icon_container,
        donor_icon_index: selected_icon.icon_index,
        donor_icon_container: selected_icon.icon_container,
        presentation_donor: None,
        damage_carrier_source: None,
        render_gear_donor: None,
        runtime_component_donors: Vec::new(),
        component_splice_sources: Vec::new(),
        runtime_pattern_source,
        gear_art_pattern_source: runtime_pattern_source,
        appearance_rig_donor: None,
        pinned_appearance: None,
        animation_pattern_source: None,
        animation_action_sources: Vec::new(),
        type_marker_pattern_source: None,
        socket_column_indices,
        has_authored_shader,
        subclass_list: None,
        dye_rows: None,
        nameplate,
        screen_art: None,
        drawn_icon: None,
    })
}

/// Another emblem's nameplate container, which an emblem's nameplate images can come from.
fn emblem_nameplate(sources: &ProjectSources, item_hash: u32) -> AuthoringResult<TagHash> {
    let DonorItem {
        definition_tag,
        string_tag,
        ..
    } = resolve_donor_item(sources, item_hash, "Nameplate emblem")?;
    let definition = read_tag(&sources.manager, definition_tag, "nameplate emblem")?;
    native_slot(&definition, ItemKind::Emblem)
        .map_err(|_| invalid(format!("Item 0x{item_hash:08X} is not an emblem")))?;
    let strings = read_tag(&sources.manager, string_tag, "nameplate emblem item-string")?;
    stock_item_icon_container(
        &sources.stock_item_icons,
        read_u16(&strings, ITEM_STRING_SECONDARY_ICON_INDEX_OFFSET)?,
    )
}

/// A gear socket keeps its base's plug set beside its authored choices, so mods, shaders and
/// energy upgrades still apply in game. A column that names a plug set of its own keeps that one.
/// A removed socket or one given another role takes none: the base set holds the old role's plugs.
fn keep_base_plug_sets(
    definition: &[u8],
    columns: &mut [Option<ResolvedSocketColumn>],
) -> AuthoringResult<()> {
    let resource = relative_target(definition, ITEM_ORDINARY_SOCKET_POINTER_OFFSET)?;
    let (count, _, rows, _) = array_at(definition, resource)?;
    for (lane, column) in columns.iter_mut().enumerate().take(count) {
        let row = rows + lane * ITEM_ORDINARY_SOCKET_ROW_SIZE;
        let base_type = read_u16(definition, row)?;
        if let Some(column) = column
            && column.reusable_plug_set_index.is_none()
            && column
                .socket_type
                .is_none_or(|socket_type| socket_type == base_type)
        {
            let set = read_u16(
                definition,
                row + ITEM_ORDINARY_SOCKET_REUSABLE_PLUG_SET_OFFSET,
            )?;
            column.reusable_plug_set_index = (set != u16::MAX).then_some(set);
        }
    }
    Ok(())
}

/// A shader's dye rows keep the stock shape: the same rows in the custom and default arrays, none
/// locked, each key once, and every dye one the installed table holds.
fn validate_shader_dye_rows(
    catalog: &InvestmentCatalog,
    spec: &WeaponCloneSpec,
    arrays: &[Vec<WeaponDyeReferenceOverride>; 3],
) -> AuthoringResult<()> {
    let [custom, default, locked] = arrays;
    let mut keys = BTreeSet::new();
    let shaped = custom == default
        && locked.is_empty()
        && custom.len() <= 32
        && custom.iter().all(|row| keys.insert(row.channel_index));
    if !shaped {
        return Err(invalid(format!(
            "Shader {:?} needs the same dye rows in its custom and default arrays, each channel once",
            spec.text.name
        )));
    }
    #[cfg(feature = "d2-model-importer")]
    let source = spec
        .overrides
        .imported_graph
        .as_ref()
        .map(|g| -> AuthoringResult<BTreeSet<i8>> {
            let graph: serde_json::Value = serde_json::from_slice(
                &fs::read(g.directory.join("asset-graph.json"))
                    .map_err(|e| invalid(e.to_string()))?,
            )
            .map_err(|e| invalid(e.to_string()))?;
            graph["dyes"]
                .as_array()
                .ok_or_else(|| invalid("Source channels missing"))?
                .iter()
                .map(|d| {
                    i8::try_from(
                        d["channel"]
                            .as_i64()
                            .ok_or_else(|| invalid("Source channel missing"))?,
                    )
                    .map_err(|e| invalid(e.to_string()))
                })
                .collect()
        })
        .transpose()?;
    let base = catalog.item_render_dye_rows(spec.donor_item_hash);
    let base_keys = base[0]
        .iter()
        .map(|row| row.channel_index)
        .collect::<BTreeSet<_>>();
    #[cfg(feature = "d2-model-importer")]
    let base_keys = if let Some(source) = source {
        for row in custom {
            if let Some(channel) = crate::shader::source_channel(row.dye_reference_index)
                && channel != row.channel_index
            {
                return Err(invalid("Source dye row names another source channel"));
            }
        }
        source
    } else {
        base_keys
    };
    if keys != base_keys {
        return Err(invalid(format!(
            "Shader {:?} must keep its material channels",
            spec.text.name
        )));
    }
    Ok(())
}

/// A shader's translation block. It has no art rows, so the weapon topology check does not apply.
fn shader_translation_root(definition: &[u8]) -> AuthoringResult<usize> {
    let root = relative_target(definition, ITEM_TRANSLATION_BLOCK_POINTER_OFFSET)?;
    if root < 4
        || root + ITEM_TRANSLATION_BLOCK_SIZE > definition.len()
        || read_u32(definition, root - 4)? != ITEM_TRANSLATION_BLOCK_CLASS
    {
        return Err(invalid("The base item has no 0x808077AF translation block"));
    }
    Ok(root)
}

/// The three dye arrays of a shader's translation block, empty where a descriptor is zero.
pub(super) fn shader_dye_rows(
    definition: &[u8],
) -> AuthoringResult<[Vec<WeaponDyeReferenceOverride>; 3]> {
    let root = shader_translation_root(definition)?;
    let mut arrays: [Vec<WeaponDyeReferenceOverride>; 3] = Default::default();
    for (array, offset) in TRANSLATION_DYE_DESCRIPTOR_OFFSETS.into_iter().enumerate() {
        let descriptor = root + offset;
        if definition
            .get(descriptor..descriptor + 16)
            .is_some_and(|bytes| bytes == [0; 16])
        {
            continue;
        }
        let (count, _, rows, class) = array_at(definition, descriptor)?;
        if class != TRANSLATION_DYE_ROW_CLASS {
            return Err(invalid("A shader dye array has the wrong row class"));
        }
        for index in 0..count {
            let row = rows + index * TRANSLATION_DYE_ROW_SIZE;
            arrays[array].push(WeaponDyeReferenceOverride {
                channel_index: i8::from_le_bytes([read_u8(definition, row)?]),
                dye_reference_index: read_u16(definition, row + 2)?,
            });
        }
    }
    Ok(arrays)
}

/// Writes a shader's dye rows into its translation block and reads them back.
pub(super) fn set_shader_dye_rows(
    definition: &mut Vec<u8>,
    arrays: &[Vec<WeaponDyeReferenceOverride>; 3],
) -> AuthoringResult<()> {
    let root = shader_translation_root(definition)?;
    for (array, rows) in arrays.iter().enumerate() {
        let mut encoded = Vec::with_capacity(rows.len() * TRANSLATION_DYE_ROW_SIZE);
        for row in rows {
            encoded.extend_from_slice(&row.channel_index.to_le_bytes());
            encoded.push(0);
            encoded.extend_from_slice(&row.dye_reference_index.to_le_bytes());
        }
        replace_translation_array(
            definition,
            root + TRANSLATION_DYE_DESCRIPTOR_OFFSETS[array],
            TRANSLATION_DYE_ROW_CLASS,
            &encoded,
            TRANSLATION_DYE_ROW_SIZE,
        )?;
    }
    if shader_dye_rows(definition)? != *arrays {
        return Err(validation("Authored shader did not keep its dye rows"));
    }
    Ok(())
}

/// The Collections entry that stands in for a base with none of its own: the one for an item that
/// shares the base's localized name and client classification, which is the same piece in another
/// version. Shadowkeep's Armor 2.0 reissues of older sets have no entries, and Collections shows
/// the original, so the authored item lands where that piece does.
pub(super) fn stand_in_collectible(
    sources: &ProjectSources,
    strings: &[u8],
) -> AuthoringResult<Option<usize>> {
    let name = crate::tag_payload::read_array::<8>(strings, ITEM_NAME_REFERENCE_OFFSET)?;
    let classification = client_classification(strings)?;
    Ok((0..sources.stock_collectible_count).find(|&index| {
        (|| -> AuthoringResult<bool> {
            let row = sources.collectible_rows + index * COLLECTIBLE_ROW_SIZE;
            let item = usize::from(read_u16(
                &sources.stock_collectibles,
                row + COLLECTIBLE_ITEM_INDEX_OFFSET,
            )?);
            if item >= sources.stock_item_count {
                return Ok(false);
            }
            let tag = read_u32(
                &sources.stock_item_strings,
                sources.string_rows + item * ITEM_ROW_SIZE + 16,
            )?;
            let candidate = read_tag(&sources.manager, TagHash(tag), "Collections stand-in")?;
            Ok(
                crate::tag_payload::read_array::<8>(&candidate, ITEM_NAME_REFERENCE_OFFSET)?
                    == name
                    && client_classification(&candidate)? == classification,
            )
        })()
        .unwrap_or(false)
    }))
}

/// The collectible whose unlock an authored gear item clones.
struct UnlockExemplar {
    index: usize,
    flag: u16,
    /// Whether it sits on the base's own page, so the page's counts can follow its flag.
    on_page: bool,
}

/// The base's own collectible when its acquisition names one unlock flag. Leveling sets, world
/// drops and vendor items often test a progression value or several flags instead, so a sibling
/// on the same page with the same parents stands in, the way weapons use a page exemplar, and
/// failing that another collectible for an item in the same inventory bucket. The authored item
/// still gets its own unlock and is placed beside its base either way.
fn unlock_exemplar(
    sources: &ProjectSources,
    base_index: usize,
    parents: &[u16],
    page: u16,
    bucket: u8,
) -> AuthoringResult<UnlockExemplar> {
    let single_flag = |index: usize| {
        collection_unlock_index(
            &sources.stock_collectibles,
            sources.collectible_rows + index * COLLECTIBLE_ROW_SIZE,
        )
        .ok()
        .filter(|flag| *flag < sources.stock_unlock_count)
        .and_then(|flag| u16::try_from(flag).ok())
    };
    if let Some(flag) = single_flag(base_index) {
        return Ok(UnlockExemplar {
            index: base_index,
            flag,
            on_page: true,
        });
    }
    let siblings = crate::progression::node_collectible_children(&sources.stock_nodes, page)?;
    for index in siblings.into_iter().filter(|index| *index != base_index) {
        let same_parents =
            template_presentation_parents(&sources.stock_nodes, &sources.stock_collectibles, index)
                .is_ok_and(|candidate| candidate == parents);
        if same_parents && let Some(flag) = single_flag(index) {
            return Ok(UnlockExemplar {
                index,
                flag,
                on_page: true,
            });
        }
    }
    (0..sources.stock_collectible_count)
        .find_map(|index| {
            let flag = single_flag(index)?;
            (collectible_item_bucket(sources, index) == Some(bucket)).then_some(UnlockExemplar {
                index,
                flag,
                on_page: false,
            })
        })
        .ok_or_else(|| {
            invalid("No Collections entry for this kind of item has a single unlock. Choose another base.")
        })
}

/// The inventory-bucket byte of the item a stock collectible awards.
fn collectible_item_bucket(sources: &ProjectSources, collectible_index: usize) -> Option<u8> {
    let row = sources.collectible_rows + collectible_index * COLLECTIBLE_ROW_SIZE;
    let item = usize::from(
        read_u16(
            &sources.stock_collectibles,
            row + COLLECTIBLE_ITEM_INDEX_OFFSET,
        )
        .ok()?,
    );
    if item >= sources.stock_item_count {
        return None;
    }
    let tag = read_u32(
        &sources.stock_item_table,
        sources.item_rows + item * ITEM_ROW_SIZE + 16,
    )
    .ok()?;
    let definition = read_tag(&sources.manager, TagHash(tag), "Collections exemplar item").ok()?;
    read_u8(&definition, ITEM_INVENTORY_SLOT_OFFSET).ok()
}

/// Identity, class, rarity, icon and text for an authored gear item. Slot and the client's type
/// keys stay the base item's own. Returns the collectible's reacquisition material set, which is
/// the base item's own.
pub(super) fn apply_presentation(
    definition: &mut Vec<u8>,
    strings: &mut [u8],
    donor: &ResolvedWeapon,
    authored_icon_index: u16,
    nameplate_row: Option<u16>,
    material_set: u16,
) -> AuthoringResult<u16> {
    let spec = &donor.weapon;
    let identity = spec.identity;
    let slot = native_slot(definition, spec.kind)?;
    let classification = client_classification(strings)?;
    if let Some(authored) = spec.overrides.rarity {
        set_rarity(definition, spec.kind, authored)?;
    }
    if let Some(selected) = spec.overrides.armor_class {
        super::armor::apply_class(definition, selected)?;
    }
    // Custom dyes replace the rows they edit, so their rows win over the recipe's own.
    if let Some(arrays) = donor
        .dye_rows
        .as_ref()
        .or(spec.overrides.render_dye_rows.as_ref())
    {
        set_shader_dye_rows(definition, arrays)?;
    }
    write_u16(strings, ITEM_STRING_ICON_INDEX_OFFSET, authored_icon_index)?;
    if let Some(row) = nameplate_row {
        write_u16(strings, ITEM_STRING_SECONDARY_ICON_INDEX_OFFSET, row)?;
    }
    clear_item_string_watermark_overrides(strings)?;
    write_localized_reference(
        strings,
        ITEM_NAME_REFERENCE_OFFSET,
        LOCALIZATION_DONOR_TABLE_INDEX as u32,
        identity.name_hash,
    )?;
    if spec.effective_type_name().is_some() {
        write_localized_reference(
            strings,
            ITEM_TYPE_REFERENCE_OFFSET,
            LOCALIZATION_DONOR_TABLE_INDEX as u32,
            identity.type_hash,
        )?;
    }
    write_localized_reference(
        strings,
        ITEM_DESCRIPTION_REFERENCE_OFFSET,
        LOCALIZATION_DONOR_TABLE_INDEX as u32,
        identity.flavor_hash,
    )?;
    if spec.text.inventory_hint.is_some() {
        write_localized_reference(
            strings,
            ITEM_DISPLAY_SOURCE_REFERENCE_OFFSET,
            LOCALIZATION_DONOR_TABLE_INDEX as u32,
            identity.inventory_hint_hash,
        )?;
    } else {
        write_localized_reference(
            strings,
            ITEM_DISPLAY_SOURCE_REFERENCE_OFFSET,
            BLANK_LOCALIZED_REFERENCE_TABLE_INDEX,
            BLANK_LOCALIZED_REFERENCE_HASH,
        )?;
    }
    if native_slot(definition, spec.kind)? != slot
        || client_classification(strings)? != classification
    {
        return Err(validation(
            "Authored gear changed its base item's slot or client classification",
        ));
    }
    Ok(material_set)
}

/// The reacquisition material set on the base item's own collectible row.
pub(super) fn base_material_set(
    collectibles: &[u8],
    collectible_index: usize,
) -> AuthoringResult<u16> {
    let (count, _, rows, class) = array_at(collectibles, 8)?;
    if class != COLLECTIBLE_DEFINITION_ROW_CLASS || collectible_index >= count {
        return Err(invalid("The base item's collectible is outside the table"));
    }
    read_u16(
        collectibles,
        rows + collectible_index * COLLECTIBLE_ROW_SIZE + COLLECTIBLE_MATERIAL_SET_OFFSET,
    )
}

/// Where a base names itself: its hash field, and where its labels hash the same identity string
/// as the item, the stack label at the head of its inventory block and the unique-equip label of
/// its equipment block. 436 of 484 stock emblems name themselves in the stack label and 93 of those
/// in the unique-equip label too. A copy takes its own hash at each, so it shares no stack or equip
/// group with its base. Any other place is refused.
pub(super) fn identity_offsets(definition: &[u8], item_hash: u32) -> AuthoringResult<Vec<usize>> {
    let offsets = matching_u32_offsets(definition, item_hash);
    let equip_label = (read_i64(definition, ITEM_EQUIPMENT_BLOCK_POINTER_OFFSET)? != 0)
        .then(|| relative_target(definition, ITEM_EQUIPMENT_BLOCK_POINTER_OFFSET))
        .transpose()?
        .map(|block| block + EQUIPMENT_UNIQUE_LABEL_OFFSET);
    let label = |offset: usize| offset == ITEM_STACK_LABEL_OFFSET || Some(offset) == equip_label;
    match offsets.split_first() {
        Some((&ITEM_DEFINITION_HASH_OFFSET, labels)) if labels.iter().all(|&at| label(at)) => {
            Ok(offsets)
        }
        _ => Err(invalid(
            "Donor embeds its item identity at unsupported offsets",
        )),
    }
}

/// The final check on an authored gear payload: new identity only at `identity_offsets`, where
/// its base named itself, the recipe's text, rarity and stats in place, and every authored socket
/// column written.
pub(super) fn validate_payloads(
    definition: &[u8],
    strings: &[u8],
    socket_column_indices: Option<&[Option<ResolvedSocketColumn>]>,
    (spec, identity_offsets): (&WeaponCloneSpec, &[usize]),
    dye_rows: Option<&[Vec<WeaponDyeReferenceOverride>; 3]>,
) -> AuthoringResult<()> {
    validate_item_root_holder_bounds(definition)?;
    native_slot(definition, spec.kind)?;
    if let Some(selected) = spec.overrides.armor_class {
        let actual =
            sundial::package_authoring::investment_schema::armor_equipment_class(definition)
                .map_err(validation)?;
        if actual != selected.native_class() {
            return Err(validation(
                "Authored armor did not retain its selected equip class",
            ));
        }
    }
    if let Some(arrays) = dye_rows.or(spec.overrides.render_dye_rows.as_ref())
        && shader_dye_rows(definition)? != *arrays
    {
        return Err(validation("Authored shader did not keep its dye rows"));
    }
    let text_matches = matching_u32_offsets(definition, spec.donor_item_hash).is_empty()
        && matching_u32_offsets(definition, spec.identity.item_hash) == identity_offsets
        && matching_u32_offsets(strings, spec.donor_item_hash).is_empty()
        && read_u32(strings, ITEM_NAME_REFERENCE_OFFSET)? == LOCALIZATION_DONOR_TABLE_INDEX as u32
        && read_u32(strings, ITEM_NAME_REFERENCE_OFFSET + 4)? == spec.identity.name_hash
        && read_u32(strings, ITEM_DESCRIPTION_REFERENCE_OFFSET + 4)? == spec.identity.flavor_hash
        && spec.effective_type_name().is_none_or(|_| {
            read_u32(strings, ITEM_TYPE_REFERENCE_OFFSET + 4).ok() == Some(spec.identity.type_hash)
        });
    if !text_matches {
        return Err(validation(
            "Authored gear identity or text references are inconsistent",
        ));
    }
    if spec
        .overrides
        .rarity
        .is_some_and(|authored| rarity(definition).ok() != Some(authored))
    {
        return Err(validation("Authored gear did not keep its rarity"));
    }
    if !spec.overrides.investment_stats.is_empty() {
        let resource = relative_target(definition, ITEM_INVESTMENT_STAT_POINTER_OFFSET)?;
        let (count, _, rows, class) = array_at(definition, resource)?;
        let stats_match = class == ITEM_INVESTMENT_STAT_ROW_CLASS
            && spec
                .overrides
                .investment_stats
                .iter()
                .all(|(definition_index, value)| {
                    (0..count).any(|index| {
                        let row = rows + index * ITEM_INVESTMENT_STAT_ROW_SIZE;
                        read_u8(definition, row).ok().map(u16::from) == Some(*definition_index)
                            && read_i32(definition, row + 4).ok() == Some(*value)
                    })
                });
        if !stats_match {
            return Err(validation("Authored gear stats were not written"));
        }
    }
    if let Some(columns) = socket_column_indices {
        let resource = relative_target(definition, ITEM_ORDINARY_SOCKET_POINTER_OFFSET)?;
        let (socket_count, _, socket_rows, _) = array_at(definition, resource)?;
        let socket_types = (0..socket_count)
            .map(|lane| {
                read_u16(
                    definition,
                    socket_rows + lane * ITEM_ORDINARY_SOCKET_ROW_SIZE,
                )
            })
            .collect::<AuthoringResult<Vec<_>>>()?;
        validate_weapon_socket_columns(definition, columns, &socket_types)?;
    }
    Ok(())
}
