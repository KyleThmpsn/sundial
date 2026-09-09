use super::*;

#[test]
#[ignore = "requires PARHELION_HUD_TEST_PACKAGES and PARHELION_HUD_STAGE_ROOT"]
fn real_hud_recipe_stages_with_private_texture_and_runtime() {
    let packages =
        PathBuf::from(std::env::var_os("PARHELION_HUD_TEST_PACKAGES").expect("packages"));
    let out = PathBuf::from(std::env::var_os("PARHELION_HUD_STAGE_ROOT").expect("stage root"));
    let mut png = std::io::Cursor::new(vec![]);
    image::RgbaImage::from_pixel(137, 76, image::Rgba([240, 240, 240, 180]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let mut recipe = crate::WeaponRecipe::every_end();
    recipe.overrides.hud_icon =
        Some(crate::hud_icon::HudImage::from_png(&png.into_inner()).unwrap());
    let snapshot = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
        package_directory: packages.clone(),
        staging_root: out,
        ignore_installed_authored_overlays: true,
        recipes: vec![recipe],
    })
    .unwrap();
    let build = crate::build_and_stage_snapshot_with_progress(&snapshot, |p| {
        eprintln!("HUD stage: {} {}/{}", p.phase.label(), p.completed, p.total)
    })
    .unwrap();
    assert!(
        build
            .artifacts
            .iter()
            .any(|a| a.file_name == "w64_ui_037e_6.pkg")
    );
    // Overlay rows can still reference stock blocks; reopen a complete isolated package view.
    let view = tempfile::tempdir_in(packages.parent().unwrap()).unwrap();
    let view_packages = view.path().join("packages");
    fs::create_dir(&view_packages).unwrap();
    for entry in fs::read_dir(&packages).unwrap() {
        let entry = entry.unwrap();
        if entry.path().extension().is_some_and(|ext| ext == "pkg")
            && !crate::package_profile::CANONICAL_ARTIFACT_FILE_NAMES
                .iter()
                .any(|name| entry.file_name() == *name)
        {
            fs::hard_link(entry.path(), view_packages.join(entry.file_name())).unwrap();
        }
    }
    for artifact in &build.artifacts {
        fs::hard_link(
            build.run_directory.join(&artifact.file_name),
            view_packages.join(&artifact.file_name),
        )
        .unwrap();
    }
    let bin = view.path().join("bin/x64");
    fs::create_dir_all(&bin).unwrap();
    fs::hard_link(
        packages
            .parent()
            .unwrap()
            .join("bin/x64/oo2core_3_win64.dll"),
        bin.join("oo2core_3_win64.dll"),
    )
    .unwrap();
    let staged = open_manager(&view_packages).unwrap();
    let table = staged.read_tag(crate::hud_icon::assets::TABLE).unwrap();
    assert_eq!(read_u64(&table, 8).unwrap(), 113);
    let identity = snapshot.request.recipes[0].to_spec().unwrap().identity;
    let row = table[48..]
        .chunks_exact(112)
        .find(|row| read_u32(row, 0).unwrap() == identity.type_hash)
        .unwrap();
    let layer = staged.read_tag(TagHash(read_u32(row, 4).unwrap())).unwrap();
    let texture = TagHash(read_u32(&layer, 0x80).unwrap());
    assert_eq!(texture.pkg_id(), PARHELION_ASSET_PACKAGE_ID);
    let pixels = staged
        .read_tag(TagHash(staged.get_entry(texture).unwrap().reference))
        .unwrap();
    assert_eq!(
        pixels,
        snapshot.request.recipes[0]
            .overrides
            .hud_icon
            .as_ref()
            .unwrap()
            .rgba()
    );
    let assignments = staged
        .read_tag(TagHash(SANDBOX_PATTERN_ENTITY_ASSIGNMENT_TAG))
        .unwrap();
    let entity = weapon_entity_assignment(&assignments, identity.pattern_global_id_hash)
        .unwrap()
        .unwrap();
    let entity = staged.read_tag(TagHash(entity)).unwrap();
    let binding = weapon_component_bindings(&entity, 0x5F0DD954).unwrap()[0];
    let owner = staged.read_tag(TagHash(binding.owner_tag)).unwrap();
    for patch in crate::hud_icon::runtime::patches(&staged, &entity, identity.type_hash).unwrap() {
        let offset = binding.resource_offset as usize + patch.offset as usize;
        assert_eq!(read_u32(&owner, offset).unwrap(), identity.type_hash);
    }
    eprintln!("HUD staged at {}", build.run_directory.display());
}

