use super::*;

#[test]
#[ignore = "requires PARHELION_SOCKET_TEST_PACKAGES, reads through an isolated package view"]
fn native_perk_icons_author_private_wrappers_and_preserve_stock_pixels() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_SOCKET_TEST_PACKAGES").unwrap());
    let ignored = crate::package_profile::CANONICAL_ARTIFACT_FILE_NAMES
        .iter()
        .map(|s| (*s).to_owned())
        .collect::<Vec<_>>();
    let view = crate::workflow::FilteredPackageView::create(&packages, &ignored).unwrap();
    let sources = sources::load_project_sources(view.path()).unwrap();
    let mut selected = None;
    crate::icon_edit::package_icons::scan(&sources.manager, |_, _, entry| {
        if let Some(entry) = entry.filter(|entry| entry.white) {
            selected = Some(entry.tag);
            false
        } else {
            true
        }
    });
    let texture = selected.expect("Installed transparent icons");
    let source_pixels = sources
        .manager
        .read_tag(TagHash(
            sources.manager.get_entry(texture).unwrap().reference,
        ))
        .unwrap();
    let mut weapon = crate::WeaponRecipe::from_json_str(include_str!(
        "../../../../recipes/redacted.parhelion.json"
    ))
    .unwrap()
    .to_spec()
    .unwrap();
    weapon.overrides.socket_plug_variants[0].icon = Some(crate::perk::Icon::Texture {
        tag: texture.0.into(),
    });
    let resolved = resolve::resolve_project_weapons(&sources, &[weapon.clone()]).unwrap();
    let templates = PerkTemplates::read(&sources).unwrap();
    let mut plugs = custom_plugs::plan(&sources, &resolved, &templates.strings).unwrap();
    let stock_container = plugs[0].source_icon_container;
    assert_stock_perk_quality(&sources.manager, stock_container);
    let stock_payload = sources.manager.read_tag(stock_container).unwrap();
    let assets = plan(
        &sources.manager,
        &resolved,
        1,
        &mut plugs,
        crate::branding::Branding::for_packages(view.path()),
    )
    .unwrap();
    let icons = author_icon_rows(
        sources.stock_item_icons.clone(),
        &resolved,
        &assets,
        &mut plugs,
    )
    .unwrap();
    let plug = &plugs[0];
    let authored_container = plug.authored_icon_container.unwrap();
    assert_ne!(authored_container, stock_container);
    assert_eq!(authored_container.pkg_id(), PARHELION_ASSET_PACKAGE_ID);
    let index = read_u16(&plug.source_strings, ITEM_STRING_ICON_INDEX_OFFSET).unwrap();
    validate_authored_item_icon(
        &icons.payload,
        &plug.source_strings,
        plug.authored_item_hash,
        index,
        authored_container,
    )
    .unwrap();
    let container = &assets.badge.new_tags[usize::from(authored_container.entry_index())].payload;
    let layer_tag = TagHash(read_u32(container, 0x14).unwrap());
    let layer = &assets.badge.new_tags[usize::from(layer_tag.entry_index())].payload;
    let glyph_textures = crate::icon_edit::texture_reference_offsets(layer, layer_tag).unwrap();
    let glyph_sizes = glyph_textures
        .iter()
        .map(|(_, tag)| {
            let header = &assets.badge.new_tags[usize::from(tag.entry_index())].payload;
            [read_u16(header, 14).unwrap(), read_u16(header, 16).unwrap()]
        })
        .collect::<Vec<_>>();
    assert_eq!(glyph_sizes, [[96, 96], [40, 40]]);
    assert_preserved_icon_layers(&stock_payload, container);
    assert_eq!(
        sources.manager.read_tag(stock_container).unwrap(),
        stock_payload
    );
    assert_eq!(
        sources
            .manager
            .read_tag(TagHash(
                sources.manager.get_entry(texture).unwrap().reference
            ))
            .unwrap(),
        source_pixels
    );
    eprintln!("Verified native texture {texture} and private icon {authored_container}");
    let private_strings = plug.authored_string_tag;
    let icon_table = sources.item_icon_table_tag;
    let finished_table = sources.finished_sandbox_perk_table_tag;
    let expected_container = authored_container;
    let mut png = std::io::Cursor::new(Vec::new());
    let pixels = image::RgbaImage::from_fn(96, 96, |x, y| {
        image::Rgba(if (24..72).contains(&x) && (24..72).contains(&y) {
            [255; 4]
        } else {
            [0; 4]
        })
    });
    pixels.write_to(&mut png, image::ImageFormat::Png).unwrap();
    let imported = crate::perk::Icon::Image {
        name: "test.png".into(),
        image: crate::icon_edit::ImportedIcon::from_bytes(png.get_ref()).unwrap(),
    };
    let mut imported_nodes = Vec::new();
    let mut references = Vec::new();
    let imported_container = crate::icon_edit::package_icons::author(
        &sources.manager,
        &imported,
        stock_container,
        &mut imported_nodes,
        &mut references,
    )
    .unwrap();
    assert_eq!(imported_container.pkg_id(), PARHELION_ASSET_PACKAGE_ID);
    assert!(
        !references.is_empty(),
        "Imported artwork owns its pixel resources"
    );
    assert!(
        imported_nodes
            .iter()
            .any(|node| node.payload == *pixels.as_raw())
    );
    drop(sources);
    let bundle = build_weapon_project_after_catalog_validation(
        view.path(),
        &WeaponProjectSpec {
            weapons: vec![weapon],
        },
    )
    .unwrap();
    let staged = tempfile::tempdir().unwrap();
    for artifact in &bundle.artifacts {
        let path = staged.path().join(&artifact.plan.output_file_name);
        fs::write(&path, artifact.bytes()).unwrap();
        view.add_overlay(&path).unwrap();
    }
    let manager = open_shadowkeep_package_manager(view.path()).unwrap();
    let strings = manager.read_tag(private_strings).unwrap();
    let icons = manager.read_tag(icon_table).unwrap();
    validate_authored_item_icon(
        &icons,
        &strings,
        plug.authored_item_hash,
        index,
        expected_container,
    )
    .unwrap();
    let container = manager.read_tag(expected_container).unwrap();
    assert_eq!(TagHash(read_u32(&container, 0x14).unwrap()), layer_tag);
    let layer = manager.read_tag(layer_tag).unwrap();
    for ((_, tag), expected_size) in glyph_textures.iter().zip(glyph_sizes) {
        assert!(layer.windows(4).any(|b| b == tag.0.to_le_bytes()));
        let header = manager.read_tag(*tag).unwrap();
        assert_eq!(
            [
                read_u16(&header, 14).unwrap(),
                read_u16(&header, 16).unwrap()
            ],
            expected_size
        );
    }
    assert_tooltip_icon(&manager, finished_table, icon_table, plug);
    eprintln!("Reopened the generated package with the private perk icon intact");
}

