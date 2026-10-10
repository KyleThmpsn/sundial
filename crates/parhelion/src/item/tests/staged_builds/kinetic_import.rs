//! Configured source import, persisted reload and independent staged dependency readback.
use super::*;
use crate::perk::import::{Request, prepare};
use crate::recipe::{HexHash, WeaponRecipe, WeaponSocketColumnRecipe};
use serde_json::json;
use sha2::{Digest, Sha256};
use sundial::investment::InvestmentCatalog;
use sundial::package_authoring::sandbox_perk::action;

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES, PARHELION_IMPORT_MODERN_PACKAGES and SUNDIAL_TEST_ARTIFACTS"]
fn kinetic_source_import_reloads_and_stages_private_pulses() {
    let packages = crate::test_support::stock_packages();
    let output = crate::test_support::artifact_dir("kinetic-import");
    fs::create_dir_all(&output).unwrap();
    let catalog = InvestmentCatalog::load_with_cache_path(
        packages.parent().unwrap(),
        &output.join("catalog.json"),
        true,
        |_| {},
    )
    .unwrap();
    let stock = open_shadowkeep_package_manager(&packages).unwrap();
    let globals = stock
        .read_tag(resolve_live_named_tag(&stock, "investment_globals", None).unwrap())
        .unwrap();
    let (donor, socket, plug, perk, socket_count) = catalog.weapon_donors().into_iter()
        .filter(|donor| donor.type_name == "Pulse Rifle")
        .find_map(|summary| {
            let donor = catalog.weapon_donor(summary.hash)?;
            donor.sockets.iter().enumerate().skip(4).find_map(|(socket, row)| {
                let plug = row.native_default?;
                catalog.item_sandbox_perk_indices(plug).into_iter().find_map(|perk| {
                    sundial::package_authoring::sandbox_perk::load_sandbox_perk_runtime_action(&stock, &globals, usize::from(perk)).ok()?;
                    Some((summary.hash, socket, plug, perk, donor.sockets.len()))
                })
            })
        }).expect("configured native corpus has a pulse rifle with a runtime trait");
    let mut recipe =
        WeaponRecipe::new_weapon_for_donor("parhelion.kinetic.e2e", donor, "").unwrap();
    recipe.name = "Kinetic Tremors Import".into();
    recipe
        .overrides
        .socket_columns
        .resize_with(socket_count, || None);
    recipe.overrides.socket_columns[socket] = Some(WeaponSocketColumnRecipe {
        choices: vec![HexHash::new(plug)],
        ..Default::default()
    });
    let modern = PathBuf::from(
        std::env::var_os("PARHELION_IMPORT_MODERN_PACKAGES").expect("modern packages"),
    );
    let prepared = prepare(&Request {
        modern_packages: &modern,
        native_packages: &packages,
        output: &output.join("import"),
        recipe: &recipe,
        plug_hash: 0xE7F42379,
        socket_index: socket as u16,
        choice_index: 0,
        source_plug_hash: plug,
        source_perk_index: perk,
        profile_key: 0xEA641291,
        name: "",
    })
    .unwrap();
    prepared.apply(&mut recipe).unwrap();
    let saved = output.join("weapon.parhelion.json");
    fs::write(&saved, recipe.to_json_pretty().unwrap()).unwrap();
    let reloaded = WeaponRecipe::load_json(&saved).unwrap();
    let variant = &reloaded.overrides.socket_plug_variants[0];
    assert_eq!(variant.name.as_deref(), Some("Kinetic Tremors"));
    assert!(variant.icon.is_some());
    assert!(
        variant
            .description
            .as_deref()
            .is_some_and(|text| text.contains("shockwave"))
    );
    let bundle = build_weapon_project_after_catalog_validation(
        &packages,
        &WeaponProjectSpec {
            weapons: vec![reloaded.to_spec().unwrap()],
        },
    )
    .unwrap();
    let view = staged_view(&packages, ".parhelion-kinetic-", &bundle);
    let staged = open_manager(&view.path().join("packages")).unwrap();
    let private = &bundle.plan.weapons[0].custom_plugs[0].perks[0];
    let map = staged
        .read_tag(TagHash(SANDBOX_PERK_RUNTIME_MAP_TAG))
        .unwrap();
    let assignment = sandbox_perk_runtime_assignment(&map, private.runtime_key)
        .unwrap()
        .unwrap();
    let bytes = staged.read_tag(TagHash(assignment.runtime_tag)).unwrap();
    let decoded = action::decode(&bytes).unwrap();
    let trigger = decoded
        .conditions()
        .into_iter()
        .find(|node| node.kind == 4)
        .unwrap();
    let counter = relative_target(&bytes, trigger.offset + 0xC8).unwrap();
    assert_eq!(read_u32(&bytes, counter - 4).unwrap(), 0x80803DF0);
    assert_eq!(read_u32(&bytes, counter).unwrap(), 11f32.to_bits());
    let spawn = decoded.effects().find(|node| node.kind == 3).unwrap();
    let root = read_u32(&bytes, spawn.offset + 16).unwrap();
    assert_ne!(root, 0x80BC57DD);
    let graph = staged.read_tag(TagHash(root)).unwrap();
    let (count, _, rows, _) = array_at(&graph, 16).unwrap();
    let mut sequence = None;
    for index in 0..count {
        let owner = staged
            .read_tag(TagHash(read_u32(&graph, rows + index * 12).unwrap()))
            .unwrap();
        let d = relative_target(&owner, 24).unwrap();
        if read_u32(&owner, d - 4).unwrap() == 0x808084E9 {
            sequence = Some(owner);
        }
    }
    let sequence = sequence.expect("private pulse sequence");
    let d = relative_target(&sequence, 24).unwrap();
    let (count, _, controls, _) = array_at(&sequence, d + 0x158).unwrap();
    assert!(count > 0);
    let mut finite = false;
    for index in 0..count {
        let control = relative_target(&sequence, controls + index * 24 + 16).unwrap();
        finite |= sequence[control + 0x31] == 3 && sequence[control + 0x32] == 0;
    }
    assert!(finite, "three finite pulses must survive allocation");
    assert_eq!(
        staged.read_tag(TagHash(0x80BC57DD)).unwrap(),
        stock.read_tag(TagHash(0x80BC57DD)).unwrap()
    );
    fs::write(output.join("action.bin"), &bytes).unwrap();
    fs::write(output.join("root.bin"), &graph).unwrap();
    fs::write(output.join("sequence.bin"), &sequence).unwrap();
    let stage = output.join("packages");
    fs::create_dir(&stage).unwrap();
    bundle.write_new(&stage).unwrap();
    fs::write(output.join("readback.json"), serde_json::to_vec_pretty(&json!({
        "source_packages":packages,"modern_packages":modern,"donor":donor,
        "recipe":saved,"root":root,"action":assignment.runtime_tag,
        "action_sha256":Sha256::digest(&bytes).iter().map(|byte| format!("{byte:02x}")).collect::<String>(),
        "provenance":prepared.provenance,"stock_unchanged":true,"gameplay_verified":false
    })).unwrap()).unwrap();
}
