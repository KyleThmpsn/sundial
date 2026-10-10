use std::path::Path;

use tiger_pkg::{DestinyVersion, GameVersion};

use super::*;

#[test]
fn renderer_preserves_an_opaque_donor_mask_and_untouched_gradient_side_margins() {
    let source = decode_source().expect("canonical source should decode");
    for (width, height, expected_size, left_margin) in [
        (LOW_WIDTH, LOW_HEIGHT, LOW_DATA_SIZE, 41usize),
        (HIGH_WIDTH, HIGH_HEIGHT, HIGH_DATA_SIZE, 86usize),
    ] {
        let donor = vec![255; expected_size];
        let data = render_card(
            &source,
            &donor,
            width,
            height,
            crate::branding::Branding::Sunrise,
        )
        .expect("card should render");
        assert_eq!(data.len(), expected_size);
        assert!(data.chunks_exact(4).all(|pixel| pixel[3] == 255));
        assert!(data.chunks_exact(4).enumerate().any(|(index, pixel)| {
            let x = index as u32 % width;
            let y = index as u32 / width;
            pixel != card_background(x, y, width, height).0
        }));
        for y in 0..height as usize {
            let row = &data[y * width as usize * 4..(y + 1) * width as usize * 4];
            let background = card_background(0, y as u32, width, height).0;
            assert!(
                row[..left_margin * 4]
                    .chunks_exact(4)
                    .all(|pixel| pixel == background)
            );
            assert!(
                row[(width as usize - left_margin) * 4..]
                    .chunks_exact(4)
                    .all(|pixel| pixel == background)
            );
        }
    }
}

#[test]
fn renderer_copies_rounded_alpha_and_wraps_highlight_around_top_edge() {
    let source = decode_source().expect("canonical source should decode");
    let width = 12;
    let height = 8;
    let mut donor = vec![0; width as usize * height as usize * 4];
    for x in 0..width {
        let edge_y = if matches!(x, 0 | 11) {
            2
        } else if matches!(x, 1 | 10) {
            1
        } else {
            0
        };
        for y in edge_y..height {
            let offset = ((y * width + x) * 4) as usize;
            donor[offset..offset + 3].fill(if y < edge_y + 2 { 80 } else { 20 });
            donor[offset + 3] = if y == edge_y && matches!(x, 0 | 11) {
                128
            } else {
                255
            };
        }
    }

    let data = render_card(
        &source,
        &donor,
        width,
        height,
        crate::branding::Branding::Sunrise,
    )
    .expect("card should render");
    assert!(
        data.chunks_exact(4)
            .zip(donor.chunks_exact(4))
            .all(|(pixel, donor_pixel)| pixel[3] == donor_pixel[3])
    );
    for &(x, y) in &[(0, 2), (1, 1), (10, 1), (11, 2)] {
        let pixel = donor_pixel(&data, width, x, y);
        let plain = card_background(x, y, width, height);
        assert!(pixel[0] > plain[0]);
        assert!(pixel[1] > plain[1]);
        assert!(pixel[2] > plain[2]);
    }
}

#[test]
fn alpha_compositing_is_straight_and_fully_opaque() {
    let background = card_background(0, 0, LOW_WIDTH, LOW_HEIGHT);
    assert_eq!(
        composite_onto_opaque(Rgba([200, 100, 50, 0]), background),
        background
    );
    assert_eq!(
        composite_onto_opaque(Rgba([200, 100, 50, 255]), background),
        Rgba([200, 100, 50, 255])
    );
    assert_eq!(
        composite_onto_opaque(Rgba([200, 100, 50, 128]), Rgba([0, 0, 0, 255])),
        Rgba([100, 50, 25, 255])
    );
}

#[test]
fn rejects_unencodable_destination_indices() {
    let ordinals = ordinals(2).expect("ordinals should be assigned");
    let error = assigned_tags(0x0197, 8184, ordinals)
        .expect_err("seven icon tags cannot overflow the package entry table");
    assert!(error.to_string().contains("package-table limit"));
}

