use super::*;
use eframe::egui;
use serde_json::json;

fn defaults() -> Value {
    serde_json::from_str(include_str!(
        "../../../tests/fixtures/sunrise-v6-4aebb148-defaults.json"
    ))
    .unwrap()
}

#[test]
fn dawn_missing_flags_are_valid_default_off_and_preserved() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = Runtime::inspect(&dir.path().join("steam_api64.dll"));
    let document = defaults();
    let before = document.clone();
    assert!(!executor_enabled(&document));
    assert_eq!(runtime.validate(&document), Ok(()));
    assert_eq!(document, before);
    assert!(document.get("experiments").is_none());
}

#[test]
fn dawn_checks_every_known_boolean_and_preserves_unknown_extensions() {
    for (group, fields) in [
        (OMEGA, OMEGA_FLAGS.as_slice()),
        (CLIENT, CLIENT_FLAGS.as_slice()),
    ] {
        for (key, _) in fields {
            for invalid in [Value::Null, json!(0), json!("false"), json!([]), json!({})] {
                let mut document = defaults();
                assert!(set_flag(&mut document, group, key, true));
                *document.pointer_mut(&format!("{group}/{key}")).unwrap() = invalid;
                assert!(
                    settings_issues(&document)
                        .iter()
                        .any(|e| e.contains(&dotted(&format!("{group}/{key}"))))
                );
                assert!(set_flag(&mut document, group, key, false));
                assert!(settings_issues(&document).is_empty());
            }
        }
    }
    let mut document = defaults();
    document["experiments"] = json!({"unknown": null, "omega": {"future_flag": ["opaque"]}});
    document["client"]["future_flag"] = json!({"keep": true});
    assert!(settings_issues(&document).is_empty());
    let before = document.clone();
    assert!(set_flag(&mut document, OMEGA, "coo_executor", true));
    assert!(executor_enabled(&document));
    document["experiments"]["omega"]
        .as_object_mut()
        .unwrap()
        .remove("coo_executor");
    assert_eq!(document, before);
}

#[test]
fn dawn_malformed_groups_are_reported_without_discarding_them() {
    for group in ["/experiments", OMEGA, CLIENT] {
        let mut document = defaults();
        document["experiments"] = json!({"omega": {}});
        *document.pointer_mut(group).unwrap() = json!(["keep this"]);
        let before = document.clone();
        assert!(
            settings_issues(&document)
                .iter()
                .any(|e| e == &format!("{} must be an object.", dotted(group)))
        );
        let flag_group = if group == CLIENT { CLIENT } else { OMEGA };
        assert!(!set_flag(&mut document, flag_group, "coo_executor", true));
        assert_eq!(document, before);
    }
}

#[test]
fn dawn_executor_requires_a_readable_nonempty_script_beside_its_dll() {
    let dir = tempfile::tempdir().unwrap();
    let mut document = defaults();
    set_flag(&mut document, OMEGA, "coo_executor", true);
    for relative in ["", "bin/x64"] {
        let dll = dir.path().join(relative).join("steam_api64.dll");
        let missing = Runtime::inspect(&dll);
        assert!(
            missing
                .validate(&document)
                .unwrap_err()
                .contains("omega.lua")
        );
        fs::create_dir_all(missing.script_path.parent().unwrap()).unwrap();
        fs::write(&missing.script_path, b" \r\n\t").unwrap();
        assert!(
            Runtime::inspect(&dll)
                .validate(&document)
                .unwrap_err()
                .contains("empty")
        );
        fs::write(&missing.script_path, b"return {}").unwrap();
        assert_eq!(Runtime::inspect(&dll).validate(&document), Ok(()));
        if relative.is_empty() {
            // The bin/x64 iteration must still fail while the root script exists.
            assert!(
                Runtime::inspect(&dir.path().join("bin/x64/steam_api64.dll"))
                    .validate(&document)
                    .is_err()
            );
        }
    }
    let dll = dir.path().join("steam_api64.dll");
    let present = Runtime::inspect(&dll);
    fs::remove_file(&present.script_path).unwrap();
    assert!(Runtime::inspect(&dll).validate(&document).is_err());
    set_flag(&mut document, OMEGA, "coo_executor", false);
    assert_eq!(Runtime::inspect(&dll).validate(&document), Ok(()));
}

fn draw_page(
    context: &egui::Context,
    size: egui::Vec2,
    events: Vec<egui::Event>,
    document: &mut Value,
    runtime: Option<&mut Runtime>,
    tab: &mut crate::game_settings::Tab,
) -> (egui::FullOutput, bool) {
    let mut changed = false;
    let mut runtime = runtime;
    let output = context.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let edits = crate::game_settings::draw_page(
                    ui,
                    crate::game_settings::PageContext {
                        json_document: document,
                        account_settings: Err("No account needed on this tab"),
                        bindings_editable: false,
                        json_account: true,
                        extended_fov: false,
                        dawn: runtime.as_deref_mut(),
                        tab,
                        key_bindings: &mut Default::default(),
                    },
                );
                changed |= edits.json_changed;
                assert!(edits.account_commands.is_empty());
            });
        },
    );
    (output, changed)
}

fn text_position(output: &egui::FullOutput, text: &str) -> Option<egui::Pos2> {
    output.shapes.iter().find_map(|shape| match &shape.shape {
        egui::Shape::Text(shape) if shape.galley.job.text == text => {
            Some(shape.pos + egui::vec2(3.0, 3.0))
        }
        _ => None,
    })
}

#[test]
fn dawn_page_is_gated_by_detection_and_readonly_until_clicked() {
    for theme in [egui::Theme::Dark, egui::Theme::Light] {
        for size in [egui::vec2(640.0, 480.0), egui::vec2(1240.0, 900.0)] {
            let dir = tempfile::tempdir().unwrap();
            let mut runtime = Runtime::inspect(&dir.path().join("steam_api64.dll"));
            let context = egui::Context::default();
            context.set_theme(theme);
            let mut document = defaults();
            document["experiments"] = json!({"omega": {"unknown": [1, 2, 3]}});
            let before = document.clone();
            let mut tab = crate::game_settings::Tab::Dawn;
            let (output, changed) =
                draw_page(&context, size, vec![], &mut document, None, &mut tab);
            assert!(tab == crate::game_settings::Tab::Player);
            assert!(text_position(&output, "Dawn").is_none());
            assert!(!changed);
            assert_eq!(document, before);
            tab = crate::game_settings::Tab::Dawn;
            let mut position = None;
            for _ in 0..3 {
                let (output, changed) = draw_page(
                    &context,
                    size,
                    vec![],
                    &mut document,
                    Some(&mut runtime),
                    &mut tab,
                );
                assert!(!changed);
                position = text_position(&output, "Omega Lua Executor");
            }
            assert_eq!(document, before);
            let pos = position.expect("executor checkbox must be visible");
            assert!(egui::Rect::from_min_size(egui::Pos2::ZERO, size).contains(pos));
            let mut clicked = false;
            for pressed in [true, false] {
                let events = vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ];
                clicked |= draw_page(
                    &context,
                    size,
                    events,
                    &mut document,
                    Some(&mut runtime),
                    &mut tab,
                )
                .1;
            }
            assert!(clicked);
            let mut expected = before;
            expected["experiments"]["omega"]["coo_executor"] = json!(true);
            assert_eq!(document, expected);
        }
    }
}
