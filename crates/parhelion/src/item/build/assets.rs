//! Allocate icon assets before runtime tags and keep request-to-row ordering explicit.
use super::*;
#[cfg(test)]
mod tests;

pub(super) struct Plan {
    pub custom_badges: BTreeMap<String, TagHash>,
    pub weapon_tag_count: usize,
    pub weapon_runtime_start: usize,
    pub watermark: crate::watermark::WatermarkPlan,
    pub badge: crate::badge_icon::BadgeIconPlan,
    pub hud_table: Option<ReplacementSpec>,
    pub hud_asset_start: usize,
    /// The HUD icon layers the project's own HUD status images make, by image and by the stock
    /// status whose layer each copies.
    pub hud_status_layers: BTreeMap<(String, u32), TagHash>,
    pub weapon_icon_containers: Vec<TagHash>,
    /// Each authored subclass entry's own icon container, by item ordinal and entry.
    pub entry_icon_containers: Vec<Vec<Option<TagHash>>>,
    /// Each emblem's own nameplate container, by item ordinal.
    pub nameplate_containers: Vec<Option<TagHash>>,
    pub perk_icon_dependencies: Vec<TagHash>,
}

pub(super) fn plan(
    manager: &PackageManager,
    resolved: &[resolve::ResolvedWeapon],
    weapon_count: usize,
    custom_plugs: &mut [ResolvedCustomPlug],
    programs: &[sundial::package_authoring::sandbox_perk::program::Program],
    branding: crate::branding::Branding,
) -> AuthoringResult<Plan> {
    // Each item and private plug takes a pair, definition and strings. A subclass list takes
    // two: the list and its companion, then the display record and its companion. Each authored
    // ability or path node takes one more: its pool and its node record.
    let subclass_pairs = resolved
        .iter()
        .filter_map(|donor| donor.subclass_list.as_ref())
        .map(crate::subclass::authoring::ResolvedList::tag_pairs)
        .sum::<usize>();
    let weapon_tag_ordinal_base = weapon_count
        .checked_add(custom_plugs.len())
        .and_then(|count| count.checked_add(subclass_pairs))
        .ok_or_else(|| invalid("Authored host-tag count overflowed"))?
        .checked_mul(2)
        .ok_or_else(|| invalid("Authored host-tag count overflowed"))?;
    let icon_requests = resolved
        .iter()
        .map(|donor| {
            Ok(WeaponIconRequest {
                donor_container_tag: donor.donor_icon_container,
                icon_edit: donor.weapon.overrides.icon_edit.clone(),
                rarity: donor
                    .weapon
                    .overrides
                    .rarity
                    .map_or_else(
                        || {
                            if donor.weapon.kind.is_weapon() {
                                weapon_rarity(&donor.definition)
                            } else {
                                gear::rarity(&donor.definition)
                            }
                        },
                        Ok,
                    )
                    .map_err(|error| donor.weapon.in_recipe(error))?,
                plain: donor.weapon.kind == crate::ItemKind::Subclass,
            })
        })
        .collect::<AuthoringResult<Vec<_>>>()?;
    let watermark_plan = crate::watermark::build_presented_watermark_plan(
        manager,
        HOST_PACKAGE_ID,
        HOST_EXPECTED_ENTRY_COUNT,
        weapon_tag_ordinal_base,
        &icon_requests,
        crate::watermark::Presentation {
            branding,
            artwork: &resolved
                .iter()
                .map(|donor| donor.weapon.overrides.corner_icon.clone())
                .collect::<Vec<_>>(),
        },
        &|index| {
            format!(
                "{}\nIcon Resource: {}",
                resolved[index].weapon.icon_error_context(),
                resolved[index].donor_icon_container
            )
        },
    )?;
    let authored_weapon_icon_containers = resolved
        .iter()
        .enumerate()
        .map(|(request_index, donor)| {
            watermark_plan
                .container_for_request(request_index)
                .ok_or_else(|| {
                    validation(format!(
                        "No authored Sunrise icon container was produced for donor {}",
                        donor.donor_icon_container
                    ))
                })
                .map_err(|error| {
                    donor
                        .weapon
                        .in_recipe_as(error, donor.weapon.icon_error_context())
                })
        })
        .collect::<AuthoringResult<Vec<_>>>()?;
    let default_badge = branding.badge()?;
    let mut badge_icon_plan = crate::badge_icon::build_icon_plan(
        manager,
        PARHELION_ASSET_PACKAGE_ID,
        0,
        0,
        default_badge.as_ref(),
    )?;
    let mut badges = BTreeMap::new();
    for donor in resolved {
        if let Some(badge) = &donor.weapon.overrides.badge {
            if let Some(previous) = badges.insert(badge.name.clone(), badge) {
                if previous != badge {
                    return Err(invalid(format!(
                        "Badge {:?} has conflicting settings",
                        badge.name
                    ))
                    .context(badge_context(resolved, &badge.name)));
                }
            }
        }
    }
    let mut custom_badges = BTreeMap::new();
    for (name, badge) in badges {
        let plan = crate::badge_icon::build_icon_plan_with_branding(
            manager,
            PARHELION_ASSET_PACKAGE_ID,
            0,
            badge_icon_plan.new_tags.len(),
            badge.icon.as_ref().or(default_badge.as_ref()),
            branding,
        )
        .map_err(|error| error.context(badge_context(resolved, &name)))?;
        custom_badges.insert(name, plan.container_tag);
        badge_icon_plan.new_tags.extend(plan.new_tags);
        badge_icon_plan
            .reference_overrides
            .extend(plan.reference_overrides);
    }
    let hud_asset_start = badge_icon_plan.new_tags.len();
    let hud_table = crate::hud_icon::assets::build(
        manager,
        resolved.iter().filter_map(|donor| {
            donor
                .weapon
                .overrides
                .hud_icon
                .as_ref()
                .map(|image| (donor.weapon.identity.type_hash, image))
        }),
        &mut badge_icon_plan.new_tags,
        &mut badge_icon_plan.reference_overrides,
    )
    .map_err(|error| {
        error.context(format!(
            "HUD Icons Used By:\n{}",
            resolved
                .iter()
                .filter(|donor| donor.weapon.overrides.hud_icon.is_some())
                .map(|donor| format!(
                    "{}\nHUD Key: 0x{:08X}",
                    donor.weapon.error_context(),
                    donor.weapon.identity.type_hash
                ))
                .collect::<Vec<_>>()
                .join("\n\n")
        ))
    })?;
    // HUD status images follow the ammunition HUD icons, within the enrolled HUD range.
    let hud_status_layers = super::super::hud_status::author_images(
        manager,
        programs,
        &mut badge_icon_plan.new_tags,
        &mut badge_icon_plan.reference_overrides,
    )?;
    let mut perk_icon_dependencies = Vec::new();
    // A private icon container goes in the asset package, and `author` appends it with its
    // companion and anything it paints.
    type ContainerAuthor<'a> = dyn FnMut(
            &mut Vec<crate::NewTagSpec>,
            &mut Vec<crate::NewTagReferenceOverride>,
        ) -> AuthoringResult<TagHash>
        + 'a;
    let mut private_container = |author: &mut ContainerAuthor<'_>| {
        let container = author(
            &mut badge_icon_plan.new_tags,
            &mut badge_icon_plan.reference_overrides,
        )?;
        let companion = crate::shared_tag_memory::adjacent_companion_tag(container)?;
        let dependencies = crate::shared_tag_memory::validate_shared_tag_companion_payload(
            &badge_icon_plan.new_tags[usize::from(companion.entry_index())].payload,
            companion,
            container,
        )?;
        // Private glyph textures are enrolled below with the appended assets. Any untouched
        // donor layers still need their stock textures kept resident too.
        perk_icon_dependencies.extend(
            dependencies
                .into_iter()
                .map(TagHash)
                .filter(|tag| tag.pkg_id() != PARHELION_ASSET_PACKAGE_ID),
        );
        AuthoringResult::Ok(container)
    };
    let mut private_icon = |icon: &crate::perk::Icon, source_container: TagHash| {
        private_container(&mut |nodes, references| {
            crate::icon_edit::package_icons::author(
                manager,
                icon,
                source_container,
                nodes,
                references,
            )
        })
    };
    for plug in custom_plugs.iter_mut() {
        if let Some(icon) = &plug.icon {
            plug.authored_icon_container = Some(private_icon(icon, plug.source_icon_container)?);
        }
    }
    // An authored subclass ability or node with artwork of its own shows it the same way.
    let entry_icon_containers = resolved
        .iter()
        .map(|donor| {
            donor
                .subclass_list
                .iter()
                .flat_map(|list| &list.entries)
                .map(|entry| {
                    entry
                        .artwork
                        .as_ref()
                        .map(|artwork| private_icon(&artwork.icon, artwork.source_container))
                        .transpose()
                        .map_err(|error| {
                            donor.weapon.in_recipe_as(
                                error,
                                format!("{}\n{}", donor.weapon.error_context(), entry.label),
                            )
                        })
                })
                .collect::<AuthoringResult<Vec<_>>>()
        })
        .collect::<AuthoringResult<Vec<_>>>()?;
    // An emblem with images of its own gets a nameplate container of its own, and a subclass
    // with pictures of its own a screen art container. Both take the same strings row.
    let nameplate_containers = resolved
        .iter()
        .map(|donor| {
            if let Some(art) = &donor.screen_art {
                return private_container(&mut |nodes, references| {
                    crate::subclass::art::author(manager, art, nodes, references)
                })
                .map(Some)
                .map_err(|error| {
                    donor.weapon.in_recipe_as(
                        error,
                        format!("{}\nScreen Art", donor.weapon.error_context()),
                    )
                });
            }
            donor
                .nameplate
                .as_ref()
                .map(|nameplate| {
                    private_container(&mut |nodes, references| {
                        crate::emblem::author(manager, nameplate, nodes, references)
                    })
                })
                .transpose()
                .map_err(|error| {
                    donor.weapon.in_recipe_as(
                        error,
                        format!("{}\nNameplate", donor.weapon.error_context()),
                    )
                })
        })
        .collect::<AuthoringResult<Vec<_>>>()?;
    let weapon_runtime_tag_start = HOST_EXPECTED_ENTRY_COUNT
        .checked_add(weapon_tag_ordinal_base)
        .and_then(|count| count.checked_add(watermark_plan.new_tags.len()))
        .ok_or_else(|| invalid("Authored host runtime-tag start overflowed"))?;
    Ok(Plan {
        custom_badges,
        weapon_tag_count: weapon_tag_ordinal_base,
        weapon_runtime_start: weapon_runtime_tag_start,
        watermark: watermark_plan,
        badge: badge_icon_plan,
        hud_table,
        hud_asset_start,
        hud_status_layers,
        weapon_icon_containers: authored_weapon_icon_containers,
        entry_icon_containers,
        nameplate_containers,
        perk_icon_dependencies,
    })
}

