use super::super::cards::Card;
use super::*;
use sundial::package_authoring::sandbox_perk::program::{
    Action, Asset, NativeProgram, Position, Program, Trigger,
};

fn setup() -> (egui::Context, Workbench) {
    let ctx = egui::Context::default();
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    ctx.set_fonts(fonts);
    let mut recipe = PerkRecipe::new();
    recipe.name = "Effect Organization".into();
    let mut spawn = program::new_effect(1178);
    let program = spawn.program.as_mut().unwrap();
    program.name = "Spawn on Kill".into();
    program.trigger = Trigger::WeaponKill;
    program.actions.push(Action::Spawn {
        asset: Asset::default(),
        position: Position::Event,
    });
    let mut native = program::new_effect(1179);
    let program = native.program.as_mut().unwrap();
    program.name = "Additional Conditions".into();
    program.native = Some(NativeProgram::empty());
    recipe.effects = vec![spawn, native, PerkRecipe::effect(421)];
    (
        ctx,
        Workbench {
            initialized: true,
            open: true,
            drafts_writable: true,
            page: Page::Effects,
            documents: vec![Document::new(recipe, None)],
            ..Default::default()
        },
    )
}

fn render(ctx: &egui::Context, workbench: &mut Workbench, size: egui::Vec2) -> egui::FullOutput {
    let mut output = egui::FullOutput::default();
    for _ in 0..4 {
        output = frame(ctx, workbench, true, size, vec![]);
    }
    output
}

fn rects(output: &egui::FullOutput, name: &str) -> Vec<egui::Rect> {
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == name => {
                Some(text.galley.rect.translate(text.pos.to_vec2()))
            }
            _ => None,
        })
        .collect()
}

fn pointer(
    ctx: &egui::Context,
    workbench: &mut Workbench,
    size: egui::Vec2,
    pos: egui::Pos2,
    pressed: bool,
) {
    frame(
        ctx,
        workbench,
        true,
        size,
        vec![
            egui::Event::PointerMoved(pos),
            egui::Event::PointerButton {
                pos,
                pressed,
                button: egui::PointerButton::Primary,
                modifiers: Default::default(),
            },
        ],
    );
}

fn collapsed(ctx: &egui::Context, workbench: &Workbench) {
    let recipe = &workbench.documents[0].recipe;
    for (position, effect) in recipe.effects.iter().enumerate() {
        Card::new(
            &recipe.id,
            effect.source_perk_index,
            position,
            recipe.effects.len(),
        )
        .set_expanded(ctx, false);
    }
}

#[test]
fn validation_reveals_the_incomplete_action_inside_a_collapsed_effect() {
    let size = egui::vec2(1320.0, 900.0);
    let (ctx, mut workbench) = setup();
    collapsed(&ctx, &workbench);
    let before = workbench.documents[0].recipe.clone();
    let issue = workbench.validation_issue(&before).unwrap();
    assert_eq!(
        issue.location.unwrap(),
        validation::Location {
            document: before.id.clone(),
            effect: 1178,
            action: Some(0),
            native: None,
        }
    );
    let output = render(&ctx, &mut workbench, size);
    let button = label(&output, "Show Problem").unwrap().center();
    pointer(&ctx, &mut workbench, size, button, true);
    pointer(&ctx, &mut workbench, size, button, false);
    let output = render(&ctx, &mut workbench, size);
    assert!(Card::new(&before.id, 1178, 0, 3).expanded(&ctx));
    assert!(label(&output, "Choose Object or Effect…").is_some());
    assert_eq!(workbench.documents[0].recipe, before);
    assert!(workbench.reveal_problem.is_none());
}

