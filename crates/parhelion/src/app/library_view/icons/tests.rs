use super::super::*;

#[test]
fn viewport_navigation_requests_visible_rows_and_bounds_retained_icons() {
    let library_root = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(library_root.path().join("recipes")).unwrap();
    let template = library.scan().unwrap().entries.remove(0);
    let entries = (0..400)
        .map(|index| {
            let mut entry = template.clone();
            entry.path = library
                .root()
                .join(format!("row-{index:04}.parhelion.json"));
            entry.name = format!("Recipe {index}");
            entry
        })
        .collect::<Vec<_>>();
    let ctx = egui::Context::default();
    let mut icons = LibraryIcons::default();
    let _ = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(640.0, 240.0),
            )),
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .max_height(160.0)
                    .show(ui, |ui| {
                        for entry in &entries {
                            draw_library_row(
                                ui,
                                &mut icons,
                                entry,
                                "Weapon",
                                LibraryRowState::default(),
                            );
                        }
                    });
            });
        },
    );
    let visible = icons.wanted.clone();
    assert!(visible.contains(&entries[0].path));
    assert!(
        visible.len() < 10,
        "Offscreen rows queued {} icon reads",
        visible.len()
    );
    assert!(!visible.contains(&entries[100].path));
    let (sender, receiver) = mpsc::channel();
    icons.receiver = Some(receiver);
    for batch in entries.chunks(8) {
        for entry in batch {
            let key = IconKey {
                corner_icon: None,
                item_hash: entry.icon_hash,
                container_tag: 7,
                rarity: crate::AuthoredWeaponRarity::Legendary,
                edit: entry.icon_edit.clone(),
                plain: false,
                art: None,
            };
            icons.pending.insert(entry.path.clone(), key.clone());
            sender
                .send((
                    entry.path.clone(),
                    key,
                    Ok(egui::ColorImage::filled([96, 96], egui::Color32::WHITE)),
                ))
                .unwrap();
        }
        icons.poll(&ctx);
        assert!(
            icons.previews.len() < 200,
            "Browsing the library retained every visited image"
        );
    }
    assert!(!icons.previews.contains_key(&entries[0].path));
    assert!(icons.previews.contains_key(&entries[399].path));
    assert!(icons.pending.is_empty());
    drop(sender);
    if let Some(output) = std::env::var_os("SUNDIAL_CACHE_VERIFICATION_DIRECTORY") {
        let output = std::path::PathBuf::from(output);
        std::fs::create_dir_all(&output).unwrap();
        std::fs::write(output.join("library-icons.json"), serde_json::to_vec_pretty(&serde_json::json!({"rows": entries.len(), "visible_requests": visible.len(), "retained_after_navigation": icons.previews.len(), "first_evicted": true, "last_visible_retained": true})).unwrap()).unwrap();
    }
}
