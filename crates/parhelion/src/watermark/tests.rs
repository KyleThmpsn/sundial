use std::path::Path;

use tiger_pkg::{DestinyVersion, GameVersion};

use super::*;
use crate::{
    build_badge_icon_plan, build_standalone_package_with_references,
    format::PackageLayout,
    package_profile::{PARHELION_ASSET_FILE_NAME, PARHELION_ASSET_PACKAGE_ID},
};

const ARC_LOGIC_ICON_CONTAINER: TagHash = TagHash(0x8132_57B1);
const MOUNTAINTOP_ICON_CONTAINER: TagHash = TagHash(0x8132_54F3);
const MISFIT_ICON_CONTAINER: TagHash = TagHash(0x8132_5796);
const MISFIT_PRIMARY_LAYER: TagHash = TagHash(0x8132_5795);
const MISFIT_PRIMARY_HEADER: TagHash = TagHash(0x8132_5793);
const MISFIT_PRIMARY_DATA: TagHash = TagHash(0x8132_5794);

#[test]
#[ignore = "requires SUNDIAL_TEST_PACKAGES pointing to Shadowkeep packages"]
fn cross_rarity_icons_keep_art_and_rebuild_exact_resource_dependencies() {
    use AuthoredWeaponRarity as R;
    let packages = std::path::PathBuf::from(std::env::var_os("SUNDIAL_TEST_PACKAGES").unwrap());
    let manager = sundial::package_authoring::open_shadowkeep_package_manager(&packages).unwrap();
    let exotic = TagHash(0x8132_36D9); // Cerberus+1
    let donor = manager.read_tag(exotic).unwrap();
    let requests = [
        R::Common,
        R::Uncommon,
        R::Rare,
        R::Legendary,
        R::Exotic,
        R::Legendary,
    ]
    .map(|rarity| WeaponIconRequest {
        donor_container_tag: exotic,
        icon_edit: WeaponIconEdit::default(),
        rarity,
    });
    let plan = build_watermark_plan(&manager, PARHELION_ASSET_PACKAGE_ID, 0, 0, &requests).unwrap();
    assert_eq!(plan.icon_containers.len(), 5);
    assert_eq!(plan.container_for_request(3), plan.container_for_request(5));
    for container in &plan.icon_containers {
        let payload = &plan.new_tags[container.ordinal].payload;
        validate_only_patched_fields(
            &donor,
            payload,
            &[
                ICON_CONTENT_FINGERPRINT_OFFSET,
                ICON_RARITY_BACKGROUND_LAYER_OFFSET,
                ICON_WATERMARK_LAYER_OFFSET,
            ],
            "rarity icon",
        )
        .unwrap();
        assert_eq!(
            read_tag(payload, ICON_RARITY_BACKGROUND_LAYER_OFFSET).unwrap(),
            container.rarity.icon_background_layer()
        );
        let mut expected =
            collect_unchanged_container_dependencies(&manager, exotic, payload, false).unwrap();
        for pair in &plan.texture_pairs {
            expected.insert(pair.data_tag.0);
            expected.insert(pair.header_tag.0);
        }
        expected.extend([
            plan.watermark_layer_tag.0,
            container.tag.0,
            container.companion_tag.0,
        ]);
        let actual = crate::shared_tag_memory::validate_shared_tag_companion_payload(
            &plan.new_tags[container.companion_ordinal].payload,
            container.companion_tag,
            container.tag,
        )
        .unwrap();
        assert_eq!(actual, expected);
    }
    assert_eq!(manager.read_tag(exotic).unwrap(), donor);
    drop(manager);
    let artifact = build_standalone_package_with_references(
        &packages,
        PARHELION_ASSET_PACKAGE_ID,
        PARHELION_ASSET_FILE_NAME,
        &plan.new_tags,
        &plan.reference_overrides,
    )
    .unwrap();
    assert_eq!(
        PackageLayout::parse(artifact.bytes())
            .unwrap()
            .shared_tag_enrollment_count(),
        5
    );
}
const MISFIT_WATERMARK_LAYER: TagHash = TagHash(0x8131_8639);
const TEST_DESTINATION_PACKAGE_ID: u16 = 0x0914;
const TEST_DESTINATION_ENTRY_COUNT: usize = 5_452;
const TEST_APPENDED_ORDINAL_BASE: usize = 2;

