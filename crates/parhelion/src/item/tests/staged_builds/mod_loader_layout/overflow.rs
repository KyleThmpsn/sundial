//! Persisted batch, native package emission and independent table traversal across record capacity.
//! Failure inventory: docs/record-package-overflow-2026-10-27/acceptance.md.
use super::*;
use sha2::{Digest, Sha256};

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES, SUNRISE_MOD_LOADER_DLL and SUNDIAL_TEST_ARTIFACTS"]
#[allow(
    clippy::cognitive_complexity,
    reason = "Keeps the persisted batch and independent native readback in one E2E workflow"
)]
fn a_large_batch_keeps_its_records_and_artwork_readable_across_packages() {
    let packages = crate::test_support::stock_packages();
    let runtime = crate::test_support::mod_loader_runtime();
    let output = crate::test_support::artifact_dir("record-package-overflow");
    fs::create_dir(&output).expect("the verification output must be fresh");
    let source = open_manager(&packages).unwrap();
    let stock_tags = [
        definition(&source, BOSS_SPEC),
        definition(&source, BREACHLIGHT),
    ];
    let stock_payloads = stock_tags.map(|tag| source.read_tag(tag).unwrap());
    let catalog = InvestmentCatalog::load_with_cache_path(
        packages.parent().unwrap(),
        &output.join("stock-catalog.json"),
        true,
        |_| {},
    )
    .unwrap();
    let donor = catalog.weapon_donor(BREACHLIGHT).unwrap();
    let socket = donor
        .sockets
        .iter()
        .find(|socket| socket.label.ends_with("Weapon Mod"))
        .unwrap();
    let mut perk = PerkRecipe::new();
    perk.template_plug = BOSS_SPEC.into();
    perk.classification = Some(BOSS_SPEC.into());
    perk.name = "Overflow Spec".into();
    let mut weapon = crate::WeaponRecipe::new_weapon_for_donor(
        "parhelion.record-overflow.weapon",
        BREACHLIGHT,
        "Breachlight",
    )
    .unwrap();
    weapon.name = "Overflow Weapon".into();
    weapon.overrides.socket_columns = vec![None; donor.sockets.len()];
    weapon.overrides.socket_columns[socket.index] = Some(WeaponSocketColumnRecipe {
        choices: vec![BOSS_SPEC.into()],
        reusable_plug_set_index: socket.reusable_plug_set_index,
        ..Default::default()
    });
    // This one perk exercises private runtime alongside the large investment record set.
    let mut active = perk.clone();
    active.effects = catalog
        .item_sandbox_perk_indices(BOSS_SPEC)
        .into_iter()
        .map(PerkRecipe::effect)
        .collect();
    assert!(!active.effects.is_empty());
    weapon.overrides.socket_plug_variants = vec![active.at_socket(socket.index as u16, 0)];
    let mut recipes = vec![weapon];
    // The item pairs and private plug leave fourteen slots for the six watermark texture
    // pairs and layer. The first icon owner would occupy the last slot, stranding its companion
    // in the next package. Legacy records already overflow before the artwork starts.
    for index in 0..crate::appended_tags::MAX_PACKAGE_ENTRY_COUNT / 2 - 9 {
        let mut item = crate::WeaponRecipe::new_unbound_kind(crate::ItemKind::Mod).unwrap();
        item.set_donor(BOSS_SPEC, "Boss Spec");
        item.rename_authored_item(format!("Overflow Mod {index:04}"))
            .unwrap();
        let mut item_perk = perk.clone();
        item_perk.name = item.name.clone();
        item_perk.description = item.flavor.clone();
        item_perk.offer_everywhere = true;
        if index < 1800 {
            // Distinct block-aligned icons keep content sharing from collapsing this capacity
            // fixture. Each eight-pixel stripe carries one bit of the item's index.
            let pixels = image::RgbaImage::from_fn(96, 96, |x, _| {
                let shade = if index & (1 << (x / 8)) == 0 { 0 } else { 255 };
                image::Rgba([shade, shade, shade, 255])
            });
            item_perk.icon = Some(crate::perk::Icon::Image {
                name: "capacity.png".into(),
                image: crate::icon_edit::ImportedIcon::from_normalized(pixels).unwrap(),
            });
        }
        item.overrides.socket_plug_variants = vec![item_perk.at_socket(0, 0)];
        recipes.push(item);
    }
    let saved = output.join("recipes");
    fs::create_dir(&saved).unwrap();
    let recipes = recipes
        .into_iter()
        .enumerate()
        .map(|(index, recipe)| {
            let path = saved.join(format!("{index:04}.parhelion.json"));
            recipe.save_json(&path).unwrap();
            crate::WeaponRecipe::load_json(&path).unwrap()
        })
        .collect::<Vec<_>>();
    let mut receipts = Vec::new();
    for (label, runtime) in [("legacy", None), ("loader", Some(runtime.as_path()))] {
        let view = installation(&packages, runtime);
        assert_eq!(
            crate::install::uses_mod_loader(view.path()),
            label == "loader"
        );
        let snapshot = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
            package_directory: view.path().to_owned(),
            staging_root: output.join(label),
            ignore_installed_authored_overlays: false,
            recipes: recipes.clone(),
            maps: Vec::new(),
        })
        .unwrap();
        let mut events = Vec::new();
        let built = crate::build_and_stage_snapshot_with_progress(&snapshot, |event| {
            events.push(format!("{event:?}"));
        })
        .unwrap();
        for artifact in &built.artifacts {
            view.add_overlay(&built.run_directory.join(&artifact.file_name))
                .unwrap();
        }
        let layout = layout(&view, &built);
        let mut package_ids = BTreeSet::new();
        for (name, id, entries, stock) in &layout {
            assert!(package_ids.insert(*id), "duplicate package ID {id:04X}");
            assert!(*entries <= 8192, "{name} exceeds the native entry field");
            if label == "loader"
                && let Some(stock) = stock
            {
                assert_eq!(entries, stock, "loader patches retain stock table lengths");
            }
        }
        let manager = open_manager(view.path()).unwrap();
        let loader_contract = (label == "loader").then(|| read_loader_contract(&built, &manager));
        for (tag, payload) in stock_tags.iter().zip(&stock_payloads) {
            assert_eq!(
                &manager.read_tag(*tag).unwrap(),
                payload,
                "stock donor changed"
            );
        }
        let globals = manager
            .read_tag(resolve_live_named_tag(&manager, "investment_globals", None).unwrap())
            .unwrap();
        let strings = manager
            .read_tag(globals_child_tag(&globals, GLOBALS_ITEM_STRING_TABLE_SLOT).unwrap())
            .unwrap();
        let icons = manager
            .read_tag(globals_child_tag(&globals, GLOBALS_ITEM_ICON_TABLE_SLOT).unwrap())
            .unwrap();
        let (string_count, _, string_rows, _) = array_at(&strings, 8).unwrap();
        let (icon_count, _, icon_rows, _) = array_at(&icons, 8).unwrap();
        let loaded = crate::shared_tag_dependency_index::dependency_entries(
            &manager.read_tag(RUNTIME_DEPENDENCY_COMPANION).unwrap(),
            RUNTIME_DEPENDENCY_COMPANION,
            RUNTIME_DEPENDENCY_ROOT,
        )
        .unwrap();
        let asset_ids = layout
            .iter()
            .filter(|(name, _, _, _)| name.starts_with("w64_parhelion_sandbox_"))
            .map(|(_, id, _, _)| *id)
            .collect::<BTreeSet<_>>();
        assert!(
            asset_ids.len() > 1,
            "private artwork must exceed one asset package"
        );
        let mut records = BTreeSet::new();
        let mut checked_icons = BTreeSet::new();
        let mut items = Vec::new();
        for recipe in &recipes {
            let hash = recipe.identity.item_hash.parse_u32().unwrap();
            let tag = definition(&manager, hash);
            records.insert(tag.pkg_id());
            assert_eq!(
                read_u32(&manager.read_tag(tag).unwrap(), ITEM_DEFINITION_HASH_OFFSET).unwrap(),
                hash
            );
            let row = find_u32_row_key(&strings, string_rows, string_count, ITEM_ROW_SIZE, hash)
                .unwrap()
                .unwrap();
            let text_tag =
                TagHash(read_u32(&strings, string_rows + row * ITEM_ROW_SIZE + 16).unwrap());
            assert_eq!(resolve_item_name(&manager, text_tag).unwrap(), recipe.name);
            for record in [tag, text_tag] {
                if record.pkg_id() != HOST_PACKAGE_ID {
                    assert!(loaded.contains(&(record.pkg_id(), record.entry_index())));
                }
            }
            let text = manager.read_tag(text_tag).unwrap();
            let icon_index = usize::from(read_u16(&text, ITEM_STRING_ICON_INDEX_OFFSET).unwrap());
            assert!(icon_index < icon_count);
            let icon = TagHash(
                read_u32(
                    &icons,
                    icon_rows + icon_index * ITEM_ICON_ROW_SIZE + ITEM_ICON_CONTAINER_OFFSET,
                )
                .unwrap(),
            );
            records.insert(icon.pkg_id());
            if checked_icons.insert(icon) && !asset_ids.contains(&icon.pkg_id()) {
                crate::watermark::validate_icon_definition_graph(&manager, icon).unwrap();
                let icon_payload = manager.read_tag(icon).unwrap();
                let layer = TagHash(
                    read_u32(
                        &icon_payload,
                        sundial::package_authoring::icon_schema::ICON_WATERMARK_LAYER_OFFSET,
                    )
                    .unwrap(),
                );
                let layer_payload = manager.read_tag(layer).unwrap();
                // The fixed six watermark texture lanes are decoded from native bytes, then
                // each texture directory reference is followed to its data entry.
                for offset in (0x80..0x98).step_by(4) {
                    let header = TagHash(read_u32(&layer_payload, offset).unwrap());
                    let header_payload = manager.read_tag(header).unwrap();
                    let data = TagHash(manager.get_entry(header).unwrap().reference);
                    let pixels = manager.read_tag(data).unwrap();
                    let width = usize::from(read_u16(&header_payload, 0x0e).unwrap());
                    let height = usize::from(read_u16(&header_payload, 0x10).unwrap());
                    assert_eq!(pixels.len(), width * height * 4);
                    assert_eq!(manager.get_entry(data).unwrap().reference, header.0);
                    for resource in [icon, layer, header, data] {
                        if resource.pkg_id() != HOST_PACKAGE_ID {
                            assert!(loaded.contains(&(resource.pkg_id(), resource.entry_index())));
                        }
                    }
                }
            }
            items.push(serde_json::json!({"item":hash,"definition":tag.0,"strings":text_tag.0,"icon":icon.0}));
        }
        assert!(
            records.len() > 1,
            "item records and artwork must cross a package boundary"
        );
        let mut artwork_containers = BTreeSet::new();
        for row in 0..icon_count {
            let container = TagHash(
                read_u32(
                    &icons,
                    icon_rows + row * ITEM_ICON_ROW_SIZE + ITEM_ICON_CONTAINER_OFFSET,
                )
                .unwrap(),
            );
            if !asset_ids.contains(&container.pkg_id()) || !artwork_containers.insert(container) {
                continue;
            }
            crate::watermark::validate_icon_definition_graph(&manager, container).unwrap();
            let companion = crate::shared_tag_memory::adjacent_companion_tag(container).unwrap();
            let dependencies = crate::shared_tag_memory::validate_shared_tag_companion_payload(
                &manager.read_tag(companion).unwrap(),
                companion,
                container,
            )
            .unwrap();
            for tag in dependencies.into_iter().map(TagHash) {
                let payload = manager.read_tag(tag).unwrap();
                assert!(!payload.is_empty());
                if checked_icons.contains(&container) {
                    assert!(loaded.contains(&(tag.pkg_id(), tag.entry_index())));
                }
                let entry = manager.get_entry(tag).unwrap();
                if entry.file_type == 32
                    && entry.file_subtype == 1
                    && asset_ids.contains(&tag.pkg_id())
                {
                    let data = TagHash(entry.reference);
                    let pixels = manager.read_tag(data).unwrap();
                    assert_eq!(manager.get_entry(data).unwrap().reference, tag.0);
                    assert_eq!(
                        pixels.len(),
                        usize::from(read_u16(&payload, 14).unwrap())
                            * usize::from(read_u16(&payload, 16).unwrap())
                            * 4
                    );
                }
            }
        }
        assert!(artwork_containers.len() >= 1801);
        assert!(
            artwork_containers
                .iter()
                .map(|tag| tag.pkg_id())
                .collect::<BTreeSet<_>>()
                .len()
                > 1
        );
        let catalog = InvestmentCatalog::load_with_cache_path(
            view.path().parent().unwrap(),
            &output.join(format!("{label}-catalog.json")),
            true,
            |_| {},
        )
        .unwrap();
        let weapon = catalog
            .weapon_donor(recipes[0].identity.item_hash.parse_u32().unwrap())
            .unwrap();
        let private = weapon.sockets[socket.index].native_default.unwrap();
        assert_ne!(private, BOSS_SPEC);
        assert_eq!(catalog.plug_label(private, false), "Overflow Spec");
        assert!(!catalog.item_sandbox_perk_indices(private).is_empty());
        let digests = built
            .artifacts
            .iter()
            .map(|artifact| {
                let data = fs::read(built.run_directory.join(&artifact.file_name)).unwrap();
                (
                    artifact.file_name.clone(),
                    hex::encode(Sha256::digest(data)),
                )
            })
            .collect::<BTreeMap<_, _>>();
        receipts.push(
            serde_json::json!({"layout":label,"packages":layout,"items":items,
            "sha256":digests,"manifest":built.manifest_path,"selection":built.selection_fingerprint,
            "events":events,"private_plug":private,"loader_contract":loader_contract,
            "artwork_containers":artwork_containers.into_iter().map(|tag|tag.0).collect::<Vec<_>>()}),
        );
    }
    let revision = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned());
    let executable = std::env::current_exe().unwrap();
    fs::write(output.join("readback.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "native_build":"86657.20.08.23.1800.d2_rc___release","version":env!("CARGO_PKG_VERSION"),
        "checkout_revision":revision,"test_executable_sha256":crate::artifact::digest_file(&executable).unwrap().sha256,
        "runtime_sha256":crate::artifact::digest_file(&runtime).unwrap().sha256,
        "source_packages":packages,"source_definitions":stock_tags.map(|tag|tag.0),
        "source_sha256":stock_payloads.map(|data|hex::encode(Sha256::digest(data))),
        "recipes":saved,"runs":receipts,
        "limits":"Persisted recipes and staged native package readback. No installation, loader execution or gameplay."
    })).unwrap()).unwrap();
}

