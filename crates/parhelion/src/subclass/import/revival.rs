//! Saved self-revive candidates using the existing private ability and perk authoring paths.
use super::*;
use crate::subclass::{AttunementPath, SubclassPathNode};
use sundial::investment::WeaponDamageType;
use sundial::package_authoring::sandbox_perk::program::{
    Action, Program, PropertyOperation, Trigger,
};

/// Native solar presentation is an explicit preparation policy. The public ability picker stays
/// hidden until this candidate passes package loading and complete gameplay acceptance.
pub fn prepare(request: &Request<'_>) -> Result<Prepared, String> {
    let path = AttunementPath::Middle;
    let entries = path.entries();
    let position = entries.iter().position(|&entry| entry == request.entry);
    if request.recipe.kind != ItemKind::Subclass || !matches!(position, Some(2 | 3)) {
        return Err(
            "Self-revive preparation requires a passive node in the middle attunement".into(),
        );
    }
    let position = position.expect("validated passive position") as u8;
    let super_entry = entries[0];
    request
        .recipe
        .to_spec()
        .map_err(error)?
        .validate()
        .map_err(error)?;
    let source_path = packages_path(request.source_packages).map_err(error)?;
    let native_path = request.native_packages.canonicalize().map_err(error)?;
    let output =
        outside(request.output, source_path.parent().unwrap_or(&source_path)).map_err(error)?;
    outside(&output, native_path.parent().unwrap_or(&native_path)).map_err(error)?;
    if output.exists() {
        return Err("Choose a new ability import output folder".into());
    }
    let native_stamp = package_stamp(&native_path).map_err(error)?;
    let source = Source::open(&source_path, |_, _| {}).map_err(error)?;
    let manager = open_shadowkeep_package_manager(&native_path)?;
    fs::create_dir_all(&output).map_err(error)?;
    let catalog = InvestmentCatalog::load_with_cache_path(
        native_path
            .parent()
            .ok_or("The native packages have no installation parent")?,
        &output.join("native-catalog.json"),
        true,
        |_| {},
    )?;
    let subclasses = catalog.subclasses(crate::package_profile::is_stock_item_definition);
    let donors = subclasses
        .iter()
        .filter(|candidate| {
            candidate.class_type == 2 && candidate.damage_type == Some(WeaponDamageType::Solar)
        })
        .collect::<Vec<_>>();
    let [donor] = donors.as_slice() else {
        return Err("Native solar Warlock presentation is missing or ambiguous".into());
    };
    let base = request.recipe.donor.item_hash.parse_u32().map_err(error)?;
    if !subclasses
        .iter()
        .any(|candidate| candidate.hash == base && candidate.class_type == 2)
    {
        return Err("Self-revive preparation currently requires a Warlock subclass base".into());
    }
    // Top and bottom share entry 10, while the middle path owns entry 20. Both need the
    // compatible foundation. Each path selects its own passive self-revive grant.
    // Native presentation qualifies the graph envelope, not active Super or revival gameplay.
    let presentation = *donor
        .entry_entities
        .get(&super_entry)
        .ok_or("The native solar presentation has no Super graph")?;
    let translated = source
        .prepare_revival(&manager, request.choice, presentation, "self-revive")
        .map_err(error)?;
    let globals_tag = resolve_live_named_tag(&manager, "investment_globals", None)?;
    let globals = manager.read_tag(globals_tag).map_err(error)?;
    let source_perk = catalog
        .item_sandbox_perk_indices(crate::perk::DEFAULT_PLUG_LAYOUT)
        .into_iter()
        .find(|&index| {
            load_sandbox_perk_runtime_action(&manager, &globals, usize::from(index)).is_ok()
        })
        .ok_or("The native private-perk template has no usable action")?;
    let program = Program {
        name: request.choice.name.trim().to_owned(),
        trigger: Trigger::Always,
        duration_ms: 0,
        actions: translated
            .keys
            .iter()
            .map(|&key| Action::AbilityProperty {
                target: AbilityTarget::Super,
                key,
                option: PropertyOperation::Apply,
            })
            .collect(),
        ..Program::default()
    };
    program.validate()?;
    let mut effect = PerkRecipe::effect(source_perk);
    effect.program = Some(program);
    let mut perk = PerkRecipe::new();
    perk.name = request.choice.name.trim().to_owned();
    perk.description = request.choice.description.clone();
    perk.effects.push(effect);
    let directory = output.join("ability");
    if package_stamp(&native_path).map_err(error)? != native_stamp
        || package_stamp(&source_path).map_err(error)? != source.report().stamp
    {
        return Err("The package inputs changed during ability import preparation".into());
    }
    translated.save(&directory).map_err(error)?;
    let imported = super::super::ImportedAbility {
        directory: directory.clone(),
        sha256: parhelion_import::d2_mot::native::attachment::fingerprint(&directory)
            .map_err(error)?,
        symbol: translated.root.clone(),
    };
    let super_node = SubclassPathNode {
        position: 0,
        source: donor.hash,
        source_path: path,
        source_position: 0,
        edits: EntryEdits {
            name: Some(translated.ability.name.clone()),
            description: Some(translated.ability.description.clone()),
            imported_ability: Some(imported.clone()),
            removed_perks: donor
                .entry_perks
                .get(&super_entry)
                .into_iter()
                .flatten()
                .copied()
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
            removed_modifiers: donor
                .entry_modifiers
                .get(&super_entry)
                .into_iter()
                .flatten()
                .map(|&(key, row)| StockModifier { key, row })
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
            ..EntryEdits::default()
        },
    };
    let mut recipe = request.recipe.clone();
    let abilities = recipe.overrides.subclass_abilities.get_or_insert_default();
    abilities.set_path_node(path, base, super_node);
    abilities.set_ability(
        base,
        SubclassChoice {
            entry: layout::SUPER,
            source: donor.hash,
            source_entry: layout::SUPER,
            edits: EntryEdits {
                name: Some(translated.ability.name.clone()),
                description: Some(translated.ability.description.clone()),
                imported_ability: Some(imported),
                removed_perks: donor
                    .entry_perks
                    .get(&layout::SUPER)
                    .into_iter()
                    .flatten()
                    .copied()
                    .collect(),
                removed_modifiers: donor
                    .entry_modifiers
                    .get(&layout::SUPER)
                    .into_iter()
                    .flatten()
                    .map(|&(key, row)| StockModifier { key, row })
                    .collect(),
                ..EntryEdits::default()
            },
        },
    );
    let mut perk_entries = Vec::new();
    for target_path in AttunementPath::ALL {
        let entry = target_path.entries()[usize::from(position)];
        let mut selected_perk = perk.clone();
        selected_perk.id = digest(
            &serde_json::to_vec(&json!({
                "source":"d1_roi_self_revive", "namespace":request.recipe.namespace,
                "entry":entry, "choice":request.choice
            }))
            .map_err(error)?,
        );
        abilities.set_path_node(
            target_path,
            base,
            SubclassPathNode {
                position,
                source: donor.hash,
                source_path: target_path,
                source_position: position,
                edits: EntryEdits {
                    name: Some(selected_perk.name.clone()),
                    description: Some(selected_perk.description.clone()),
                    custom_perks: vec![selected_perk],
                    removed_perks: donor
                        .entry_perks
                        .get(&entry)
                        .into_iter()
                        .flatten()
                        .copied()
                        .collect(),
                    removed_modifiers: donor
                        .entry_modifiers
                        .get(&entry)
                        .into_iter()
                        .flatten()
                        .map(|&(key, row)| StockModifier { key, row })
                        .collect(),
                    ..EntryEdits::default()
                },
            },
        );
        perk_entries.push(entry);
    }
    if abilities
        .attunement(path)
        .is_some_and(|attunement| attunement.name.is_none())
    {
        abilities.set_path_name(path, base, Some(translated.ability.name.clone()));
    }
    recipe.to_spec().map_err(error)?.validate().map_err(error)?;
    if package_stamp(&native_path).map_err(error)? != native_stamp
        || package_stamp(&source_path).map_err(error)? != source.report().stamp
    {
        return Err("The package inputs changed before ability candidate publication".into());
    }
    let encoded = recipe.to_json_pretty().map_err(error)?;
    let provenance = json!({"status":"prepared self-revive candidate","gameplay_verified":false,
        "baseline_recipe_sha256":digest(request.recipe.to_json_pretty().map_err(error)?.as_bytes()),
        "prepared_recipe_sha256":digest(encoded.as_bytes()),"source":source.report(),
        "native_stamp":native_stamp,"native_presentation":presentation,"translation":translated.evidence,
        "super_entries":[layout::SUPER,super_entry],"revival_perk_entries":perk_entries});
    fs::write(
        output.join("provenance.json"),
        serde_json::to_vec_pretty(&provenance).map_err(error)?,
    )
    .map_err(error)?;
    fs::write(output.join("prepared-subclass.parhelion.json"), encoded).map_err(error)?;
    Ok(Prepared {
        baseline: request.recipe.clone(),
        recipe,
        provenance,
    })
}
