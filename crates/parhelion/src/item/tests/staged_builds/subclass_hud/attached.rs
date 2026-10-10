//! Attached HUD edits must survive a saved recipe and reach the actual private child references.
//! Failure boundaries: missing variants, editing the Super instead of its child, shared donor
//! mutation, icon selection changing the inherited color, lost artwork, missing color bindings,
//! two recipes sharing private glyphs, stale edits after reset and unreachable targets accepted.
//! Color selection also covers a native Solar child inheriting red, with and without icon edits,
//! an explicit child color, a separately colored Super, restoration and later stock selection.
use super::*;
use crate::subclass::{AttachedAbility, EntryEdits, EntryIcon, Place, layout};
use sundial::package_authoring::entity::weapon_component_bindings;

// Native Shadowkeep fixture. These are independent source-format oracles, not discovery results.
const SUPER: u32 = 0x80BA_A847;
const CHILDREN: [u32; 2] = [0x80BA_A866, 0x80BA_A9D5];
const COLORS: [[u8; 3]; 2] = [[36, 214, 173], [232, 57, 156]];

/// Follow the copied parent's corresponding native binding, then the field naming its child.
fn child(
    manager: &PackageManager,
    parent: u32,
    source: u32,
    binding: u32,
    index: usize,
    owner: u32,
    offset: usize,
) -> u32 {
    let stock = manager.read_tag(TagHash(source)).unwrap();
    let copy = manager.read_tag(TagHash(parent)).unwrap();
    let source = weapon_component_bindings(&stock, binding).unwrap();
    let copies = weapon_component_bindings(&copy, binding).unwrap();
    assert_eq!(
        source[index].owner_tag, owner,
        "the native reference starts at the expected binding"
    );
    let binding = &copies[index];
    let payload = manager.read_tag(TagHash(binding.owner_tag)).unwrap();
    let at = binding.resource_offset as usize + offset;
    u32::from_le_bytes(payload[at..at + 4].try_into().unwrap())
}

/// Read both controller fields through actual component references, without the HUD writer.
fn controller_glyph(manager: &PackageManager, entity: u32, class: u32) -> u32 {
    let payload = manager.read_tag(TagHash(entity)).unwrap();
    let bindings = weapon_component_bindings(&payload, 0x95A60F29).unwrap();
    let controller = bindings.iter().find(|b| b.concrete_class == class).unwrap();
    let owner = manager.read_tag(TagHash(controller.owner_tag)).unwrap();
    let at = controller.resource_offset as usize;
    let definition = u64::from_le_bytes(owner[at + 8..at + 16].try_into().unwrap()) as usize;
    let read = |start| u32::from_le_bytes(owner[start..start + 4].try_into().unwrap());
    let glyph = read(at + 0x1C8);
    assert_eq!(
        glyph,
        read(definition + 0x1C8),
        "instance and definition agree"
    );
    glyph
}

