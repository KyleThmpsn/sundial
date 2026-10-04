use super::*;
use crate::perk::import::{Request as PerkImportRequest, prepare as prepare_perk_import};
use crate::recipe::{HexHash, WeaponRecipe};
use parhelion_import::d2_mot::{gameplay::perks, payload::Payload, reader::Reader};
use sha2::{Digest, Sha256};
use sundial::package_authoring::{
    runtime::load_weapon_runtime_entity_with_manager,
    sandbox_perk::{action, program::Action},
};

const SWORD_ITEM_HASH: u32 = 0x0222_2CBF;
const SWORD_BINDING: u32 = 0xCD2B_CEAC;
const PROFILE_KEY: u32 = 0xE6A6_4E01;
const PROFILE_DESCRIPTORS: [usize; 7] = [0x1140, 0x1998, 0x11B8, 0x2AD0, 0x3398, 0x3B78, 0x2B48];
const PROFILE_RELATIVE_FIELDS: [usize; 13] = [
    0x20, 0x78, 0x88, 0xB0, 0xC0, 0xE8, 0xF8, 0x110, 0x120, 0x1A8, 0x1C8, 0x1D8, 0x1F8,
];

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn scalar(owner: &[u8], row: usize, program: usize) -> u32 {
    let (code_count, _, code, code_class) = array_at(owner, row + program).unwrap();
    assert_eq!((code_count, code_class), (4, 0x8080_0009));
    assert_eq!(&owner[code..code + 4], &[0x34, 0, 0x3E, 0]);
    let (constant_count, _, constant, constant_class) =
        array_at(owner, row + program + 16).unwrap();
    assert_eq!((constant_count, constant_class), (1, 0x8080_0090));
    let bits = read_u32(owner, constant).unwrap();
    for lane in 1..4 {
        assert_eq!(read_u32(owner, constant + lane * 4).unwrap(), bits);
    }
    bits
}

