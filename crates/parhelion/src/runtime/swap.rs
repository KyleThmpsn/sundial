//! Plan and check a complete donor change without writing packages or mutating the open recipe.
use super::*;
use crate::{
    WeaponDonorReference, WeaponRecipe,
    weapon::{preflight_runtime_edits, runtime_hud_key},
};
use sundial::{
    investment::WeaponDonorSummary,
    package_authoring::{
        weapon_entity::{coupled_weapon_component_bindings, weapon_component_bindings},
        weapon_runtime::{
            WeaponRuntimeField, WeaponRuntimeFieldLocator, WeaponRuntimeValueKind,
            WeaponRuntimeValueOverride, encode_weapon_runtime_value, resolve_weapon_runtime_field,
            runtime_fields_share_semantics,
        },
    },
};
use tiger_pkg::PackageManager;

#[derive(Clone, Debug)]
pub(crate) struct Preview {
    pub before: WeaponRecipe,
    pub after: WeaponRecipe,
    pub binding_hash: u32,
    pub donor_hash: u32,
    pub group: Vec<u32>,
    pub kept: usize,
    pub transferred: Vec<String>,
    pub resets: Vec<String>,
    pub error: Option<String>,
}

/// Group replacement is atomic and leaves every independent donor selection intact.
fn replace_group(
    recipe: &mut WeaponRecipe,
    key: &mut RuntimeGraphKey,
    group: &[u32],
    donor: &WeaponDonorSummary,
    follows_baseline: bool,
) {
    recipe.runtime_component_donors.retain(|component| {
        component
            .binding_hash
            .parse_u32()
            .map_or(true, |hash| !group.contains(&hash))
    });
    key.component_donors
        .retain(|(binding, _, _)| !group.contains(binding));
    if !follows_baseline {
        for &binding in group {
            recipe.set_runtime_component_donor(
                binding,
                Some(WeaponDonorReference {
                    item_hash: donor.hash.into(),
                    expected_name: Some(donor.name.clone()),
                }),
            );
            key.component_donors
                .push((binding, donor.weapon_pattern_index, donor.hash));
        }
        key.component_donors.sort_unstable();
    }
}

pub(crate) fn preview(
    packages: &Path,
    recipe: &WeaponRecipe,
    key: &RuntimeGraphKey,
    binding_hash: u32,
    donor: &WeaponDonorSummary,
    donors: &[WeaponDonorSummary],
) -> Result<Preview, String> {
    let manager = open_shadowkeep_package_manager(packages)?;
    let baseline_key = RuntimeGraphKey::new(key.pattern_index, key.fallback_item_hash, []);
    let baseline = load_effective_runtime_entity(&manager, &baseline_key)?;
    let group = coupled_weapon_component_bindings(&baseline.payload, binding_hash)?;
    let pattern = donor
        .weapon_pattern_index
        .ok_or("The donor has no active runtime row")?;
    let selected = load_weapon_runtime_entity_at_pattern_index_with_manager(&manager, pattern)?;
    let mut after = recipe.clone();
    let mut after_key = key.clone();
    replace_group(
        &mut after,
        &mut after_key,
        &group,
        donor,
        selected.item_hash == baseline.item_hash,
    );
    let target = load_effective_runtime_entity(&manager, &after_key)?;
    // A conflicting saved combination can still be repaired. It cannot supply evidence for
    // semantic remapping, but exact settings that resolve against the new entity can survive.
    let source = load_effective_runtime_entity(&manager, key).ok();
    let graph = (!recipe.overrides.runtime_values.is_empty())
        .then(|| {
            load_weapon_runtime_graph_for_entity(
                &manager,
                target.item_hash,
                target.pattern_global_id_hash,
                target.entity_tag,
                &target.payload,
            )
        })
        .and_then(Result::ok);
    let mut result = Preview {
        before: recipe.clone(),
        after,
        binding_hash,
        donor_hash: donor.hash,
        group,
        kept: 0,
        transferred: Vec::new(),
        resets: Vec::new(),
        error: None,
    };
    transfer_settings(
        &manager,
        source.as_ref(),
        &target,
        graph.as_ref(),
        &mut result,
    );
    result.error = check_recipe(&manager, &result.after, &after_key, donors, &target).err();
    Ok(result)
}

fn changed(
    source: Option<&WeaponRuntimeEntitySource>,
    target: &WeaponRuntimeEntitySource,
    binding: u32,
    index: u16,
) -> bool {
    let identity = |entity: &WeaponRuntimeEntitySource| {
        weapon_component_bindings(&entity.payload, binding)
            .ok()?
            .get(usize::from(index))
            .map(|resource| {
                (
                    resource.owner_tag,
                    resource.concrete_class,
                    resource.resource_offset,
                )
            })
    };
    source
        .and_then(identity)
        .is_none_or(|before| Some(before) != identity(target))
}

fn carries_value(kind: &WeaponRuntimeValueKind) -> bool {
    !matches!(
        kind,
        WeaponRuntimeValueKind::FixedBytes { .. } | WeaponRuntimeValueKind::HexIdentifier { .. }
    )
}