fn projectile_fixture() -> PrivatePerkRuntimeGraph {
    let mut loaded = fixture();
    loaded.action_tag = 0;
    loaded.action_payload.clear();
    let mut resource = super::canvas::movement_resource();
    for (root, offset, value) in [
        (&mut resource.instance, 0x168, 300.0_f32),
        (resource.definition.as_mut().unwrap(), 0x74, 300.0_f32),
    ] {
        let mut field = root.fields[0].clone();
        field.locator.value_offset = offset;
        field.owner_offset = offset;
        field.value =
            WeaponRuntimeValue::Bytes([value.to_le_bytes(), 0x7FC12345u32.to_le_bytes()].concat());
        root.fields.push(field);
    }
    let root = resource.definition.as_mut().unwrap();
    let mut gravity = root.fields[0].clone();
    gravity.locator.value_offset = 0xD8;
    gravity.owner_offset = 0xD8;
    gravity.value = WeaponRuntimeValue::Bytes(vec![0; 8]);
    root.fields.push(gravity);
    loaded.graphs[0].1.resources = vec![resource];
    loaded.graphs[0].1.scope_fields();
    loaded
}

#[test]
fn projectile_properties_edit_inline_for_typed_and_native_effects_and_undo_together() {
    for native in [false, true] {
        for size in [egui::vec2(640.0, 780.0), egui::vec2(1320.0, 900.0)] {
            let (ctx, mut workbench) = setup();
            let loaded = Arc::new(projectile_fixture());
            let tag = loaded.graphs[0].0;
            workbench.properties.remember(tag, loaded.clone());
            workbench.documents[0].recipe.effects.truncate(1);
            let effect = workbench.documents[0].recipe.effects[0]
                .program
                .as_mut()
                .unwrap();
            effect.trigger = Trigger::Drawn;
            effect.name = "Projectile Controls".into();
            effect.actions = vec![Action::Pattern {
                asset: Asset {
                    graph: tag,
                    ..Asset::default()
                },
            }];
            if native {
                *effect = Program {
                    name: effect.name.clone(),
                    native: Some(
                        sundial::package_authoring::sandbox_perk::program::native_draft(effect)
                            .unwrap(),
                    ),
                    ..Default::default()
                };
            }
            let output = render(&ctx, &mut workbench, size);
            let before = workbench.documents[0].recipe.clone();
            for name in [
                "Projectile Speed Multiplier",
                "Gravity Multiplier",
                "Travel Distance Limit",
            ] {
                let rect = label(&output, name)
                    .unwrap_or_else(|| panic!("{name} missing for native={native} at {size:?}"));
                assert!(
                    rect.left() >= 0.0 && rect.right() <= size.x,
                    "{name} exceeds window: {rect:?}"
                );
            }
            capture::write(
                &ctx,
                &output,
                &format!(
                    "projectile-inline-{}-{}",
                    if native { "native" } else { "typed" },
                    size.x
                ),
            );
            assert!(workbench.editor.is_none());
            if size.x < 1000.0 {
                continue;
            }
            let control = output
                .shapes
                .iter()
                .find_map(|shape| {
                    let egui::Shape::Text(text) = &shape.shape else {
                        return None;
                    };
                    text.galley
                        .job
                        .text
                        .ends_with('×')
                        .then(|| text.galley.rect.translate(text.pos.to_vec2()).center())
                })
                .unwrap();
            pointer(&ctx, &mut workbench, size, control, true);
            frame(
                &ctx,
                &mut workbench,
                true,
                size,
                vec![egui::Event::PointerMoved(control + egui::vec2(30.0, 0.0))],
            );
            pointer(
                &ctx,
                &mut workbench,
                size,
                control + egui::vec2(30.0, 0.0),
                false,
            );
            let edited = workbench.documents[0].recipe.clone();
            let asset = edited.effects[0]
                .program
                .as_ref()
                .unwrap()
                .asset(0)
                .unwrap();
            let parameters = projectile::parameters::discover(&loaded.graphs[0].1);
            assert!(parameters[0].value(&asset.values).unwrap() > 1.0);
            assert_eq!(parameters[1].value(&asset.values).unwrap(), 0.0);
            assert_eq!(parameters[2].value(&asset.values).unwrap(), 300.0);
            workbench.restore_history(false);
            assert_eq!(workbench.documents[0].recipe, before);
            workbench.restore_history(true);
            assert_eq!(workbench.documents[0].recipe, edited);
        }
    }
}

