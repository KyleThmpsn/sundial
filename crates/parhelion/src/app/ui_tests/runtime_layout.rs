use super::*;
use sundial::package_authoring::weapon_runtime::{
    WeaponRuntimeBinding, WeaponRuntimeOwner, WeaponRuntimeRoot, WeaponRuntimeRootKind,
};

fn runtime_graph() -> WeaponRuntimeGraph {
    WeaponRuntimeGraph {
        item_hash: 1,
        pattern_global_id_hash: 2,
        entity_tag: 3,
        bindings: PRIMARY_RUNTIME_COMPONENTS
            .iter()
            .map(|control| WeaponRuntimeBinding {
                binding_hash: control.binding_hash,
                binding_label: control.label.into(),
                resource_index: 0,
                resource_count: 1,
                owner_tag: control.binding_hash,
                concrete_class: 1,
                resource_offset: 0,
            })
            .collect(),
        resources: Vec::new(),
        owners: (0..30)
            .map(|index| WeaponRuntimeOwner {
                anchor_binding_hash: index + 1,
                anchor_resource_index: 0,
                owner_tag: index + 1,
                roots: vec![WeaponRuntimeRoot {
                    kind: WeaponRuntimeRootKind::Instance,
                    schema: 1,
                    owner_offset: 0,
                    byte_size: 4,
                    generated_schema: true,
                    fields: vec![field(
                        WeaponRuntimeValueKind::Float32,
                        WeaponRuntimeValue::Float32Bits(1.0_f32.to_bits()),
                    )],
                }],
            })
            .collect(),
    }
}

#[test]
fn binary_patches_follow_donors_and_remain_visible_without_scrolling_values() {
    for size in [
        egui::vec2(480.0, 640.0),
        egui::vec2(900.0, 760.0),
        egui::vec2(1320.0, 760.0),
    ] {
        for decoded in [false, true] {
            assert_runtime_patch_viewport(size, decoded);
        }
    }
}

fn assert_runtime_patch_viewport(size: egui::Vec2, decoded: bool) {
    let key = RuntimeGraphKey::new(None, 1, []);
    let mut app = PackageAuthoringApp {
        show_experimental_options: true,
        show_plug_safety_warnings: false,
        runtime_graph_target: Some(key.clone()),
        runtime_graph: decoded.then(|| (key, Arc::new(runtime_graph()))),
        ..Default::default()
    };
    let before = app.recipe.clone();
    let context = egui::Context::default();
    let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, size);
    let mut output = egui::FullOutput::default();
    for _ in 0..3 {
        output = context.run(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    workbench_style(ui);
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        app.draw_runtime_component_donors(ui);
                    });
                });
            },
        );
    }
    assert_patch_layout(&output, screen, decoded);
    assert_eq!(
        app.recipe, before,
        "Layout and warnings must not change the recipe"
    );
}

fn assert_patch_layout(output: &egui::FullOutput, screen: egui::Rect, decoded: bool) {
    let size = screen.size();
    let labels = text(output);
    assert!(labels.contains("high risk of crashes. Use with caution."));
    assert_eq!(labels.matches("Binary Runtime Patches").count(), 1);
    let patches = text_origin(output, "Binary Runtime Patches");
    let donors = text_origin(output, "Runtime Component Donors");
    let values = text_origin(output, "Runtime Values");
    assert!(screen.contains(patches), "{size:?}, decoded={decoded}");
    assert!(patches.y > donors.y);
    if decoded {
        let last_donor = text_origin(output, "Advanced Runtime Bindings… (0)");
        assert!(patches.y > last_donor.y);
        assert!(patches.y - last_donor.y < 50.0);
        assert!(labels.contains("with saved field edits"));
        assert!(labels.contains("not final in-game stats"));
    }
    if size.x < 1180.0 {
        assert!(
            patches.y < values.y,
            "Patches must precede the long value list"
        );
    } else {
        assert!(
            patches.x < values.x,
            "Patches must stay in the donor column"
        );
    }
    let (clip, rect) = output
        .shapes
        .iter()
        .find_map(|clipped| match &clipped.shape {
            egui::Shape::Text(value) if value.galley.job.text == "Binary Runtime Patches" => {
                Some((
                    clipped.clip_rect,
                    egui::Rect::from_min_size(value.pos, value.galley.size()),
                ))
            }
            _ => None,
        })
        .unwrap();
    assert!(
        clip.contains_rect(rect),
        "Patch heading is clipped at {size:?}"
    );
}
