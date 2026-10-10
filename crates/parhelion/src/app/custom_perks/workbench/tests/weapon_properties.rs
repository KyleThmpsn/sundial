//! Change Weapon Properties from the Add Action picker to the effect's card, over the stock
//! packages. The ways this could be wrong, each checked on screen:
//!
//! - the row is missing from the picker, or adds its action before its rows are read
//! - choosing it adds a node without the asset's neutral rows, or one that attaches to the
//!   player
//! - the card does not show the rows, or shows them with the stock amounts
//! - Add Row adds nothing to the attachment, or the added row does not show
//! - an older saved attachment has unset optional keys and cannot be applied after reopening
//! - repairing those keys loses an edited modifier or changes a chosen cleanup policy
//! - the row is offered again once the effect attaches the entity, or offers no reason
use super::super::{
    canvas::{self, Backend, Canvas, Editing},
    cards::Card,
};
use super::*;
use crate::app::WeaponPropertyPart as Part;
use crate::test_support::driver::{label, painted_text, tap, texts};
use std::time::Instant;
use sundial::package_authoring::{
    open_shadowkeep_package_manager, resolve_live_named_tag,
    runtime::WeaponRuntimeValue,
    sandbox_perk::{
        action, load_sandbox_perk_runtime_action,
        program::{AttachmentTarget, Program},
    },
};

const TEMPLATE: u32 = super::super::behaviors::WEAPON_PROPERTIES_GRAPH;
/// Tall enough for the whole card, so nothing below the fold goes unpainted.
const SIZE: egui::Vec2 = egui::vec2(1320.0, 6000.0);

/// One frame of the effect's card, with the workbench's property reads polled first.
fn frame(
    ctx: &egui::Context,
    workbench: &mut Workbench,
    program: &mut Program,
    packages: &Path,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    workbench.properties.sync(packages, None);
    let labels = BTreeMap::new();
    ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SIZE)),
            events,
            ..Default::default()
        },
        |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                crate::app::style::perk_workbench_style(ui);
                canvas::draw_effect(
                    ui,
                    Canvas {
                        name: "",
                        backend: Backend::Program {
                            program,
                            stock: None,
                            description: None,
                            labels: &labels,
                            editing: Some(Editing { workbench }),
                        },
                        header: None,
                        footer: None,
                        trigger_command: None,
                    },
                    Card::new("weapon-properties", 421, 0, 1),
                );
            });
        },
    )
}