#[test]
fn collapse_hides_only_the_body_and_keeps_recipe_data_and_document_state_separate() {
    for size in [egui::vec2(640.0, 480.0), egui::vec2(1320.0, 900.0)] {
        let (ctx, mut workbench) = setup();
        let before = workbench.documents[0].recipe.clone();
        let modified = workbench.documents[0].modified;
        collapsed(&ctx, &workbench);
        let output = render(&ctx, &mut workbench, size);
        assert_eq!(rects(&output, egui_phosphor::regular::CARET_RIGHT).len(), 3);
        assert!(label(&output, "Trigger").is_none());
        assert!(label(&output, "Edit Behavior…").is_some());
        capture::write(&ctx, &output, &format!("effects-collapsed-{}", size.x));

        let toggle = rects(&output, egui_phosphor::regular::CARET_RIGHT)[0].center();
        pointer(&ctx, &mut workbench, size, toggle, true);
        pointer(&ctx, &mut workbench, size, toggle, false);
        let output = render(&ctx, &mut workbench, size);
        assert!(label(&output, "Trigger").is_some());
        // Expanded content may scroll the other headers out of a short viewport.
        for (position, index) in [(1, 1179), (2, 421)] {
            assert!(!Card::new(&before.id, index, position, 3).expanded(&ctx));
        }
        assert_eq!(before, workbench.documents[0].recipe);
        assert_eq!(workbench.documents[0].modified, modified);
        assert!(Card::new("another-document", 1179, 1, 3).expanded(&ctx));
        capture::write(&ctx, &output, &format!("effects-expanded-{}", size.x));
    }
}

#[test]
fn dragging_moves_whole_effects_in_both_directions_and_escape_cancels() {
    let size = egui::vec2(1320.0, 900.0);
    let (ctx, mut workbench) = setup();
    let original = workbench.documents[0].recipe.effects.clone();
    collapsed(&ctx, &workbench);
    for (from, to, after, expected) in [
        (
            0,
            2,
            true,
            vec![
                original[1].clone(),
                original[2].clone(),
                original[0].clone(),
            ],
        ),
        (2, 0, false, original.clone()),
    ] {
        let output = render(&ctx, &mut workbench, size);
        let grips = rects(&output, egui_phosphor::regular::DOTS_SIX_VERTICAL);
        let start = grips[from].center();
        let end = egui::pos2(
            grips[to].center().x + 70.0,
            grips[to].center().y + if after { 6.0 } else { -6.0 },
        );
        pointer(&ctx, &mut workbench, size, start, true);
        for _ in 0..3 {
            frame(
                &ctx,
                &mut workbench,
                true,
                size,
                vec![egui::Event::PointerMoved(end)],
            );
        }
        pointer(&ctx, &mut workbench, size, end, false);
        render(&ctx, &mut workbench, size);
        assert_eq!(workbench.documents[0].recipe.effects, expected);
        for (position, effect) in expected.iter().enumerate() {
            assert!(
                !Card::new(
                    &workbench.documents[0].recipe.id,
                    effect.source_perk_index,
                    position,
                    3
                )
                .expanded(&ctx)
            );
        }
    }
    let output = render(&ctx, &mut workbench, size);
    let grips = rects(&output, egui_phosphor::regular::DOTS_SIX_VERTICAL);
    pointer(&ctx, &mut workbench, size, grips[0].center(), true);
    frame(
        &ctx,
        &mut workbench,
        true,
        size,
        vec![egui::Event::PointerMoved(grips[2].center())],
    );
    frame(
        &ctx,
        &mut workbench,
        true,
        size,
        vec![egui::Event::Key {
            key: egui::Key::Escape,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }],
    );
    pointer(&ctx, &mut workbench, size, grips[2].center(), false);
    assert_eq!(workbench.documents[0].recipe.effects, original);
}

