//! Item pages read against an installed game: a perk's behavior in plain words, a subclass's
//! damage type, an emblem's second icon container, a weapon's named slot and a dummy's header.
use super::item::ItemPage;
use super::*;
use crate::test_support::{primary_press, text_position};

const SALT: &str = "almanac-facts";
const SCREEN: egui::Vec2 = egui::vec2(1500.0, 1100.0);

fn frame(
    ctx: &egui::Context,
    catalog: &Catalog,
    state: &mut HashInspectionState,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, SCREEN)),
            events,
            ..Default::default()
        },
        |ui| {
            let ctx = ui.ctx().clone();
            draw_catalog_hash_window(&ctx, catalog, None, false, state, SALT);
        },
    );
    crate::test_support::capture::record(&output);
    output
}

/// Every painted text, so a check can say what a page showed when it fails.
fn texts(output: &egui::FullOutput) -> Vec<String> {
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.job.text.clone()),
            _ => None,
        })
        .collect()
}

/// A context set up as the app sets its own, with the installed game's fonts.
fn context(install: &std::path::Path) -> egui::Context {
    let ctx = egui::Context::default();
    crate::app::ui::configure_style(&ctx);
    let _ = crate::app::preferences::configure_destiny_symbol_fonts(&ctx, install);
    ctx
}

/// Opens `hash` on `page` and draws until the page settles.
fn open(
    ctx: &egui::Context,
    catalog: &Catalog,
    hash: u64,
    page: ItemPage,
) -> (HashInspectionState, egui::FullOutput) {
    let mut state = HashInspectionState::default();
    state.open(hash);
    let page_id = egui::Id::new(("definition_inspector", SALT)).with(("item_page", hash));
    ctx.data_mut(|data| data.insert_temp(page_id, page));
    let mut output = frame(ctx, catalog, &mut state, Vec::new());
    for _ in 0..6 {
        output = frame(ctx, catalog, &mut state, Vec::new());
    }
    (state, output)
}

fn first_item(catalog: &Catalog, accept: impl Fn(&ItemDef, &ItemPackageMetadata) -> bool) -> u64 {
    catalog
        .items
        .iter()
        .find(|item| {
            !item.name.trim().is_empty()
                && catalog
                    .item_package_metadata(item.hash)
                    .is_some_and(|metadata| accept(item, metadata))
        })
        .map(|item| item.hash)
        .expect("the installed catalog has such an item")
}

