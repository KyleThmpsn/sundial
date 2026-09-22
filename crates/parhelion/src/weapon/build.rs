mod assembly;
mod assets;
mod collections;
mod custom_plugs;
mod definition;
mod progress;
mod runtime;
mod tables;

use super::*;
pub(crate) use progress::Phase;
pub(super) use progress::Progress;

// Shared compilation work plus donor resolution, runtime authoring, and definitions per weapon.
const SHARED_OPERATIONS: usize = 12;

pub(super) fn canonical_project_weapons(
    project: &WeaponProjectSpec,
) -> AuthoringResult<Vec<WeaponCloneSpec>> {
    if project.weapons.is_empty() {
        return Err(AuthoringError::InvalidInput(
            "A weapon project must contain at least one weapon".to_owned(),
        ));
    }
    // Capacity depends on each recipe's definitions, plugs and imported assets.
    // Enforce the actual package entry/block limits during allocation/emission.
    let mut namespaces = BTreeSet::new();
    let mut item_hashes = BTreeSet::new();
    let mut collectible_hashes = BTreeSet::new();
    let mut unlock_hashes = BTreeSet::new();
    let mut pattern_global_ids = BTreeSet::new();
    let mut localized_hashes = BTreeSet::new();
    let mut weapons = project.weapons.clone();
    for weapon in &weapons {
        let weapon_label = weapon.error_context();
        weapon
            .validate()
            .map_err(|error| error.context(weapon_label.clone()))?;
        if !namespaces.insert(weapon.namespace.as_bytes().to_vec()) {
            return Err(AuthoringError::InvalidInput(format!(
                "{weapon_label}: duplicate project namespace {:?}",
                weapon.namespace
            )));
        }
        if !item_hashes.insert(weapon.identity.item_hash) {
            return Err(AuthoringError::InvalidInput(format!(
                "{weapon_label}: duplicate authored item hash 0x{:08X}",
                weapon.identity.item_hash
            )));
        }
        if !collectible_hashes.insert(weapon.identity.collectible_hash) {
            return Err(AuthoringError::InvalidInput(format!(
                "{weapon_label}: duplicate authored collectible hash 0x{:08X}",
                weapon.identity.collectible_hash
            )));
        }
        if !unlock_hashes.insert(weapon.identity.unlock_hash) {
            return Err(AuthoringError::InvalidInput(format!(
                "{weapon_label}: duplicate authored unlock hash 0x{:08X}",
                weapon.identity.unlock_hash
            )));
        }
        if !pattern_global_ids.insert(weapon.identity.pattern_global_id_hash) {
            return Err(AuthoringError::InvalidInput(format!(
                "{weapon_label}: duplicate authored sandbox-pattern global identity 0x{:08X}",
                weapon.identity.pattern_global_id_hash
            )));
        }
        for hash in [
            weapon.identity.name_hash,
            weapon.identity.type_hash,
            weapon.identity.flavor_hash,
            weapon.identity.source_hash,
            weapon.identity.collection_name_hash,
            weapon.identity.collection_description_hash,
            weapon.identity.inventory_hint_hash,
            weapon.identity.collection_requirement_hash,
        ] {
            if !localized_hashes.insert(hash) {
                return Err(AuthoringError::InvalidInput(format!(
                    "{weapon_label}: duplicate authored localized hash 0x{hash:08X}"
                )));
            }
        }
    }
    let badge_count = weapons
        .iter()
        .filter_map(|weapon| weapon.overrides.badge.as_ref().map(|badge| &badge.name))
        .collect::<BTreeSet<_>>()
        .len();
    if badge_count > crate::presentation::MAX_CUSTOM_BADGES {
        return Err(invalid(
            "A build can contain at most 24 custom badges within Sunrise’s presentation node capacity",
        ));
    }
    project_authored_localized_values(&weapons, &[], 0, crate::branding::Branding::Sunrise)?;
    weapons.sort_by(|left, right| {
        (
            left.namespace.as_bytes(),
            left.identity.item_hash,
            left.identity.collectible_hash,
            left.identity.unlock_hash,
        )
            .cmp(&(
                right.namespace.as_bytes(),
                right.identity.item_hash,
                right.identity.collectible_hash,
                right.identity.unlock_hash,
            ))
    });
    Ok(weapons)
}