#[test]
fn move_menu_keeps_custom_effect_data() {
    let size = egui::vec2(1320.0, 900.0);
    let (ctx, mut workbench) = setup();
    collapsed(&ctx, &workbench);
    let original = workbench.documents[0].recipe.effects.clone();
    let output = render(&ctx, &mut workbench, size);
    let title = label(&output, "Spawn on Kill").unwrap();
    let menu = rects(&output, "…")
        .into_iter()
        .find(|rect| (rect.center().y - title.center().y).abs() < 5.0)
        .unwrap()
        .center();
    pointer(&ctx, &mut workbench, size, menu, true);
    pointer(&ctx, &mut workbench, size, menu, false);
    let output = render(&ctx, &mut workbench, size);
    let down = label(&output, "Move Down").unwrap().center();
    pointer(&ctx, &mut workbench, size, down, true);
    pointer(&ctx, &mut workbench, size, down, false);
    assert_eq!(
        workbench.documents[0].recipe.effects,
        vec![
            original[1].clone(),
            original[0].clone(),
            original[2].clone()
        ]
    );
}

#[test]
fn dragging_scrolls_a_long_effect_list_and_reaches_later_cards() {
    let size = egui::vec2(640.0, 480.0);
    let (ctx, mut workbench) = setup();
    for index in 1180..1200 {
        workbench.documents[0]
            .recipe
            .effects
            .push(program::new_effect(index));
    }
    collapsed(&ctx, &workbench);
    let output = render(&ctx, &mut workbench, size);
    let start = rects(&output, egui_phosphor::regular::DOTS_SIX_VERTICAL)[0].center();
    pointer(&ctx, &mut workbench, size, start, true);
    let edge = egui::pos2(start.x + 80.0, 370.0);
    for _ in 0..80 {
        frame(
            &ctx,
            &mut workbench,
            true,
            size,
            vec![egui::Event::PointerMoved(edge)],
        );
    }
    let output = frame(&ctx, &mut workbench, true, size, vec![]);
    assert!(
        label(&output, "Spawn on Kill").is_none(),
        "drag did not scroll the first card out of view"
    );
    // Use a visible header in the scrolled viewport, excluding the floating grip.
    let target = rects(&output, egui_phosphor::regular::CARET_RIGHT)
        .into_iter()
        .find(|rect| rect.center().y > 200.0 && rect.center().y < 340.0)
        .expect("later effect in the viewport")
        .center()
        + egui::vec2(70.0, 6.0);
    for _ in 0..3 {
        frame(
            &ctx,
            &mut workbench,
            true,
            size,
            vec![egui::Event::PointerMoved(target)],
        );
    }
    pointer(&ctx, &mut workbench, size, target, false);
    let position = workbench.documents[0]
        .recipe
        .effects
        .iter()
        .position(|effect| effect.source_perk_index == 1178)
        .unwrap();
    assert!(
        position > 3,
        "effect was not inserted among later cards: {position}"
    );
}

#[test]
fn native_validation_reveals_the_required_asset_in_execution_order() {
    let size = egui::vec2(1320.0, 900.0);
    let (ctx, mut workbench) = setup();
    let draft = Program {
        trigger: Trigger::Always,
        actions: vec![
            Action::add_rounds(1),
            Action::Spawn {
                asset: Asset::default(),
                position: Position::Owner,
            },
        ],
        ..Program::default()
    };
    workbench.documents[0].recipe.effects.truncate(1);
    workbench.documents[0].recipe.effects[0].program = Some(Program {
        native: Some(
            sundial::package_authoring::sandbox_perk::program::native_draft(&draft).unwrap(),
        ),
        ..Program::default()
    });
    let before = workbench.documents[0].recipe.clone();
    let issue = workbench
        .validation_issue(&before)
        .unwrap()
        .location
        .unwrap()
        .native
        .unwrap();
    assert_eq!((issue.group, issue.action, issue.field), (0, 1, "Object"));
    collapsed(&ctx, &workbench);
    let output = render(&ctx, &mut workbench, size);
    let button = label(&output, "Show Problem").unwrap().center();
    pointer(&ctx, &mut workbench, size, button, true);
    pointer(&ctx, &mut workbench, size, button, false);
    let output = render(&ctx, &mut workbench, size);
    assert!(label(&output, "Object: choose an object or effect.").is_some());
    assert_eq!(workbench.documents[0].recipe, before);
    capture::write(&ctx, &output, "native-required-asset");
}