#[test]
fn higher_resolution_output_keeps_the_approved_design_in_all_six_lanes() {
    for (index, (width, height)) in TEXTURE_DIMENSIONS.into_iter().enumerate() {
        let output = render_output_texture(index).unwrap();
        assert_eq!(output.dimensions(), (width * 4, height * 4));
        assert_eq!(output.as_raw().len(), (width * height * 4 * 16) as usize);
        assert!(output.pixels().any(|pixel| pixel[3] == 0));
        assert!(output.pixels().any(|pixel| pixel[3] >= 200));
        let source = decode_authored_texture(index, width, height).unwrap();
        let reduced = crate::icon_edit::fit_rgba_image(&output, width, height);
        // Resampling may soften an edge slightly, but must not replace or reposition the
        // mark. Compare premultiplied channels so invisible RGB does not skew the check.
        let error: f64 = source
            .chunks_exact(4)
            .zip(reduced.pixels())
            .map(|(a, b)| {
                (0..3)
                    .map(|channel| {
                        (f64::from(a[channel]) * f64::from(a[3]) / 255.0
                            - f64::from(b[channel]) * f64::from(b[3]) / 255.0)
                            .abs()
                    })
                    .sum::<f64>()
                    + (f64::from(a[3]) - f64::from(b[3])).abs()
            })
            .sum();
        assert!(error / f64::from(width * height * 4) < 3.0, "lane {index}");
        if let Some(directory) = std::env::var_os("SUNDIAL_TEST_ICON_PREVIEW_DIR") {
            output
                .save(
                    std::path::Path::new(&directory).join(format!("watermark-output-{index}.png")),
                )
                .unwrap();
        }
    }
}

#[test]
fn authored_assets_cover_all_six_native_texture_lanes() {
    let mut decoded = Vec::new();
    for (index, (width, height)) in TEXTURE_DIMENSIONS.into_iter().enumerate() {
        let pixels = decode_authored_texture(index, width, height)
            .expect("pre-rendered watermark texture should decode");
        assert_eq!(pixels.len(), width as usize * height as usize * 4);
        assert!(pixels.chunks_exact(4).any(|pixel| pixel[3] == 0));
        assert!(pixels.chunks_exact(4).any(|pixel| pixel[3] != 0));
        decoded.push(pixels);
    }
    assert!(
        decoded[0]
            .chunks_exact(4)
            .any(|pixel| { pixel[3] >= 200 && pixel[0..3].iter().all(|channel| *channel >= 220) })
    );
    assert!(
        decoded[4]
            .chunks_exact(4)
            .any(|pixel| { pixel[3] >= 200 && pixel[0..3].iter().all(|channel| *channel <= 32) })
    );
    let dark_alpha = decoded[2]
        .chunks_exact(4)
        .map(|pixel| pixel[3])
        .collect::<Vec<_>>();
    let light_alpha = decoded[3]
        .chunks_exact(4)
        .map(|pixel| pixel[3])
        .collect::<Vec<_>>();
    assert_eq!(dark_alpha, light_alpha);
    assert_eq!(
        Sha1::digest(&dark_alpha).as_slice(),
        AUTHORED_STANDALONE_ALPHA_SHA1
    );
    assert_eq!(
        alpha_bounds(&dark_alpha, 45),
        Some(AUTHORED_STANDALONE_ALPHA_BOUNDS)
    );
    assert!(
        decoded[2]
            .chunks_exact(4)
            .all(|pixel| pixel[..3] == [0x58, 0x34, 0x41])
    );
    assert!(
        decoded[3]
            .chunks_exact(4)
            .all(|pixel| pixel[..3] == [0xFF, 0xFF, 0xFF])
    );
}