pub(super) struct IconRows {
    pub custom_badges: BTreeMap<String, u16>,
    pub payload: Vec<u8>,
    pub badge_index: u16,
    pub weapon_indices: Vec<u16>,
    /// Each authored subclass entry's own icon row, by item ordinal and entry.
    pub entry_indices: Vec<Vec<Option<u16>>>,
    /// Each emblem's own nameplate row and container, by item ordinal.
    pub nameplates: Vec<Option<(u16, TagHash)>>,
}

pub(super) fn author_icon_rows(
    stock_icons: Vec<u8>,
    resolved: &[resolve::ResolvedWeapon],
    assets: &Plan,
    custom_plugs: &mut [ResolvedCustomPlug],
) -> AuthoringResult<IconRows> {
    let (mut authored_item_icons, badge_icon_index) =
        append_badge_icon_row(stock_icons, assets.badge.container_tag)?;
    let mut authored_weapon_icon_indices = Vec::with_capacity(resolved.len());
    for (donor, authored_container) in resolved
        .iter()
        .zip(assets.weapon_icon_containers.iter().copied())
    {
        (|| -> AuthoringResult<()> {
            let (icons, icon_index) = append_authored_weapon_icon_row(
                std::mem::take(&mut authored_item_icons),
                donor.donor_icon_index,
                donor.weapon.identity.item_hash,
                authored_container,
            )?;
            authored_item_icons = icons;
            apply_array_row_raw_payload_patches(
                &mut authored_item_icons,
                8,
                usize::from(icon_index),
                ITEM_ICON_ROW_SIZE,
                ITEM_ICON_ROW_CLASS,
                WeaponRawPayloadTarget::ItemIconRow,
                &donor.weapon.overrides.raw_payload_patches,
            )?;
            authored_weapon_icon_indices.push(icon_index);
            Ok(())
        })()
        .map_err(|error| {
            donor
                .weapon
                .in_recipe_as(error, donor.weapon.icon_error_context())
        })?;
    }
    let mut custom_badges = BTreeMap::new();
    for (name, &container) in &assets.custom_badges {
        let (icons, index) = append_authored_weapon_icon_row(
            authored_item_icons,
            crate::badge::LUNAR_BADGE_ICON_ROW_INDEX as u16,
            crate::presentation::text_hash(name, "badge-icon"),
            container,
        )?;
        authored_item_icons = icons;
        custom_badges.insert(name.clone(), index);
    }
    let mut entry_indices = Vec::with_capacity(resolved.len());
    for (donor, containers) in resolved.iter().zip(&assets.entry_icon_containers) {
        let entries = donor.subclass_list.iter().flat_map(|list| &list.entries);
        let mut indices = Vec::with_capacity(containers.len());
        for (entry, container) in entries.zip(containers) {
            let (Some(artwork), Some(container)) = (&entry.artwork, container) else {
                indices.push(None);
                continue;
            };
            let (icons, index) = append_authored_weapon_icon_row(
                authored_item_icons,
                artwork.source_row,
                crate::presentation::text_hash(
                    &donor.weapon.namespace,
                    &format!("{}-icon", entry.key),
                ),
                *container,
            )?;
            authored_item_icons = icons;
            indices.push(Some(index));
        }
        entry_indices.push(indices);
    }
    let mut nameplates = Vec::with_capacity(resolved.len());
    for (donor, container) in resolved.iter().zip(&assets.nameplate_containers) {
        let template_row = donor
            .nameplate
            .as_ref()
            .map(|nameplate| nameplate.template_row)
            .or_else(|| donor.screen_art.as_ref().map(|art| art.template_row));
        let (Some(template_row), Some(container)) = (template_row, container) else {
            nameplates.push(None);
            continue;
        };
        let (icons, index) = append_authored_weapon_icon_row(
            authored_item_icons,
            template_row,
            crate::presentation::text_hash(&donor.weapon.namespace, "nameplate"),
            *container,
        )?;
        authored_item_icons = icons;
        nameplates.push(Some((index, *container)));
    }
    for plug in custom_plugs {
        if let Some(container) = plug.authored_icon_container {
            let source_index = read_u16(&plug.source_strings, ITEM_STRING_ICON_INDEX_OFFSET)?;
            let (icons, index) = append_authored_weapon_icon_row(
                authored_item_icons,
                source_index,
                plug.authored_item_hash,
                container,
            )?;
            authored_item_icons = icons;
            write_u16(
                &mut plug.source_strings,
                ITEM_STRING_ICON_INDEX_OFFSET,
                index,
            )?;
        }
    }
    Ok(IconRows {
        custom_badges,
        payload: authored_item_icons,
        badge_index: badge_icon_index,
        weapon_indices: authored_weapon_icon_indices,
        entry_indices,
        nameplates,
    })
}

fn badge_context(resolved: &[resolve::ResolvedWeapon], name: &str) -> String {
    let owners = resolved
        .iter()
        .filter(|donor| {
            donor
                .weapon
                .overrides
                .badge
                .as_ref()
                .is_some_and(|badge| badge.name == name)
        })
        .map(|donor| donor.weapon.error_context())
        .collect::<Vec<_>>()
        .join("\n\n");
    format!("Badge: {name:?}\nUsed By:\n{owners}")
}