/// Decode the loader's metadata and raw table inputs, and follow the install planner's routes.
fn read_loader_contract(
    built: &crate::BuildReport,
    manager: &sundial::package_authoring::PackageManager,
) -> serde_json::Value {
    use sundial::package_authoring::mod_content::{
        merge::{recipes, sidecar::Sidecar},
        plaintext,
    };
    let manifest: crate::manifest::ManifestDocument =
        serde_json::from_slice(&fs::read(&built.manifest_path).unwrap()).unwrap();
    manifest.validate().unwrap();
    let sources = manifest
        .artifacts
        .iter()
        .map(|artifact| {
            (
                artifact.clone(),
                built.run_directory.join(&artifact.file_name),
            )
        })
        .collect::<Vec<_>>();
    let incoming = crate::mod_family::incoming(&manifest.mods, &sources, Vec::new()).unwrap();
    let mut decoded = Vec::new();
    let mut raw_count = 0;
    for (artifact, path) in &sources {
        let records = plaintext::records(path).unwrap();
        let sidecar_path = path.with_extension("merge");
        let sidecar_name = sidecar_path.file_name().unwrap().to_str().unwrap();
        assert!(
            manifest
                .merge_data
                .iter()
                .any(|artifact| artifact.file_name == sidecar_name)
        );
        Sidecar::parse(
            &fs::read(&sidecar_path).unwrap(),
            records.package,
            records.patch,
            &records.digest,
        )
        .unwrap();
        let standalone = (0x0AA0..=0x0CFF).contains(&records.package);
        if standalone {
            assert!(incoming.packages.contains(path));
            assert!(!incoming.patches.contains(path));
            let indices = (0..records.rows.len())
                .filter(|&index| recipes::has_class_rules(records.class(index)))
                .map(|index| index as u32)
                .collect::<Vec<_>>();
            let payloads = plaintext::read_entries(path, &indices).unwrap();
            for (&index, payload) in indices.iter().zip(&payloads) {
                assert_eq!(
                    &manager
                        .read_tag(TagHash::new(records.package, index as u16))
                        .unwrap(),
                    payload
                );
            }
            raw_count += indices.len();
            decoded.push(serde_json::json!({"package":artifact.file_name,"raw_entries":indices,"sidecar":sidecar_name}));
        } else {
            assert!(incoming.patches.contains(path));
            assert!(!incoming.packages.contains(path));
        }
    }
    assert!(
        raw_count > 0,
        "the loader must read authored item table references"
    );
    serde_json::json!({"packages":decoded,"raw_entries":raw_count,"routing":"managed packages and patches"})
}
