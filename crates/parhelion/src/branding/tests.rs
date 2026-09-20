use super::*;

#[test]
#[ignore = "requires SUNDIAL_TEST_PACKAGES in a Dawn installation"]
fn dawn_detection_and_native_artwork_plans_match_previews() {
    use crate::{AuthoredWeaponRarity, WeaponIconEdit, watermark::WeaponIconRequest};
    use tiger_pkg::TagHash;
    let packages = std::path::PathBuf::from(std::env::var_os("SUNDIAL_TEST_PACKAGES").unwrap());
    assert_eq!(Branding::for_packages(&packages), Branding::Dawn);
    let install = packages.parent().unwrap();
    let dll = [
        install.join("steam_api64.dll"),
        install.join("bin/x64/steam_api64.dll"),
    ]
    .into_iter()
    .find(|path| path.is_file())
    .unwrap();
    let temporary = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(temporary.path().join("bin/x64")).unwrap();
    std::fs::copy(dll, temporary.path().join("bin/x64/steam_api64.dll")).unwrap();
    assert_eq!(Branding::detect(temporary.path()), Branding::Dawn);
    std::fs::write(
        temporary.path().join("steam_api64.dll"),
        b"unrecognized root module",
    )
    .unwrap();
    assert_eq!(Branding::detect(temporary.path()), Branding::Sunrise);

    let manager = sundial::package_authoring::open_shadowkeep_package_manager(&packages).unwrap();
    let dawn = Branding::Dawn.badge().unwrap();
    let sunrise = crate::badge_icon::build_icon_plan(&manager, 0x0aa0, 0, 0, None).unwrap();
    let plan = crate::badge_icon::build_icon_plan(&manager, 0x0aa0, 0, 0, dawn.as_ref()).unwrap();
    let mask = crate::badge_icon::preview_mask(&manager).unwrap();
    assert_eq!(
        plan.new_tags[2].payload,
        crate::badge_icon::preview(dawn.as_ref(), Some(&mask))
            .unwrap()
            .into_raw()
    );
    assert_ne!(
        plan.new_tags[5].payload, sunrise.new_tags[5].payload,
        "Branding must change the native image fingerprint"
    );
    let requests = [WeaponIconRequest {
        donor_container_tag: TagHash(0x8132_57B1),
        icon_edit: WeaponIconEdit::default(),
        rarity: AuthoredWeaponRarity::Legendary,
    }];
    let plan = crate::watermark::build_presented_watermark_plan(
        &manager,
        0x0aa0,
        0,
        0,
        &requests,
        crate::watermark::Presentation {
            branding: Branding::Dawn,
            artwork: &[None],
        },
        &|_| "Dawn test".into(),
    )
    .unwrap();
    for index in 0..6 {
        assert_eq!(
            plan.new_tags[index * 2].payload,
            Branding::Dawn.texture(index).unwrap().into_raw()
        );
    }
}

#[test]
fn stale_dawn_folders_do_not_select_dawn() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("Dawn")).unwrap();
    std::fs::create_dir_all(root.path().join("bin/x64/Dawn")).unwrap();
    assert_eq!(Branding::detect(root.path()), Branding::Sunrise);
}

#[test]
fn badge_and_watermark_use_runtime_art_without_altering_sunrise() {
    assert!(Branding::Sunrise.badge().unwrap().is_none());
    assert!(Branding::Sunrise.corner().unwrap().is_none());
    assert_eq!(
        Branding::Sunrise.watermark().unwrap(),
        crate::watermark::render_output_texture(0).unwrap()
    );
    let artwork = Branding::Dawn.badge().unwrap().unwrap();
    let badge = crate::badge_icon::preview(Some(&artwork), None).unwrap();
    assert_eq!(badge.dimensions(), (440, 268));
    assert!(badge.pixels().any(|pixel| pixel.0 == [255; 4]));
    assert_eq!(badge.get_pixel(0, 0).0, [3, 15, 38, 255]);
    assert_eq!(badge.get_pixel(0, 267).0, [8, 89, 242, 255]);
    assert_ne!(
        Branding::Dawn.watermark().unwrap(),
        Branding::Sunrise.watermark().unwrap()
    );
    if let Some(directory) = std::env::var_os("PARHELION_BRANDING_PREVIEWS") {
        let directory = Path::new(&directory);
        std::fs::create_dir_all(directory).unwrap();
        badge.save(directory.join("dawn-badge.png")).unwrap();
        artwork
            .render(208, 126)
            .save(directory.join("dawn-badge-low.png"))
            .unwrap();
        Branding::Dawn
            .watermark()
            .unwrap()
            .save(directory.join("dawn-watermark.png"))
            .unwrap();
        for (index, (width, height)) in [(96, 96), (54, 54), (45, 45), (45, 45), (96, 96), (54, 54)]
            .into_iter()
            .enumerate()
        {
            let image = Branding::Dawn.texture(index).unwrap();
            image
                .save(directory.join(format!("dawn-watermark-{index}.png")))
                .unwrap();
            crate::image_import::fit(&image, width, height)
                .save(directory.join(format!("dawn-watermark-{index}-native.png")))
                .unwrap();
        }
    }
}
