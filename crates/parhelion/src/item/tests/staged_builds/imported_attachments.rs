//! Persisted attachment imports on an ordinary native weapon, read through staged packages.
use super::*;
use crate::recipe::{
    HexHash, WeaponRecipe, WeaponSocketColumnRecipe, WeaponSocketPlugVariantRecipe,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use sundial::package_authoring::sandbox_perk::{
    action,
    program::{Action, Asset, ImportedAsset, Position, Program, Trigger},
};

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES and SUNDIAL_TEST_ARTIFACTS"]
#[allow(clippy::cognitive_complexity)]
fn persisted_attachment_groups_bind_private_actions_and_owners() {
    let packages = crate::test_support::stock_packages();
    let output = crate::test_support::artifact_dir("imported-attachments");
    fs::create_dir_all(&output).unwrap();
    let stock = open_shadowkeep_package_manager(&packages).unwrap();
    let source_root = 0x80BC_57DD;
    let source_graph = stock.read_tag(TagHash(source_root)).unwrap();
    let (count, _, rows, class) = array_at(&source_graph, 16).unwrap();
    assert!(count > 0);
    assert_eq!(class, 0x8080_9C04);
    let source_owner_tag = read_u32(&source_graph, rows).unwrap();
    let source_owner = stock.read_tag(TagHash(source_owner_tag)).unwrap();
    let graph_directory = output.join("group");
    fs::create_dir_all(&graph_directory).unwrap();
    let mut nodes = Vec::new();
    for (symbol, template, original) in [
        ("attachment-fixture-root", source_root, &source_graph),
        ("attachment-fixture-owner", source_owner_tag, &source_owner),
    ] {
        let mut bytes = original.clone();
        let mut patches = Vec::new();
        for at in (0..bytes.len().saturating_sub(3)).step_by(4) {
            if read_u32(&bytes, at).unwrap() == source_owner_tag {
                bytes[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
                patches.push(json!({"offset":at,"symbol":"attachment-fixture-owner"}));
            }
        }
        assert!(!patches.is_empty());
        fs::write(graph_directory.join(format!("{symbol}.bin")), bytes).unwrap();
        nodes.push(json!({"symbol":symbol,"template":template,"file":format!("{symbol}.bin"),"patches":patches}));
    }
    fs::write(
        graph_directory.join("asset-graph.json"),
        serde_json::to_vec_pretty(&json!({
            "attachments":{"installable":true,"roots":["attachment-fixture-root"],"nodes":nodes}
        }))
        .unwrap(),
    )
    .unwrap();
    let (socket, plug, perk, socket_count) = super::stock::runtime_plug(&stock, 0x0222_2CBF);
    let mut recipe =
        WeaponRecipe::new_weapon_for_donor("parhelion.imported-attachment.e2e", 0x0222_2CBF, "")
            .unwrap();
    recipe.name = "Private Attachment Fixture".into();
    recipe
        .overrides
        .socket_columns
        .resize_with(socket_count, || None);
    recipe.overrides.socket_columns[socket] = Some(WeaponSocketColumnRecipe {
        choices: vec![HexHash::new(plug)],
        ..Default::default()
    });
    let mut effect = crate::perk::PerkRecipe::effect(perk);
    effect.program = Some(Program {
        name: "Imported Attachment".into(),
        trigger: Trigger::WeaponKill,
        actions: vec![Action::Spawn {
            asset: Asset {
                graph: source_root,
                ..Default::default()
            },
            position: Position::Event,
        }],
        imported_assets: vec![ImportedAsset {
            action_index: 0,
            directory: graph_directory.clone(),
            sha256: parhelion_import::d2_mot::native::attachment::fingerprint(&graph_directory)
                .unwrap(),
            symbol: "attachment-fixture-root".into(),
        }],
        ..Default::default()
    });
    recipe
        .overrides
        .socket_plug_variants
        .push(WeaponSocketPlugVariantRecipe {
            socket_index: socket as u16,
            choice_index: 0,
            source_plug_hash: HexHash::new(plug),
            name: Some("Imported Attachment".into()),
            replace_effects: true,
            sandbox_perks: vec![effect],
            investment_stats: Vec::new(),
            classification_donor_hash: None,
            icon: None,
            description: None,
            offer_everywhere: false,
            additional_sandbox_perks: Vec::new(),
        });
    let saved = output.join("fixture.parhelion.json");
    fs::write(&saved, recipe.to_json_pretty().unwrap()).unwrap();
    let reloaded = WeaponRecipe::load_json(&saved).unwrap();
    assert!(reloaded.overrides.imported_graph.is_none());
    let bundle = build_weapon_project_after_catalog_validation(
        &packages,
        &WeaponProjectSpec {
            weapons: vec![reloaded.to_spec().unwrap()],
        },
    )
    .unwrap();
    let view = staged_view(&packages, ".parhelion-imported-attachment-", &bundle);
    let staged = open_manager(&view.path().join("packages")).unwrap();
    let private = &bundle.plan.weapons[0].custom_plugs[0].perks[0];
    let runtime_map = staged
        .read_tag(TagHash(SANDBOX_PERK_RUNTIME_MAP_TAG))
        .unwrap();
    let assignment = sandbox_perk_runtime_assignment(&runtime_map, private.runtime_key)
        .unwrap()
        .unwrap();
    let action_bytes = staged.read_tag(TagHash(assignment.runtime_tag)).unwrap();
    let decoded = action::decode(&action_bytes).unwrap();
    let spawn = decoded.effects().find(|effect| effect.kind == 3).unwrap();
    let root_tag = read_u32(&action_bytes, spawn.offset + 16).unwrap();
    assert_ne!(root_tag, source_root);
    assert_eq!(
        staged.get_entry(TagHash(root_tag)).unwrap().reference,
        0x8080_9C0F
    );
    let graph = staged.read_tag(TagHash(root_tag)).unwrap();
    let (actual_count, _, actual_rows, _) = array_at(&graph, 16).unwrap();
    assert_eq!(actual_count, count);
    let owner_tag = read_u32(&graph, actual_rows).unwrap();
    assert_ne!(owner_tag, source_owner_tag);
    let owner = staged.read_tag(TagHash(owner_tag)).unwrap();
    let mut normalized_graph = graph.clone();
    let mut normalized_owner = owner.clone();
    for bytes in [&mut normalized_graph, &mut normalized_owner] {
        for at in (0..bytes.len().saturating_sub(3)).step_by(4) {
            if read_u32(bytes, at).unwrap() == owner_tag {
                bytes[at..at + 4].copy_from_slice(&source_owner_tag.to_le_bytes());
            }
        }
    }
    assert_eq!(normalized_graph, source_graph);
    assert_eq!(normalized_owner, source_owner);
    assert_eq!(staged.read_tag(TagHash(source_root)).unwrap(), source_graph);
    assert_eq!(
        staged.read_tag(TagHash(source_owner_tag)).unwrap(),
        source_owner
    );
    let changed_path = graph_directory.join("attachment-fixture-owner.bin");
    let original = fs::read(&changed_path).unwrap();
    let mut changed = original.clone();
    changed[0] ^= 1;
    fs::write(&changed_path, changed).unwrap();
    let changed_result = build_weapon_project_after_catalog_validation(
        &packages,
        &WeaponProjectSpec {
            weapons: vec![reloaded.to_spec().unwrap()],
        },
    );
    fs::write(&changed_path, original).unwrap();
    assert!(
        changed_result
            .err()
            .unwrap()
            .to_string()
            .contains("changed after preparation")
    );
    fs::write(output.join("action.bin"), &action_bytes).unwrap();
    fs::write(output.join("attachment.bin"), &graph).unwrap();
    fs::write(output.join("owner.bin"), &owner).unwrap();
    fs::write(output.join("readback.json"), serde_json::to_vec_pretty(&json!({
        "recipe":saved,"recipe_sha256":sha(&fs::read(&saved).unwrap()),
        "source_packages":packages,"source_root":format!("{source_root:08X}"),
        "source_graph_sha256":sha(&source_graph),"source_owner_sha256":sha(&source_owner),
        "item_hash":bundle.plan.weapons[0].item_hash,"runtime_key":private.runtime_key,
        "action_tag":assignment.runtime_tag,"root_tag":root_tag,"owner_tag":owner_tag,
        "action_sha256":sha(&action_bytes),"graph_sha256":sha(&graph),"owner_sha256":sha(&owner),
        "stock_unchanged":true,"installed":false,"gameplay_verified":false,
        "executable_sha256":sha(&fs::read(std::env::current_exe().unwrap()).unwrap())
    })).unwrap()).unwrap();
}