#[test]
fn private_icon_fingerprint_tracks_composition_and_pixel_revisions() {
    let mut container = vec![0u8; ICON_CONTAINER_SIZE];
    let original = private_icon_fingerprint(&container, b"original pixels");
    assert_ne!(
        original,
        private_icon_fingerprint(&container, b"new pixels")
    );
    container[ICON_WATERMARK_LAYER_OFFSET] = 1;
    assert_ne!(
        original,
        private_icon_fingerprint(&container, b"original pixels")
    );
    let changed = private_icon_fingerprint(&container, b"original pixels");
    container[ICON_CONTENT_FINGERPRINT_OFFSET] = 1;
    assert_eq!(
        changed,
        private_icon_fingerprint(&container, b"original pixels")
    );
}

#[test]
fn item_icon_row_changes_only_the_identity_and_container_fields() {
    let donor = (0..ITEM_ICON_ROW_SIZE as u8).collect::<Vec<_>>();
    let item = 0x5355_4E44;
    let container = TagHash(0x8132_1234);
    let authored = item_icon_row_with_container(&donor, item, container).expect("row should patch");
    assert_eq!(
        read_u32(&authored, ITEM_ICON_IDENTITY_OFFSET).unwrap(),
        item
    );
    assert_eq!(
        read_tag(&authored, ITEM_ICON_CONTAINER_OFFSET).unwrap(),
        container
    );
    for (offset, (before, after)) in donor.iter().zip(&authored).enumerate() {
        let identity = (ITEM_ICON_IDENTITY_OFFSET..ITEM_ICON_IDENTITY_OFFSET + 4).contains(&offset);
        let container =
            (ITEM_ICON_CONTAINER_OFFSET..ITEM_ICON_CONTAINER_OFFSET + 4).contains(&offset);
        if !identity && !container {
            assert_eq!(before, after);
        }
    }
}

#[test]
fn assigned_tags_reject_package_table_overflow() {
    let error = assigned_tag(0x0914, 8191, 1).expect_err("index 8192 must fail");
    assert!(error.to_string().contains("package-table limit"));
}

#[test]
fn absent_resource_references_are_only_the_ffffffff_sentinel() {
    let absent = validate_optional_resource_reference(
        u32::MAX,
        false,
        "optional icon layer",
        |_| -> AuthoringResult<()> { panic!("the absent sentinel must not be resolved") },
    )
    .expect("0xFFFFFFFF should be accepted as absent");
    assert_eq!(absent, None);

    let error = validate_optional_resource_reference(
        0x811C_9DC5,
        false,
        "optional icon layer",
        |_| -> AuthoringResult<()> { Err(invalid("no live resource entry")) },
    )
    .expect_err("a syntactically valid but unresolved tag must not count as absent");
    assert!(error.to_string().contains("811C9DC5"));
    assert!(error.to_string().contains("unresolved or incompatible"));
}

#[test]
fn icon_layer_validation_visits_every_texture_header_reference() {
    let first = TagHash(0x8131_1111);
    let second = TagHash(0x8131_2222);
    let mut layer = vec![0u8; 0x88];
    layer[0x20..0x28].copy_from_slice(&1u64.to_le_bytes());
    layer[0x28..0x30].copy_from_slice(&0x18i64.to_le_bytes());
    layer[0x3C..0x40].copy_from_slice(&LAYER_ARRAY_CLASS.to_le_bytes());
    layer[0x40..0x48].copy_from_slice(&1u64.to_le_bytes());
    layer[0x48..0x4C].copy_from_slice(&LAYER_LANE_CLASS.to_le_bytes());
    layer[0x50..0x58].copy_from_slice(&2u64.to_le_bytes());
    layer[0x58..0x60].copy_from_slice(&0x18i64.to_le_bytes());
    layer[0x6C..0x70].copy_from_slice(&LAYER_ARRAY_CLASS.to_le_bytes());
    layer[0x70..0x78].copy_from_slice(&2u64.to_le_bytes());
    layer[0x78..0x7C].copy_from_slice(&LAYER_TEXTURE_CLASS.to_le_bytes());
    layer[0x80..0x84].copy_from_slice(&u32::from(first).to_le_bytes());
    layer[0x84..0x88].copy_from_slice(&u32::from(second).to_le_bytes());

    let mut visited = Vec::new();
    validate_layer_texture_references(&layer, "test icon layer", |tag| {
        visited.push(tag);
        Ok(())
    })
    .expect("the complete layer graph should validate");
    assert_eq!(visited, vec![first, second]);

    layer[0x84..0x88].copy_from_slice(&0x811C_9DC5u32.to_le_bytes());
    let error = validate_layer_texture_references(&layer, "test icon layer", |tag| {
        if tag == TagHash(0x811C_9DC5) {
            Err(invalid("no live texture-header entry"))
        } else {
            Ok(())
        }
    })
    .expect_err("an unresolved header reference must fail the entire graph");
    assert!(error.to_string().contains("811C9DC5"));
}