#[test]
#[ignore = "requires PARHELION_SOCKET_TEST_PACKAGES, writes only to temporary package views"]
fn bundled_custom_images_reach_private_weapon_tooltip_rows() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_SOCKET_TEST_PACKAGES").unwrap());
    let ignored = crate::package_profile::CANONICAL_ARTIFACT_FILE_NAMES
        .iter()
        .map(|s| (*s).to_owned())
        .collect::<Vec<_>>();
    let view = crate::workflow::FilteredPackageView::create(&packages, &ignored).unwrap();
    let weapons = [
        include_str!("../../../../recipes/hammer-time.parhelion.json"),
        include_str!("../../../../recipes/suros-renaissance.parhelion.json"),
        include_str!("../../../../recipes/ravenous-horizon.parhelion.json"),
    ]
    .map(|json| {
        crate::WeaponRecipe::from_json_str(json)
            .unwrap()
            .to_spec()
            .unwrap()
    });
    let project = WeaponProjectSpec {
        weapons: weapons.into(),
    };
    let weapons = canonical_project_weapons(&project).unwrap();
    let sources = sources::load_project_sources(view.path()).unwrap();
    let resolved = resolve::resolve_project_weapons(&sources, &weapons).unwrap();
    let templates = PerkTemplates::read(&sources).unwrap();
    let plugs = custom_plugs::plan(&sources, &resolved, &templates.strings).unwrap();
    assert_eq!(plugs.len(), 5);
    assert!(
        plugs
            .iter()
            .all(|plug| matches!(plug.icon, Some(crate::perk::Icon::Image { .. })))
    );
    let finished_table = sources.finished_sandbox_perk_table_tag;
    let icon_table = sources.item_icon_table_tag;
    drop(sources);
    let bundle = build_weapon_project_after_catalog_validation(view.path(), &project).unwrap();
    let staged = tempfile::tempdir().unwrap();
    for artifact in &bundle.artifacts {
        let path = staged.path().join(&artifact.plan.output_file_name);
        fs::write(&path, artifact.bytes()).unwrap();
        view.add_overlay(&path).unwrap();
    }
    let manager = open_shadowkeep_package_manager(view.path()).unwrap();
    for plug in &plugs {
        assert_tooltip_icon(&manager, finished_table, icon_table, plug);
    }
}

