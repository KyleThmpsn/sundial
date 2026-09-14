use super::*;
use sundial::package_authoring::weapon_runtime::{NativeStructure, NativeStructureField};

#[test]
fn decoded_structure_is_readable_without_enabling_byte_edits() {
    let mut loaded = super::super::tests::fixture();
    let root = &mut loaded.graphs[0].1.owners[0].roots[0];
    root.structure = Arc::new(NativeStructure {
        managed_ranges: Vec::new(),
        fields: vec![
            NativeStructureField {
                path: Vec::new(),
                storage: None,
                owner_offset: 0x144,
                schema: 0x8080_37C9,
                schema_offset: 0x54,
                label: "Unnamed Field +0x54".into(),
                representation: "Float32".into(),
                value: "1 (0x3F800000)".into(),
            },
            NativeStructureField {
                path: Vec::new(),
                storage: None,
                owner_offset: 0x200,
                schema: 0x8080_FF00,
                schema_offset: 0,
                label: "Unnamed Field +0x0".into(),
                representation: "Unmapped Storage: Runtime-Selected Structure".into(),
                value: "Wire operation 35".into(),
            },
        ],
        issues: vec!["One nested schema could not be read.".into()],
    });
    let original_fields = loaded.graphs[0].1.fields().cloned().collect::<Vec<_>>();
    for width in [480.0, 1100.0] {
        let context = egui::Context::default();
        context.style_mut(|style| style.animation_time = 0.0);
        let mut output = egui::FullOutput::default();
        let mut header = egui::Pos2::ZERO;
        for frame in 0..4 {
            let events = match frame {
                1 | 2 => vec![
                    egui::Event::PointerMoved(header),
                    egui::Event::PointerButton {
                        pos: header,
                        button: egui::PointerButton::Primary,
                        pressed: frame == 1,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
                _ => Vec::new(),
            };
            output = context.run(
                egui::RawInput {
                    events,
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 720.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        draw(ui, &loaded, "");
                        assert!(ui.min_rect().right() <= width);
                    });
                },
            );
            if frame == 0 {
                header = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Text(text)
                            if text.galley.job.text == "Decoded Native Structure" =>
                        {
                            Some(text.pos + egui::vec2(4.0, 4.0))
                        }
                        _ => None,
                    })
                    .expect("The structure inspector header should be visible");
            }
        }
        let text = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("Read-only values"), "{text}");
        assert!(text.contains("Float32"), "{text}");
        assert!(text.contains("1 Readable Entries, 1 Unmapped"), "{text}");
        assert!(text.contains("Runtime-Selected Structure"), "{text}");
        assert!(
            text.contains("One nested schema could not be read."),
            "{text}"
        );
        assert_eq!(
            loaded.graphs[0].1.fields().cloned().collect::<Vec<_>>(),
            original_fields
        );
    }
}