fn assert_shared_watermark_plan(plan: &WatermarkPlan, repeated: &WatermarkPlan) {
    assert_eq!(plan.new_tags.len(), SHARED_TAG_COUNT + 4);
    assert_eq!(plan.reference_overrides.len(), 12);
    assert_eq!(plan.icon_containers.len(), 2);
    assert_eq!(plan.watermark_layer_ordinal, 14);
    assert_eq!(plan.watermark_layer_tag, TagHash::new(0x0914, 5_466));
    assert_eq!(
        plan.container_for_donor(ARC_LOGIC_ICON_CONTAINER),
        Some(TagHash::new(0x0914, 5_467))
    );
    assert_eq!(
        plan.container_for_donor(MOUNTAINTOP_ICON_CONTAINER),
        Some(TagHash::new(0x0914, 5_469))
    );
    assert_eq!(
        plan.new_tags
            .iter()
            .map(|tag| (&tag.template_tag, &tag.payload))
            .collect::<Vec<_>>(),
        repeated
            .new_tags
            .iter()
            .map(|tag| (&tag.template_tag, &tag.payload))
            .collect::<Vec<_>>()
    );
}

fn assert_authored_watermark_layer(plan: &WatermarkPlan) {
    let authored_layer = &plan.new_tags[12].payload;
    for (index, pair) in plan.texture_pairs.iter().enumerate() {
        assert_eq!(
            read_tag(
                authored_layer,
                WATERMARK_TEXTURE_REFERENCE_START + index * 4
            )
            .unwrap(),
            pair.header_tag
        );
    }
}

fn assert_watermarked_container(
    manager: &PackageManager,
    plan: &WatermarkPlan,
    index: usize,
    container: &WatermarkedIconContainer,
) {
    let donor = manager.read_tag(container.donor_container_tag).unwrap();
    let definition_index = SHARED_TAG_COUNT + index * 2;
    let companion_index = definition_index + 1;
    let authored = &plan.new_tags[definition_index].payload;
    validate_only_patched_fields(
        &donor,
        authored,
        &[ICON_CONTENT_FINGERPRINT_OFFSET, ICON_WATERMARK_LAYER_OFFSET],
        "test container",
    )
    .unwrap();
    assert_eq!(
        read_tag(authored, ICON_PRIMARY_LAYER_OFFSET).unwrap(),
        read_tag(&donor, ICON_PRIMARY_LAYER_OFFSET).unwrap()
    );
    assert_eq!(
        read_tag(authored, ICON_RARITY_BACKGROUND_LAYER_OFFSET).unwrap(),
        read_tag(&donor, ICON_RARITY_BACKGROUND_LAYER_OFFSET).unwrap()
    );
    assert_eq!(
        read_tag(authored, ICON_WATERMARK_LAYER_OFFSET).unwrap(),
        plan.watermark_layer_tag
    );
    assert_eq!(
        plan.new_tags[definition_index].storage,
        NewTagStorageMode::InheritTemplate
    );
    assert_eq!(
        plan.new_tags[companion_index].storage,
        NewTagStorageMode::InheritTemplate
    );
    assert_eq!(container.companion_ordinal, container.ordinal + 1);
    assert_eq!(
        container.companion_tag,
        TagHash::new(0x0914, 5_468 + (index as u16) * 2)
    );
    assert_eq!(
        container.donor_companion_tag,
        crate::shared_tag_memory::adjacent_companion_tag(container.donor_container_tag).unwrap()
    );
    assert_eq!(
        plan.new_tags[companion_index].template_tag,
        container.donor_companion_tag
    );
    assert_eq!(
        read_tag(&plan.new_tags[companion_index].payload, 0x08).unwrap(),
        container.companion_tag
    );
    assert_eq!(
        read_tag(&plan.new_tags[companion_index].payload, 0x0C).unwrap(),
        container.tag
    );

    let mut expected_dependencies = collect_unchanged_container_dependencies(
        manager,
        container.donor_container_tag,
        authored,
        false,
    )
    .unwrap();
    for pair in &plan.texture_pairs {
        assert_resized_texture_header(manager, plan, pair);
        expected_dependencies.insert(u32::from(pair.data_tag));
        expected_dependencies.insert(u32::from(pair.header_tag));
    }
    expected_dependencies.insert(u32::from(plan.watermark_layer_tag));
    expected_dependencies.insert(u32::from(container.tag));
    expected_dependencies.insert(u32::from(container.companion_tag));
    assert_eq!(
        crate::shared_tag_memory::validate_shared_tag_companion_payload(
            &plan.new_tags[companion_index].payload,
            container.companion_tag,
            container.tag,
        )
        .unwrap(),
        expected_dependencies
    );
}

