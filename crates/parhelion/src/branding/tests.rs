use super::*;

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES in a Dawn installation"]
fn dawn_detection_and_native_artwork_plans_match_previews() {
    use crate::{AuthoredWeaponRarity, WeaponIconEdit, watermark::WeaponIconRequest};
    use tiger_pkg::TagHash;
    let packages = crate::test_support::stock_packages();
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
        plain: false,
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