#[test]
#[ignore = "Requires SUNDIAL_INSTALL and installed packages"]
fn item_pages_show_what_the_packages_say() {
    let install = crate::test_support::install();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    std::hash::Hash::hash(&install, &mut hasher);
    let cache = std::env::temp_dir()
        .join("sundial-test-catalogs")
        .join(format!("{:016x}.json", std::hash::Hasher::finish(&hasher)));
    let catalog = Catalog::load_or_scan_with_progress(&install, cache, false, |_| {}).unwrap();
    let ctx = context(&install);
    let mut receipt = serde_json::Map::new();

    // A perk's behavior, read from its action, leads the opened Behavior fold.
    let perk = first_item(&catalog, |_, metadata| !metadata.sandbox_perks.is_empty());
    let (mut state, output) = open(&ctx, &catalog, perk, ItemPage::Runtime);
    let fold = text_position(&output, "Behavior")
        .unwrap_or_else(|| panic!("no Behavior fold: {:?}", texts(&output)));
    frame(&ctx, &catalog, &mut state, primary_press(fold, true));
    frame(&ctx, &catalog, &mut state, primary_press(fold, false));
    let output = frame(&ctx, &catalog, &mut state, Vec::new());
    // The fold reads the perk when asked. Its button comes after the weapon runtime's.
    let load = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text == "Load from Packages" => {
                Some(text.pos + egui::vec2(3.0, 3.0))
            }
            _ => None,
        })
        .max_by(|a, b| a.y.total_cmp(&b.y))
        .unwrap_or_else(|| panic!("no load button: {:?}", texts(&output)));
    frame(&ctx, &catalog, &mut state, primary_press(load, true));
    let mut output = frame(&ctx, &catalog, &mut state, primary_press(load, false));
    let started = std::time::Instant::now();
    while text_position(&output, "Runtime Action").is_none() {
        assert!(
            started.elapsed() < std::time::Duration::from_secs(120),
            "the perk's action never loaded: {:?}",
            texts(&output)
        );
        std::thread::sleep(std::time::Duration::from_millis(50));
        output = frame(&ctx, &catalog, &mut state, Vec::new());
    }
    for _ in 0..4 {
        output = frame(&ctx, &catalog, &mut state, Vec::new());
    }
    let shown = texts(&output);
    assert!(
        ["Trigger", "Actions"]
            .iter()
            .all(|heading| shown.iter().any(|text| text == heading)),
        "the behavior reading names its trigger and actions: {shown:?}"
    );
    if shown.iter().any(|text| text == "Set Host Mode") {
        assert!(
            shown.iter().any(|text| text.starts_with("Mode: ")
                && [" · Kinetic", " · Solar", " · Arc", " · Void"]
                    .iter()
                    .any(|name| text.ends_with(name))),
            "Set Host Mode names the damage type it sets: {shown:?}"
        );
    }
    crate::test_support::capture::write(&ctx, &output, "inspector-perk-behavior");
    receipt.insert("perk".into(), serde_json::json!(format_hash_hex(perk)));

    // A subclass's damage type comes from its strings.
    let subclass = first_item(&catalog, |item, metadata| {
        item.bucket_hash == crate::catalog::SUBCLASS_BUCKET_HASH
            && metadata.damage_type.is_none()
            && metadata.display_damage_type.is_some()
    });
    let (_, output) = open(&ctx, &catalog, subclass, ItemPage::Overview);
    let shown = texts(&output);
    assert!(
        shown.iter().any(|text| text == "Damage Type"),
        "the subclass header names its damage type: {shown:?}"
    );
    crate::test_support::capture::write(&ctx, &output, "inspector-subclass-header");
    receipt.insert(
        "subclass".into(),
        serde_json::json!(format_hash_hex(subclass)),
    );

    // An emblem's nameplate container is the strings' second icon row.
    let emblem = first_item(&catalog, |_, metadata| {
        metadata.secondary_icon_container_tag.is_some()
    });
    let (_, output) = open(&ctx, &catalog, emblem, ItemPage::Technical);
    let shown = texts(&output);
    assert!(
        shown
            .iter()
            .any(|text| text == "Secondary Icon Container Tag"),
        "the technical page lists the second icon container: {shown:?}"
    );
    assert!(
        !shown.iter().any(|text| text == "Damage Type"),
        "an emblem's strings name no damage type: {shown:?}"
    );
    crate::test_support::capture::write(&ctx, &output, "inspector-emblem-technical");
    receipt.insert("emblem".into(), serde_json::json!(format_hash_hex(emblem)));

    // A weapon's equipment slot is named.
    let weapon = first_item(&catalog, |_, metadata| metadata.equipment_slot == Some(8));
    let (_, output) = open(&ctx, &catalog, weapon, ItemPage::Technical);
    let shown = texts(&output);
    assert!(
        shown.iter().any(|text| text == "8 · Energy"),
        "the technical page names the slot: {shown:?}"
    );
    crate::test_support::capture::write(&ctx, &output, "inspector-weapon-technical");
    receipt.insert("weapon".into(), serde_json::json!(format_hash_hex(weapon)));

    // A dummy says so in its kind.
    let dummy = catalog
        .items
        .iter()
        .find(|item| !item.name.trim().is_empty() && catalog.is_dummy(item.hash))
        .map(|item| item.hash)
        .expect("the installed catalog has a dummy");
    let (_, output) = open(&ctx, &catalog, dummy, ItemPage::Overview);
    let shown = texts(&output);
    assert!(
        shown.iter().any(|text| text.starts_with("Dummy ")),
        "the dummy's kind says so: {shown:?}"
    );
    crate::test_support::capture::write(&ctx, &output, "inspector-dummy-header");
    receipt.insert("dummy".into(), serde_json::json!(format_hash_hex(dummy)));

    crate::test_support::artifact(
        "inspector-almanac-facts.json",
        &serde_json::Value::Object(receipt),
    );
}