fn portable_target<'a>(
    source: &WeaponRuntimeField,
    graph: &'a WeaponRuntimeGraph,
) -> Option<&'a WeaponRuntimeField> {
    let locator = &source.locator;
    let candidates = graph
        .fields()
        .filter(|field| {
            let binding = (field.locator.binding_hash, field.locator.resource_index);
            let matches_binding = binding == (locator.binding_hash, locator.resource_index)
                || graph.resources.iter().any(|resource| {
                    (resource.binding_hash, resource.resource_index) == binding
                        && resource
                            .alias_bindings
                            .contains(&(locator.binding_hash, locator.resource_index))
                });
            matches_binding && runtime_fields_share_semantics(source, field).unwrap_or(false)
        })
        .collect::<Vec<_>>();
    match candidates.as_slice() {
        [field] => Some(*field),
        _ => None,
    }
}

fn transfer_value(
    manager: &PackageManager,
    source: Option<&WeaponRuntimeEntitySource>,
    target: &WeaponRuntimeEntitySource,
    graph: Option<&WeaponRuntimeGraph>,
    value: &WeaponRuntimeValueOverride,
) -> Option<(WeaponRuntimeValueOverride, Option<String>)> {
    let locator = &value.locator;
    if let Ok(resolved) = resolve_weapon_runtime_field(manager, &target.payload, locator)
        && (!changed(source, target, locator.binding_hash, locator.resource_index)
            || carries_value(&resolved.field.kind))
        && encode_weapon_runtime_value(&resolved.field.kind, &value.value).is_ok()
    {
        return Some((value.clone(), None));
    }
    let original = resolve_weapon_runtime_field(manager, &source?.payload, locator).ok()?;
    let field = portable_target(&original.field, graph?)?;
    encode_weapon_runtime_value(&field.kind, &value.value).ok()?;
    Some((
        WeaponRuntimeValueOverride {
            locator: field.locator.clone(),
            value: value.value.clone(),
        },
        Some(field.path_label.clone()),
    ))
}

fn setting_label(
    manager: &PackageManager,
    source: Option<&WeaponRuntimeEntitySource>,
    locator: &WeaponRuntimeFieldLocator,
) -> String {
    source
        .and_then(|source| resolve_weapon_runtime_field(manager, &source.payload, locator).ok())
        .map_or_else(
            || {
                format!(
                    "Saved setting in binding 0x{:08X} at 0x{:X}",
                    locator.binding_hash, locator.value_offset
                )
            },
            |resolved| resolved.field.path_label,
        )
}

fn transfer_settings(
    manager: &PackageManager,
    source: Option<&WeaponRuntimeEntitySource>,
    target: &WeaponRuntimeEntitySource,
    graph: Option<&WeaponRuntimeGraph>,
    result: &mut Preview,
) {
    let values = std::mem::take(&mut result.after.overrides.runtime_values);
    for value in values {
        if let Some((mapped, label)) = transfer_value(manager, source, target, graph, &value) {
            if let Some(label) = label {
                result.transferred.push(label);
            } else {
                result.kept += 1;
            }
            result.after.overrides.runtime_values.push(mapped);
        } else if result.group.contains(&value.locator.binding_hash) {
            result
                .resets
                .push(setting_label(manager, source, &value.locator));
        } else {
            // Do not silently discard an unrelated invalid edit to make this preview succeed.
            result.after.overrides.runtime_values.push(value);
        }
    }
    result
        .after
        .overrides
        .runtime_resource_patches
        .retain(|patch| {
            let reset = patch.binding_hash.parse_u32().is_ok_and(|binding| {
                result.group.contains(&binding)
                    && changed(source, target, binding, patch.resource_index)
            });
            if reset {
                result
                    .resets
                    .push(format!("Binary component patch at 0x{:X}", patch.offset));
            }
            !reset
        });
    let entity_changed = source.is_none_or(|source| source.payload != target.payload);
    result.after.overrides.raw_payload_patches.retain(|patch| {
        let reset =
            entity_changed && patch.target == crate::RecipeRawPayloadTarget::RuntimeWeaponEntity;
        if reset {
            result
                .resets
                .push(format!("Binary weapon patch at 0x{:X}", patch.offset));
        }
        !reset
    });
}

fn check_recipe(
    manager: &PackageManager,
    recipe: &WeaponRecipe,
    key: &RuntimeGraphKey,
    donors: &[WeaponDonorSummary],
    entity: &WeaponRuntimeEntitySource,
) -> Result<(), String> {
    let spec = recipe.to_spec().map_err(|error| error.to_string())?;
    let appearance_hash = recipe
        .presentation_donor
        .as_ref()
        .unwrap_or(&recipe.donor)
        .item_hash
        .parse_u32()
        .map_err(|error| error.to_string())?;
    let appearance_pattern = donors
        .iter()
        .find(|donor| donor.hash == appearance_hash)
        .and_then(|donor| donor.weapon_pattern_index);
    let content_graft = key
        .component_donors
        .iter()
        .any(|(binding, pattern, _)| *binding == 0x5F0_DD954 && *pattern != appearance_pattern);
    let hud_key = runtime_hud_key(
        manager,
        spec.overrides
            .hud_icon
            .as_ref()
            .map(|_| spec.identity.type_hash),
        appearance_pattern.is_some() && key.pattern_index == appearance_pattern && !content_graft,
        || {
            let pattern = appearance_pattern.ok_or_else(|| {
                crate::error::invalid("The appearance donor has no active runtime row")
            })?;
            let source = load_weapon_runtime_entity_at_pattern_index_with_manager(manager, pattern)
                .map_err(crate::error::invalid)?;
            Ok(Some((source.payload, source.weapon_content_group_hash)))
        },
    )
    .map_err(|error| error.to_string())?;
    preflight_runtime_edits(manager, &entity.payload, &spec.overrides, hud_key)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests;