fn assert_real_plan_shape(plan: &BadgeIconPlan, donor_low_header: &[u8], donor_high_header: &[u8]) {
    assert_eq!(plan.new_tags.len(), TAGS_PER_ICON);
    assert_eq!(plan.ordinals.low_data, 2);
    assert_eq!(plan.ordinals.container, 7);
    assert_eq!(plan.ordinals.companion, 8);
    assert_eq!(plan.container_tag, TagHash::new(0x0197, 1007));
    assert_eq!(plan.tags.companion, TagHash::new(0x0197, 1008));
    assert_eq!(plan.new_tags[1].payload, donor_low_header);
    assert_eq!(plan.new_tags[3].payload, donor_high_header);
    assert_eq!(plan.new_tags[6].template_tag, DONOR_COMPANION);
    assert!(
        plan.new_tags
            .iter()
            .all(|tag| tag.storage == NewTagStorageMode::InheritTemplate)
    );
}

fn assert_real_plan_links(plan: &BadgeIconPlan, donor_layer: &[u8], donor_container: &[u8]) {
    validate_only_patched_ranges(
        donor_layer,
        &plan.new_tags[4].payload,
        &[
            LOW_HEADER_TAG_OFFSET..LOW_HEADER_TAG_OFFSET + 4,
            HIGH_HEADER_TAG_OFFSET..HIGH_HEADER_TAG_OFFSET + 4,
        ],
        "test layer",
    )
    .expect("only the layer's two host tags should change");
    let container_layer_tag_range = ICON_PRIMARY_LAYER_OFFSET..ICON_PRIMARY_LAYER_OFFSET + 4;
    let container_fingerprint_range = 0x10..0x14;
    validate_only_patched_ranges(
        donor_container,
        &plan.new_tags[5].payload,
        &[container_layer_tag_range, container_fingerprint_range],
        "test container",
    )
    .expect("only the container's layer host tag and content fingerprint should change");
    assert_eq!(
        read_u32(&plan.new_tags[4].payload, LOW_HEADER_TAG_OFFSET)
            .expect("low layer link should decode"),
        u32::from(TagHash::new(0x0197, 1003))
    );
    assert_eq!(
        read_u32(&plan.new_tags[4].payload, HIGH_HEADER_TAG_OFFSET)
            .expect("high layer link should decode"),
        u32::from(TagHash::new(0x0197, 1005))
    );
    assert_eq!(
        read_u32(&plan.new_tags[5].payload, ICON_PRIMARY_LAYER_OFFSET)
            .expect("container layer link should decode"),
        u32::from(TagHash::new(0x0197, 1006))
    );
    assert_eq!(
        read_u32(&plan.new_tags[6].payload, 0x08).expect("companion self tag should decode"),
        u32::from(TagHash::new(0x0197, 1008))
    );
    assert_eq!(
        read_u32(&plan.new_tags[6].payload, 0x0C).expect("companion owner tag should decode"),
        u32::from(TagHash::new(0x0197, 1007))
    );
    assert_eq!(
        crate::shared_tag_memory::validate_shared_tag_companion_payload(
            &plan.new_tags[6].payload,
            plan.tags.companion,
            plan.tags.container,
        )
        .expect("authored badge companion should be canonical"),
        dependency_set((1002..=1008).map(|index| TagHash::new(0x0197, index)))
    );
}

