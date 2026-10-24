use super::*;

#[test]
fn inspector_stills_drop_previous_generation_and_wait_for_package_access() {
    let catalog = crate::catalog::Catalog::for_test(vec![], Default::default());
    let access = catalog.inspection_access();
    let ctx = egui::Context::default();
    let id = egui::Id::new("guarded-inline");
    let packages = tempfile::tempdir().unwrap();
    let appearance = Appearance {
        arrangement: 12,
        dyes: vec![],
        dye_textures: vec![],
    };
    let state = Arc::new(Mutex::new(Still::default()));
    ctx.data_mut(|data| data.insert_temp(id, state.clone()));
    let frame = || {
        ctx.run_ui(egui::RawInput::default(), |ui| {
            show_inspected(
                ui,
                id,
                packages.path(),
                appearance.clone(),
                egui::vec2(180., 120.),
                access.clone(),
            );
        })
    };
    // Establish the actual UI request while access is blocked, without touching a package.
    catalog.suspend_package_access();
    frame();
    assert!(state.lock().unwrap().pending.is_none());
    let first = state.lock().unwrap().wanted.clone().unwrap();
    let (sender, receiver) = mpsc::channel();
    {
        let mut still = state.lock().unwrap();
        still.shown = Some(first.clone());
        still.model = Some(Arc::new(Model::default()));
        still.texture = Some(ctx.load_texture(
            "old image",
            egui::ColorImage::new([1, 1], vec![egui::Color32::RED]),
            Default::default(),
        ));
        still.pending = Some((first, receiver));
    }
    // A second suspension advances the package generation even with identical appearance IDs.
    catalog.resume_package_access();
    catalog.suspend_package_access();
    sender.send(Ok(Model::default())).unwrap();
    frame();
    {
        let still = state.lock().unwrap();
        assert!(still.model.is_none() && still.texture.is_none() && still.pending.is_none());
    }
    catalog.resume_package_access();
    frame();
    assert!(
        state.lock().unwrap().pending.is_some(),
        "resume must request the identical appearance again"
    );
    let output = frame();
    crate::test_support::capture::write(&ctx, &output, "inspector-inline-resumed");
}