fn assert_resized_texture_header(
    manager: &PackageManager,
    plan: &WatermarkPlan,
    pair: &WatermarkTexturePair,
) {
    let resource = &plan.new_tags[pair.header_ordinal - TEST_APPENDED_ORDINAL_BASE];
    let donor = manager.read_tag(resource.template_tag).unwrap();
    assert!(donor_mutation_is_limited_to(
        &donor,
        &resource.payload,
        &[0..4, 14..18]
    ));
    assert!(is_stock_straight_rgba8_texture_header(
        &resource.payload,
        pair.width,
        pair.height,
        pair.width as usize * pair.height as usize * 4
    ));
}

#[test]
#[ignore = "requires SUNDIAL_TEST_PACKAGES pointing to Shadowkeep packages"]
fn real_stock_chain_authors_one_shared_resource_for_multiple_items_when_configured() {
    let package_directory = std::env::var_os("SUNDIAL_TEST_PACKAGES")
        .expect("SUNDIAL_TEST_PACKAGES must point to Shadowkeep packages");
    let manager = PackageManager::new(
        Path::new(&package_directory),
        GameVersion::Destiny(DestinyVersion::Destiny2Shadowkeep),
        None,
    )
    .expect("configured Shadowkeep packages should open");
    let donors = [
        ARC_LOGIC_ICON_CONTAINER,
        MOUNTAINTOP_ICON_CONTAINER,
        ARC_LOGIC_ICON_CONTAINER,
    ]
    .map(|donor_container_tag| WeaponIconRequest {
        donor_container_tag,
        icon_edit: WeaponIconEdit::default(),
        rarity: AuthoredWeaponRarity::Legendary,
    });
    let plan = build_watermark_plan(&manager, 0x0914, 5_452, 2, &donors)
        .expect("real stock watermark plan should build");
    let repeated = build_watermark_plan(&manager, 0x0914, 5_452, 2, &donors)
        .expect("real stock watermark plan should be deterministic");

    assert_shared_watermark_plan(&plan, &repeated);
    assert_authored_watermark_layer(&plan);
    for (index, container) in plan.icon_containers.iter().enumerate() {
        assert_watermarked_container(&manager, &plan, index, container);
    }
}