#[test]
fn legacy_badge_background_matches_the_active_runtime() {
    use crate::{branding::Branding, presentation::Artwork};
    let artwork = Artwork::from_png(include_bytes!(
        "../../../../assets/parhelion/dawn-badge-source.png"
    ))
    .unwrap();
    assert!(artwork.composition().is_none());
    let donor = vec![255; (HIGH_WIDTH * HIGH_HEIGHT * 4) as usize];
    for branding in [Branding::Sunrise, Branding::Dawn] {
        let pixels =
            render_artwork(Some(&artwork), &donor, HIGH_WIDTH, HIGH_HEIGHT, branding).unwrap();
        let preview = preview_with_branding(Some(&artwork), Some(&donor), branding).unwrap();
        assert_eq!(preview.as_raw(), &pixels);
        let expected = match branding {
            Branding::Sunrise => card_background(2, 100, HIGH_WIDTH, HIGH_HEIGHT),
            Branding::Dawn => crate::branding::dawn_background(100, HIGH_HEIGHT),
        };
        assert_eq!(*preview.get_pixel(2, 100), expected);
    }
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES pointing to Shadowkeep packages"]
fn edited_badge_preview_matches_emitted_pixels_and_custom_backgrounds() {
    use crate::presentation::{
        Artwork,
        composition::{Background, Composition, Fit},
    };
    let packages = crate::test_support::stock_packages();
    let manager =
        sundial::package_authoring::open_shadowkeep_package_manager(Path::new(&packages)).unwrap();
    let donor = read_and_validate_donor(&manager).unwrap();
    let source = RgbaImage::from_pixel(160, 90, Rgba([235, 60, 25, 255]));
    for (background, scale) in [
        (
            Background::Solid {
                color: [15, 75, 110],
            },
            50,
        ),
        (
            Background::Gradient {
                start: [10, 25, 30],
                end: [120, 190, 200],
                angle: 90,
            },
            50,
        ),
        (Background::Sunrise, 100),
    ] {
        let artwork = Artwork::from_source(source.clone())
            .unwrap()
            .with_composition(Composition {
                fit: Fit::Cover,
                background,
                scale,
                ..Default::default()
            })
            .unwrap();
        let saved = serde_json::to_string(&artwork).unwrap();
        let artwork: Artwork = serde_json::from_str(&saved).unwrap();
        let plan = build_icon_plan(&manager, 0x0aa0, 0, 0, Some(&artwork)).unwrap();
        let preview = preview(Some(&artwork), Some(&donor.high_data)).unwrap();
        assert_eq!(plan.new_tags[2].payload, preview.as_raw().as_slice());
        assert_eq!(
            plan.new_tags[0].payload,
            render_artwork(
                Some(&artwork),
                &donor.low_data,
                LOW_WIDTH,
                LOW_HEIGHT,
                crate::branding::Branding::Sunrise
            )
            .unwrap()
        );
        assert!(
            preview
                .pixels()
                .zip(donor.high_data.chunks_exact(4))
                .all(|(a, b)| a[3] == b[3])
        );
        if scale == 100 {
            assert_eq!(
                preview.get_pixel(2, 100).0,
                [235, 60, 25, 255],
                "Fill must cover the purple side margins"
            );
        } else {
            assert_ne!(
                preview.get_pixel(2, 100).0,
                card_background(2, 100, HIGH_WIDTH, HIGH_HEIGHT).0
            );
        }
    }
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES pointing to Shadowkeep packages"]
fn real_package_donor_chain_round_trips_when_configured() {
    let package_directory = crate::test_support::stock_packages();
    let manager = PackageManager::new(
        Path::new(&package_directory),
        GameVersion::Destiny(DestinyVersion::Destiny2Shadowkeep),
        None,
    )
    .expect("configured Shadowkeep packages should open");
    let plan = build_badge_icon_plan(&manager, 0x0197, 1000, 2)
        .expect("real Lunar icon chain should author");
    let donor_low_header = manager
        .read_tag(DONOR_LOW_HEADER)
        .expect("low donor header should read");
    let donor_high_header = manager
        .read_tag(DONOR_HIGH_HEADER)
        .expect("high donor header should read");
    let donor_layer = manager
        .read_tag(DONOR_LAYER)
        .expect("donor layer should read");
    let donor_container = manager
        .read_tag(DONOR_CONTAINER)
        .expect("donor container should read");
    assert_real_plan_shape(&plan, &donor_low_header, &donor_high_header);
    assert_real_plan_links(&plan, &donor_layer, &donor_container);
}
