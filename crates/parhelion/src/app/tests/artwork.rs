//! Artwork editing from a nameplate page through saved recipes and exported pixels.
use super::*;
use crate::app::custom_perks::workbench::tests::capture;
use crate::emblem::{NameplateImage, NameplatePart};
use crate::image_import::EmbeddedImage;
use crate::test_support::driver::{label, tap};

fn frame(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    size: egui::Vec2,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                workbench_style(ui);
                // The page opens scrolled to the nameplate, then the wheel moves it, so a control
                // the larger text pushed out of view can still be scrolled to.
                let opened = egui::Id::new("artwork-test-opened");
                let first = ui
                    .ctx()
                    .data(|data| data.get_temp::<bool>(opened).is_none());
                ui.ctx().data_mut(|data| data.insert_temp(opened, true));
                let mut area = egui::ScrollArea::vertical();
                if first {
                    area = area.vertical_scroll_offset(900.0);
                }
                area.show(ui, |ui| {
                    app.draw_emblem_nameplate(ui);
                });
            });
            app.draw_artwork_editor(ui);
        },
    );
    capture::record(&output);
    output
}

fn settle(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    size: egui::Vec2,
) -> egui::FullOutput {
    frame(ctx, app, size, vec![]);
    frame(ctx, app, size, vec![]);
    frame(ctx, app, size, vec![])
}

fn click(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    size: egui::Vec2,
    name: &str,
) -> egui::FullOutput {
    let mut output = settle(ctx, app, size);
    let mut at = None;
    for _ in 0..8 {
        let target = label(&output, name);
        if let Some(rect) = target {
            let visible = output.shapes.iter().find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.job.text == name => {
                    let visible = rect.intersect(shape.clip_rect);
                    visible.is_positive().then_some(visible.center())
                }
                _ => None,
            });
            if visible.is_some() {
                at = visible;
                break;
            }
        }
        let missing_direction =
            if matches!(name, "Before" | "After" | "Colors" | "Size and Position") {
                180.0
            } else {
                -180.0
            };
        let delta = target.map_or(missing_direction, |rect| {
            if rect.center().y < size.y * 0.5 {
                180.0
            } else {
                -180.0
            }
        });
        frame(
            ctx,
            app,
            size,
            vec![
                egui::Event::PointerMoved(egui::pos2(size.x * 0.72, size.y * 0.55)),
                egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, delta),
                    modifiers: Default::default(),
                    phase: egui::TouchPhase::Move,
                },
            ],
        );
        output = settle(ctx, app, size);
    }
    let at = at.unwrap_or_else(|| panic!("Missing visible artwork control {name}"));
    for events in tap(at) {
        frame(ctx, app, size, events);
    }
    settle(ctx, app, size)
}

fn picture(part: NameplatePart) -> EmbeddedImage {
    let (width, height) = part.size();
    EmbeddedImage::from_rgba(image::RgbaImage::from_fn(width, height, |x, _| {
        image::Rgba(if x < width / 2 {
            [200, 30, 50, 128]
        } else {
            [10, 180, 70, 255]
        })
    }))
    .unwrap()
}

fn edit_part(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    size: egui::Vec2,
    part: NameplatePart,
    theme: &str,
) {
    app.emblem_page.selected = part;
    let before = app.recipe.clone();
    click(ctx, app, size, "Edit Artwork…");
    click(ctx, app, size, "Flip Horizontally");
    click(ctx, app, size, "Colors");
    let output = click(ctx, app, size, "Invert Colors");
    assert_eq!(app.recipe, before, "editing must keep a private draft");
    capture::write(
        ctx,
        &output,
        &format!("artwork-{theme}-{}", part.label().to_lowercase()),
    );
    for name in ["Cancel", "Apply Artwork"] {
        assert!(
            egui::Rect::from_min_size(egui::Pos2::ZERO, size)
                .contains_rect(label(&output, name).unwrap()),
            "{name} outside {size:?}"
        );
    }
    click(ctx, app, size, "Before");
    click(ctx, app, size, "After");
    click(ctx, app, size, "Cancel");
    assert_eq!(app.recipe, before);

    for name in [
        "Edit Artwork…",
        "Flip Horizontally",
        "Reset All Edits",
        "Colors",
        "Invert Colors",
        "Size and Position",
        "Flip Horizontally",
        "Reset Size and Position",
        "Flip Horizontally",
        "Apply Artwork",
    ] {
        click(ctx, app, size, name);
    }
    assert!(!app.presentation_editor.editing());
    assert!(app.recipe_dirty);
}