#[test]
fn authored_nested_conditions_use_the_normal_editor_without_converting_on_read() {
    let size = egui::vec2(1320.0, 900.0);
    let (ctx, mut workbench) = setup();
    workbench.documents[0].recipe.effects.truncate(1);
    let effect = workbench.documents[0].recipe.effects[0]
        .program
        .as_mut()
        .unwrap();
    effect.actions.clear();
    effect.trigger = Trigger::Native;
    effect.native_trigger =
        Some(sundial::package_authoring::sandbox_perk::program::NativeNode::condition(31).unwrap());
    let initial = sundial::package_authoring::sandbox_perk::program::native_draft(effect).unwrap();
    let original =
        sundial::package_authoring::sandbox_perk::action::decode(&initial.graph.emit().unwrap())
            .unwrap();
    let count = original.groups[0].activation[0].subgroups.len();
    let before = workbench.documents[0].recipe.clone();
    let output = render(&ctx, &mut workbench, size);
    assert_eq!(workbench.documents[0].recipe, before);
    let button = label(&output, "Add Requirement").unwrap().center();
    pointer(&ctx, &mut workbench, size, button, true);
    pointer(&ctx, &mut workbench, size, button, false);
    let output = render(&ctx, &mut workbench, size);
    let program = workbench.documents[0].recipe.effects[0]
        .program
        .as_ref()
        .unwrap();
    let native = program.native.as_ref().unwrap();
    let decoded =
        sundial::package_authoring::sandbox_perk::action::decode(&native.graph.emit().unwrap())
            .unwrap();
    assert_eq!(decoded.groups[0].activation[0].subgroups.len(), count + 1);
    assert!(label(&output, "Requirement 1").is_some());
    capture::write(&ctx, &output, "nested-condition-editing");
    workbench.restore_history(false);
    assert_eq!(workbench.documents[0].recipe, before);
}

#[test]
fn custom_and_recovered_effects_add_or_endings_without_replacing_the_primary() {
    for recovered in [false, true] {
        for trigger in [Trigger::Always, Trigger::Drawn, Trigger::WeaponKill] {
            check_or_ending(recovered, trigger);
        }
    }
}

fn click_name(ctx: &egui::Context, workbench: &mut Workbench, size: egui::Vec2, name: &str) {
    let output = render(ctx, workbench, size);
    let pos = label(&output, name)
        .unwrap_or_else(|| {
            capture::write(ctx, &output, "missing-condition-command");
            panic!("Missing {name}")
        })
        .center();
    pointer(ctx, workbench, size, pos, true);
    pointer(ctx, workbench, size, pos, false);
}

fn check_or_ending(recovered: bool, trigger: Trigger) {
    use sundial::package_authoring::sandbox_perk::{action, program::native_draft};
    let size = egui::vec2(1320.0, 900.0);
    let (ctx, mut workbench) = setup();
    workbench.documents[0].recipe.effects.truncate(1);
    let mut program = Program {
        trigger,
        actions: vec![Action::add_rounds(1)],
        ..Program::default()
    };
    if recovered {
        program = Program {
            native: Some(native_draft(&program).unwrap()),
            ..Program::default()
        };
    }
    workbench.documents[0].recipe.effects[0].program = Some(program);
    let before = workbench.documents[0].recipe.clone();
    click_name(&ctx, &mut workbench, size, "Add End Condition…");
    click_name(&ctx, &mut workbench, size, "Show All");
    click_name(&ctx, &mut workbench, size, "Search Behaviors");
    frame(
        &ctx,
        &mut workbench,
        true,
        size,
        vec![egui::Event::Text("01 timer".into())],
    );
    let picker = render(&ctx, &mut workbench, size);
    assert_eq!(rects(&picker, "Choose a Condition").len(), 1);
    assert_eq!(rects(&picker, "01 timer").len(), 1);
    click_name(&ctx, &mut workbench, size, "After a Delay");
    click_name(&ctx, &mut workbench, size, "Use Condition");
    let output = render(&ctx, &mut workbench, size);
    let program = workbench.documents[0].recipe.effects[0]
        .program
        .as_ref()
        .unwrap();
    let decoded = action::decode(&native_draft(program).unwrap().graph.emit().unwrap()).unwrap();
    assert_eq!(
        decoded.groups[0]
            .removal
            .iter()
            .map(|node| node.kind)
            .collect::<Vec<_>>(),
        match trigger {
            Trigger::Always => vec![1],
            Trigger::Drawn => vec![17, 1],
            Trigger::WeaponKill => vec![1, 1],
            _ => unreachable!(),
        }
    );
    assert_eq!(label(&output, "Or").is_some(), trigger != Trigger::Always);
    assert_eq!(program.native.is_some(), recovered);
    capture::write(
        &ctx,
        &output,
        &format!(
            "end-alternatives-{}-{trigger:?}",
            if recovered { "recovered" } else { "custom" }
        ),
    );
    let added = workbench.documents[0].recipe.clone();
    workbench.restore_history(false);
    assert_eq!(workbench.documents[0].recipe, before);
    workbench.restore_history(true);
    assert_eq!(workbench.documents[0].recipe, added);
}