/// Builds one coherent deterministic project containing every requested weapon.
#[cfg(test)]
pub fn build_weapon_project(
    package_directory: &Path,
    project: &WeaponProjectSpec,
) -> AuthoringResult<NewWeaponProjectBundle> {
    let weapons = canonical_project_weapons(project)?;
    let install_directory = package_directory.parent().ok_or_else(|| {
        invalid(format!(
            "Package directory has no install root: {}",
            package_directory.display()
        ))
    })?;
    validate_weapon_clone_specs_against_catalog(install_directory, weapons.iter())?;
    build_weapon_project_canonical(package_directory, &weapons)
}

/// Compiles a project whose specs were already checked against the original installed catalog.
///
/// Workflow uses this entry only after validating the project against the live install, because
/// its package input can be a filtered temporary view that deliberately omits authored overlays.
#[cfg(test)]
pub(crate) fn build_weapon_project_after_catalog_validation(
    package_directory: &Path,
    project: &WeaponProjectSpec,
) -> AuthoringResult<NewWeaponProjectBundle> {
    compile_with_progress(
        package_directory,
        project,
        crate::branding::Branding::for_packages(package_directory),
        &mut |_, _, _, _| {},
    )
}

/// Compiles catalog-validated recipes and reports real shared and per-weapon operations.
pub(crate) fn compile_with_progress(
    package_directory: &Path,
    project: &WeaponProjectSpec,
    branding: crate::branding::Branding,
    report: &mut dyn FnMut(Phase, &str, usize, usize),
) -> AuthoringResult<NewWeaponProjectBundle> {
    let mut progress = Progress::new(SHARED_OPERATIONS + 1 + 3 * project.weapons.len(), report);
    let weapons = progress.step("Validating Recipe Identities", || {
        canonical_project_weapons(project)
    })?;
    compile_canonical(package_directory, &weapons, branding, &mut progress)
}

/// Keep allocation, table mutation, and final package validation in their native order.
#[cfg(test)]
pub(super) fn build_weapon_project_canonical(
    package_directory: &Path,
    weapons: &[WeaponCloneSpec],
) -> AuthoringResult<NewWeaponProjectBundle> {
    let mut report = |_: Phase, _: &str, _: usize, _: usize| {};
    let mut progress = Progress::new(SHARED_OPERATIONS + 3 * weapons.len(), &mut report);
    compile_canonical(
        package_directory,
        weapons,
        crate::branding::Branding::for_packages(package_directory),
        &mut progress,
    )
}

