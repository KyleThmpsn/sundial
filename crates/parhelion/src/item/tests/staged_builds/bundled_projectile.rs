//! Persist a bundled perk, attach it to two weapons, stage and reopen the native assets.
//! The failure model is dependency loss, shared mutable identities, wrong damage records,
//! incompatible firing selection, missing loading links or stock donor mutation.
use super::*;
use sha2::{Digest, Sha256};
use sundial::package_authoring::runtime::load_weapon_runtime_entity_with_manager;

#[cfg(feature = "d2-model-importer")]
mod imports;

fn digest(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

fn relative(bytes: &[u8], offset: usize) -> usize {
    offset
        .checked_add_signed(
            i64::from_le_bytes(bytes[offset..offset + 8].try_into().unwrap()) as isize,
        )
        .unwrap()
}

fn audit_values(template: u32, payload: &[u8]) {
    if [0x8153_3F23, 0x8153_3F29].contains(&template) {
        let definition = relative(payload, 24);
        assert_eq!(read_u32(payload, definition - 4).unwrap(), 0x8080_4B02);
        assert_eq!(payload[definition + 0x20], 1, "native Solar damage enum");
        let expected = if template == 0x8153_3F23 {
            21.0f32
        } else {
            45.0
        };
        for offset in [0x14, 0x18] {
            assert_eq!(
                read_u32(payload, definition + offset).unwrap(),
                expected.to_bits()
            );
        }
    }
    if template == 0x80EF_3972 {
        let definition = relative(payload, 24);
        let (_, _, groups, _) = array_at(payload, definition + 0x158).unwrap();
        let (_, _, actions, _) = array_at(payload, definition + 0x168).unwrap();
        let wave = relative(payload, groups + 3 * 24 + 16);
        let spawn = relative(payload, actions + 4 * 24 + 16);
        let distribution = relative(payload, spawn + 0xB0);
        assert_eq!(payload[wave + 0x31], 2, "two waves");
        assert_eq!(
            crate::tag_payload::read_u16(payload, distribution + 72).unwrap(),
            2,
            "two missiles per wave"
        );
        assert_eq!(read_u32(payload, wave + 0x24).unwrap(), 0.04f32.to_bits());
    }
}

fn audit_graph(manager: &PackageManager, source: &PackageManager, root: u32) -> serde_json::Value {
    let manifest: serde_json::Value = serde_json::from_str(include_str!(
        "../../../weapon/projectile/wolfpack_rounds.json"
    ))
    .unwrap();
    let nodes = manifest["nodes"].as_array().unwrap();
    assert!(!nodes.is_empty());
    let mut pending = vec![(manifest["root"].as_str().unwrap().to_owned(), root)];
    let mut seen = BTreeMap::new();
    let mut tags = BTreeMap::new();
    let mut records = Vec::new();
    while let Some((symbol, tag)) = pending.pop() {
        if let Some(previous) = seen.insert(symbol.clone(), tag) {
            assert_eq!(previous, tag);
            continue;
        }
        let node = nodes.iter().find(|node| node["symbol"] == symbol).unwrap();
        let template = node["template"].as_u64().unwrap() as u32;
        assert_ne!(tag, template, "the reachable node must be private");
        assert!(
            tags.insert(tag, template).is_none(),
            "private nodes have distinct identities"
        );
        let payload = manager.read_tag(TagHash(tag)).unwrap();
        let original = source.read_tag(TagHash(template)).unwrap();
        assert_eq!(digest(&original), node["source_sha256"].as_str().unwrap());
        assert_eq!(manager.read_tag(TagHash(template)).unwrap(), original);
        for link in node["patches"].as_array().unwrap() {
            let offset = link["offset"].as_u64().unwrap() as usize;
            pending.push((
                link["symbol"].as_str().unwrap().to_owned(),
                read_u32(&payload, offset).unwrap(),
            ));
        }
        let entry = manager.get_entry(TagHash(tag)).unwrap();
        if let Some(pair) = node["reference"].as_str() {
            pending.push((pair.to_owned(), entry.reference));
        } else {
            assert_eq!(
                entry.reference,
                node["native_reference"].as_u64().unwrap() as u32
            );
        }
        audit_values(template, &payload);
        records.push(serde_json::json!({"symbol":symbol,"template":template,"tag":tag,
            "sha256":digest(&payload),"source_sha256":digest(&original),"reference":entry.reference}));
    }
    assert_eq!(
        seen.len(),
        nodes.len(),
        "all bundled dependencies are reachable"
    );
    serde_json::json!(records)
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES and SUNDIAL_TEST_ARTIFACTS"]
fn bundled_wolfpack_survives_library_attachment_and_native_staging() {
    let packages = crate::test_support::stock_packages();
    let output = crate::test_support::artifact_dir("bundled-wolfpack");
    fs::create_dir(&output).unwrap();
    let library = crate::perk::library::Library::open(output.join("perks")).unwrap();
    library.materialize_bundled().unwrap();
    let entry = library
        .scan()
        .unwrap()
        .entries
        .into_iter()
        .find(|entry| entry.recipe.name == "Wolfpack Rounds")
        .expect("bundled perk");
    let mut saved = entry.recipe.clone();
    saved.description.push_str(" Persisted library copy.");
    library.save(&saved, Some(&entry.baseline)).unwrap();
    let saved = library
        .scan()
        .unwrap()
        .entries
        .into_iter()
        .find(|entry| entry.recipe.id == saved.id)
        .unwrap()
        .recipe;
    let catalog = InvestmentCatalog::load_with_cache_path(
        packages.parent().unwrap(),
        &output.join("catalog.json"),
        true,
        |_| {},
    )
    .unwrap();
    let mut recipes = Vec::new();
    for (suffix, donor, name) in [
        ("rocket", 0x0823_36CE, "Shining Sphere"),
        ("rifle", 0xA25B_8F8F, "Arc Logic"),
    ] {
        let mut recipe = crate::WeaponRecipe::new_weapon_for_donor(
            format!("parhelion.bundled-wolfpack-{suffix}"),
            donor,
            name,
        )
        .unwrap();
        recipe.name = format!("Bundled Wolfpack {name}");
        recipe.overrides.socket_columns =
            vec![None; catalog.weapon_donor(donor).unwrap().sockets.len()];
        recipe.overrides.socket_columns[4] = Some(crate::recipe::WeaponSocketColumnRecipe {
            choices: vec![saved.template_plug.clone()],
            ..Default::default()
        });
        recipe.overrides.socket_plug_variants = vec![saved.at_socket(4, 0)];
        let path = output.join(format!("{suffix}.weapon.json"));
        recipe.save_json(&path).unwrap();
        recipes.push(crate::WeaponRecipe::load_json(path).unwrap());
    }
    let mut conflict = recipes[0].clone();
    conflict.overrides.fired_graph =
        Some(serde_json::from_value(serde_json::json!({"source_graph": 0x80BB_D121u32})).unwrap());
    assert!(
        conflict.to_spec().is_err(),
        "conflicting projectile must be rejected"
    );
    stage(&packages, &output, recipes);
}

fn stage(packages: &Path, output: &Path, recipes: Vec<crate::WeaponRecipe>) {
    let source = open_manager(packages).unwrap();
    let bundle = build_weapon_project_after_catalog_validation(
        packages,
        &WeaponProjectSpec {
            weapons: recipes
                .iter()
                .map(|recipe| recipe.to_spec().unwrap())
                .collect(),
        },
    )
    .unwrap();
    let view = staged_view(packages, ".parhelion-bundled-projectile-", &bundle);
    let manager = open_manager(&view.path().join("packages")).unwrap();
    let mut roots = BTreeSet::new();
    let mut records = Vec::new();
    for recipe in &recipes {
        let item = recipe.identity.item_hash.parse_u32().unwrap();
        let entity = load_weapon_runtime_entity_with_manager(&manager, item).unwrap();
        let bindings = weapon_component_bindings(&entity.payload, 0x5F0D_D954).unwrap();
        assert!(!bindings.is_empty());
        let mut fired = BTreeSet::new();
        for binding in bindings {
            let owner = manager.read_tag(TagHash(binding.owner_tag)).unwrap();
            let definition =
                read_u64(&owner, binding.resource_offset as usize + 8).unwrap() as usize;
            fired.insert(read_u32(&owner, definition + 0x80 + 0xF0).unwrap());
            if read_u64(&owner, definition + 0x240).unwrap() > 0 {
                let (count, _, start, _) = array_at(&owner, definition + 0x240).unwrap();
                for index in 0..count {
                    fired.insert(read_u32(&owner, start + index * 0x1C0 + 0xF0).unwrap());
                }
            }
        }
        assert_eq!(
            fired.len(),
            1,
            "every content variant uses the private projectile"
        );
        let root = *fired.first().unwrap();
        assert_ne!(root, 0x80BB_D121);
        assert!(roots.insert(root), "each weapon owns its projectile");
        let payload = manager.read_tag(TagHash(root)).unwrap();
        assert_eq!(crate::tag_payload::read_u16(&payload, 0x96).unwrap(), 18);
        let replication = read_u32(&payload, 0x88).unwrap();
        assert_eq!(
            read_u32(&manager.read_tag(TagHash(replication)).unwrap(), 8).unwrap(),
            root
        );
        records.push(serde_json::json!({"item": item, "root": root,
            "replication": replication, "sha256": digest(&payload),
            "nodes": audit_graph(&manager, &source, root)}));
    }
    for template in [0x80BB_D121, 0x8153_3F23, 0x8153_3F29, 0x80EF_3972] {
        assert_eq!(
            manager.read_tag(TagHash(template)).unwrap(),
            source.read_tag(TagHash(template)).unwrap()
        );
    }
    let emitted = output.join("packages");
    fs::create_dir(&emitted).unwrap();
    let paths = bundle.write_new(&emitted).unwrap();
    let packages = paths.iter().map(|path| serde_json::json!({
        "file":path.file_name().unwrap().to_string_lossy(),"sha256":digest(&fs::read(path).unwrap())
    })).collect::<Vec<_>>();
    fs::write(output.join("readback.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "recipes": recipes, "weapons": records, "packages": packages, "stock_preserved": true,
        "native_build":"86657.20.08.23.1800.d2_rc___release",
        "limits": "Package readback only. Live Solar appearance, targeting and damage need gameplay verification."
    })).unwrap()).unwrap();
}