fn assert_private_primary_graph(
    manager: &PackageManager,
    plan: &WatermarkPlan,
    authored_primary: TagHash,
    primary_local: usize,
) {
    assert_eq!(
        plan.new_tags[primary_local - 2].template_tag,
        MISFIT_PRIMARY_DATA
    );
    assert_eq!(
        plan.new_tags[primary_local - 1].template_tag,
        MISFIT_PRIMARY_HEADER
    );
    assert_eq!(
        plan.new_tags[primary_local].template_tag,
        MISFIT_PRIMARY_LAYER
    );

    let donor_pixels = manager
        .read_tag(MISFIT_PRIMARY_DATA)
        .expect("Misfit primary pixels should read");
    let authored_pixels = &plan.new_tags[primary_local - 2].payload;
    assert_eq!(authored_pixels.len(), donor_pixels.len());
    let alpha_mismatches = donor_pixels
        .chunks_exact(4)
        .zip(authored_pixels.chunks_exact(4))
        .filter(|(donor, authored)| donor[3] != authored[3])
        .count();
    let changed_rgb_pixels = donor_pixels
        .chunks_exact(4)
        .zip(authored_pixels.chunks_exact(4))
        .filter(|(donor, authored)| donor[..3] != authored[..3])
        .count();
    assert_eq!(
        alpha_mismatches, 0,
        "the private color edit must preserve every alpha byte"
    );
    assert_ne!(
        changed_rgb_pixels, 0,
        "the non-identity edit must change visible RGB data"
    );
    assert_eq!(
        authored_primary,
        TagHash::new(
            TEST_DESTINATION_PACKAGE_ID,
            (TEST_DESTINATION_ENTRY_COUNT + TEST_APPENDED_ORDINAL_BASE + primary_local) as u16,
        )
    );
}

fn assert_private_container_graph(
    manager: &PackageManager,
    plan: &WatermarkPlan,
    authored_container: &WatermarkedIconContainer,
    authored_primary: TagHash,
    primary_local: usize,
) {
    let donor_container = manager
        .read_tag(MISFIT_ICON_CONTAINER)
        .expect("Misfit icon definition should read");
    assert_eq!(
        read_tag(&donor_container, ICON_PRIMARY_LAYER_OFFSET).unwrap(),
        MISFIT_PRIMARY_LAYER
    );
    assert_eq!(
        read_tag(&donor_container, ICON_WATERMARK_LAYER_OFFSET).unwrap(),
        MISFIT_WATERMARK_LAYER
    );

    let container_local = authored_container.ordinal - TEST_APPENDED_ORDINAL_BASE;
    let authored_payload = &plan.new_tags[container_local].payload;
    validate_only_patched_fields(
        &donor_container,
        authored_payload,
        &[
            ICON_CONTENT_FINGERPRINT_OFFSET,
            ICON_PRIMARY_LAYER_OFFSET,
            ICON_WATERMARK_LAYER_OFFSET,
        ],
        "edited Misfit test container",
    )
    .expect(
        "only the fingerprint, primary and watermark fields may change in the authored container",
    );
    assert_eq!(
        read_tag(authored_payload, ICON_PRIMARY_LAYER_OFFSET).unwrap(),
        authored_primary
    );
    assert_eq!(
        read_tag(authored_payload, ICON_WATERMARK_LAYER_OFFSET).unwrap(),
        plan.watermark_layer_tag
    );

    let companion_dependencies = crate::shared_tag_memory::validate_shared_tag_companion_payload(
        &plan.new_tags[container_local + 1].payload,
        authored_container.companion_tag,
        authored_container.tag,
    )
    .expect("the authored container companion should remain canonical");
    for local in [primary_local - 2, primary_local - 1, primary_local] {
        let tag = TagHash::new(
            TEST_DESTINATION_PACKAGE_ID,
            (TEST_DESTINATION_ENTRY_COUNT + TEST_APPENDED_ORDINAL_BASE + local) as u16,
        );
        assert!(companion_dependencies.contains(&u32::from(tag)));
    }
    assert!(!companion_dependencies.contains(&u32::from(MISFIT_PRIMARY_DATA)));
    assert!(!companion_dependencies.contains(&u32::from(MISFIT_PRIMARY_HEADER)));
    assert!(!companion_dependencies.contains(&u32::from(MISFIT_PRIMARY_LAYER)));
    assert_eq!(
        manager
            .read_tag(MISFIT_ICON_CONTAINER)
            .expect("Misfit icon definition should remain readable"),
        donor_container,
        "building the plan must not mutate the donor"
    );
}

