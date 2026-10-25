//! Persisted replacement effects keep the plug's presentation across runtime donors.
use super::*;
use crate::recipe::{HexHash, WeaponRecipe, WeaponSocketColumnRecipe};
use serde_json::json;
use sha2::{Digest, Sha256};
use sundial::package_authoring::sandbox_perk::{
    finished_sandbox_perk_at,
    program::{Action, Program, Trigger},
};

/// The staged action at finished perk `perk` has one effect, and it adds two rounds.
fn assert_adds_two_rounds(staged: &PackageManager, globals: &[u8], perk: u16) {
    let action = load_sandbox_perk_runtime_action(staged, globals, usize::from(perk)).unwrap();
    let (count, _, effects, _) = array_at(&action.action_payload, 0x38).unwrap();
    assert_eq!(count, 1);
    let effect = relative_target(&action.action_payload, effects).unwrap();
    assert_eq!(
        read_u32(&action.action_payload, effect - 4).unwrap(),
        0x8080_3E3F
    );
    assert_eq!(read_u32(&action.action_payload, effect + 0x6C).unwrap(), 2);
}

/// The stock finished perk rows at `indices` are the same in the staged table.
fn assert_stock_rows(finished: &[u8], original: &[u8], indices: [usize; 3]) {
    for index in indices {
        assert_eq!(
            finished_sandbox_perk_at(finished, index).unwrap(),
            finished_sandbox_perk_at(original, index).unwrap()
        );
    }
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES and SUNDIAL_TEST_ARTIFACTS"]
fn replacement_effect_tooltips_follow_the_plug_after_persisted_reload() {
    let packages = crate::test_support::stock_packages();
    let output = crate::test_support::artifact_dir("replacement-tooltips");
    fs::create_dir_all(&output).unwrap();
    let stock = open_manager(&packages).unwrap();
    let globals_tag = resolve_live_named_tag(&stock, "investment_globals", None).unwrap();
    let globals = stock.read_tag(globals_tag).unwrap();
    let stock_globals = globals.clone();
    let finished_tag = globals_child_tag(&globals, 71).unwrap();
    let original = stock.read_tag(finished_tag).unwrap();
    let (base, _) = super::companions::item_payloads(&stock, &globals, 0x6876_536E);
    let defaults = weapon_default_plug_indices(&base).unwrap();
    let root = stock
        .read_tag(globals_child_tag(&globals, 0).unwrap())
        .unwrap();
    let items = stock
        .read_tag(root_child_tag(&root, ROOT_ITEM_DEFINITION_TABLE_SLOT).unwrap())
        .unwrap();
    let rows = array_at(&items, 8).unwrap().2;
    let barrel = read_u32(&items, rows + usize::from(defaults[1]) * ITEM_ROW_SIZE).unwrap();
    let (barrel_definition, _) = super::companions::item_payloads(&stock, &globals, barrel);
    assert!(weapon_sandbox_perks(&barrel_definition).unwrap().is_empty());

    // Independent stock witnesses supply ordinary trait and intrinsic presentation.
    // Reverse the runtime carriers so either accidental inheritance is observable.
    let cases = [
        (0x45A0_BDD7, None, 1050, 421),
        (barrel, None, 1050, 421),
        (0xC684_24BC, None, 421, 1050),
        (0xC684_24BC, Some(0x45A0_BDD7), 1050, 421),
        (0x45A0_BDD7, Some(0xC684_24BC), 421, 1050),
    ];
    let mut recipes = Vec::new();
    for (index, &(layout, classification, carrier, expected)) in cases.iter().enumerate() {
        let mut recipe = WeaponRecipe::new_weapon_for_donor(
            format!("parhelion.tooltip.{index}"),
            0x6876_536E,
            "BrayTech Winter Wolf",
        )
        .unwrap();
        let slot = if expected == 1050 { 0 } else { 4 };
        recipe
            .overrides
            .socket_columns
            .resize_with(defaults.len(), || None);
        recipe.overrides.socket_columns[slot] = Some(WeaponSocketColumnRecipe {
            choices: vec![HexHash::new(layout)],
            ..Default::default()
        });
        let mut perk = crate::perk::PerkRecipe::new();
        perk.name = format!("Tooltip Choice {index}");
        perk.description = "Authored tooltip text".into();
        perk.template_plug = HexHash::new(layout);
        perk.classification = classification.map(HexHash::new);
        let mut effect = crate::perk::PerkRecipe::effect(carrier);
        effect.program = Some(Program {
            trigger: Trigger::WeaponKill,
            actions: vec![Action::add_rounds(2)],
            ..Default::default()
        });
        perk.effects = vec![effect];
        let mut variant = perk.at_socket(slot as u16, 0);
        variant.additional_sandbox_perks = vec![1178];
        recipe.overrides.socket_plug_variants.push(variant);
        let path = output.join(format!("case-{index}.parhelion.json"));
        fs::write(&path, recipe.to_json_pretty().unwrap()).unwrap();
        recipes.push(WeaponRecipe::load_json(&path).unwrap());
    }
    let bundle = build_weapon_project_after_catalog_validation(
        &packages,
        &WeaponProjectSpec {
            weapons: recipes.iter().map(|r| r.to_spec().unwrap()).collect(),
        },
    )
    .unwrap();
    let view = staged_view(&packages, ".parhelion-tooltips-", &bundle);
    let staged = open_manager(&view.path().join("packages")).unwrap();
    let globals = staged.read_tag(globals_tag).unwrap();
    let finished = staged.read_tag(finished_tag).unwrap();
    let root = staged
        .read_tag(globals_child_tag(&globals, 0).unwrap())
        .unwrap();
    let items = staged
        .read_tag(root_child_tag(&root, ROOT_ITEM_DEFINITION_TABLE_SLOT).unwrap())
        .unwrap();
    let rows = array_at(&items, 8).unwrap().2;
    let mut observations = Vec::new();
    for (index, (&(layout, classification, carrier, expected), recipe)) in
        cases.iter().zip(&recipes).enumerate()
    {
        let slot = if expected == 1050 { 0 } else { 4 };
        let hash = recipe.to_spec().unwrap().identity.item_hash;
        let (weapon, _) = super::companions::item_payloads(&staged, &globals, hash);
        let plug_index = weapon_default_plug_indices(&weapon).unwrap()[slot];
        let plug_hash = read_u32(&items, rows + usize::from(plug_index) * ITEM_ROW_SIZE).unwrap();
        let (plug, strings) = super::companions::item_payloads(&staged, &globals, plug_hash);
        let (donor, donor_strings) = super::companions::item_payloads(
            &stock,
            &stock_globals,
            classification.unwrap_or(layout),
        );
        assert_eq!(
            crate::plug_classification::PlugClassification::from_template(&plug, &strings).unwrap(),
            crate::plug_classification::PlugClassification::from_template(&donor, &donor_strings)
                .unwrap()
        );
        let perks = weapon_sandbox_perks(&plug).unwrap();
        assert_eq!(perks.len(), 2);
        let detail = finished_sandbox_perk_at(&finished, usize::from(perks[0]))
            .unwrap()
            .detail
            .unwrap();
        let witness = finished_sandbox_perk_at(&original, expected)
            .unwrap()
            .detail
            .unwrap();
        assert_eq!(
            &detail[20..24],
            &witness[20..24],
            "Tooltip inherited runtime donor grouping"
        );
        assert_ne!(
            &detail[..16],
            &finished_sandbox_perk_at(&original, usize::from(carrier))
                .unwrap()
                .detail
                .unwrap()[..16]
        );
        let hidden = finished_sandbox_perk_at(&finished, usize::from(perks[1]))
            .unwrap()
            .detail
            .unwrap();
        assert_eq!(read_u16(&hidden, 0).unwrap(), u16::MAX);
        assert_adds_two_rounds(&staged, &globals, perks[0]);
        assert_stock_rows(&finished, &original, [usize::from(carrier), expected, 1178]);
        let unchanged =
            super::companions::item_payloads(&staged, &globals, classification.unwrap_or(layout));
        assert_eq!(unchanged, (donor, donor_strings));
        observations.push(
            json!({"case":index,"item":hash,"plug":plug_hash,"perks":perks,
            "expected_stock_perk":expected,"detail":detail,"hidden_detail":hidden}),
        );
    }
    let emitted = output.join("packages");
    fs::create_dir(&emitted).unwrap();
    bundle.write_new(&emitted).unwrap();
    let sha = |bytes: &[u8]| hex::encode(Sha256::digest(bytes));
    let artifacts = fs::read_dir(&output).unwrap().map(|entry| entry.unwrap().path())
        .chain(fs::read_dir(&emitted).unwrap().map(|entry| entry.unwrap().path()))
        .filter(|path| path.is_file())
        .map(|path| json!({"file":path.strip_prefix(&output).unwrap(),"sha256":sha(&fs::read(&path).unwrap())}))
        .collect::<Vec<_>>();
    fs::write(
        output.join("readback.json"),
        serde_json::to_vec_pretty(&json!({
            "cases":observations,"native_build":"86657.20.08.23.1800.d2_rc___release",
            "version":env!("CARGO_PKG_VERSION"),"source_packages":packages,"artifacts":artifacts,
            "stock_finished_sha256":sha(&original),"emitted_finished_sha256":sha(&finished),
            "compiler_source_sha256":sha(include_bytes!("../../build/custom_plugs.rs")),
            "executable_sha256":sha(&fs::read(std::env::current_exe().unwrap()).unwrap()),
            "gameplay_verified":false
        }))
        .unwrap(),
    )
    .unwrap();
}
