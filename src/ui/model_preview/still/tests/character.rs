//! Native character loads, editable UI and cache invalidation through the inline viewer.
use super::*;

#[test]
fn character_views_reuse_decoded_models_and_respect_package_changes() {
    let Some(packages) = std::env::var_os("SUNDIAL_APPEARANCE_PACKAGES") else {
        return;
    };
    let packages = PathBuf::from(packages);
    let catalog = crate::character::Catalog::load(&packages).unwrap();
    let request = model_preview::character::Character {
        appearance: catalog.default_appearance(0, 1).unwrap(),
        armor: crate::character::tests::native_armor(&packages),
        portrait: true,
    };
    let manifest =
        crate::catalog::Catalog::for_test_with_packages(packages.parent().unwrap().to_owned());
    let access = manifest.inspection_access();
    let ctx = egui::Context::default();
    let id = egui::Id::new("character-view-lifecycle");
    let edit = egui::Id::new("character-view-notes");
    let typed = std::cell::RefCell::new(String::new());
    let frames = std::cell::RefCell::new(Vec::new());
    let draw = |portrait, events| {
        let started = std::time::Instant::now();
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(400., 300.),
                )),
                events,
                ..Default::default()
            },
            |ui| {
                ui.add(egui::TextEdit::singleline(&mut *typed.borrow_mut()).id(edit));
                let mut character = request.clone();
                character.portrait = portrait;
                show_character(
                    ui,
                    id,
                    &packages,
                    character,
                    egui::vec2(96., 144.),
                    access.clone(),
                );
            },
        );
        frames
            .borrow_mut()
            .push(started.elapsed().as_secs_f64() * 1000.);
        crate::test_support::capture::record(&output);
        output
    };
    let ready = |portrait| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(120);
        loop {
            let output = draw(portrait, vec![]);
            let state = ctx.data_mut(|data| data.get_temp::<Arc<Mutex<Still>>>(id).unwrap());
            let still = state.lock().unwrap();
            assert!(
                still.error.is_none(),
                "{:?}",
                still.error.as_ref().map(|(_, error)| error)
            );
            if still.model.is_some() && still.rendered.is_some() {
                return output;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "Character preview did not finish"
            );
            drop(still);
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    };
    draw(true, vec![]);
    ctx.memory_mut(|memory| memory.request_focus(edit));
    draw(true, vec![egui::Event::Text("portrait ".into())]);
    let portrait = ready(true);
    crate::test_support::capture::write(&ctx, &portrait, "character-view-portrait");
    draw(false, vec![egui::Event::Text("body ".into())]);
    let body = ready(false);
    crate::test_support::capture::write(&ctx, &body, "character-view-body");
    let state = ctx.data_mut(|data| data.get_temp::<Arc<Mutex<Still>>>(id).unwrap());
    let body_triangles = state
        .lock()
        .unwrap()
        .model
        .as_ref()
        .unwrap()
        .triangles
        .len();
    for portrait in [true, false, true, false] {
        draw(portrait, vec![]);
        let still = state.lock().unwrap();
        assert!(
            still.pending.is_none(),
            "A warm framing switch reopened the packages"
        );
        assert!(still.model.is_some());
        if !portrait {
            assert_eq!(
                still.model.as_ref().unwrap().triangles.len(),
                body_triangles
            );
        }
    }
    manifest.suspend_package_access();
    draw(true, vec![]);
    assert!(
        state.lock().unwrap().model.is_none(),
        "An old package generation remained visible"
    );
    manifest.resume_package_access();
    draw(false, vec![]);
    assert!(
        state.lock().unwrap().pending.is_some(),
        "The package change reused stale geometry"
    );
    ready(false);
    assert_eq!(*typed.borrow(), "portrait body ");
    if let Some(path) = crate::test_support::artifacts("character-view-lifecycle.json") {
        std::fs::write(
            path,
            serde_json::to_vec_pretty(&serde_json::json!({
                "body_triangles":body_triangles,"warm_switches":4,"package_change_reloaded":true,
                "typed":*typed.borrow(),"ui_frame_ms":*frames.borrow()
            }))
            .unwrap(),
        )
        .unwrap();
    }
}