fn label_starting_with(output: &egui::FullOutput, prefix: &str) -> Option<egui::Rect> {
    output.shapes.iter().find_map(|shape| match &shape.shape {
        egui::Shape::Text(text) if text.galley.job.text.starts_with(prefix) => {
            Some(text.galley.rect.translate(text.pos.to_vec2()))
        }
        _ => None,
    })
}

/// The effect's counter was a v0.5 support question: its trigger was titled "After Enough
/// Stacks", filed under States, hidden without Show All, and a new contributing condition
/// added 0. This walks the path a user takes and checks each of those.
#[test]
#[allow(clippy::cognitive_complexity)]
fn the_effects_counter_trigger_is_findable_and_its_contributions_count() {
    use sundial::package_authoring::sandbox_perk::{action, program::native_draft};
    let size = egui::vec2(1320.0, 900.0);
    let (ctx, mut workbench) = setup();
    workbench.documents[0].recipe.effects.truncate(1);
    // 1. Searching the trigger picker for "counter" finds it without Show All.
    let output = render(&ctx, &mut workbench, size);
    let trigger = label_starting_with(&output, "On Weapon Kill")
        .expect("trigger button")
        .center();
    pointer(&ctx, &mut workbench, size, trigger, true);
    pointer(&ctx, &mut workbench, size, trigger, false);
    click_name(&ctx, &mut workbench, size, "Search Behaviors");
    frame(
        &ctx,
        &mut workbench,
        true,
        size,
        vec![egui::Event::Text("counter".into())],
    );
    click_name(
        &ctx,
        &mut workbench,
        size,
        "When the Effect's Counter Is Reached (Accumulator)",
    );
    click_name(&ctx, &mut workbench, size, "Use Trigger");
    let program = workbench.documents[0].recipe.effects[0]
        .program
        .as_ref()
        .unwrap();
    assert_eq!(program.trigger, Trigger::Native);
    let node = program.native_trigger.as_ref().expect("counter trigger");
    assert_eq!(node.kind, 26);
    // 2. A fresh counter starts at the stock norm, not clamped to zero.
    let at = |offset: usize| f32::from_le_bytes(node.bytes[offset..offset + 4].try_into().unwrap());
    assert_eq!(
        (at(0x20), at(0x24), at(0x28), at(0x2C)),
        (1.0, -1.0, -9998.0, 100.0)
    );
    // 3. An empty counter says so, and the counter leads with its two decisions.
    let output = render(&ctx, &mut workbench, size);
    assert!(label(&output, "Contributing Conditions").is_some());
    assert!(label_starting_with(&output, "Nothing counts yet").is_some());
    assert!(label(&output, "After It Fires").is_some());
    capture::write(&ctx, &output, "effect-counter-empty");
    let choice = label(&output, "Keep counting")
        .expect("After It Fires choice")
        .center();
    pointer(&ctx, &mut workbench, size, choice, true);
    pointer(&ctx, &mut workbench, size, choice, false);
    click_name(&ctx, &mut workbench, size, "Start over");
    // Editing a native field promotes the program to the complete editor, so read the
    // trigger back through the draft, which covers both shapes.
    let program = workbench.documents[0].recipe.effects[0]
        .program
        .as_ref()
        .unwrap();
    let decoded = action::decode(&native_draft(program).unwrap().graph.emit().unwrap()).unwrap();
    let counter = &decoded.groups[0].activation[0].native;
    let at = |offset: usize| f32::from_le_bytes(counter[offset..offset + 4].try_into().unwrap());
    assert_eq!(
        (at(0x20), at(0x24)),
        (1.0, 1.0),
        "Start over resets at Count Needed"
    );
    // Adding a contributing condition creates a row that adds 1, shown as Counter Change.
    click_name(&ctx, &mut workbench, size, "Add Condition…");
    click_name(&ctx, &mut workbench, size, "Show All");
    click_name(&ctx, &mut workbench, size, "Search Behaviors");
    frame(
        &ctx,
        &mut workbench,
        true,
        size,
        vec![egui::Event::Text("02 kill".into())],
    );
    click_name(&ctx, &mut workbench, size, "On a Kill");
    click_name(&ctx, &mut workbench, size, "Use Condition");
    let program = workbench.documents[0].recipe.effects[0]
        .program
        .as_ref()
        .unwrap();
    let decoded = action::decode(&native_draft(program).unwrap().graph.emit().unwrap()).unwrap();
    let root = &decoded.groups[0].activation[0];
    assert_eq!(root.kind, 26);
    let row = root.children[0].accumulator_row.expect("contribution row");
    assert_eq!(root.children[0].kind, 2);
    assert_eq!((row.success_operation, row.success_value), (0, 1.0));
    // 4. The row's header says what it does; opening it shows plain labels, and the failure
    //    side waits under its own Advanced.
    let output = render(&ctx, &mut workbench, size);
    let header = label(&output, "Counter Change (Accumulator) · adds 1").expect("row header");
    pointer(&ctx, &mut workbench, size, header.center(), true);
    pointer(&ctx, &mut workbench, size, header.center(), false);
    let output = render(&ctx, &mut workbench, size);
    for plain in ["When It Passes", "Pass Amount"] {
        assert!(label(&output, plain).is_some(), "{plain}");
    }
    assert!(
        label(&output, "When It Fails").is_none(),
        "failure side leads"
    );
    assert!(
        label_starting_with(&output, "Nothing counts yet").is_none(),
        "the empty hint outlived its contribution"
    );
    // 5. The counter itself leads with the count it needs only.
    assert!(label(&output, "Count Needed").is_some());
    for waiting in ["Resets At", "Lowest Count", "Highest Count"] {
        assert!(label(&output, waiting).is_none(), "{waiting} leads");
    }
    capture::write(&ctx, &output, "effect-counter-contribution");
}

