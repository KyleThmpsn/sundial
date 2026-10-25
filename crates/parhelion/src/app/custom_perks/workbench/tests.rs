pub(crate) use sundial::test_support::capture;
mod cards;
mod entry;
mod gaps;
mod library_controls;
mod library_issues;
mod weapon_properties;
use super::attachment::{Change, Target};
use super::*;
use crate::test_support::driver::label;

fn frame(
    ctx: &egui::Context,
    workbench: &mut Workbench,
    experimental: bool,
    size: egui::Vec2,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let weapon = WeaponRecipe::every_end();
    ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ui| {
            assert!(
                workbench
                    .show(ui, Path::new(""), None, &[], experimental, (&weapon, None))
                    .is_none()
            );
        },
    )
}

/// A workbench over a temporary library holding one saved perk named Alpha.
fn workbench_with_saved_perk() -> (tempfile::TempDir, Library, PerkRecipe, Entry, Workbench) {
    let temporary = tempfile::tempdir().unwrap();
    let library = Library::open(temporary.path().to_owned()).unwrap();
    let mut original = PerkRecipe::new();
    original.name = "Alpha".into();
    original.effects.push(PerkRecipe::effect(405));
    let entry = library.save(&original, None).unwrap();
    let workbench = Workbench {
        initialized: true,
        library: Some(library.clone()),
        documents: vec![Document::new(
            original.clone(),
            Some(entry.baseline.clone()),
        )],
        ..Default::default()
    };
    (temporary, library, original, entry, workbench)
}

#[test]
fn deleting_a_perk_changed_outside_the_workbench_preserves_the_file() {
    let (_temporary, _library, _original, entry, mut workbench) = workbench_with_saved_perk();
    std::fs::write(&entry.path, b"{}").unwrap();
    workbench.delete(library::PerkSource::Document(0));
    assert!(workbench.error.is_some());
    assert!(entry.path.exists());
    assert_eq!(workbench.documents.len(), 1);
    assert_eq!(workbench.documents[0].recipe.name, "Alpha");
}

#[test]
fn a_deleted_bundled_perk_is_not_added_back() {
    let temporary = tempfile::tempdir().unwrap();
    let library = Library::open(temporary.path().to_owned()).unwrap();
    library.materialize_bundled().unwrap();
    let scan = library.scan().unwrap();
    assert!(!scan.entries.is_empty());
    for entry in &scan.entries {
        library.delete(&entry.recipe, &entry.baseline).unwrap();
    }
    library.materialize_bundled().unwrap();
    assert!(library.scan().unwrap().entries.is_empty());
    // Deleting an already deleted perk is not an error.
    library
        .delete(&scan.entries[0].recipe, &scan.entries[0].baseline)
        .unwrap();
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES for native stock and extra socket choices"]
fn native_workbench_roundtrip_preserves_all_effects_extra_choices_and_socket_metadata() {
    let packages = crate::test_support::stock_packages();
    let catalog = crate::test_support::catalog(packages.parent().unwrap()).unwrap();
    let donor = catalog.weapon_donor(0x23DB_942F).unwrap();
    let mut weapon = WeaponRecipe::new_weapon_for_donor(
        "parhelion.workbench-parity",
        donor.summary.hash,
        &donor.summary.name,
    )
    .unwrap();
    let socket = donor.sockets.len();
    weapon.overrides.socket_columns = vec![None; socket];
    weapon
        .overrides
        .socket_columns
        .push(Some(crate::WeaponSocketColumnRecipe {
            socket_type: Some(92),
            choices: vec![0x45A0_BDD7.into(), 0xDD5C_B37A.into()],
            choice_weight_bits: vec![2.0_f32.to_bits(), 3.0_f32.to_bits()],
            reusable_plug_set_index: Some(3),
            randomized_plug_set_index: Some(4),
            randomized_selection_program: vec![crate::WeaponNumericInstructionRecipe {
                opcode: 11,
                operand: 2,
            }],
            ..Default::default()
        }));
    let expanded = socket_editor::socket_editor_donor(&donor, &weapon).into_owned();
    let target = Target::capture(&weapon, &expanded, socket, 1).unwrap();
    let mut workbench = Workbench {
        initialized: true,
        ..Default::default()
    };
    workbench.open_target(target.clone(), &catalog);
    let recipe = &mut workbench.documents[0].recipe;
    assert_eq!(
        recipe
            .effects
            .iter()
            .map(|effect| effect.source_perk_index)
            .collect::<Vec<_>>(),
        catalog.item_sandbox_perk_indices(0xDD5C_B37A)
    );
    recipe.name = "Private Frame".into();
    recipe.description = "Preserve every sibling effect".into();
    recipe.classification = Some(0xC684_24BC.into());
    recipe.effects.push(PerkRecipe::effect(405));
    recipe.stats.push(WeaponStatOverride {
        definition_index: 255,
        value: 10,
    });
    let authored = recipe.clone();
    let before = weapon.clone();
    let change = Change {
        target,
        perk: Some(authored.clone()),
    };
    change.apply(&mut weapon, &donor).unwrap();
    assert_eq!(
        weapon.overrides.socket_columns,
        before.overrides.socket_columns
    );
    let variant = &weapon.overrides.socket_plug_variants[0];
    assert_eq!(variant.choice_index, 1);
    assert_eq!(variant.socket_index as usize, socket);
    let restored = templates::from_variant(variant, &catalog);
    assert_eq!(restored.effects, authored.effects);
    assert_eq!(restored.stats, authored.stats);
    assert_eq!(restored.classification, authored.classification);
    assert_eq!(
        WeaponRecipe::from_json_str(&weapon.to_json_pretty().unwrap()).unwrap(),
        weapon
    );
    let applied = weapon.clone();
    assert!(change.apply(&mut weapon, &donor).is_err());
    assert_eq!(weapon, applied);
    verify_extra_choices(weapon, donor, authored, socket);
}

fn verify_extra_choices(
    mut weapon: WeaponRecipe,
    donor: WeaponDonor,
    authored: PerkRecipe,
    socket: usize,
) {
    let applied = weapon.clone();
    let expanded = socket_editor::socket_editor_donor(&donor, &weapon).into_owned();
    let append = Target::capture(&weapon, &expanded, socket, 2).unwrap();
    let mut extra = authored.clone();
    extra.name = "Another Private Frame".into();
    Change {
        target: append,
        perk: Some(extra),
    }
    .apply(&mut weapon, &donor)
    .unwrap();
    assert_eq!(
        weapon.overrides.socket_plug_variants[0],
        applied.overrides.socket_plug_variants[0]
    );
    assert_eq!(
        recipe_socket_choices(&weapon, socket, &[]).unwrap().len(),
        3
    );
    let target = Target::capture(&weapon, &expanded, socket, 1).unwrap();
    Change { target, perk: None }
        .apply(&mut weapon, &donor)
        .unwrap();
    assert_eq!(weapon.overrides.socket_plug_variants.len(), 1);
    assert_eq!(weapon.overrides.socket_plug_variants[0].choice_index, 2);
    assert_eq!(
        recipe_socket_choices(&weapon, socket, &[]).unwrap()[1],
        0xDD5C_B37A
    );
}
