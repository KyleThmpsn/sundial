//! Real-package authoring and independent render inputs for private shader glow.
use super::*;
use crate::tag_payload::{array_at, relative_target};
use serde_json::json;

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES and SUNDIAL_TEST_ARTIFACTS"]
fn authored_glow_keeps_stock_materials_and_stages_private_programs() {
    let packages = crate::test_support::stock_packages();
    let output = crate::test_support::artifact_dir("glow");
    assert!(!output.exists(), "Choose a fresh artifact directory");
    fs::create_dir_all(&output).unwrap();
    let mut original = crate::WeaponRecipe::new_weapon_for_donor(
        "parhelion.glow-original.integration",
        0x1BE9_5651,
        "Nature of the Beast",
    )
    .unwrap();
    original.name = "Original Glow Control".into();
    assert!(!original.overrides.shader_glow);
    let mut enabled = original.clone();
    enabled
        .rename_authored_item("Authored Glow Control")
        .unwrap();
    enabled.overrides.shader_glow = true;
    enabled
        .save_json(output.join("glow.parhelion.json"))
        .unwrap();
    let enabled = crate::WeaponRecipe::load_json(output.join("glow.parhelion.json")).unwrap();
    assert!(enabled.overrides.shader_glow);
    let bundle = build_weapon_project(
        &packages,
        &WeaponProjectSpec {
            weapons: vec![original.to_spec().unwrap(), enabled.to_spec().unwrap()],
        },
    )
    .unwrap();
    let view = staged_view(&packages, ".parhelion-private-glow-", &bundle);
    let staged = open_manager(&view.path().join("packages")).unwrap();
    let stock = open_manager(&packages).unwrap();
    let plan = |hash| {
        bundle
            .plan
            .weapons
            .iter()
            .find(|p| p.item_hash == hash)
            .unwrap()
    };
    let before_plan = plan(original.identity.item_hash.parse_u32().unwrap());
    let after_plan = plan(enabled.identity.item_hash.parse_u32().unwrap());
    let before = tree(&staged, before_plan.definition_tag);
    let after = tree(&staged, after_plan.definition_tag);
    let source = tree(&stock, before_plan.template_definition_tag);
    assert_eq!(before.len(), after.len());
    assert_eq!(before.len(), source.len());
    let mut receipts = Vec::new();
    for (index, ((before, after), source)) in before.iter().zip(&after).zip(&source).enumerate() {
        compare_stock(&staged, before, after, source);
        if before.code == after.code {
            assert_eq!(before.material, after.material);
            continue;
        }
        receipts.push(compare_private(&staged, before, after, &output, index));
    }
    let changed = receipts.len();
    assert!(
        changed > 0,
        "No native material gained a private glow program"
    );
    let package_output = output.join("packages");
    fs::create_dir_all(&package_output).unwrap();
    bundle.write_new(&package_output).unwrap();
    fs::write(
        output.join("glow.json"),
        serde_json::to_vec_pretty(&json!({
            "materials":receipts,"changed":changed,"stock_materials_unchanged":true,
            "source_packages":packages,"gameplay_verified":false,
        }))
        .unwrap(),
    )
    .unwrap();
}

fn compare_stock(manager: &PackageManager, before: &Material, after: &Material, source: &Material) {
    assert_eq!(
        before.material, source.material,
        "Disabled control changed the source material"
    );
    assert_eq!(before.code, source.code);
    assert_eq!(
        read(manager, source.material_tag),
        source.material,
        "Stock material was mutated"
    );
    assert_eq!(
        read(manager, source.pixel_tag),
        source.header,
        "Stock pixel header was mutated"
    );
    assert_eq!(
        before.model, after.model,
        "Geometry or other part assignments changed"
    );
}