/// A comparison trigger reads as its comparison. The engine rows around it, at their
/// defaults, wait under Advanced instead of leading a card with "Native Value 0".
#[test]
fn a_comparison_trigger_leads_with_its_comparison_only() {
    let size = egui::vec2(1320.0, 900.0);
    let (ctx, mut workbench) = setup();
    workbench.documents[0].recipe.effects.truncate(1);
    let (title, _, node) = program::compiled_comparisons()
        .into_iter()
        .find(|(title, _, _)| title.starts_with("Nearby Enemy"))
        .expect("a compiled comparison");
    let effect = workbench.documents[0].recipe.effects[0]
        .program
        .as_mut()
        .unwrap();
    effect.trigger = Trigger::Native;
    effect.native_trigger = Some(node);
    let output = render(&ctx, &mut workbench, size);
    assert!(label_starting_with(&output, &title).is_some(), "{title}");
    for leading in ["Compared Value", "Comparison"] {
        assert!(label(&output, leading).is_some(), "{leading}");
    }
    for waiting in [
        "Player State",
        "Weapon State",
        "State",
        "At Least",
        "At Most",
        "Chance Source",
    ] {
        assert!(label(&output, waiting).is_none(), "{waiting} leads");
    }
    assert!(label_starting_with(&output, "Native Value").is_none());
    capture::write(&ctx, &output, "comparison-trigger-lead");
}