fn read_back(
    recipe: &WeaponRecipe,
    part: NameplatePart,
    theme: &str,
    artifacts: &Path,
    packages: &Path,
) -> serde_json::Value {
    let saved = serde_json::to_vec_pretty(recipe).unwrap();
    let reopened: WeaponRecipe = serde_json::from_slice(&saved).unwrap();
    let Some(NameplateImage::Artwork { artwork, .. }) = reopened
        .overrides
        .nameplate
        .as_ref()
        .and_then(|nameplate| nameplate.part(part))
    else {
        panic!("{part:?} did not save editable artwork");
    };
    assert_eq!(
        artwork.pixels(),
        picture(part).pixels(),
        "the source is retained in full"
    );
    let pixels = artwork.render(part.size().0, part.size().1);
    assert_eq!(pixels.dimensions(), part.size());
    assert_eq!(
        pixels.get_pixel(part.size().0 / 4, part.size().1 / 2).0,
        [245, 75, 185, 255]
    );
    assert_eq!(
        pixels.get_pixel(part.size().0 * 3 / 4, part.size().1 / 2).0,
        [55, 225, 205, 128]
    );
    let name = format!("{theme}-{}", part.label().to_lowercase());
    let png = artifacts.join(format!("{name}.png"));
    super::super::emblem_view::write_png(
        &png,
        packages,
        part,
        super::super::emblem_view::Export::Artwork(artwork.clone(), part.size()),
    )
    .unwrap();
    assert_eq!(image::open(&png).unwrap().into_rgba8(), pixels);
    std::fs::write(artifacts.join(format!("{name}.parhelion.json")), saved).unwrap();
    serde_json::json!({"theme":theme,"part":part.label(),"size":part.size(),"png":png.file_name().unwrap().to_string_lossy()})
}

#[test]
fn nameplate_artwork_edits_cancel_reset_save_reopen_and_export() {
    let temporary = tempfile::tempdir().unwrap();
    let artifacts =
        crate::test_support::artifacts("artwork").unwrap_or_else(|| temporary.path().to_owned());
    std::fs::create_dir_all(&artifacts).unwrap();
    let mut receipts = Vec::new();
    for (theme, size) in [
        ("dark-wide", egui::vec2(1080.0, 800.0)),
        ("light-narrow", egui::vec2(360.0, 640.0)),
    ] {
        let ctx = egui::Context::default();
        if theme.starts_with("light") {
            ctx.set_visuals(egui::Visuals::light());
        }
        let mut app = PackageAuthoringApp {
            recipe: WeaponRecipe::new_unbound_kind(ItemKind::Emblem).unwrap(),
            ..Default::default()
        };
        for part in NameplatePart::ALL {
            app.recipe
                .overrides
                .nameplate
                .get_or_insert_with(Default::default)
                .set(
                    part,
                    Some(NameplateImage::Image {
                        image: picture(part),
                    }),
                );
        }
        for part in NameplatePart::ALL {
            edit_part(&ctx, &mut app, size, part, theme);
            receipts.push(read_back(
                &app.recipe,
                part,
                theme,
                &artifacts,
                temporary.path(),
            ));
        }
    }
    std::fs::write(
        artifacts.join("readback.json"),
        serde_json::to_vec_pretty(&receipts).unwrap(),
    )
    .unwrap();
}