fn compile_canonical(
    package_directory: &Path,
    weapons: &[WeaponCloneSpec],
    branding: crate::branding::Branding,
    progress: &mut Progress<'_>,
) -> AuthoringResult<NewWeaponProjectBundle> {
    let mut sources = progress.step("Loading Source Tables", || {
        sources::load_project_sources(package_directory)
    })?;
    let collection_plan = progress.step("Planning Collections", || {
        placements::Plan::new(&sources, weapons)
            .map_err(|error| error.context("Collections destination planning"))
    })?;
    let resolved = resolve::resolve_project_weapons_with_progress(
        &sources,
        weapons,
        &collection_plan,
        progress,
    )?;
    let templates = progress.step("Reading Perk Templates", || PerkTemplates::read(&sources))?;
    let mut custom_plugs = progress.step("Planning Private Perks", || {
        custom_plugs::plan(&sources, &resolved, &templates.strings)
    })?;
    let assets = progress.step("Planning Artwork", || {
        assets::plan(
            &sources.manager,
            &resolved,
            weapons.len(),
            &mut custom_plugs,
            branding,
        )
    })?;
    let icons = progress.step("Compiling Icons", || {
        assets::author_icon_rows(
            std::mem::take(&mut sources.stock_item_icons),
            &resolved,
            &assets,
            &mut custom_plugs,
        )
    })?;
    let (runtime, custom_payloads) = runtime::author(
        package_directory,
        &mut sources,
        &resolved,
        &custom_plugs,
        &templates,
        assets.weapon_runtime_start,
        progress,
    )?;
    let localization = progress.step("Compiling Text", || {
        let localization = author_project_localized_strings(
            &sources.manager,
            std::mem::take(&mut sources.localized_index),
            weapons,
            &custom_plugs,
            branding,
        )?;
        validate_authored_localization_values(&localization, weapons, &custom_plugs, branding)?;
        Ok(localization)
    })?;

    let mut tables = tables::WeaponTables::take_stock(&mut sources, resolved.len());
    let context = tables::WeaponBuildContext {
        stock_item_count: sources.stock_item_count,
        stock_collectible_count: sources.stock_collectible_count,
        authored_weapon_icon_indices: &icons.weapon_indices,
        authored_weapon_icon_containers: &assets.weapon_icon_containers,
        authored_item_icons: &icons.payload,
        sandbox_perk_definition_template: &templates.definition,
        sandbox_perk_string_template: &templates.strings,
        custom_plugs: &custom_plugs,
        sandbox_pattern_layout: tables::SANDBOX_PATTERN_LAYOUT,
        authored_pattern_global_ids: &runtime.pattern_global_ids,
    };
    tables.author_weapons(&context, &resolved, progress)?;
    progress.step("Adding Private Perk Definitions", || {
        tables.append_custom_plugs(
            &custom_plugs,
            sources.stock_item_count,
            weapons.len(),
            tables::METADATA_LAYOUT,
        )
    })?;
    tables.definitions.extend(custom_payloads.definitions);
    tables.authored_strings.extend(custom_payloads.strings);

    let collections = progress.step("Building Collections", || {
        collections::author(
            &mut sources,
            &collection_plan,
            &tables.project_rows,
            icons.badge_index,
            weapons,
            &icons.custom_badges,
            &mut tables.collectibles,
        )
        .map_err(|error| error.context("Collections authoring"))
    })?;
    let lore = progress.step("Building Lore", || {
        lore::author(
            &sources.manager,
            &sources.globals_data,
            weapons,
            &mut tables.definitions,
        )
    })?;
    let output = assembly::Output {
        lore,
        assets,
        icons,
        runtime,
        tables,
        collections,
        localization,
        has_custom_plugs: !custom_plugs.is_empty(),
    };
    let emission = progress.step("Linking Package Data", || {
        assembly::prepare(sources, output)
    })?;
    let mut bundle = emission::emit_packages(package_directory, emission, weapons, progress)?;
    for plug in &custom_plugs {
        for usage in &plug.uses {
            let weapon = bundle
                .plan
                .weapons
                .get_mut(usage.weapon_ordinal)
                .ok_or_else(|| invalid("Private perk use refers to a missing authored weapon"))?;
            weapon.custom_plugs.push(NewCustomPlugPlan {
                socket_index: usage.socket_index,
                choice_index: usage.choice_index,
                name: plug.authored_name.clone(),
                item_hash: plug.authored_item_hash,
                item_index: plug.authored_item_index,
                definition_tag: plug.authored_definition_tag,
                string_tag: plug.authored_string_tag,
                icon_definition_tag: plug.authored_icon_container,
                name_hash: plug.authored_name_hash,
                description_hash: plug.authored_description_hash,
                perks: plug
                    .sandbox_perks
                    .iter()
                    .map(|perk| NewPrivatePerkPlan {
                        source_perk_index: perk.source_index,
                        perk_hash: perk.authored_perk_hash,
                        runtime_key: perk.authored_runtime_key,
                    })
                    .collect(),
            });
        }
    }
    for weapon in &mut bundle.plan.weapons {
        weapon
            .custom_plugs
            .sort_by_key(|plug| (plug.socket_index, plug.choice_index));
    }
    Ok(bundle)
}

struct PerkTemplates {
    definition: [u8; ITEM_SANDBOX_PERK_ROW_SIZE],
    strings: Vec<u8>,
}

impl PerkTemplates {
    fn read(sources: &sources::ProjectSources) -> AuthoringResult<Self> {
        let definition = canonical_weapon_sandbox_perk_row_template(
            &sources.manager,
            &sources.stock_item_table,
        )?;
        let strings = canonical_item_sandbox_perk_string_template(
            &sources.manager,
            &sources.stock_item_strings,
        )?;
        Ok(Self {
            definition,
            strings,
        })
    }
}