fn stock_sword_plug(manager: &PackageManager) -> (usize, u32, u16, usize) {
    let globals = manager
        .read_tag(resolve_live_named_tag(manager, "investment_globals", None).unwrap())
        .unwrap();
    let root = manager
        .read_tag(TagHash(read_u32(&globals, 16).unwrap()))
        .unwrap();
    let item_table = manager
        .read_tag(root_child_tag(&root, ROOT_ITEM_DEFINITION_TABLE_SLOT).unwrap())
        .unwrap();
    let (item_count, _, rows, _) = array_at(&item_table, 8).unwrap();
    let sword_row = find_u32_row_key(
        &item_table,
        rows,
        item_count,
        ITEM_ROW_SIZE,
        SWORD_ITEM_HASH,
    )
    .unwrap()
    .unwrap();
    let sword_definition = manager
        .read_tag(TagHash(
            read_u32(&item_table, rows + sword_row * ITEM_ROW_SIZE + 16).unwrap(),
        ))
        .unwrap();
    let choices = weapon_default_plug_indices(&sword_definition).unwrap();
    for (socket, choice) in choices.iter().enumerate().skip(4) {
        let index = usize::from(*choice);
        if index >= item_count {
            continue;
        }
        let row = rows + index * ITEM_ROW_SIZE;
        let plug_hash = read_u32(&item_table, row).unwrap();
        let definition = manager
            .read_tag(TagHash(read_u32(&item_table, row + 16).unwrap()))
            .unwrap();
        for perk in weapon_sandbox_perks(&definition).unwrap_or_default() {
            if sundial::package_authoring::sandbox_perk::load_sandbox_perk_runtime_action(
                manager,
                &globals,
                usize::from(perk),
            )
            .is_ok()
            {
                return (socket, plug_hash, perk, choices.len());
            }
        }
    }
    panic!("The clean sword donor has no trait plug with a runtime perk")
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES, PARHELION_IMPORT_MODERN_PACKAGES and PARHELION_EAGER_OUTPUT"]
#[expect(
    clippy::cognitive_complexity,
    reason = "End-to-end verification keeps the ordered workflow and its independent assertions together"
)]
fn keyed_sword_profiles_and_private_effect_stage_and_reopen() {
    let packages = PathBuf::from(
        std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").expect("clean stock packages"),
    );
    let output = PathBuf::from(std::env::var_os("PARHELION_EAGER_OUTPUT").expect("artifact root"))
        .join("private-dependencies");
    std::fs::create_dir_all(&output).unwrap();
    let mut source = Reader::new(
        &PathBuf::from(
            std::env::var_os("PARHELION_IMPORT_MODERN_PACKAGES").expect("modern packages"),
        ),
        &output.join("source"),
        true,
    )
    .unwrap();
    let source_action = source.tag(0x80C3_0E09, Some(0x8080_B835)).unwrap();
    let source_lunge_owner = source.tag(0x80C3_78E0, Some(0x8080_9B06)).unwrap();
    let source_tracking_owner = source.tag(0x80C3_78E1, Some(0x8080_9B06)).unwrap();
    let scales = perks::sword::angular_scales(&source_tracking_owner).unwrap();
    let (near_scale_bits, far_scale_bits) = (scales.near_bits, scales.far_bits);
    assert_eq!(
        (near_scale_bits, far_scale_bits),
        (0.5f32.to_bits(), 0.12f32.to_bits())
    );
    source.finish().unwrap();
    let stock = open_manager(&packages).unwrap();
    let source_entity = load_weapon_runtime_entity_with_manager(&stock, SWORD_ITEM_HASH).unwrap();
    let source_binding =
        weapon_component_bindings(&source_entity.payload, SWORD_BINDING).unwrap()[0];
    assert_eq!(source_binding.concrete_class, 0x8080_43D2);
    let source_owner = stock.read_tag(TagHash(source_binding.owner_tag)).unwrap();
    let lunge_graph = stock.read_tag(TagHash(0x8162_C91A)).unwrap();
    let lunge_binding = weapon_component_bindings(&lunge_graph, 0x7330_E39F).unwrap()[0];
    assert_eq!(
        (lunge_binding.owner_tag, lunge_binding.resource_offset),
        (0x8162_C919, 0x110)
    );
    let lunge_owner = stock.read_tag(TagHash(lunge_binding.owner_tag)).unwrap();
    let lunge_offset = lunge_binding.resource_offset as usize + 0x160;
    let lowered_settings = perks::lower::modifier_settings(
        &source_lunge_owner,
        0x2F0,
        &Payload(lunge_owner.clone()),
        lunge_offset,
    )
    .unwrap();
    let controller = perks::controller::read(&source_action).unwrap();
    let draw = controller
        .states
        .iter()
        .flat_map(|state| &state.transitions)
        .flat_map(|transition| &transition.conditions)
        .find(|condition| condition.class == 0x8080_30AE && condition.kind == 16)
        .expect("Eager draw transition");
    let draw_prefix = perks::lower::draw_condition_prefix(&source_action, draw.offset).unwrap();
    let active = controller
        .states
        .iter()
        .flat_map(|state| &state.transitions)
        .find(|transition| {
            transition
                .conditions
                .iter()
                .any(|condition| condition.class == 0x8080_BDCF)
        })
        .expect("Eager active transition");
    assert_eq!(
        active
            .conditions
            .iter()
            .map(|condition| condition.kind)
            .collect::<Vec<_>>(),
        [18, 1, 49, 8, 21]
    );
    let cooldown_state = controller
        .states
        .iter()
        .find(|state| state.offset == active.destination)
        .expect("Eager cooldown state");
    let cooldown_transition = cooldown_state
        .transitions
        .iter()
        .find(|transition| transition.conditions.len() == 1 && transition.conditions[0].kind == 1)
        .expect("Eager cooldown timer transition");
    let cooldown =
        perks::lower::timer_condition(&source_action, cooldown_transition.conditions[0].offset)
            .unwrap();
    let cooldown_seconds = f32::from_le_bytes(cooldown.bytes[8..12].try_into().unwrap());
    let cooldown_ms = (cooldown_seconds * 1000.0).round() as u32;
    assert_eq!(cooldown_ms, 3000);
    let (socket, plug, perk, _) = stock_sword_plug(&stock);
    let namespace = "parhelion.eager-private-dependencies.e2e";
    let mut recipe = WeaponRecipe::new_weapon_for_donor(namespace, SWORD_ITEM_HASH, "").unwrap();
    recipe.name = "Private Sword Profile Fixture".into();
    recipe.flavor = "Offline package verification.".into();
    recipe.source = "Source: Eager Edge translation fixture".into();
    let modern_packages = PathBuf::from(
        std::env::var_os("PARHELION_IMPORT_MODERN_PACKAGES").expect("modern packages"),
    );
    let prepared = prepare_perk_import(&PerkImportRequest {
        modern_packages: &modern_packages,
        native_packages: &packages,
        output: &output.join("bridge"),
        recipe: &recipe,
        plug_hash: 2_077_819_806,
        socket_index: socket as u16,
        choice_index: 0,
        source_plug_hash: plug,
        source_perk_index: perk,
        profile_key: PROFILE_KEY,
        name: "Private Sword Profile Key",
    })
    .unwrap();
    let mut changed = recipe.clone();
    changed.name.push_str(" Changed");
    let unchanged = changed.to_json_pretty().unwrap();
    assert!(prepared.apply(&mut changed).is_err());
    assert_eq!(changed.to_json_pretty().unwrap(), unchanged);
    let mut conflicting = prepared.recipe().clone();
    let conflict_column = conflicting.overrides.socket_columns[socket]
        .as_mut()
        .expect("prepared private socket column");
    assert_eq!(conflict_column.choices.len(), 1);
    conflict_column.choices.push(HexHash::new(plug));
    let conflict_variant = &mut conflicting.overrides.socket_plug_variants[0];
    conflict_variant.choice_index = 1;
    let conflict_program = conflict_variant.sandbox_perks[0]
        .program
        .as_mut()
        .expect("prepared private program");
    let conflict_key = conflict_program
        .actions
        .iter_mut()
        .find_map(|action| match action {
            Action::Native { node } if node.kind == 41 => Some(node),
            _ => None,
        })
        .expect("prepared retained profile-key effect");
    conflict_key.bytes[4..8].copy_from_slice(&(PROFILE_KEY ^ 1).to_le_bytes());
    conflicting
        .overrides
        .sword_profile
        .as_mut()
        .expect("prepared private sword profile")
        .key = HexHash::new(PROFILE_KEY ^ 1);
    let conflicting_before = conflicting.to_json_pretty().unwrap();
    std::fs::write(
        output.join("conflicting-sword-recipe.parhelion.json"),
        &conflicting_before,
    )
    .unwrap();
    let conflict_error = prepare_perk_import(&PerkImportRequest {
        modern_packages: &modern_packages,
        native_packages: &packages,
        output: &output.join("bridge-conflict"),
        recipe: &conflicting,
        plug_hash: 2_077_819_806,
        socket_index: socket as u16,
        choice_index: 0,
        source_plug_hash: plug,
        source_perk_index: perk,
        profile_key: PROFILE_KEY,
        name: "Private Sword Profile Key",
    })
    .err()
    .expect("valid baseline with an incompatible profile must reject the import");
    assert!(conflict_error.contains("incompatible"));
    assert!(conflict_error.contains("sword profile"));
    prepared.apply(&mut recipe).unwrap();
    let encoded = recipe.to_json_pretty().unwrap();
    let recipe = WeaponRecipe::from_json_str(&encoded).unwrap();
    let spec = recipe.to_spec().unwrap();
    assert_eq!(spec.overrides.sword_profile.unwrap().key, PROFILE_KEY);
    let mut unbound = spec.clone();
    unbound.overrides.sword_profile.as_mut().unwrap().key ^= 1;
    assert!(
        unbound.validate().is_err(),
        "a keyed profile without its private effect must be rejected"
    );
    let mut nonretained = spec.clone();
    let nonretained_program = nonretained.overrides.socket_plug_variants[0].sandbox_perks[0]
        .program
        .as_mut()
        .unwrap();
    let node = nonretained_program
        .actions
        .iter_mut()
        .find_map(|action| match action {
            Action::Native { node } if node.kind == 41 => Some(node),
            _ => None,
        })
        .expect("private profile-key action");
    node.bytes[1] = 0;
    assert!(
        nonretained.validate().is_err(),
        "a profile-key effect without retained cleanup must be rejected"
    );
    let mut stale = spec.clone();
    let stale_program = stale.overrides.socket_plug_variants[0].sandbox_perks[0]
        .program
        .as_mut()
        .unwrap();
    stale_program.native_asset_patches[0].patches[0].expected[0] ^= 1;
    let stale_error = build_weapon_project_after_catalog_validation(
        &packages,
        &WeaponProjectSpec {
            weapons: vec![stale],
        },
    )
    .unwrap_err();
    assert!(
        stale_error.to_string().contains("stale stock owner bytes"),
        "stale native owner preflight returned {stale_error}"
    );
    let bundle = build_weapon_project_after_catalog_validation(
        &packages,
        &WeaponProjectSpec {
            weapons: vec![spec],
        },
    )
    .unwrap();
    let view = staged_view(&packages, ".parhelion-eager-private-", &bundle);
    let staged = open_manager(&view.path().join("packages")).unwrap();
    let objective_consumption = native_objectives::verify(&staged, &output);
    let authored_entity =
        load_weapon_runtime_entity_with_manager(&staged, bundle.plan.weapons[0].item_hash).unwrap();
    let authored_binding =
        weapon_component_bindings(&authored_entity.payload, SWORD_BINDING).unwrap()[0];
    assert_ne!(authored_binding.owner_tag, source_binding.owner_tag);
    let owner = staged
        .read_tag(TagHash(authored_binding.owner_tag))
        .unwrap();
    assert_eq!(
        stock.read_tag(TagHash(source_binding.owner_tag)).unwrap(),
        source_owner
    );
    let mut profiles = 0;
    for descriptor in PROFILE_DESCRIPTORS {
        let (old_count, _, old_rows, old_class) = array_at(&source_owner, descriptor).unwrap();
        let (new_count, _, new_rows, new_class) = array_at(&owner, descriptor).unwrap();
        assert_eq!((old_class, new_class), (0x8080_2D7B, 0x8080_2D7B));
        assert_eq!(new_count, old_count * 2);
        for index in 0..old_count {
            let old = old_rows + index * 0x220;
            let active = new_rows + index * 0x440;
            let ordinary = active + 0x220;
            assert_eq!(
                read_u32(&source_owner, old).unwrap(),
                read_u32(&owner, active).unwrap()
            );
            assert_eq!(
                read_u32(&source_owner, old).unwrap(),
                read_u32(&owner, ordinary).unwrap()
            );
            assert_eq!(read_u32(&owner, active + 12).unwrap(), PROFILE_KEY);
            assert_eq!(read_u32(&owner, ordinary + 12).unwrap(), 0x811C_9DC5);
            for program in [0x70, 0xA8, 0xE0] {
                assert_eq!(
                    scalar(&source_owner, old, program),
                    scalar(&owner, ordinary, program)
                );
            }
            assert_eq!(
                scalar(&owner, active, 0x70),
                scalar(&source_owner, old, 0x70)
            );
            for (program, scale) in [
                (0xA8, f32::from_bits(near_scale_bits)),
                (0xE0, f32::from_bits(far_scale_bits)),
            ] {
                let expected = f32::from_bits(scalar(&source_owner, old, program)) * scale;
                assert_eq!(scalar(&owner, active, program), expected.to_bits());
            }
            for offset in PROFILE_RELATIVE_FIELDS {
                let old_relative = read_i64(&source_owner, old + offset).unwrap();
                for (row, keyed) in [(active, true), (ordinary, false)] {
                    let new_relative = read_i64(&owner, row + offset).unwrap();
                    if old_relative == 0 {
                        assert_eq!(new_relative, 0);
                        continue;
                    }
                    let old_target = relative_target(&source_owner, old + offset).unwrap();
                    let new_target = relative_target(&owner, row + offset).unwrap();
                    if keyed && matches!(offset, 0xC0 | 0xF8) {
                        assert!(new_target >= source_owner.len());
                    } else {
                        assert_eq!(new_target, old_target);
                    }
                    if offset == 0x110 {
                        assert_eq!(read_u32(&owner, new_target - 4).unwrap(), 0x8080_692C);
                    }
                }
            }
            profiles += 1;
        }
    }
    assert!(profiles > 0);
    let key_descriptor = source_binding.resource_offset as usize + 0x1F0;
    let (old_capacity, _, _, old_class) = array_at(&source_owner, key_descriptor).unwrap();
    let (new_capacity, _, _, new_class) = array_at(&owner, key_descriptor).unwrap();
    assert_eq!((old_class, new_class), (0x8080_43D5, 0x8080_43D5));
    assert_eq!(new_capacity, old_capacity + 1);

    let private = &bundle.plan.weapons[0].custom_plugs[0].perks[0];
    let runtime_map = staged
        .read_tag(TagHash(SANDBOX_PERK_RUNTIME_MAP_TAG))
        .unwrap();
    let assignment = sandbox_perk_runtime_assignment(&runtime_map, private.runtime_key)
        .unwrap()
        .unwrap();
    let action_bytes = staged.read_tag(TagHash(assignment.runtime_tag)).unwrap();
    let action = action::decode(&action_bytes).unwrap();
    assert_eq!(
        action.effects().map(|node| node.kind).collect::<Vec<_>>(),
        // Native execution walks the stored effect pointers in reverse order.
        [36, 41, 2]
    );
    let draw_condition = action.groups[0]
        .activation
        .iter()
        .find(|node| node.kind == 16)
        .expect("private draw trigger");
    assert_eq!(
        &action_bytes[draw_condition.offset..draw_condition.offset + draw_prefix.len()],
        draw_prefix.as_slice()
    );
    assert_eq!(
        action.groups[0]
            .removal
            .iter()
            .map(|node| node.kind)
            .collect::<Vec<_>>(),
        [18, 1, 41, 8, 20]
    );
    assert_eq!(action.groups[0].rearm[0].kind, 1);
    let expected_mask = (1u64 << 18) | (1 << 1) | (1 << 41) | (1 << 8) | (1 << 20);
    assert_eq!(
        u64::from_le_bytes(action_bytes[0x90..0x98].try_into().unwrap()),
        expected_mask
    );
    let attachment = action.effects().find(|node| node.kind == 2).unwrap();
    let authored_lunge_tag = TagHash(read_u32(&action_bytes, attachment.offset + 16).unwrap());
    assert_ne!(authored_lunge_tag, TagHash(0x8162_C91A));
    let authored_lunge = staged.read_tag(authored_lunge_tag).unwrap();
    let authored_lunge_binding =
        weapon_component_bindings(&authored_lunge, 0x7330_E39F).unwrap()[0];
    assert_ne!(authored_lunge_binding.owner_tag, lunge_binding.owner_tag);
    let other_binding = weapon_component_bindings(&authored_lunge, 0x95A6_0F29).unwrap()[0];
    assert_eq!(other_binding.owner_tag, authored_lunge_binding.owner_tag);
    let authored_lunge_owner = staged
        .read_tag(TagHash(authored_lunge_binding.owner_tag))
        .unwrap();
    assert_eq!(
        read_u32(&authored_lunge_owner, lunge_offset).unwrap(),
        authored_lunge_binding.owner_tag
    );
    assert_eq!(
        &authored_lunge_owner[lunge_offset + 4..lunge_offset + 88],
        &lowered_settings[4..],
    );
    assert_eq!(
        stock.read_tag(TagHash(lunge_binding.owner_tag)).unwrap(),
        lunge_owner
    );
    assert_eq!(stock.read_tag(TagHash(0x8162_C91A)).unwrap(), lunge_graph);
    assert!(action.effects().any(|node| node.kind == 36));
    let effect = action.effects().find(|node| node.kind == 41).unwrap();
    assert_eq!(action_bytes[effect.offset + 1], 1);
    // Complex native nodes reserve a conservative upper bound for nested state.
    assert!(action.retained_state_budget >= 3);
    assert_eq!(
        read_u32(&action_bytes, effect.offset + 4).unwrap(),
        PROFILE_KEY
    );

    std::fs::write(output.join("private-sword-owner.bin"), &owner).unwrap();
    std::fs::write(output.join("private-sword-recipe.parhelion.json"), &encoded).unwrap();
    std::fs::write(
        output.join("private-lunge-owner.bin"),
        &authored_lunge_owner,
    )
    .unwrap();
    for artifact in &bundle.artifacts {
        std::fs::write(
            output.join(&artifact.plan.output_file_name),
            artifact.bytes(),
        )
        .unwrap();
    }
    std::fs::write(
        output.join("verified-private-dependencies.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "source_sword_owner": format!("{:08X}", source_binding.owner_tag),
            "recipe_sha256": digest(encoded.as_bytes()),
            "profile_conflict": {
                "baseline_artifact": "conflicting-sword-recipe.parhelion.json",
                "baseline_sha256": digest(conflicting_before.as_bytes()),
                "error": conflict_error,
            },
            "authored_sword_owner": format!("{:08X}", authored_binding.owner_tag),
            "stock_owner_sha256": digest(&source_owner),
            "authored_owner_sha256": digest(&owner),
            "source_action_sha256": digest(&source_action.0),
            "source_lunge_owner_sha256": digest(&source_lunge_owner.0),
            "source_tracking_owner_sha256": digest(&source_tracking_owner.0),
            "stock_lunge_graph": "8162C91A",
            "authored_lunge_graph": format!("{:08X}", authored_lunge_tag.0),
            "stock_lunge_owner_sha256": digest(&lunge_owner),
            "authored_lunge_owner_sha256": digest(&authored_lunge_owner),
            "private_action": format!("{:08X}", assignment.runtime_tag),
            "private_action_sha256": digest(&action_bytes),
            "profile_banks": PROFILE_DESCRIPTORS.len(),
            "original_profiles": profiles,
            "keyed_profiles": profiles,
            "profile_key": format!("{PROFILE_KEY:08X}"),
            "stock_key_capacity": old_capacity,
            "private_key_capacity": new_capacity,
            "package_reopened": true,
            "native_objective_consumption": objective_consumption,
            "status": "experimental package candidate",
            "full_perk_installable": false,
            "gameplay_verified": false,
            "remaining": ["Native channel-2 timing proof", "Gameplay verification"]
        }))
        .unwrap(),
    )
    .unwrap();
}
