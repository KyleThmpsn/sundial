use super::*;
use crate::install::{InstallPhase, InstallProgress};

#[test]
fn slot_replacement_review_names_every_move_and_full_inventory_deletion() {
    use std::collections::BTreeMap;
    use sundial::investment::{AuthoredSlotChange, AuthoredSlotReplacement};
    let directory = tempfile::tempdir().unwrap();
    let packages = directory.path().join("packages");
    std::fs::create_dir(&packages).unwrap();
    let item = |id, hash| serde_json::json!({"instance_soid":format!("0x{id:016X}"),"definition_hash":hash,"level":106,"quantity":1,"plugs":null});
    let document = serde_json::json!({"version":8,"state":{"account":{"primary_soid":"0x0000000000000001"},"characters":[
        {"soid":"0x0000000000000002","class":0,"equipment":{"kinetic":item(10,100),"energy":item(11,200)},"inventory":[]},
        {"soid":"0x0000000000000003","class":1,"equipment":{"kinetic":item(20,100),"energy":item(21,200)},"inventory":[item(22,200)]}
    ]}});
    std::fs::write(
        directory.path().join("settings.json"),
        serde_json::to_vec(&document).unwrap(),
    )
    .unwrap();
    let review = crate::install::test_review_with_slots(
        &packages,
        BTreeSet::new(),
        vec![],
        Some(AuthoredSlotReplacement {
            changes: vec![AuthoredSlotChange {
                definition_hash: 100,
                previous_bucket: 0,
                incoming_bucket: 1,
            }],
            incoming_buckets: BTreeMap::from([(100, 1), (200, 1)]),
            weapon_capacities: [10, 2, 10],
        }),
    );
    for dark in [true, false] {
        for width in [560.0, 960.0] {
            let mut app = fixture("review");
            app.replacement_review = Some(Ok(review.clone()));
            let ctx = egui::Context::default();
            ctx.set_visuals(if dark {
                egui::Visuals::dark()
            } else {
                egui::Visuals::light()
            });
            let mut output = egui::FullOutput::default();
            for _ in 0..3 {
                output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, 760.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default()
                            .show(ctx, |ui| app.draw_install_confirmation(ui));
                    },
                );
            }
            let labels = text(&output);
            assert!(
                labels.contains("Move Item 0x00000064 from Character 1"),
                "{labels}"
            );
            assert!(
                labels.contains("Delete Item 0x00000064 from Character 2"),
                "{labels}"
            );
            assert!(
                labels.contains("There is no room in inventory for its new Energy slot."),
                "{labels}"
            );
            assert!(labels.contains("Back Up, Remove & Install"));
            capture(
                &ctx,
                output,
                &format!(
                    "slot-review-{}-{width}",
                    if dark { "dark" } else { "light" }
                ),
                width,
            );
        }
    }
}

#[test]
fn build_flow_stays_read_only_and_keeps_progress_and_actions_visible() {
    for dark in [true, false] {
        for width in [560.0, 960.0] {
            for state in ["build", "ready", "review", "install", "failure"] {
                let mut app = fixture(state);
                let recipe = app.recipe.clone();
                let ctx = egui::Context::default();
                ctx.set_visuals(if dark {
                    egui::Visuals::dark()
                } else {
                    egui::Visuals::light()
                });
                let mut output = egui::FullOutput::default();
                for _ in 0..3 {
                    output = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                egui::vec2(width, 760.0),
                            )),
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default().show(ctx, |_ui| {});
                            app.draw_build_status_window(ctx);
                        },
                    );
                }
                let labels = text(&output);
                assert!(
                    labels.contains("1. Build")
                        && labels.contains("2. Review")
                        && labels.contains("3. Install")
                );
                if matches!(state, "build" | "install") {
                    assert!(labels.contains("Current Operation"));
                    assert!(labels.contains("Copy Progress") && labels.contains("Elapsed"));
                }
                let rect = ctx
                    .memory(|memory| memory.area_rect(egui::Id::new("parhelion_build_status")))
                    .unwrap();
                assert!(
                    rect.right() <= width + 1.0 && rect.bottom() <= 761.0,
                    "{state}, {width}: {rect:?}"
                );
                assert_eq!(app.recipe, recipe);
                capture(
                    &ctx,
                    output,
                    &format!("{state}-{}-{width}", if dark { "dark" } else { "light" }),
                    width,
                );
            }
        }
    }
}

