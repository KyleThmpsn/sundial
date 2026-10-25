use super::*;

#[test]
fn inspector_corner_viewer_keeps_access_and_invalidates_after_its_source_closes() {
    let catalog = crate::catalog::Catalog::for_test(vec![], Default::default());
    let access = catalog.inspection_access();
    let ctx = egui::Context::default();
    let packages = tempfile::tempdir().unwrap();
    let id = egui::Id::new("guarded-corner");
    let over = egui::Rect::from_min_size(egui::pos2(20., 20.), egui::vec2(200., 140.));
    let at = corner_rect(over, 0).center();
    let appearance = Appearance {
        arrangement: 12,
        dyes: vec![],
        dye_textures: vec![],
    };
    let frame = |events| {
        ctx.run_ui(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ui| {
                super::super::super::inspected_pop_out(
                    ui,
                    (id, id),
                    over,
                    packages.path(),
                    (appearance.clone(), "Guarded Model"),
                    access.clone(),
                );
            },
        )
    };
    catalog.suspend_package_access();
    frame(vec![egui::Event::PointerMoved(at)]);
    frame(crate::test_support::primary_press(at, true));
    frame(crate::test_support::primary_press(at, false));
    let state = shared(&ctx);
    assert!(state.lock().unwrap().open);
    // Drive the viewer independently, as when the Inspector navigates away from the source.
    let viewer = || {
        ctx.run_ui(egui::RawInput::default(), |ui| {
            state.lock().unwrap().draw_request(ui)
        })
    };
    viewer();
    assert!(state.lock().unwrap().pending.is_none());
    catalog.resume_package_access();
    let (sender, receiver) = mpsc::channel();
    {
        let mut preview = state.lock().unwrap();
        let selection = preview.request.as_ref().unwrap().selection.clone();
        preview.selection = Some(selection.clone());
        preview.model = Some(Arc::new(Model::default()));
        preview.pending = Some((selection, receiver));
    }
    catalog.suspend_package_access();
    catalog.resume_package_access();
    sender.send(Ok(Model::default())).unwrap();
    let output = viewer();
    assert!(state.lock().unwrap().model.is_none());
    assert!(
        state.lock().unwrap().pending.is_some(),
        "new generation must read again"
    );
    crate::test_support::capture::write(&ctx, &output, "inspector-corner-resumed");
}