#[test]
#[ignore = "requires SUNDIAL_TEST_PACKAGES pointing to Shadowkeep packages"]
fn real_misfit_edit_authors_a_private_primary_graph_when_configured() {
    let package_directory = std::env::var_os("SUNDIAL_TEST_PACKAGES")
        .expect("SUNDIAL_TEST_PACKAGES must point to Shadowkeep packages");
    let manager = PackageManager::new(
        Path::new(&package_directory),
        GameVersion::Destiny(DestinyVersion::Destiny2Shadowkeep),
        None,
    )
    .expect("configured Shadowkeep packages should open");
    let edit = WeaponIconEdit {
        hue_shift_degrees: 24,
        brightness: 8,
        invert: false,
        ..WeaponIconEdit::default()
    };
    let plan = build_watermark_plan(
        &manager,
        TEST_DESTINATION_PACKAGE_ID,
        TEST_DESTINATION_ENTRY_COUNT,
        TEST_APPENDED_ORDINAL_BASE,
        &[WeaponIconRequest {
            donor_container_tag: MISFIT_ICON_CONTAINER,
            icon_edit: edit.clone(),
            rarity: AuthoredWeaponRarity::Legendary,
        }],
    )
    .expect("Misfit should support a private edited primary-image graph");

    assert_eq!(plan.new_tags.len(), SHARED_TAG_COUNT + 5);
    assert_eq!(plan.reference_overrides.len(), 14);
    assert_eq!(plan.icon_containers.len(), 1);
    let authored_container = &plan.icon_containers[0];
    let authored_primary = authored_container
        .authored_primary_layer_tag
        .expect("a non-identity edit must have a private primary layer");
    assert_eq!(authored_container.icon_edit, edit);
    assert_eq!(plan.container_for_request(0), Some(authored_container.tag));

    let primary_local = authored_primary.entry_index() as usize
        - TEST_DESTINATION_ENTRY_COUNT
        - TEST_APPENDED_ORDINAL_BASE;
    assert_private_primary_graph(&manager, &plan, authored_primary, primary_local);
    assert_private_container_graph(
        &manager,
        &plan,
        authored_container,
        authored_primary,
        primary_local,
    );
}

#[test]
#[ignore = "requires SUNDIAL_TEST_PACKAGES pointing to Shadowkeep packages"]
fn real_standalone_asset_package_round_trips_when_configured() {
    let package_directory = std::env::var_os("SUNDIAL_TEST_PACKAGES")
        .expect("SUNDIAL_TEST_PACKAGES must point to Shadowkeep packages");
    let package_directory = Path::new(&package_directory);
    let manager = PackageManager::new(
        package_directory,
        GameVersion::Destiny(DestinyVersion::Destiny2Shadowkeep),
        None,
    )
    .expect("configured Shadowkeep packages should open");
    let watermark = build_watermark_plan(
        &manager,
        PARHELION_ASSET_PACKAGE_ID,
        0,
        0,
        &[WeaponIconRequest {
            donor_container_tag: ARC_LOGIC_ICON_CONTAINER,
            icon_edit: WeaponIconEdit::default(),
            rarity: AuthoredWeaponRarity::Legendary,
        }],
    )
    .expect("standalone watermark graph should build");
    let badge = build_badge_icon_plan(
        &manager,
        PARHELION_ASSET_PACKAGE_ID,
        0,
        watermark.new_tags.len(),
    )
    .expect("standalone badge graph should build");
    drop(manager);

    let mut tags = watermark.new_tags;
    tags.extend(badge.new_tags);
    let mut references = watermark.reference_overrides;
    references.extend(badge.reference_overrides);
    let artifact = build_standalone_package_with_references(
        package_directory,
        PARHELION_ASSET_PACKAGE_ID,
        PARHELION_ASSET_FILE_NAME,
        &tags,
        &references,
    )
    .expect("standalone asset package should build and round-trip");
    let layout = PackageLayout::parse(artifact.bytes())
        .expect("standalone asset package should retain a native layout");

    assert_eq!(layout.package_id, PARHELION_ASSET_PACKAGE_ID);
    assert_eq!(layout.patch_id, 0);
    assert_eq!(layout.entry_count, tags.len());
    assert_eq!(layout.shared_tag_enrollment_count(), 2);
    assert_eq!(artifact.plan.original_entry_count, 0);
    assert_eq!(artifact.plan.final_entry_count, tags.len());
    assert_eq!(artifact.plan.appended_tags.len(), tags.len());
}
