//! Allocate icon assets before runtime tags and keep request-to-row ordering explicit.
use super::*;

pub(super) struct Plan {
    pub custom_badges: BTreeMap<String, TagHash>,
    pub weapon_tag_count: usize,
    pub weapon_runtime_start: usize,
    pub watermark: crate::watermark::WatermarkPlan,
    pub badge: crate::badge_icon::BadgeIconPlan,
    pub hud_table: Option<ReplacementSpec>,
    pub hud_asset_start: usize,
    pub weapon_icon_containers: Vec<TagHash>,
}

pub(super) fn plan(
    manager: &PackageManager,
    resolved: &[resolve::ResolvedWeapon],
    weapon_count: usize,
    custom_plug_count: usize,
) -> AuthoringResult<Plan> {
    let weapon_tag_ordinal_base = weapon_count
        .checked_add(custom_plug_count)
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
                    .map_or_else(|| weapon_rarity(&donor.definition), Ok)
                    .map_err(|error| error.context(donor.weapon.error_context()))?,
            })
        })
        .collect::<AuthoringResult<Vec<_>>>()?;
    let watermark_plan = crate::watermark::build_presented_watermark_plan(
        manager,
        HOST_PACKAGE_ID,
        HOST_EXPECTED_ENTRY_COUNT,
        weapon_tag_ordinal_base,
        &icon_requests,
        &resolved
            .iter()
            .map(|donor| donor.weapon.overrides.corner_icon.clone())
            .collect::<Vec<_>>(),
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
                .map_err(|error| error.context(donor.weapon.icon_error_context()))
        })
        .collect::<AuthoringResult<Vec<_>>>()?;
    let mut badge_icon_plan = build_badge_icon_plan(manager, PARHELION_ASSET_PACKAGE_ID, 0, 0)?;
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
        let plan = crate::badge_icon::build_icon_plan(
            manager,
            PARHELION_ASSET_PACKAGE_ID,
            0,
            badge_icon_plan.new_tags.len(),
            badge.icon.as_ref(),
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
        weapon_icon_containers: authored_weapon_icon_containers,
    })
}

pub(super) struct IconRows {
    pub custom_badges: BTreeMap<String, u16>,
    pub payload: Vec<u8>,
    pub badge_index: u16,
    pub weapon_indices: Vec<u16>,
}

pub(super) fn author_icon_rows(
    stock_icons: &[u8],
    resolved: &[resolve::ResolvedWeapon],
    assets: &Plan,
) -> AuthoringResult<IconRows> {
    let (mut authored_item_icons, badge_icon_index) =
        append_badge_icon_row(stock_icons.to_vec(), assets.badge.container_tag)?;
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
        .map_err(|error| error.context(donor.weapon.icon_error_context()))?;
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
    Ok(IconRows {
        custom_badges,
        payload: authored_item_icons,
        badge_index: badge_icon_index,
        weapon_indices: authored_weapon_icon_indices,
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