#[test]
#[ignore = "requires PARHELION_HUD_TEST_PACKAGES pointing to Shadowkeep packages"]
#[expect(
    clippy::cognitive_complexity,
    reason = "Independent byte-level audit checks all content variants and preserves each stock HUD row"
)]
fn hud_icon_private_graph_preserves_source_and_every_content_variant() {
    use sundial::package_authoring::weapon_runtime::load_weapon_runtime_entity_with_manager;
    let packages =
        PathBuf::from(std::env::var_os("PARHELION_HUD_TEST_PACKAGES").expect("packages"));
    let manager = open_manager(&packages).unwrap();
    let source = load_weapon_runtime_entity_with_manager(&manager, 0x02222CBF).unwrap();
    let binding = weapon_component_bindings(&source.payload, 0x5F0DD954).unwrap()[0];
    let stock = read_tag(&manager, TagHash(binding.owner_tag), "content").unwrap();
    let key = 0xEF123456;
    let patches = crate::hud_icon::runtime::patches(&manager, &source.payload, key).unwrap();
    assert_eq!(patches.len(), 7);
    let expected = std::iter::once(0x368)
        .chain((0..6).map(|i| 0x4e0 + i * 0x1c0 + 0xe0))
        .collect::<Vec<_>>();
    let actual = patches
        .iter()
        .map(|p| binding.resource_offset as usize + p.offset as usize)
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
    let allocator = AppendedTagAllocator::new(HOST_PACKAGE_ID, HOST_EXPECTED_ENTRY_COUNT + 256);
    let mut entity = source.payload.clone();
    let mut tags = vec![];
    append_patched_runtime_resource_owners(
        &manager,
        &mut entity,
        &[],
        &patches,
        allocator,
        &mut tags,
    )
    .unwrap();
    assert_eq!(tags.len(), 1);
    let authored = allocator.assigned_tag(0, "test", "HUD owner").unwrap();
    let mut normalized = tags[0].payload.clone();
    for at in expected {
        assert_eq!(&normalized[at..at + 4], &key.to_le_bytes());
        normalized[at..at + 4].copy_from_slice(&stock[at..at + 4]);
    }
    retarget_weapon_component_owner_payload(
        &mut normalized,
        &entity,
        authored.0,
        binding.owner_tag,
    )
    .unwrap();
    assert_eq!(normalized, stock, "all non-HUD content bytes must survive");
    let mut png = std::io::Cursor::new(vec![]);
    image::RgbaImage::from_pixel(137, 76, image::Rgba([10, 20, 30, 140]))
        .write_to(&mut png, image::ImageFormat::Png)
        .unwrap();
    let image = crate::hud_icon::HudImage::from_png(&png.into_inner()).unwrap();
    let mut assets = vec![];
    let mut refs = vec![];
    let replacement = crate::hud_icon::assets::build(
        &manager,
        std::iter::once((key, &image)),
        &mut assets,
        &mut refs,
    )
    .unwrap()
    .unwrap();
    assert_eq!(assets.len(), 3);
    assert_eq!(refs.len(), 2);
    assert_eq!(assets[0].payload, image.rgba());
    let before = manager.read_tag(replacement.tag).unwrap();
    assert_eq!(read_u64(&replacement.payload, 8).unwrap(), 113);
    let rows: std::collections::BTreeMap<_, _> = replacement.payload[48..]
        .chunks_exact(112)
        .map(|r| (read_u32(r, 0).unwrap(), r))
        .collect();
    for row in before[48..].chunks_exact(112) {
        assert_eq!(rows[&read_u32(row, 0).unwrap()], row);
    }
    assert_eq!(
        read_u32(rows[&key], 4).unwrap(),
        TagHash::new(PARHELION_ASSET_PACKAGE_ID, 2).0
    );
    assert_eq!(manager.read_tag(replacement.tag).unwrap(), before);
    assert!(
        crate::hud_icon::assets::build(
            &manager,
            std::iter::once((0x08491234, &image)),
            &mut vec![],
            &mut vec![]
        )
        .is_err()
    );
}
