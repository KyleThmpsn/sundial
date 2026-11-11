//! Prepared abilities must use the existing private HUD route after a persisted package build.
//! Failure boundaries precede implementation in the task plan. Two choices share a native group,
//! but require different artwork and colors. Follow actual controller, bank and bitmap references,
//! preserve the original group and stock, and retain a repeatable package/readback artifact.
use super::*;
use crate::icon_edit::ImportedIcon;
use std::collections::BTreeSet;
use sundial::package_authoring::{ability_hud, ability_modifier};

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES, SUNDIAL_TEST_ARTIFACTS and SUNDIAL_TEST_REVISION"]
#[allow(clippy::cognitive_complexity)]
fn prepared_choices_keep_independent_hud_artwork_and_colors() {
    let packages = crate::test_support::stock_packages();
    let output = crate::test_support::artifact_dir("imported-ability-hud");
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::create_dir(&output).expect("use a fresh artifact directory");
    let (view, source) = stock_view(&packages, ".parhelion-imported-hud-", Oodle::Required);
    fs::write(view.path().join("destiny2.exe"), []).unwrap();
    let catalog = crate::test_support::catalog(view.path()).unwrap();
    let base = catalog
        .subclasses(crate::package_profile::is_stock_item_definition)
        .into_iter()
        .find(|item| item.name == "Stormcaller")
        .unwrap();
    let fixture = catalog
        .subclasses(crate::package_profile::is_stock_item_definition)
        .into_iter()
        .find(|item| item.name == "Gunslinger")
        .unwrap();
    let stock = open_manager(&source).unwrap();
    let native_root = fixture.entry_entities[&2];
    let graph = stock.read_tag(TagHash(native_root)).unwrap();
    let (count, _, first, _) = array_at(&graph, 16).unwrap();
    let native_bank = ability_modifier::entity_bank(&graph).unwrap().unwrap();
    let bank_index = (0..count)
        .find(|&index| read_u32(&graph, first + index * 12).unwrap() == native_bank)
        .unwrap();
    let mut originals = BTreeMap::from([(native_root, graph.clone())]);
    for index in 0..count {
        let tag = read_u32(&graph, first + index * 12).unwrap();
        originals.insert(tag, stock.read_tag(TagHash(tag)).unwrap());
    }
    let directory = output.join("group");
    fs::create_dir(&directory).unwrap();
    let symbol = |tag| format!("hud-fixture-{tag:08X}");
    let mut nodes = Vec::new();
    for (&tag, original) in &originals {
        let mut bytes = original.clone();
        let mut patches = Vec::new();
        for at in (0..bytes.len().saturating_sub(3)).step_by(4) {
            let target = read_u32(&bytes, at).unwrap();
            if originals.contains_key(&target) {
                bytes[at..at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
                patches.push(json!({"offset":at,"symbol":symbol(target)}));
            }
        }
        let file = format!("{}.bin", symbol(tag));
        fs::write(directory.join(&file), bytes).unwrap();
        nodes.push(json!({"symbol":symbol(tag),"template":tag,"file":file,"patches":patches}));
    }
    fs::write(
        directory.join("asset-graph.json"),
        serde_json::to_vec_pretty(&json!({
            "attachments":{"installable":true,"roots":[symbol(native_root)],"nodes":nodes}
        }))
        .unwrap(),
    )
    .unwrap();
    let fingerprint =
        parhelion_import::d2_mot::native::attachment::fingerprint(&directory).unwrap();
    let imported = ImportedAbility {
        directory: directory.clone(),
        sha256: fingerprint.clone(),
        symbol: symbol(native_root),
        source: None,
    };
    let mut recipe =
        crate::WeaponRecipe::new_weapon_for_donor("parhelion.imported-hud", base.hash, &base.name)
            .unwrap();
    recipe.kind = crate::ItemKind::Subclass;
    recipe.name = "Imported HUD Fixture".into();
    let mut abilities = SubclassAbilities::default();
    let colors = [[47, 209, 111], [132, 196, 235]];
    let alphas = [173, 219];
    for (index, entry) in [2, 3].into_iter().enumerate() {
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(54, 54, image::Rgba([255, 255, 255, alphas[index]]))
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let mut choice = crate::subclass::SubclassChoice::stock(entry, fixture.hash, 2);
        choice.edits = EntryEdits {
            imported_ability: Some(imported.clone()),
            removed_perks: fixture.entry_perks.get(&2).cloned().unwrap_or_default(),
            color: Some(colors[index]),
            hud_artwork: Some(crate::perk::Icon::Image {
                name: format!("HUD {index}"),
                image: ImportedIcon::from_bytes(&png.into_inner()).unwrap(),
            }),
            ..Default::default()
        };
        abilities.choices.push(choice);
    }
    recipe.overrides.subclass_abilities = Some(abilities);
    let saved = output.join("fixture.parhelion.json");
    recipe.save_json(&saved).unwrap();
    let recipe = crate::WeaponRecipe::load_json(&saved).unwrap();
    let snapshot = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
        package_directory: source,
        staging_root: output.join("staging"),
        ignore_installed_authored_overlays: false,
        recipes: vec![recipe],
        maps: Vec::new(),
    })
    .unwrap();
    let build = crate::build_and_stage_snapshot_with_progress(&snapshot, |_| {}).unwrap();
    let (staged_view, staged_path) = stock_view(
        &packages,
        ".parhelion-imported-hud-readback-",
        Oodle::Required,
    );
    fs::write(staged_view.path().join("destiny2.exe"), []).unwrap();
    for artifact in &build.artifacts {
        let target = staged_path.join(&artifact.file_name);
        if target.exists() {
            fs::remove_file(&target).unwrap();
        }
        fs::copy(build.run_directory.join(&artifact.file_name), target).unwrap();
    }
    let staged = open_manager(&staged_path).unwrap();
    let catalog = crate::test_support::catalog(staged_view.path()).unwrap();
    let authored = catalog
        .subclasses(|_| true)
        .into_iter()
        .find(|item| item.name == "Imported HUD Fixture")
        .unwrap();
    let table = staged.read_tag(TagHash(ability_hud::GLYPH_TABLE)).unwrap();
    let source_table = stock.read_tag(TagHash(ability_hud::GLYPH_TABLE)).unwrap();
    let mut roots = BTreeSet::new();
    let mut glyphs = BTreeSet::new();
    let mut observations = Vec::new();
    for (index, entry) in [2, 3].into_iter().enumerate() {
        let root = authored.entry_entities[&entry];
        assert!(roots.insert(root));
        let graph = staged.read_tag(TagHash(root)).unwrap();
        let site = ability_hud::glyph_site(&staged, &graph).unwrap().unwrap();
        assert!(glyphs.insert(site.key));
        // The stock-bank lookup accepts stock identities only. Follow the emitted component
        // reference at the qualified native bank position to read this entry's private bank.
        let (component_count, _, components, _) = array_at(&graph, 16).unwrap();
        assert_eq!(component_count, count);
        let bank = read_u32(&graph, components + bank_index * 12).unwrap();
        assert_ne!(bank, native_bank);
        let bank_bytes = staged.read_tag(TagHash(bank)).unwrap();
        let keys = fixture.entry_modifiers[&2]
            .iter()
            .map(|&(key, _)| key)
            .collect::<Vec<_>>();
        let variants = ability_hud::variant_glyphs(&bank_bytes, &keys).unwrap();
        assert!(
            !variants.is_empty(),
            "the selected Dodge variant supplies the HUD glyph"
        );
        for variant in variants {
            assert_eq!(variant.glyph, site.key);
        }
        let row = table[48..]
            .chunks_exact(112)
            .find(|row| read_u32(row, 0).unwrap() == site.key)
            .unwrap();
        for vector in [0x20, 0x40] {
            for (channel, expected) in colors[index].into_iter().enumerate() {
                let at = vector + channel * 4;
                let linear = f32::from_le_bytes(row[at..at + 4].try_into().unwrap());
                assert_eq!(
                    (f64::from(linear).powf(1.0 / 2.2) * 255.0).round() as u8,
                    expected
                );
            }
        }
        assert_eq!(row[0x60] & 5, 5);
        let layer = staged.read_tag(TagHash(read_u32(row, 4).unwrap())).unwrap();
        let texture = TagHash(read_u32(&layer, 0x80).unwrap());
        let pixels = staged
            .read_tag(TagHash(staged.get_entry(texture).unwrap().reference))
            .unwrap();
        assert!(!pixels.is_empty());
        assert!(
            pixels
                .chunks_exact(4)
                .all(|pixel| pixel == [255, 255, 255, alphas[index]])
        );
        fs::write(output.join(format!("glyph-{}.rgba", site.key)), &pixels).unwrap();
        observations.push(json!({"entry":entry,"root":root,"glyph":site.key,"bank":bank,"rgba_sha256":sha(&pixels),"color":colors[index]}));
    }
    for row in source_table[48..].chunks_exact(112) {
        let key = read_u32(row, 0).unwrap();
        assert_eq!(
            table[48..]
                .chunks_exact(112)
                .find(|r| read_u32(r, 0).unwrap() == key)
                .unwrap(),
            row
        );
    }
    for (&tag, bytes) in &originals {
        assert_eq!(&staged.read_tag(TagHash(tag)).unwrap(), bytes);
    }
    assert_eq!(
        parhelion_import::d2_mot::native::attachment::fingerprint(&directory).unwrap(),
        fingerprint
    );
    fs::write(output.join("readback.json"),serde_json::to_vec_pretty(&json!({
        "revision":std::env::var("SUNDIAL_TEST_REVISION").unwrap(),"native_build":"86657.20.08.23.1800.d2_rc___release",
        "manifest":build.manifest_path,"recipe":saved,"prepared_sha256":fingerprint,"readback":observations,
        "stock_unchanged":true,"prepared_unchanged":true,"installed":false,"gameplay_verified":false,
        "executable_sha256":sha(&fs::read(std::env::current_exe().unwrap()).unwrap())
    })).unwrap()).unwrap();
}