/// A tap on the label `name`, which must be on screen.
fn tap_label(
    ctx: &egui::Context,
    workbench: &mut Workbench,
    program: &mut Program,
    packages: &Path,
    output: &mut egui::FullOutput,
    name: &str,
) {
    let at = label(output, name)
        .unwrap_or_else(|| panic!("{name} on screen:\n{}", painted_text(output)))
        .center();
    for events in tap(at) {
        *output = frame(ctx, workbench, program, packages, events);
    }
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES and a fresh SUNDIAL_TEST_ARTIFACTS directory"]
#[expect(
    clippy::cognitive_complexity,
    reason = "End-to-end verification keeps the ordered workflow and its independent assertions together"
)]
fn change_weapon_properties_goes_from_the_picker_to_the_card() {
    let artifacts = crate::test_support::artifact_dir("weapon-properties");
    assert!(!artifacts.exists(), "use a fresh artifact directory");
    std::fs::create_dir_all(&artifacts).unwrap();
    let packages = crate::test_support::stock_packages();
    let manager = open_shadowkeep_package_manager(&packages).unwrap();
    let globals = manager
        .read_tag(resolve_live_named_tag(&manager, "investment_globals", None).unwrap())
        .unwrap();
    let stock = load_sandbox_perk_runtime_action(&manager, &globals, 421).unwrap();
    let mut program = Program::from_native(&stock.action_payload, "Weapon Properties").unwrap();
    let mut workbench = Workbench::default();
    let ctx = egui::Context::default();
    ctx.global_style_mut(|style| style.interaction.tooltip_delay = 0.0);
    let mut output = egui::FullOutput::default();
    for _ in 0..3 {
        output = frame(&ctx, &mut workbench, &mut program, &packages, vec![]);
    }
    tap_label(
        &ctx,
        &mut workbench,
        &mut program,
        &packages,
        &mut output,
        "Add Action…",
    );
    for _ in 0..2 {
        output = frame(&ctx, &mut workbench, &mut program, &packages, vec![]);
    }
    tap_label(
        &ctx,
        &mut workbench,
        &mut program,
        &packages,
        &mut output,
        "Change Weapon Properties",
    );
    // The row refuses the action until its rows are read, then adds the attachment with every
    // row neutral.
    let started = Instant::now();
    let added = loop {
        tap_label(
            &ctx,
            &mut workbench,
            &mut program,
            &packages,
            &mut output,
            "Add Action",
        );
        let native = program.native.as_ref().unwrap();
        if let Some(asset) = native.assets.iter().find(|asset| asset.graph == TEMPLATE) {
            break asset.clone();
        }
        assert!(
            started.elapsed() < Duration::from_secs(180),
            "the rows were not read in three minutes:\n{}",
            painted_text(&output)
        );
        std::thread::sleep(Duration::from_millis(50));
        output = frame(&ctx, &mut workbench, &mut program, &packages, vec![]);
    };
    assert!(
        !added.values.is_empty(),
        "the attachment carries its neutral rows"
    );
    let properties = crate::app::weapon_properties(&packages, TEMPLATE).unwrap();
    for (index, (_, row)) in properties.rows.iter().enumerate() {
        assert_eq!(
            properties
                .value(index, Part::Amount, &added.values)
                .unwrap(),
            WeaponRuntimeValue::Float32Bits(row.neutral().to_bits()),
            "row {index} neutral"
        );
    }
    // The node attaches the entity to the item itself.
    let bytes = program.native.as_ref().unwrap().graph.emit().unwrap();
    let decoded = action::decode(&bytes).unwrap();
    let attach = decoded
        .effects()
        .find(|effect| effect.kind == 1 && effect.referenced_tag == Some(TEMPLATE))
        .expect("an attach node for the template");
    assert_eq!(attach.native[2], AttachmentTarget::ThisItem.byte());
    assert!(
        super::super::validation::choice_issue(Some(&program)).is_none(),
        "the new attachment already has a lifetime and needs no removal parameter"
    );
    // The card shows the rows with their neutral amounts, not the stock ones.
    for _ in 0..3 {
        output = frame(&ctx, &mut workbench, &mut program, &packages, vec![]);
    }
    let text = painted_text(&output);
    for name in ["Rounds per Burst", "Magazine Size", "Time Between Shots"] {
        assert!(text.contains(name), "{name} on the card:\n{text}");
    }
    for stock in ["0.38", "0.95", "0.75"] {
        assert!(
            !text.contains(stock),
            "stock amount {stock} on the card:\n{text}"
        );
    }
    // Add Row adds one neutral row to the attachment, shown under the table. The stock
    // attachment above it has rows of its own, so the template's button is the last one.
    let add_row = texts(&output)
        .into_iter()
        .filter(|(text, _)| text == "Add Row")
        .map(|(_, rect)| rect)
        .max_by(|a, b| a.min.y.total_cmp(&b.min.y))
        .unwrap_or_else(|| {
            panic!(
                "Add Row on screen:
{}",
                painted_text(&output)
            )
        })
        .center();
    for events in tap(add_row) {
        output = frame(&ctx, &mut workbench, &mut program, &packages, events);
    }
    let added = program
        .native
        .as_ref()
        .unwrap()
        .assets
        .iter()
        .find(|asset| asset.graph == TEMPLATE)
        .map(|asset| asset.rows.clone())
        .unwrap_or_default();
    assert_eq!(added.len(), 1, "one row added");
    assert_eq!(added[0].amount(), 0.0);
    assert!(
        program
            .native
            .as_ref()
            .unwrap()
            .assets
            .iter()
            .all(|asset| asset.graph == TEMPLATE || asset.rows.is_empty()),
        "no other attachment gained a row"
    );
    for _ in 0..2 {
        output = frame(&ctx, &mut workbench, &mut program, &packages, vec![]);
    }
    let text = painted_text(&output);
    for name in ["Added Rows", "Remove"] {
        assert!(
            text.contains(name),
            "{name} on the card:
{text}"
        );
    }
    // The row is refused once the effect attaches the entity, and says why on hover.
    tap_label(
        &ctx,
        &mut workbench,
        &mut program,
        &packages,
        &mut output,
        "Add Action…",
    );
    for _ in 0..2 {
        output = frame(&ctx, &mut workbench, &mut program, &packages, vec![]);
    }
    tap_label(
        &ctx,
        &mut workbench,
        &mut program,
        &packages,
        &mut output,
        "Change Weapon Properties",
    );
    let text = painted_text(&output);
    assert!(
        text.contains("This effect already changes weapon properties."),
        "the refusal beside Add Action:\n{text}"
    );
    // The refused button adds nothing.
    tap_label(
        &ctx,
        &mut workbench,
        &mut program,
        &packages,
        &mut output,
        "Add Action",
    );
    let attachments = program
        .native
        .as_ref()
        .unwrap()
        .assets
        .iter()
        .filter(|asset| asset.graph == TEMPLATE)
        .count();
    assert_eq!(attachments, 1);

    // Reopen the real saved representation used by earlier workbench drafts. The chosen
    // attachment target and edited modifier rows must survive fixing the optional keys.
    let temporary = tempfile::tempdir().unwrap();
    let library = Library::open(temporary.path().join("library")).unwrap();
    let mut recipe = PerkRecipe::new();
    recipe.name = "Weapon Properties".into();
    let mut effect = PerkRecipe::effect(421);
    effect.program = Some(program.clone());
    recipe.effects.push(effect);
    let entry = library.save(&recipe, None).unwrap();
    let mut legacy = serde_json::to_value(&recipe).unwrap();
    let blocks = legacy["effects"][0]["program"]["native"]["graph"]["blocks"]
        .as_array_mut()
        .unwrap();
    let block = blocks
        .iter_mut()
        .find(|block| {
            block["class"] == 0x8080_3E45u32
                && block["bytes"].as_array().unwrap()[16..20]
                    .iter()
                    .map(|byte| byte.as_u64().unwrap() as u8)
                    .eq(TEMPLATE.to_le_bytes())
        })
        .unwrap();
    let bytes = block["bytes"].as_array_mut().unwrap();
    for at in [0x18, 0x1C, 0x30] {
        for byte in &mut bytes[at..at + 4] {
            *byte = 0.into();
        }
    }
    let legacy_bytes = serde_json::to_vec_pretty(&legacy).unwrap();
    std::fs::write(&entry.path, &legacy_bytes).unwrap();
    let reopened = Library::read(&entry.path).unwrap();
    assert_eq!(
        reopened.recipe.effects[0]
            .program
            .as_ref()
            .unwrap()
            .native
            .as_ref()
            .unwrap()
            .assets,
        program.native.as_ref().unwrap().assets,
        "reopening keeps every modifier edit and added row"
    );
    let mut restored = reopened.recipe.effects[0].program.clone().unwrap();
    assert!(super::super::validation::choice_issue(Some(&restored)).is_none());
    for _ in 0..3 {
        output = frame(&ctx, &mut workbench, &mut restored, &packages, vec![]);
    }
    let text = painted_text(&output);
    assert!(text.contains("Ends with the Effect"));
    assert!(!text.contains("Choose the Lifetime"));
    assert!(!text.contains("Choose the Value Written on Removal"));
    assert!(!text.contains("Choose the Driven Value"));
    let compiled =
        sundial::package_authoring::sandbox_perk::program::compile(&manager, &restored).unwrap();
    let decoded = action::decode(&compiled.payload).unwrap();
    let attach = decoded
        .effects()
        .find(|effect| effect.referenced_tag == Some(TEMPLATE))
        .unwrap();
    // Native +0x18 controls retirement, +0x1C the removal write and +0x30 the creation write.
    for at in [0x18, 0x1C, 0x30] {
        assert_eq!(&attach.native[at..at + 4], &0x811C_9DC5u32.to_le_bytes());
    }
    assert_eq!(attach.native[2], AttachmentTarget::ThisItem.byte());
    let resaved = library
        .save(&reopened.recipe, Some(&reopened.baseline))
        .unwrap();
    assert_eq!(
        Library::read(&resaved.path).unwrap().recipe,
        reopened.recipe
    );
    std::fs::write(artifacts.join("legacy.perk.json"), legacy_bytes).unwrap();
    std::fs::write(artifacts.join("reopened.perk.json"), resaved.baseline).unwrap();
    std::fs::write(artifacts.join("action.bin"), compiled.payload).unwrap();
    capture::write(&ctx, &output, "weapon-properties-reopened");

    // Explicit keys, including unfamiliar names, are authored choices and stay exact.
    let native = restored.native.as_mut().unwrap();
    let attach = native
        .graph
        .blocks
        .iter_mut()
        .find(|block| {
            block.class == 0x8080_3E45
                && block.bytes.get(16..20) == Some(TEMPLATE.to_le_bytes().as_slice())
        })
        .unwrap();
    let selected = [0xD071_8374u32, 0x69B8_AFE2u32];
    for (at, key) in [0x18, 0x1C].into_iter().zip(selected) {
        attach.bytes[at..at + 4].copy_from_slice(&key.to_le_bytes());
    }
    let mut chosen = reopened.recipe;
    chosen.effects[0].program = Some(restored);
    let saved = library
        .save(&chosen, Some(&std::fs::read(&resaved.path).unwrap()))
        .unwrap();
    let reread = Library::read(&saved.path).unwrap();
    let compiled = sundial::package_authoring::sandbox_perk::program::compile(
        &manager,
        reread.recipe.effects[0].program.as_ref().unwrap(),
    )
    .unwrap();
    let decoded = action::decode(&compiled.payload).unwrap();
    let attach = decoded
        .effects()
        .find(|effect| effect.referenced_tag == Some(TEMPLATE))
        .unwrap();
    for (at, key) in [0x18, 0x1C].into_iter().zip(selected) {
        assert_eq!(&attach.native[at..at + 4], &key.to_le_bytes());
    }
    std::fs::write(artifacts.join("chosen-keys.perk.json"), saved.baseline).unwrap();
    std::fs::write(artifacts.join("chosen-keys-action.bin"), compiled.payload).unwrap();
}