fn child_glyph(manager: &PackageManager, entity: u32) -> u32 {
    controller_glyph(manager, entity, 0x808041B1)
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES, SUNDIAL_TEST_ARTIFACTS and SUNDIAL_TEST_REVISION"]
#[expect(
    clippy::cognitive_complexity,
    reason = "The E2E keeps saved inputs and independent emitted-reference assertions together"
)]
fn attached_ability_hud_survives_save_build_and_reset_without_changing_stock() {
    let packages = crate::test_support::stock_packages();
    let artifacts = crate::test_support::artifact_dir("attached-hud");
    fs::create_dir_all(&artifacts).unwrap();
    let revision = std::env::var("SUNDIAL_TEST_REVISION").expect("source revision");
    let ignored = crate::package_profile::CANONICAL_ARTIFACT_FILE_NAMES
        .iter()
        .map(|name| (*name).to_owned())
        .collect::<Vec<_>>();
    let view = crate::workflow::FilteredPackageView::create(&packages, &ignored).unwrap();
    let install = view.path().parent().unwrap();
    fs::write(install.join("destiny2.exe"), []).unwrap();
    let cache = artifacts.join("catalog.json");
    let catalog = InvestmentCatalog::load_with_cache_path(install, &cache, true, |_| {}).unwrap();
    let subclasses = catalog.subclasses(crate::package_profile::is_stock_item_definition);
    let base = subclasses
        .iter()
        .find(|s| s.name == "Sunbreaker")
        .expect("Sunbreaker corpus");
    assert_eq!(base.entry_entities[&layout::SUPER], SUPER);
    let donor = subclasses
        .iter()
        .find(|s| s.name == "Arcstrider")
        .expect("Arcstrider corpus");
    let donor_entry = layout::GRENADES[0];
    let source = open_manager(view.path()).unwrap();
    let stock_rows = rows(&source);
    let stock_super = glyph(&source, SUPER).unwrap();
    let donor_glyph = glyph(&source, donor.entry_entities[&donor_entry]).unwrap();
    for child in CHILDREN {
        assert_eq!(child_glyph(&source, child), stock_super);
    }
    let mut recipes = Vec::new();
    for (index, color) in [
        Some(COLORS[0]),
        Some(COLORS[1]),
        None,
        Some(COLORS[0]),
        None,
        None,
        None,
        None,
        Some(COLORS[0]),
        None,
        None,
    ]
    .into_iter()
    .enumerate()
    {
        let mut recipe = crate::WeaponRecipe::new_unbound_kind(crate::ItemKind::Subclass).unwrap();
        recipe.set_donor(base.hash, base.name.clone());
        recipe
            .rename_authored_item(format!("Attached HUD {index}"))
            .unwrap();
        let mut abilities = SubclassAbilities {
            hud_color: (index >= 6).then_some([255, 0, 0]),
            ..Default::default()
        };
        let mut edits = EntryEdits {
            color: (index == 9).then_some(COLORS[1]),
            ..Default::default()
        };
        for graph in CHILDREN.into_iter().filter(|_| index != 6) {
            edits.set_attached(AttachedAbility {
                graph,
                icon: if index == 3 {
                    None
                } else if index == 5 {
                    let pixels = image::RgbaImage::from_fn(96, 96, |x, y| {
                        image::Rgba(if (20..76).contains(&x) && (20..76).contains(&y) {
                            [255; 4]
                        } else {
                            [0; 4]
                        })
                    });
                    let mut png = std::io::Cursor::new(Vec::new());
                    pixels.write_to(&mut png, image::ImageFormat::Png).unwrap();
                    Some(EntryIcon::Artwork {
                        artwork: crate::perk::Icon::Image {
                            name: "Square".into(),
                            image: crate::icon_edit::ImportedIcon::from_bytes(png.get_ref())
                                .unwrap(),
                        },
                    })
                } else {
                    Some(EntryIcon::Ability {
                        subclass: donor.hash,
                        entry: donor_entry,
                    })
                },
                color: if matches!(index, 2 | 10) {
                    Some(COLORS[0])
                } else {
                    color
                },
            });
        }
        abilities.set_edits(base.hash, Place::Ability(layout::SUPER), edits);
        recipe.overrides.subclass_abilities = Some(abilities);
        let file = artifacts.join(format!("attached-{index}.parhelion.json"));
        recipe.save_json(&file).unwrap();
        recipe = crate::WeaponRecipe::load_json(&file).unwrap();
        if matches!(index, 2 | 10) {
            let abilities = recipe.overrides.subclass_abilities.as_mut().unwrap();
            let mut edits = abilities.edits(base.hash, Place::Ability(layout::SUPER));
            for graph in CHILDREN {
                edits.set_attached(AttachedAbility {
                    graph,
                    icon: None,
                    color: None,
                });
            }
            abilities.set_edits(base.hash, Place::Ability(layout::SUPER), edits);
            recipe.save_json(&file).unwrap();
            recipe = crate::WeaponRecipe::load_json(&file).unwrap();
        }
        recipes.push(recipe);
    }
    // These attached abilities have valid empty bank row descriptors. Controller-only edits
    // must not require a populated property table or mutate the shared empty bank.
    let rowless = [
        (
            "Striker", 0x80BAA0A2, 0x80BAA00A, 0xECC9DFCB, 3, 0x80BC3CB1, 4568, 0x808041C3,
        ),
        (
            "Dawnblade",
            0x80BAAB34,
            0x80BAAC05,
            0x95A60F29,
            4,
            0x80BC41E0,
            3072,
            0x808041B1,
        ),
    ];
    for (name, root, attached, ..) in rowless {
        let base = subclasses.iter().find(|s| s.name == name).unwrap();
        assert_eq!(base.entry_entities[&layout::SUPER], root);
        let mut recipe = crate::WeaponRecipe::new_unbound_kind(crate::ItemKind::Subclass).unwrap();
        recipe.set_donor(base.hash, base.name.clone());
        recipe
            .rename_authored_item(format!("Attached {name}"))
            .unwrap();
        let mut abilities = SubclassAbilities::default();
        let mut edits = EntryEdits::default();
        edits.set_attached(AttachedAbility {
            graph: attached,
            icon: None,
            color: Some(COLORS[0]),
        });
        abilities.set_edits(base.hash, Place::Ability(layout::SUPER), edits);
        recipe.overrides.subclass_abilities = Some(abilities);
        let file = artifacts.join(format!("attached-{name}.parhelion.json"));
        recipe.save_json(&file).unwrap();
        recipes.push(crate::WeaponRecipe::load_json(&file).unwrap());
    }
    let snapshot = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
        package_directory: view.path().to_path_buf(),
        staging_root: artifacts.join("staging"),
        ignore_installed_authored_overlays: false,
        recipes,
    })
    .unwrap();
    let build = crate::build_and_stage_snapshot_with_progress(&snapshot, |_| {}).unwrap();
    for artifact in &build.artifacts {
        view.add_overlay(&build.run_directory.join(&artifact.file_name))
            .unwrap();
    }
    let staged = open_manager(view.path()).unwrap();
    let catalog = InvestmentCatalog::load_with_cache_path(install, &cache, true, |_| {}).unwrap();
    let subclasses = catalog.subclasses(|_| true);
    let written = rows(&staged);
    check_stock_rows(&stock_rows, &written);
    let mut private = BTreeSet::new();
    let mut readback = Vec::new();
    for index in [0, 1, 3, 4, 5] {
        let authored = subclasses
            .iter()
            .find(|s| s.name == format!("Attached HUD {index}"))
            .unwrap();
        let root = authored.entry_entities[&layout::SUPER];
        assert_ne!(root, SUPER);
        assert_eq!(
            glyph(&staged, root),
            Some(stock_super),
            "the Super keeps its own glyph"
        );
        let children = [
            child(&staged, root, SUPER, 0x95A60F29, 2, 0x80BC3FBD, 3248),
            child(&staged, root, SUPER, 0x95A60F29, 4, 0x80BC3FBF, 3904),
        ];
        for (stock_child, copied) in CHILDREN.into_iter().zip(children) {
            assert_ne!(copied, stock_child);
            let key = child_glyph(&staged, copied);
            assert!(!stock_rows.contains_key(&key));
            assert!(
                private.insert(key),
                "every authored child has a private glyph"
            );
            let row = &written[&key];
            let mut expected = stock_rows[&stock_super].clone();
            if index != 3 && index != 5 {
                expected[4..0x1C].copy_from_slice(&stock_rows[&donor_glyph][4..0x1C]);
            }
            if index < 4 {
                check_color(row, &expected, COLORS[usize::from(index == 1)]);
            } else {
                assert_eq!(
                    &row[0x20..],
                    &expected[0x20..],
                    "changing the icon preserves its inherited color"
                );
                if index == 4 {
                    assert_eq!(&row[4..0x1C], &expected[4..0x1C]);
                } else {
                    let layer = u32::from_le_bytes(row[4..8].try_into().unwrap());
                    assert_eq!(
                        staged.get_entry(TagHash(layer)).unwrap().reference,
                        0x80804A69
                    );
                    assert!(row[8..0x1C].chunks_exact(4).all(|slot| slot == [0xFF; 4]));
                    assert_ne!(
                        row[4..8],
                        expected[4..8],
                        "the glyph draws the embedded artwork's private layer"
                    );
                    let layer = staged.read_tag(TagHash(layer)).unwrap();
                    let lanes = 0x28usize
                        .checked_add_signed(i64::from_le_bytes(
                            layer[0x28..0x30].try_into().unwrap(),
                        ) as isize)
                        .unwrap();
                    let pointer = lanes + 16 + 8;
                    let textures = pointer
                        .checked_add_signed(i64::from_le_bytes(
                            layer[pointer..pointer + 8].try_into().unwrap(),
                        ) as isize)
                        .unwrap();
                    let texture =
                        u32::from_le_bytes(layer[textures + 16..textures + 20].try_into().unwrap());
                    let pixels =
                        crate::icon_edit::render_texture_preview(&staged, TagHash(texture))
                            .unwrap();
                    let center = pixels[(pixels.size[0] / 2, pixels.size[1] / 2)];
                    assert!(
                        center.a() > 240 && center.r() > 240,
                        "the emitted artwork has a white center"
                    );
                    assert_eq!(
                        pixels[(0, 0)].a(),
                        0,
                        "the emitted artwork keeps transparent corners"
                    );
                }
            }
            assert_eq!(child_glyph(&staged, stock_child), stock_super);
            readback.push(serde_json::json!({"case":index,"item":authored.hash,"root":root,"stock_child":stock_child,"child":copied,"glyph":key,"glyph_row":row}));
        }
    }
    for (name, original, attached, binding, index, owner, offset, class) in rowless {
        let item = subclasses
            .iter()
            .find(|s| s.name == format!("Attached {name}"))
            .unwrap();
        let root = item.entry_entities[&layout::SUPER];
        let copied = child(&staged, root, original, binding, index, owner, offset);
        assert_ne!(copied, attached);
        let original_glyph = controller_glyph(&source, attached, class);
        let key = controller_glyph(&staged, copied, class);
        assert!(private.insert(key));
        check_color(&written[&key], &stock_rows[&original_glyph], COLORS[0]);
        assert_eq!(controller_glyph(&staged, attached, class), original_glyph);
        assert_eq!(glyph(&staged, root), glyph(&source, original));
        readback.push(serde_json::json!({"item":name,"root":root,"child":copied,"glyph":key,"empty_bank":true}));
    }
    let restored = subclasses
        .iter()
        .find(|s| s.name == "Attached HUD 2")
        .unwrap();
    assert_eq!(
        restored.entry_entities[&layout::SUPER],
        SUPER,
        "reset removes the private path"
    );
    let mut selected_colors = Vec::new();
    for index in 0..=10 {
        let item = subclasses
            .iter()
            .find(|s| s.name == format!("Attached HUD {index}"))
            .unwrap();
        let root = item.entry_entities[&layout::SUPER];
        let super_key = glyph(&staged, root).unwrap();
        let expected = match index {
            0 | 3 | 8 => COLORS[0],
            1 => COLORS[1],
            6..=10 => [255, 0, 0],
            _ => published(&stock_rows[&stock_super]),
        };
        for (binding, owner, offset) in [(2, 0x80BC3FBD, 3248), (4, 0x80BC3FBF, 3904)] {
            let graph = child(&staged, root, SUPER, 0x95A60F29, binding, owner, offset);
            let child_key = child_glyph(&staged, graph);
            if index >= 6 {
                assert_eq!(
                    published(&written[&super_key]),
                    if index == 9 { COLORS[1] } else { [255, 0, 0] }
                );
            }
            selected_colors.push(serde_json::json!({
                "case":index,"child":graph,"glyph_row":written[&child_key],
                "super_row":written[&super_key],
                "route":color::check(&staged,&written,super_key,child_key,expected)
            }));
        }
        if index == 9 {
            selected_colors.push(color::check(
                &staged, &written, super_key, super_key, COLORS[1],
            ));
        }
    }
    let voidwalker = subclasses.iter().find(|s| s.name == "Voidwalker").unwrap();
    let void_glyph = glyph(&staged, voidwalker.entry_entities[&layout::SUPER]).unwrap();
    // Changing the selected Super changes the inherited theme even for a Solar child glyph.
    // Restoring the stock selection must not leave an authored recipe's color behind.
    for super_key in [stock_super, void_glyph, stock_super] {
        selected_colors.push(color::check(
            &staged,
            &written,
            super_key,
            stock_super,
            published(&stock_rows[&super_key]),
        ));
    }
    let ui = [
        (0x80BC6F57, 0x80BC6F5A),
        (0x80BC6FB5, 0x80BC6FB6),
        (0x80BC7261, 0x80BC7262),
    ]
    .map(|(widget, hierarchy)| cui_routes(&source, &staged, widget, hierarchy, 2));
    // A reachable graph with no HUD controller and an unrelated live ability are both invalid.
    // Resolve against a clean view so authored overlays cannot masquerade as source inputs.
    let invalid_view = crate::workflow::FilteredPackageView::create(&packages, &ignored).unwrap();
    let mut rejected = Vec::new();
    for target in [0x80BAAB36, donor.entry_entities[&donor_entry]] {
        let mut recipe = snapshot.request.recipes[0].clone();
        let abilities = recipe.overrides.subclass_abilities.as_mut().unwrap();
        let mut edits = abilities.edits(base.hash, Place::Ability(layout::SUPER));
        edits.attached_abilities = vec![AttachedAbility {
            graph: target,
            icon: None,
            color: Some(COLORS[0]),
        }];
        abilities.set_edits(base.hash, Place::Ability(layout::SUPER), edits);
        let invalid = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
            package_directory: invalid_view.path().to_path_buf(),
            staging_root: artifacts.join(format!("rejected-{target:08X}")),
            ignore_installed_authored_overlays: false,
            recipes: vec![recipe],
        })
        .unwrap();
        let error = crate::build_and_stage_snapshot_with_progress(&invalid, |_| {})
            .expect_err("invalid attached HUD target must fail");
        assert!(
            error.to_string().contains("attached"),
            "failure must identify the attached ability: {error}"
        );
        rejected.push(serde_json::json!({"target":target,"error":error.to_string()}));
    }
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(&build.manifest_path).unwrap()).unwrap();
    fs::write(artifacts.join("readback.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "source_build":"86657.20.08.23.1800.d2_rc___release", "revision":revision,
        "inputs":packages,"recipes":snapshot.request.recipes,"manifest":manifest,"readback":readback,"ui":ui,"rejected":rejected,
        "selected_colors":selected_colors,
        "limits":"Saved recipes and independent package readback. In-game activation, repeated use, selection changes and cleanup remain acceptance checks."
    })).unwrap()).unwrap();
}