fn fixture(state: &str) -> PackageAuthoringApp {
    let mut app = PackageAuthoringApp {
        build_status_open: true,
        ..Default::default()
    };
    let elapsed = Duration::from_secs(48);
    app.build_activity.push(
        Duration::from_secs(2),
        "Inspecting source packages (3/3)".into(),
    );
    app.build_activity.push(
        Duration::from_secs(40),
        "Compiling weapon project (1/1)".into(),
    );
    app.build_activity
        .push(Duration::from_secs(44), "Writing package set (1/1)".into());
    app.build_activity.push(
        elapsed,
        "Validating package artifacts (3/9): w64_investment_0361_7.pkg".into(),
    );
    if state == "build" {
        app.build_receiver = Some(mpsc::channel().1);
        app.build_progress = Some(TimedBuildProgress {
            phase: BuildPhase::ValidatingPackages,
            current_artifact: Some("w64_investment_0361_7.pkg".into()),
            completed: 3,
            total: 9,
            elapsed,
        });
    } else {
        app.latest_build = Some(Ok(BuildReport {
            weapons: ["The Wanderer", "Stargazer", "Every End"]
                .into_iter()
                .map(|name| crate::workflow::WeaponBuildReport {
                    name: name.into(),
                    namespace: "parhelion.preview".into(),
                    item_hash: 1,
                    icon_definition_hash: 2,
                    item_index: 3,
                    collectible_hash: 4,
                    collectible_index: 5,
                    unlock_hash: 6,
                    unlock_definition_index: 7,
                    unlock_bank: 1,
                    unlock_slot: 8,
                })
                .collect(),
            run_directory: PathBuf::from("C:/Parhelion/staging/review"),
            manifest_path: PathBuf::from("C:/Parhelion/staging/review/manifest.json"),
            artifacts: CANONICAL_ARTIFACT_FILE_NAMES
                .iter()
                .map(|name| crate::artifact::ArtifactMetadata {
                    file_name: (*name).into(),
                    byte_length: 120_000,
                    sha256: "0".repeat(64),
                })
                .collect(),
            selection_fingerprint: "preview".into(),
            staged_recipe_paths: vec![],
        }));
    }
    if state == "review" {
        app.build_dialog_step = BuildDialogStep::ReviewInstall;
        let directory = tempfile::tempdir().unwrap();
        let packages = directory.path().join("packages");
        std::fs::create_dir(&packages).unwrap();
        std::fs::write(directory.path().join("settings.json"), br#"{"version":8,"state":{"account":{"primary_soid":"0x0000000000000001","profile_items":[]},"characters":[]}}"#).unwrap();
        app.replacement_review = Some(Ok(crate::install::test_review(&packages, BTreeSet::new())));
    }
    if matches!(state, "install" | "failure") {
        app.build_dialog_step = BuildDialogStep::Install;
        app.install_status.elapsed = Duration::from_secs(18);
        app.install_status.activity.push(
            Duration::from_secs(5),
            "Backing up existing files (9/9)".into(),
        );
        app.install_status.activity.push(
            Duration::from_secs(12),
            "Preparing package files (9/9)".into(),
        );
        app.install_status.activity.push(
            Duration::from_secs(18),
            "Installing packages (3/9): w64_investment_0361_7.pkg".into(),
        );
        app.install_status.progress = Some(InstallProgress {
            phase: InstallPhase::Installing,
            current_artifact: Some("w64_investment_0361_7.pkg".into()),
            completed: 3,
            total: 9,
        });
        if state == "install" {
            app.install_receiver = Some(mpsc::channel().1);
        } else {
            app.latest_install = Some(Err("A target package changed after review. Review the installation again before continuing.".into()));
        }
    }
    app
}

// Optional QA output uses the actual egui meshes and font atlas, without opening an app or installing files.
pub(in crate::app) fn capture(
    ctx: &egui::Context,
    output: egui::FullOutput,
    name: &str,
    width: f32,
) {
    let Some(directory) = std::env::var_os("PARHELION_UI_CAPTURE_DIR").map(PathBuf::from) else {
        return;
    };
    std::fs::create_dir_all(&directory).unwrap();
    let atlas = ctx.fonts(|fonts| fonts.image());
    let pixels = atlas
        .srgba_pixels(None)
        .flat_map(|color| color.to_array())
        .collect::<Vec<_>>();
    image::save_buffer(
        directory.join(format!("{name}-atlas.png")),
        &pixels,
        atlas.size[0] as u32,
        atlas.size[1] as u32,
        image::ColorType::Rgba8,
    )
    .unwrap();
    let meshes = ctx.tessellate(output.shapes, 1.0).into_iter().filter_map(|primitive| {
        let egui::epaint::Primitive::Mesh(mesh) = primitive.primitive else { return None; };
        assert_eq!(mesh.texture_id, egui::TextureId::Managed(0));
        Some(serde_json::json!({
            "clip": [primitive.clip_rect.min.x, primitive.clip_rect.min.y, primitive.clip_rect.max.x, primitive.clip_rect.max.y],
            "indices": mesh.indices,
            "vertices": mesh.vertices.iter().map(|v| serde_json::json!([v.pos.x, v.pos.y, v.uv.x, v.uv.y, v.color.to_array()])).collect::<Vec<_>>()
        }))
    }).collect::<Vec<_>>();
    std::fs::write(
        directory.join(format!("{name}.json")),
        serde_json::to_vec(&serde_json::json!({"width": width, "height": 760, "meshes": meshes}))
            .unwrap(),
    )
    .unwrap();
}