fn compare_private(
    manager: &PackageManager,
    before: &Material,
    after: &Material,
    output: &Path,
    index: usize,
) -> serde_json::Value {
    assert_ne!(after.material_tag, before.material_tag);
    assert_ne!(after.pixel_tag, before.pixel_tag);
    assert_ne!(after.data_tag, before.data_tag);
    let mut material = after.material.clone();
    material[0x2C8..0x2CC].copy_from_slice(&before.material[0x2C8..0x2CC]);
    assert_eq!(
        material, before.material,
        "Glow changed other material resources or states"
    );
    assert_eq!(
        manager
            .get_entry(TagHash(after.data_tag))
            .unwrap()
            .reference,
        after.pixel_tag
    );
    assert_eq!(
        read_u32(&after.header, 8).unwrap() as usize,
        after.code.len()
    );
    let name = format!("material-{index}");
    fs::write(output.join(format!("{name}-before.dxbc")), &before.code).unwrap();
    fs::write(output.join(format!("{name}-after.dxbc")), &after.code).unwrap();
    fs::write(output.join(format!("{name}.bin")), &before.material).unwrap();
    json!({"name":name,"source_material":format!("{:08X}",before.material_tag),
        "private_material":format!("{:08X}",after.material_tag),
        "private_pixel":format!("{:08X}",after.pixel_tag),
        "private_bytecode":format!("{:08X}",after.data_tag)})
}

struct Material {
    model: Vec<u8>,
    material_tag: u32,
    material: Vec<u8>,
    pixel_tag: u32,
    header: Vec<u8>,
    data_tag: u32,
    code: Vec<u8>,
}

fn read(manager: &PackageManager, tag: u32) -> Vec<u8> {
    manager.read_tag(TagHash(tag)).unwrap()
}

fn rows(data: &[u8], offset: usize, stride: usize) -> Vec<usize> {
    let (count, _, start, _) = array_at(data, offset).unwrap();
    (0..count).map(|i| start + i * stride).collect()
}

/// Read the package links independently of the private-resource writer. Blank only
/// material pointers when comparing the complete model and retain all geometry bindings.
fn tree(manager: &PackageManager, definition: TagHash) -> Vec<Material> {
    let definition = read(manager, definition.0);
    let globals =
        sundial::package_authoring::resolve_live_named_tag(manager, "investment_globals", None)
            .unwrap();
    let globals = read(manager, globals.0);
    let art = read(manager, read_u32(&globals, 16 + 66 * 16).unwrap());
    let art_rows = rows(&art, 8, 32);
    let assignments = read(manager, 0x80EC3F61);
    let assignments = rows(&assignments, 8, 8)
        .into_iter()
        .map(|row| {
            (
                read_u32(&assignments, row).unwrap(),
                read_u32(&assignments, row + 4).unwrap(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut result = Vec::new();
    for arrangement in weapon_art_arrangements(&definition).unwrap() {
        let row = art_rows[usize::from(arrangement.arrangement)];
        let mut keys = vec![
            read_u32(&art, row + 8).unwrap(),
            read_u32(&art, row + 12).unwrap(),
        ];
        for slot in rows(&art, row + 16, 8) {
            let resource = relative_target(&art, slot).unwrap();
            keys.extend(
                rows(&art, resource + 8, 4)
                    .iter()
                    .map(|&at| read_u32(&art, at).unwrap()),
            );
        }
        for key in keys
            .into_iter()
            .filter(|k| ![0, u32::MAX, 0x811C9DC5].contains(k))
        {
            let parent = read(manager, assignments[&key]);
            let entity = read_u32(&parent, 16).unwrap();
            if entity == u32::MAX {
                continue;
            }
            let entity = read(manager, entity);
            for component in rows(&entity, 16, 12) {
                let owner = read(manager, read_u32(&entity, component).unwrap());
                let data = relative_target(&owner, 24).unwrap();
                if data < 4 || read_u32(&owner, data - 4).unwrap() != 0x808072BD {
                    continue;
                }
                let model = read(manager, read_u32(&owner, data + 0x1DC).unwrap());
                let mut normalized = model.clone();
                let parts = rows(&model, 16, 136)
                    .into_iter()
                    .flat_map(|mesh| rows(&model, mesh + 24, 32))
                    .collect::<Vec<_>>();
                for &part in &parts {
                    normalized[part..part + 4].fill(0);
                }
                for part in parts {
                    let tag = read_u32(&model, part).unwrap();
                    if [0, u32::MAX].contains(&tag) {
                        continue;
                    }
                    let material = read(manager, tag);
                    let pixel = read_u32(&material, 0x2C8).unwrap();
                    if [0, u32::MAX].contains(&pixel) {
                        continue;
                    }
                    let header = read(manager, pixel);
                    let data_tag = manager.get_entry(TagHash(pixel)).unwrap().reference;
                    result.push(Material {
                        model: normalized.clone(),
                        material_tag: tag,
                        material,
                        pixel_tag: pixel,
                        header,
                        data_tag,
                        code: read(manager, data_tag),
                    });
                }
            }
        }
    }
    result
}