fn assert_tooltip_icon(
    manager: &PackageManager,
    finished_table: TagHash,
    icon_table: TagHash,
    plug: &ResolvedCustomPlug,
) {
    let strings = manager.read_tag(plug.authored_string_tag).unwrap();
    let definition = manager.read_tag(plug.authored_definition_tag).unwrap();
    let finished = manager.read_tag(finished_table).unwrap();
    let icons = manager.read_tag(icon_table).unwrap();
    let expected_icon = read_u16(&strings, ITEM_STRING_ICON_INDEX_OFFSET).unwrap();
    let (count, _, rows, class) = array_at(&icons, 8).unwrap();
    assert_eq!(class, ITEM_ICON_ROW_CLASS);
    assert!(usize::from(expected_icon) < count);
    let container = TagHash(
        read_u32(
            &icons,
            rows + usize::from(expected_icon) * ITEM_ICON_ROW_SIZE + ITEM_ICON_CONTAINER_OFFSET,
        )
        .unwrap(),
    );
    assert_eq!(container.pkg_id(), PARHELION_ASSET_PACKAGE_ID);
    assert_ne!(container, plug.source_icon_container);
    let mut checked = 0;
    for index in weapon_sandbox_perks(&definition).unwrap() {
        let perk = sundial::package_authoring::sandbox_perk::finished_sandbox_perk_at(
            &finished,
            usize::from(index),
        )
        .unwrap();
        if let Some(expected) = plug
            .sandbox_perks
            .iter()
            .find(|p| p.authored_perk_hash == perk.perk_hash)
        {
            let detail = perk.detail.unwrap();
            assert_eq!(
                read_u16(&detail, 16).unwrap(),
                if expected.hidden {
                    u16::MAX
                } else {
                    expected_icon
                },
                "Private perk 0x{:08X} must use its authored tooltip icon",
                perk.perk_hash
            );
            checked += 1;
        }
    }
    assert_eq!(checked, plug.sandbox_perks.len());
}

fn assert_preserved_icon_layers(stock: &[u8], authored: &[u8]) {
    for offset in sundial::package_authoring::icon_schema::ICON_LAYER_REFERENCE_OFFSETS {
        if offset != sundial::package_authoring::icon_schema::ICON_PRIMARY_LAYER_OFFSET {
            assert_eq!(
                read_u32(authored, offset).unwrap(),
                read_u32(stock, offset).unwrap()
            );
        }
    }
}

fn assert_stock_perk_quality(manager: &PackageManager, stock_container: TagHash) {
    let stock_texture =
        crate::icon_edit::package_icons::primary_texture(manager, stock_container).unwrap();
    let stock_icon = crate::icon_edit::package_icons::load_for(
        manager,
        stock_texture,
        crate::artwork_browser::Purpose::Badge,
    )
    .unwrap();
    assert_eq!(
        stock_icon.size,
        [96, 96],
        "Stock perk texture {stock_texture}"
    );
    assert!(
        crate::artwork_browser::perk_quality::accepts(
            stock_icon.pixels.iter().map(|p| p.to_srgba_unmultiplied())
        ),
        "Stock perk texture {stock_texture} must pass the glyph filter"
    );
    eprintln!("Verified stock perk {stock_texture} is a 96 × 96 white glyph");
}
