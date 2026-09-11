use super::*;

#[test]
fn completed_import_waits_until_recipe_controls_are_enabled() {
    let image = HudImage::from_png(include_bytes!(
        "../../../../../assets/parhelion/watermark/sunrise-watermark-0-96x96.png"
    ))
    .unwrap();
    let (sender, receiver) = mpsc::channel();
    sender.send(Ok(Some(image.clone()))).unwrap();
    let mut editor = Editor {
        pending: Some(receiver),
        ..Default::default()
    };
    let mut draft = None;
    let ctx = egui::Context::default();
    for enabled in [false, false, true] {
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.add_enabled_ui(enabled, |ui| {
                    editor.draw(
                        ui,
                        &mut draft,
                        Appearance {
                            packages: Path::new(""),
                            pattern_index: None,
                            name: "Test Weapon",
                        },
                    )
                });
            });
        });
        assert_eq!(draft, enabled.then(|| image.clone()));
        assert_eq!(editor.pending.is_none(), enabled);
    }
}
