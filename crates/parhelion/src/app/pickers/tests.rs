//! A popup keeps its own open state, so a control inside it cannot dismiss it.
use std::cell::Cell;

struct Probe {
    drawn: Cell<bool>,
    openness: Cell<f32>,
}

impl Probe {
    fn new() -> Self {
        Self {
            drawn: Cell::new(false),
            openness: Cell::new(0.0),
        }
    }
}

/// One frame of a panel holding the picker popup, with an Advanced section inside it.
fn run(ctx: &egui::Context, probe: &Probe, events: Vec<egui::Event>) -> egui::FullOutput {
    probe.drawn.set(false);
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 700.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let mut query = String::new();
                let picked = super::popup(
                    ui,
                    "picker-under-test",
                    "Open Picker",
                    &mut query,
                    |ui, _query, _reset, _height| {
                        probe.drawn.set(true);
                        let header = egui::CollapsingHeader::new("Advanced")
                            .id_salt("advanced-under-test")
                            .show(ui, |ui| {
                                ui.label("Technical Detail");
                            });
                        probe.openness.set(header.openness);
                        Option::<()>::None
                    },
                );
                assert!(picked.is_none());
            });
        },
    )
}

fn label(output: &egui::FullOutput, name: &str) -> Option<egui::Rect> {
    output.shapes.iter().find_map(|shape| match &shape.shape {
        egui::Shape::Text(text) if text.galley.job.text == name => {
            Some(text.galley.rect.translate(text.pos.to_vec2()))
        }
        _ => None,
    })
}

/// Press and release over one position, one frame each, the way a pointer reports a click.
fn click(ctx: &egui::Context, probe: &Probe, at: egui::Pos2) -> egui::FullOutput {
    let mut output = egui::FullOutput::default();
    for pressed in [true, false] {
        output = run(
            ctx,
            probe,
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::default(),
                },
            ],
        );
    }
    output
}

#[test]
fn an_advanced_section_inside_a_picker_popup_does_not_dismiss_it() {
    let ctx = egui::Context::default();
    let probe = Probe::new();
    let output = run(&ctx, &probe, vec![]);
    assert!(!probe.drawn.get(), "the popup starts closed");
    let anchor = label(&output, "Open Picker")
        .expect("the anchor button")
        .center();
    click(&ctx, &probe, anchor);
    let output = run(&ctx, &probe, vec![]);
    assert!(probe.drawn.get(), "the anchor opens the popup");
    let advanced = label(&output, "Advanced")
        .expect("the Advanced section inside the popup")
        .center();
    click(&ctx, &probe, advanced);
    run(&ctx, &probe, vec![]);
    assert!(
        probe.drawn.get(),
        "clicking Advanced inside the popup closed it"
    );
    assert!(
        probe.openness.get() > 0.0,
        "clicking Advanced did not open the section"
    );
}

/// One frame of a panel holding the picker popup, with a Detail dropdown inside it.
fn run_combo(ctx: &egui::Context, probe: &Probe, events: Vec<egui::Event>) -> egui::FullOutput {
    probe.drawn.set(false);
    ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 700.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let mut query = String::new();
                let picked = super::popup(
                    ui,
                    "picker-under-test",
                    "Open Picker",
                    &mut query,
                    |ui, _query, _reset, _height| {
                        probe.drawn.set(true);
                        let mut detail = "Standard";
                        egui::ComboBox::from_id_salt("detail-under-test")
                            .selected_text(detail)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut detail, "Standard", "Standard");
                                ui.selectable_value(&mut detail, "Advanced", "Advanced");
                            });
                        Option::<()>::None
                    },
                );
                assert!(picked.is_none());
            });
        },
    )
}

#[test]
fn a_dropdown_inside_a_picker_popup_does_not_dismiss_it() {
    let ctx = egui::Context::default();
    let probe = Probe::new();
    let output = run_combo(&ctx, &probe, vec![]);
    let anchor = label(&output, "Open Picker")
        .expect("the anchor button")
        .center();
    for pressed in [true, false] {
        run_combo(
            &ctx,
            &probe,
            vec![
                egui::Event::PointerMoved(anchor),
                egui::Event::PointerButton {
                    pos: anchor,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::default(),
                },
            ],
        );
    }
    let output = run_combo(&ctx, &probe, vec![]);
    assert!(probe.drawn.get(), "the anchor opens the popup");
    let combo = label(&output, "Standard")
        .expect("the Detail dropdown inside the popup")
        .center();
    for pressed in [true, false] {
        run_combo(
            &ctx,
            &probe,
            vec![
                egui::Event::PointerMoved(combo),
                egui::Event::PointerButton {
                    pos: combo,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::default(),
                },
            ],
        );
    }
    run_combo(&ctx, &probe, vec![]);
    assert!(
        probe.drawn.get(),
        "opening a dropdown inside the popup closed the popup"
    );
}

#[test]
fn a_click_outside_the_picker_popup_still_closes_it() {
    let ctx = egui::Context::default();
    let probe = Probe::new();
    let output = run(&ctx, &probe, vec![]);
    let anchor = label(&output, "Open Picker")
        .expect("the anchor button")
        .center();
    click(&ctx, &probe, anchor);
    run(&ctx, &probe, vec![]);
    assert!(probe.drawn.get(), "the anchor opens the popup");
    click(&ctx, &probe, egui::pos2(880.0, 680.0));
    run(&ctx, &probe, vec![]);
    assert!(!probe.drawn.get(), "a click outside left the popup open");
}

#[test]
fn escape_closes_the_picker_popup() {
    let ctx = egui::Context::default();
    let probe = Probe::new();
    let output = run(&ctx, &probe, vec![]);
    let anchor = label(&output, "Open Picker")
        .expect("the anchor button")
        .center();
    click(&ctx, &probe, anchor);
    run(&ctx, &probe, vec![]);
    assert!(probe.drawn.get(), "the anchor opens the popup");
    run(
        &ctx,
        &probe,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::default(),
        }],
    );
    run(&ctx, &probe, vec![]);
    assert!(!probe.drawn.get(), "Escape left the popup open");
}
