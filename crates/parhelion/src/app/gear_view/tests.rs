//! Gear from the toolbar to staged packages. Each non-weapon kind is created from the New menu,
//! edited on its page, given a lore tab of its own and a custom perk from its socket's menu, built
//! the way Build & Stage builds it and read back from the staged packages through Sundial's
//! catalog. Armor 1.0 keeps two choices in one socket, one made the default from its menu. A shader
//! remixes another shader's dyes and gives two surfaces custom colors and iridescence, and its
//! preview of those unbuilt edits must match the built shader drawn the same way. A subclass
//! takes a grenade from another class and an attunement from another subclass of its own, and
//! authors its middle path: a name of its own, and a passive node taken from another path with
//! its own name, description and one more perk. Its second grenade is authored as an ability of
//! its own: a name, a description, another class's icon, and a custom perk made in the workbench
//! from the grenade's New Custom Perk. The perk it adds is then edited as a custom perk from its
//! chip's menu, which takes the stock perk's place. It takes two extra charges and a value of one
//! of its bank's script parameters, which reach it as pool records naming bank rows of the build's
//! own, and one value of its entity is changed, which the build writes to a copy under an ability
//! row of its own, while the stock grenade keeps its value. The authored node gives the other
//! grenade an extra charge the same way.
//! A second subclass adds a perk that names one of its grenades, as Sunbracers' Helium Spirals
//! names the Solar Grenade, to that grenade and to a node, and changes a value of that grenade.
//! No stock subclass grants such a perk itself. Each added perk must be replaced where it stands
//! by a private copy that names the grenade's copy.
//! Each gear page's model preview shows an Open in Window corner on hover, and the last one opens
//! the model viewer through it.
//! An emblem offers no lore tab. It takes another emblem's banner, searched for in its page's
//! Nameplate section, flips that banner in the artwork editor, keeps its base's overlay and
//! inverts an imported background. Each image's tile names its native canvas size. Its strings
//! and presentation row name a private nameplate container. Edited textures preserve the banner's
//! donor colors, and the background exports with its saved edits.
//!
//! `SUNDIAL_INSTALL` names the installed packages and
//! `SUNDIAL_TEST_ARTIFACTS` a folder on the same drive. The folder keeps the recipes, the staged
//! run and `readback.json`. `SUNDIAL_TEST_ARTIFACTS` adds page captures, and
//! `PARHELION_GEAR_KINDS` (such as `Shader` or `Armor,Ship`) authors only the kinds it names.
//! Nothing installed is changed: the read-back view is hard links to the packages plus the
//! staged files.
use super::*;
mod armor_class;
mod armor_collections;
mod emblem;
mod emblem_trackers;
#[cfg(feature = "d2-model-importer")]
mod imported_preview;
mod shader;
mod shader_socket;
mod subclass;
mod vehicles;
use crate::app::custom_perks::workbench::{Workbench, tests::capture};
use crate::app::emblem_view;
use crate::app::shader_view;
use crate::app::subclass_view::SubclassSelection;
use crate::dye::{
    DyeChannel, DyeEdit, DyeSurface, DyeTextureEdit, DyeValue, GearType, surface_edit,
    texture_edit, write_vectors,
};
use crate::emblem::{NameplateImage, NameplatePart};
use crate::image_import::EmbeddedImage;
use crate::subclass::{
    AbilityModifier, ArtImage, ArtPart, AttunementPath, EffectGrade, EntryIcon, ModifierEffect,
    PaletteEdit, Place, ScreenArt, SpawnSwap, SubclassAbilities, SubclassPathNode, TintEdit,
    layout,
};
use crate::test_support::driver::{accessible, texts};
use emblem::*;
use shader::*;
use std::fs;
use std::sync::Arc;
use std::time::{Duration, Instant};
use subclass::*;
use sundial::investment::SubclassSummary;
use sundial::package_authoring::ability_materials::material_routes;
use sundial::package_authoring::investment_schema::{
    GLOBALS_ITEM_DENSE_PRESENTATION_TABLE_SLOT, investment_globals_table_tag,
};
use sundial::package_authoring::runtime::{
    WeaponRuntimeFieldSource, WeaponRuntimeValue, WeaponRuntimeValueKind,
    WeaponRuntimeValueOverride, load_weapon_runtime_graph_for_entity, resolve_weapon_runtime_field,
};
use sundial::package_authoring::sandbox_perk::load_sandbox_perk_runtime_action;
use sundial::package_authoring::{
    PackageManager, ability_modifier, ability_movement, ability_palette, ability_reference,
    ability_spawns, ability_tint, open_shadowkeep_package_manager, resolve_live_named_tag,
};

#[cfg(feature = "d2-model-importer")]
#[test]
#[expect(
    clippy::cognitive_complexity,
    reason = "End-to-end verification keeps the ordered workflow and its independent assertions together"
)]
fn imported_shader_opens_material_controls_without_a_base_catalog() {
    use parhelion_import::GraphReference;
    let folder = tempfile::tempdir().unwrap();
    let mut recipe = WeaponRecipe::new_unbound_kind(ItemKind::Shader).unwrap();
    recipe.set_donor(0x3001, "Native Shader");
    recipe.name = "Local Source".into();
    let item = recipe.identity.item_hash.parse_u32().unwrap();
    let mut nodes = Vec::new();
    let mut dyes = Vec::new();
    for channel in 0..3 {
        let mut scope = vec![0u8; 0x370];
        let mut vectors = [[0.0f32; 4]; 27];
        vectors[9] = [0.1, 0.2, 0.3, 1.0];
        vectors[13] = [0.3, 0.2, 0.1, 1.0];
        vectors[11][0] = -1.0;
        vectors[15][0] = -1.0;
        crate::tag_payload::append_native_array(
            &mut scope,
            0x88,
            0x80800090,
            27,
            &vectors
                .into_iter()
                .flatten()
                .flat_map(f32::to_le_bytes)
                .collect::<Vec<_>>(),
        )
        .unwrap();
        for (role, bytes) in [("scope", scope), ("parent", vec![0; 24])] {
            let symbol = format!("dye-{channel}-{role}");
            fs::write(folder.path().join(format!("{symbol}.bin")), bytes).unwrap();
            nodes.push(
                serde_json::json!({"symbol":symbol,"file":format!("{symbol}.bin"),"patches":[]}),
            );
        }
        dyes.push(serde_json::json!({"channel":channel,"manifest":channel+1,"parent":format!("dye-{channel}-parent")}));
    }
    fs::write(folder.path().join("source-icon.png"), {
        let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            52,
            52,
            image::Rgba([34, 45, 56, 255]),
        ));
        let mut output = std::io::Cursor::new(Vec::new());
        image
            .write_to(&mut output, image::ImageFormat::Png)
            .unwrap();
        output.into_inner()
    })
    .unwrap();
    fs::write(
        folder.path().join("asset-graph.json"),
        serde_json::to_vec(&serde_json::json!({
            "kind":"shader","item_hash":item,"source_item":0xABCD1234u32,"source_name":"Local Source","source_icon_png":"source-icon.png","nodes":nodes,"dyes":dyes
        }))
        .unwrap(),
    )
    .unwrap();
    recipe.overrides.imported_graph = Some(GraphReference::new(folder.path(), item).unwrap());
    let original_icon = crate::shader::source_icon(&recipe).unwrap();
    // Saving and sharing must carry source materials, so reopening cannot depend on this folder.
    let portable = recipe.to_json_pretty().unwrap();
    let document: serde_json::Value = serde_json::from_str(&portable).unwrap();
    let mut damaged = document.clone();
    damaged["overrides"]["imported_graph"]["embedded_assets"]["files"]["dye-0-scope.bin"] =
        serde_json::json!("AAAA");
    assert!(WeaponRecipe::from_json_str(&damaged.to_string()).is_err());
    let mut escaped = document.clone();
    escaped["overrides"]["imported_graph"]["embedded_assets"]["files"]["../escaped.bin"] =
        serde_json::json!("AAAA");
    assert!(WeaponRecipe::from_json_str(&escaped.to_string()).is_err());
    folder.close().unwrap();
    let reopened = WeaponRecipe::from_json_str(&portable).unwrap();
    assert!(recipe.same_saved_content(&reopened));
    assert_eq!(crate::shader::source_materials(&reopened).unwrap().len(), 3);
    assert_eq!(
        crate::shader::source_icon(&reopened).unwrap(),
        original_icon
    );
    assert!(WeaponRecipe::from_json_str(&damaged.to_string()).is_err());
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&reopened.to_json_pretty().unwrap()).unwrap(),
        document,
    );
    let mut app = PackageAuthoringApp {
        recipe: reopened,
        catalog: None,
        packages: PathBuf::new(),
        ..PackageAuthoringApp::default()
    };
    let ctx = context();
    let started = Instant::now();
    let output = loop {
        let output = frame_at(&ctx, &mut app, 1320.0);
        let labels = texts(&output)
            .into_iter()
            .map(|(s, _)| s)
            .collect::<Vec<_>>()
            .join("\n");
        if labels.contains("#597C95") {
            break output;
        }
        assert!(
            started.elapsed() < Duration::from_secs(10),
            "Source surfaces did not load: {labels}"
        );
        std::thread::sleep(Duration::from_millis(20));
    };
    let labels = texts(&output)
        .into_iter()
        .map(|(s, _)| s)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(labels.contains("Source Shader") && labels.contains("Local Source"));
    assert!(labels.contains("0xABCD1234"));
    assert!(
        labels.contains("Dyes") && labels.contains("Metalness") && labels.contains("Detail Tiling")
    );
    assert!(!labels.contains("Base Shader") && !labels.contains("not available yet"));
    capture::write(&ctx, &output, "imported-shader-editable");

    // A shared source must survive the editor's name-derived identity and Duplicate. Neither
    // operation may rewrite the source graph or bind the copy to the first recipe's identity.
    let original = app.recipe.clone();
    app.edit_weapon_name("Renamed Source".into());
    assert!(app.invalid_weapon_name.is_none());
    assert_ne!(app.recipe.identity.item_hash, original.identity.item_hash);
    let library_root = tempfile::tempdir().unwrap();
    let library = RecipeLibrary::open(library_root.path().join("recipes")).unwrap();
    let saved = library.save_new(&app.recipe).unwrap();
    let saved = WeaponRecipe::load_json(saved).unwrap();
    assert!(saved.same_saved_content(&app.recipe));
    let copy = WeaponRecipe::load_json(library.duplicate(&saved).unwrap()).unwrap();
    assert_ne!(copy.identity.item_hash, saved.identity.item_hash);
    assert_eq!(copy.name, "Renamed Source Copy");
    assert_eq!(
        copy.overrides.imported_graph,
        original.overrides.imported_graph
    );
    assert_eq!(
        crate::shader::source_materials(&copy).unwrap(),
        crate::shader::source_materials(&original).unwrap(),
    );
    assert_eq!(crate::shader::source_icon(&copy).unwrap(), original_icon);
    let bundle = library_root
        .path()
        .join("source-variants.parhelion-bundle.json");
    library.export_bundle(&[saved, copy], &bundle).unwrap();
    let recipient = RecipeLibrary::open(library_root.path().join("recipient/recipes")).unwrap();
    let imported = recipient.import_files(std::slice::from_ref(&bundle));
    assert!(imported.errors.is_empty(), "{:?}", imported.errors);
    assert_eq!(imported.paths.len(), 2);
    let edited = app.recipe.to_json_pretty().unwrap();
    if let Some(output) = crate::test_support::artifacts("captures") {
        fs::copy(bundle, output.join("source-variants.parhelion-bundle.json")).unwrap();
        fs::write(
            output.join("imported-shader-editable.parhelion.json"),
            edited,
        )
        .unwrap();
    }
}
use sundial::ui::model_preview::{Appearance, SurfaceOverride, still};

const WIDTH: f32 = 1320.0;
/// Tall enough to keep an ability's Technical section, under its Gameplay cards, in view.
const HEIGHT: f32 = 1800.0;

/// One authored item and what the build must carry for it.
struct Authored {
    kind: ItemKind,
    recipe: WeaponRecipe,
    base: u32,
    /// The base, or for a reissue the same piece's Collections entry that places it.
    collections_base: u32,
    perk_socket: usize,
    perk_name: String,
    perk_stats: Vec<WeaponStatOverride>,
    /// Whether the custom perk is set to Offer Everywhere, as an Armor 2.0 mod is.
    offered: bool,
    energy: Option<(usize, u32)>,
    /// The plug the perk's socket keeps as a second choice.
    alternative: Option<u32>,
}

fn context() -> egui::Context {
    let ctx = egui::Context::default();
    // Inspect settled UI content. A collapsing header's first animation frames can clip the
    // loading label itself, which would make a check for absent loading text finish too early.
    ctx.all_styles_mut(|style| style.animation_time = 0.0);
    let mut fonts = egui::FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    ctx.set_fonts(fonts);
    // Pictures with no text, such as the model preview, are found by their accessible names.
    ctx.enable_accesskit();
    ctx
}

/// The toolbar's New button, the open recipe's page and the perk window, as the app lays them.
fn frame(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let output = ctx.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(WIDTH, HEIGHT),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            egui::Panel::top("gear-flow-toolbar").show(ui, |ui| {
                let mut replaced = false;
                ui.horizontal(|ui| app.draw_new_item_button(ui, &mut replaced));
            });
            egui::CentralPanel::default().show(ui, |ui| {
                workbench_style(ui);
                // The page the app's dispatch draws: a subclass's Appearance, else the editor.
                egui::ScrollArea::vertical()
                    .id_salt(("parhelion-workbench", app.workbench_page))
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        if app.recipe.kind == ItemKind::Subclass
                            && app.workbench_page == WorkbenchPage::Appearance
                        {
                            app.draw_subclass_appearance(ui);
                        } else if !app.recipe.kind.is_weapon() {
                            app.draw_gear_editor(ui);
                        }
                    });
            });
            app.draw_perk_workbench(ui);
            app.draw_discard_confirmation(ui);
            app.draw_artwork_editor(ui);
        },
    );
    // Captures draw the images earlier frames uploaded, such as icons and the preview.
    capture::record(&output);
    output
}

fn settle(ctx: &egui::Context, app: &mut PackageAuthoringApp) -> egui::FullOutput {
    frame(ctx, app, Vec::new());
    frame(ctx, app, Vec::new())
}

fn find(
    output: &egui::FullOutput,
    label: &str,
    matches: impl Fn(&str, egui::Rect) -> bool,
) -> egui::Pos2 {
    let found = texts(output);
    found
        .iter()
        .find(|(text, rect)| matches(text, *rect))
        .map(|(_, rect)| rect.center())
        .unwrap_or_else(|| {
            panic!(
                "{label} not on screen. Text drawn: {:?}",
                found.iter().map(|(text, _)| text).collect::<Vec<_>>()
            )
        })
}

fn click(ctx: &egui::Context, app: &mut PackageAuthoringApp, position: egui::Pos2) {
    press(ctx, app, position, egui::PointerButton::Primary);
}

/// Opens the menu of whatever sits at `position`, such as a socket choice.
fn right_click(ctx: &egui::Context, app: &mut PackageAuthoringApp, position: egui::Pos2) {
    press(ctx, app, position, egui::PointerButton::Secondary);
}

fn press(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    position: egui::Pos2,
    button: egui::PointerButton,
) {
    for events in crate::test_support::driver::button_frames(position, button) {
        frame(ctx, app, events);
    }
}

/// Where `label` is drawn, for checks on where a page puts it.
fn rect_of(output: &egui::FullOutput, label: &str) -> egui::Rect {
    texts(output)
        .into_iter()
        .find(|(text, _)| text == label)
        .map_or_else(|| panic!("{label} not on screen"), |(_, rect)| rect)
}

/// Fails when the gear page runs past the right edge at a narrow, a medium or the full width. The
/// message names what is drawn past the edge and the rightmost text, which place the widget. It
/// draws in the test's own context, since package icons load into the context that first asks.
fn assert_page_fits(ctx: &egui::Context, app: &mut PackageAuthoringApp, page: &str) {
    fn rects(shape: &egui::Shape, found: &mut Vec<egui::Rect>) {
        match shape {
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| rects(shape, found)),
            shape => found.push(shape.visual_bounding_rect()),
        }
    }
    for width in [480.0, 900.0, WIDTH] {
        let mut edge = width;
        let mut overflow = 0.0_f32;
        let mut output = None;
        for _ in 0..2 {
            output = Some(ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 1400.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    egui::CentralPanel::default().show(ui, |ui| {
                        workbench_style(ui);
                        edge = ui.max_rect().right();
                        app.draw_gear_editor(ui);
                        overflow = (ui.min_rect().right() - edge).max(0.0);
                    });
                },
            ));
            if let Some(output) = &output {
                capture::record(output);
            }
        }
        if overflow <= 1.0 {
            continue;
        }
        let output = output.unwrap();
        let mut drawn = texts(&output);
        let mut past = Vec::new();
        for clipped in &output.shapes {
            rects(&clipped.shape, &mut past);
        }
        let past = past
            .into_iter()
            .filter(|rect| rect.right() > edge + 1.0)
            .map(|rect| {
                let inside = drawn
                    .iter()
                    .filter(|(_, text)| rect.contains_rect(*text))
                    .map(|(text, _)| text.as_str())
                    .collect::<Vec<_>>();
                format!("{rect:?} holding {inside:?}")
            })
            .collect::<Vec<_>>();
        drawn.sort_by(|a, b| b.1.right().total_cmp(&a.1.right()));
        let rightmost = drawn
            .iter()
            .take(6)
            .map(|(text, rect)| format!("{text:?} to {:.1}", rect.right()))
            .collect::<Vec<_>>();
        panic!(
            "{page} page overflows by {overflow} at {width}, edge {edge}. Drawn past it: {past:?}. \
             Rightmost text: {rightmost:?}"
        );
    }
}

/// A capture name for an item: "armor", "exotic-armor", "ghost-shell".
/// The kind of base an item is authored on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Origin {
    /// A base with its own Collections entry.
    Collections,
    Exotic,
    /// An Armor 2.0 reissue with no Collections entry of its own, placed beside the same piece's.
    Reissue,
    /// Armor 1.0 with its own Collections entry, whose sockets hold perks as a weapon's do.
    Legacy,
}

impl Origin {
    const fn prefix(self) -> &'static str {
        match self {
            Self::Collections => "",
            Self::Exotic => "Exotic ",
            Self::Reissue => "Reissued ",
            Self::Legacy => "Legacy ",
        }
    }
}

/// A capture name for an item: "armor", "exotic-armor", "reissued-armor", "ghost-shell".
fn slug(kind: ItemKind, origin: Origin) -> String {
    format!("{}{}", origin.prefix(), kind.label())
        .to_lowercase()
        .replace(' ', "-")
}

/// New makes another of the open kind in one click, and the caret lists every kind. Picks `kind`
/// from it.
fn new_from_menu(ctx: &egui::Context, app: &mut PackageAuthoringApp, kind: ItemKind) {
    let output = settle(ctx, app);
    let open = format!("New {}", app.recipe.kind.label());
    find(&output, &open, |text, _| text == open);
    click(
        ctx,
        app,
        find(&output, "New Item caret", |text, _| {
            text == egui_phosphor::regular::CARET_DOWN
        }),
    );
    let output = settle(ctx, app);
    // The menu draws above the page, so where a page shows the same label, the menu's comes last.
    let entry = |label: &str| {
        texts(&output)
            .into_iter()
            .rev()
            .find(|(text, _)| text == label)
            .map(|(_, rect)| rect.center())
            .unwrap_or_else(|| panic!("{label} is not in the New menu"))
    };
    for listed in ItemKind::ALL {
        entry(listed.label());
    }
    capture::write(ctx, &output, "gear-new-menu");
    let chosen = entry(kind.label());
    click(ctx, app, chosen);
    if app.pending_recipe_action.is_some() {
        let output = settle(ctx, app);
        capture::write(ctx, &output, "gear-new-unsaved-confirmation");
        click(
            ctx,
            app,
            find(&output, "Discard and Continue", |text, _| {
                text == "Discard and Continue"
            }),
        );
        settle(ctx, app);
    }
    assert!(app.pending_recipe_action.is_none());
    assert_eq!(
        app.recipe.kind, kind,
        "the New menu opened a {kind:?} recipe"
    );
    assert_ne!(
        app.recipe.donor.item_hash.parse_u32().unwrap_or_default(),
        0,
        "a new {kind:?} starts on a stock base"
    );
}

/// Moves the recipe to the first base of `origin` that has a socket labelled one of `labels`, and
/// returns that socket. A legacy base is Armor 1.0 whose sockets all name their plugs, and its
/// socket holds two choices.
fn choose_base(
    app: &mut PackageAuthoringApp,
    kind: ItemKind,
    labels: &[&str],
    origin: Origin,
    energy: bool,
) -> (usize, u32) {
    let catalog = app.catalog.as_ref().unwrap();
    let donors = &app.gear_donors[&kind];
    let (hash, name, socket, collections_base) = donors
        .iter()
        .filter(|donor| {
            (donor.rarity == WeaponRarity::Exotic) == (origin == Origin::Exotic)
                && donor.collection_backed == (origin != Origin::Reissue)
        })
        .find_map(|summary| {
            // A reissue has no entry of its own, so the same piece's entry places it.
            let collections_base = if origin == Origin::Reissue {
                donors
                    .iter()
                    .find(|other| {
                        other.collection_backed
                            && other.name == summary.name
                            && other.type_name == summary.type_name
                            && catalog.item_class_type(other.hash)
                                == catalog.item_class_type(summary.hash)
                    })?
                    .hash
            } else {
                summary.hash
            };
            let donor = catalog.gear_donor(summary.hash)?;
            if energy
                && !donor
                    .sockets
                    .iter()
                    .any(|socket| ENERGY_SOCKET_TYPES.contains(&socket.socket_type))
            {
                return None;
            }
            // Plain Armor 1.0: nothing rolls at random, and no stat plug of Armor 2.0's kind.
            if origin == Origin::Legacy
                && (is_armor_2(donor.sockets.iter().map(|socket| socket.socket_type))
                    || donor.sockets.iter().any(|socket| {
                        socket.randomized_plug_set_index.is_some()
                            || socket.label.ends_with("Stat Allocation")
                    }))
            {
                return None;
            }
            let choices = |socket: &WeaponSocket| {
                inherited_socket_choices(
                    socket.native_default,
                    &socket.ordered_embedded_choices,
                    authored_socket_choice_limit(socket.socket_type),
                )
                .len()
            };
            let sets = catalog.supported_plug_sets(summary.hash, &[]).ok()?;
            // A socket that only offers placeholders, such as Random Mod, has no perk to edit.
            let real = |plug: &u32| {
                let label = catalog.plug_label(*plug, false);
                !label.is_empty() && !label.starts_with("Empty") && !label.starts_with("Random")
            };
            let socket = labels.iter().find_map(|label| {
                donor.sockets.iter().find(|socket| {
                    socket.label.split_once(". ").map(|(_, name)| name) == Some(*label)
                        && perk_destination(socket)
                        && socket.native_default.is_some()
                        && (origin != Origin::Legacy || choices(socket) >= 2)
                        && (socket.native_default.as_ref().is_some_and(real)
                            || sets.iter().any(|set| {
                                set.socket_index == socket.index && set.plug_hashes.iter().any(real)
                            }))
                })
            })?;
            Some((
                summary.hash,
                summary.name.clone(),
                socket.index,
                collections_base,
            ))
        })
        .unwrap_or_else(|| panic!("no {origin:?} {kind:?} base with a {labels:?} socket"));
    app.recipe.set_donor(hash, name);
    (socket, collections_base)
}

/// Gives a socket a real plug when it starts on a placeholder, the way a reader picks one from
/// the socket's list before customizing it.
fn concrete_plug(app: &mut PackageAuthoringApp, donor: &WeaponDonor, socket_index: usize) {
    let socket = &donor.sockets[socket_index];
    let sets = app.gear_plug_sets(donor.summary.hash).unwrap();
    let catalog = app.catalog.as_ref().unwrap();
    let placeholder = |plug: u32| {
        let label = catalog.plug_label(plug, false);
        label.starts_with("Empty") || label.starts_with("Random")
    };
    if !socket.native_default.is_some_and(placeholder) {
        return;
    }
    let mut choices = sets
        .iter()
        .find(|set| set.socket_index == socket_index)
        .unwrap()
        .plug_hashes
        .iter()
        .map(|&plug| (catalog.plug_label(plug, false), plug))
        .filter(|(label, plug)| !placeholder(*plug) && !label.is_empty())
        .collect::<Vec<_>>();
    choices.sort();
    let (_, plug) = choices.first().expect("the socket offers a real plug");
    set_socket_plug(&mut app.recipe, donor, socket, *plug);
}

/// The choices a socket holds now: the recipe's, or its base's.
fn socket_choices(app: &PackageAuthoringApp, donor: &WeaponDonor, socket_index: usize) -> Vec<u32> {
    let socket = &donor.sockets[socket_index];
    let inherited = inherited_socket_choices(
        socket.native_default,
        &socket.ordered_embedded_choices,
        authored_socket_choice_limit(socket.socket_type),
    );
    recipe_socket_choices(&app.recipe, socket_index, &inherited).unwrap()
}

/// Where the socket's choice labelled `label` is drawn. A row names its socket at the left, and
/// two sockets can share a name, so the row is ranked among the listed sockets of that name.
fn choice_tile(
    output: &egui::FullOutput,
    donor: &WeaponDonor,
    socket_index: usize,
    label: &str,
) -> egui::Pos2 {
    let name = |socket: &WeaponSocket| {
        socket
            .label
            .split_once(". ")
            .map_or_else(|| socket.label.clone(), |(_, name)| name.to_owned())
    };
    let socket = &donor.sockets[socket_index];
    let rank = donor.sockets[..socket_index]
        .iter()
        .filter(|other| name(other) == name(socket) && perk_destination(other))
        .count();
    let mut rows = texts(output)
        .into_iter()
        .filter(|(text, _)| *text == name(socket))
        .map(|(_, rect)| rect.center())
        .collect::<Vec<_>>();
    rows.sort_by(|a, b| a.y.total_cmp(&b.y));
    let row = *rows
        .get(rank)
        .unwrap_or_else(|| panic!("no row {rank} named {:?}", name(socket)));
    find(output, label, |text, rect| {
        text == label && rect.center().x > row.x && (rect.center().y - row.y).abs() < 12.0
    })
}

/// Right-clicks the socket's first choice, picks Edit as Custom Perk, edits the perk, ticks
/// Offer Everywhere when `offer` asks, and applies it. The socket keeps its other choices.
fn custom_perk(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    kind: ItemKind,
    (item_slug, socket_index): (&str, usize),
    stats: &[(&str, i32)],
    offer: bool,
) -> (String, Vec<WeaponStatOverride>) {
    let donor = app.current_gear_donor().unwrap();
    let choices = socket_choices(app, &donor, socket_index);
    let plug = choices[0];
    let plug_label = app.catalog.as_ref().unwrap().plug_label(plug, false);
    let output = settle(ctx, app);
    right_click(
        ctx,
        app,
        choice_tile(&output, &donor, socket_index, &plug_label),
    );
    let output = settle(ctx, app);
    click(
        ctx,
        app,
        find(&output, "Edit as Custom Perk…", |text, _| {
            text == "Edit as Custom Perk…"
        }),
    );
    settle(ctx, app);
    // Effect checks read the workbench's perk discovery, so wait for it before applying.
    let start = std::time::Instant::now();
    while !app.perk_workbench.discovery_settled() {
        assert!(
            start.elapsed() < std::time::Duration::from_secs(900),
            "perk discovery did not finish"
        );
        frame(ctx, app, Vec::new());
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    app.perk_workbench.remove_flagged_effects();
    let name = format!("Parhelion {} Perk", kind.label());
    let catalog = app.catalog.as_ref().unwrap();
    let definitions = catalog.perk_stat_choices();
    let stats = stats
        .iter()
        .map(|(stat, value)| WeaponStatOverride {
            definition_index: definitions
                .iter()
                .find(|choice| choice.name.trim() == *stat)
                .unwrap_or_else(|| panic!("no {stat} stat"))
                .definition_index,
            value: *value,
        })
        .collect::<Vec<_>>();
    let perk = app
        .perk_workbench
        .open_perk_mut()
        .expect("Edit as Custom Perk opens the socket's perk");
    assert_eq!(perk.template_plug.parse_u32().ok(), Some(plug));
    perk.name.clone_from(&name);
    perk.description = format!("A custom {} perk.", kind.noun());
    for stat in &stats {
        perk.stats
            .retain(|existing| existing.definition_index != stat.definition_index);
        perk.stats.push(stat.clone());
    }
    let stats = perk.stats.clone();
    let mut output = settle(ctx, app);
    if offer {
        click(
            ctx,
            app,
            find(&output, "Offer Everywhere", |text, _| {
                text == "Offer Everywhere"
            }),
        );
        output = settle(ctx, app);
        assert!(
            app.perk_workbench
                .open_perk_mut()
                .is_some_and(|perk| perk.offer_everywhere),
            "Offer Everywhere turns on"
        );
    }
    capture::write(ctx, &output, &format!("gear-{item_slug}-workbench"));
    let apply = format!("Apply to {}", kind.label());
    click(ctx, app, find(&output, &apply, |text, _| text == apply));
    let variant = app
        .recipe
        .overrides
        .socket_plug_variants
        .iter()
        .find(|variant| usize::from(variant.socket_index) == socket_index)
        .expect("Apply adds the custom perk to the socket");
    assert_eq!(variant.choice_index, 0);
    assert_eq!(variant.name.as_deref(), Some(name.as_str()));
    assert_eq!(variant.source_plug_hash.parse_u32().ok(), Some(plug));
    assert_eq!(variant.offer_everywhere, offer);
    let column = app.recipe.overrides.socket_columns[socket_index]
        .as_ref()
        .expect("a custom perk pins its socket");
    assert_eq!(
        column.choices,
        choices
            .iter()
            .copied()
            .map(HexHash::new)
            .collect::<Vec<_>>(),
        "the socket keeps its choices"
    );
    assert_eq!(column.randomized_plug_set_index, None);
    // The reader closes the workbench, which otherwise sits over the next item's sockets.
    app.perk_workbench.open = false;
    (name, stats)
}

/// Runs frames until the base item's model has loaded, so a capture shows the page as a reader
/// sees it.
fn settle_page(ctx: &egui::Context, app: &mut PackageAuthoringApp) -> egui::FullOutput {
    let start = Instant::now();
    loop {
        let output = settle(ctx, app);
        let model_loading = texts(&output)
            .iter()
            .any(|(text, _)| text == "Loading Model");
        if !model_loading {
            return output;
        }
        assert!(
            start.elapsed() < Duration::from_secs(180),
            "the base item's model did not load"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Opens or closes the page's Text Presentation section, which folds over a few frames.
fn toggle_text_presentation(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    output: &egui::FullOutput,
) -> egui::FullOutput {
    click(
        ctx,
        app,
        find(output, "Text Presentation", |text, _| {
            text == "Text Presentation"
        }),
    );
    for _ in 0..12 {
        frame(ctx, app, Vec::new());
    }
    settle(ctx, app)
}

/// Frames until the base item's lore has loaded in the open Text Presentation section.
fn settle_lore(ctx: &egui::Context, app: &mut PackageAuthoringApp) -> egui::FullOutput {
    let start = Instant::now();
    loop {
        let output = settle(ctx, app);
        if app.presentation_editor.lore_loaded() {
            return output;
        }
        assert!(
            start.elapsed() < Duration::from_secs(180),
            "the base item's lore did not load"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The page's own layout. Above, the icon under the base in one column, the item's text in the
/// next and the preview in the last. Below, the stats under the base with the sockets beside them,
/// or the sockets from the page's edge where there are no stats. Armor names its generation on its
/// base.
#[expect(
    clippy::cognitive_complexity,
    reason = "End-to-end verification keeps the ordered workflow and its independent assertions together"
)]
fn check_gear_page(output: &egui::FullOutput, kind: ItemKind, origin: Origin, donor: &WeaponDonor) {
    let drawn = texts(output)
        .into_iter()
        .map(|(text, _)| text)
        .collect::<Vec<_>>();
    let armor_2 = is_armor_2(donor.sockets.iter().map(|socket| socket.socket_type));
    let expected: &[&str] = match kind {
        ItemKind::Armor if armor_2 && origin != Origin::Exotic => &[
            "Energy Type",
            "Energy Capacity",
            "Armor Stats",
            "Mobility",
            "Total",
        ],
        ItemKind::Armor => &["Armor Stats", "Mobility", "Total"],
        ItemKind::Sparrow => &["Sparrow Stats", "Speed", "Boost", "Durability"],
        _ => &[],
    };
    for label in expected.iter().chain(&[
        "Perks & Sockets",
        "Rarity",
        "Inventory Icon",
        "Text Presentation",
    ]) {
        assert!(
            drawn.iter().any(|text| text == label),
            "{kind:?} page lacks {label}"
        );
    }
    // The lore tab sits in the folded Text Presentation section.
    assert!(
        !drawn.iter().any(|text| text.contains("Lore Tab")),
        "the {kind:?} page shows its lore tab outside Text Presentation"
    );
    let base = rect_of(output, &format!("Base {}", kind.label()));
    let icon = rect_of(output, "Inventory Icon");
    let text = rect_of(output, "Text Presentation");
    let preview = accessible(output, "Model Preview")
        .unwrap_or_else(|| panic!("the {kind:?} page has no model preview"));
    let sockets = rect_of(output, "Perks & Sockets");
    assert!(
        icon.top() > base.bottom()
            && (icon.left() - base.left()).abs() < 1.0
            && icon.bottom() < sockets.top(),
        "the {kind:?} page puts the icon under its base: base {base:?}, icon {icon:?}"
    );
    assert!(
        text.left() > icon.right()
            && preview.left() > text.right()
            && preview.top() < icon.top()
            && text.bottom().max(preview.bottom()) < sockets.top(),
        "the {kind:?} page puts its text beside the base and its preview beside that: base \
         {base:?}, text {text:?}, preview {preview:?}"
    );
    if expected.is_empty() {
        assert!(
            (sockets.left() - base.left()).abs() < 1.0,
            "the {kind:?} page has no stats, so its sockets start at the page's edge: sockets \
             {sockets:?}"
        );
    } else {
        let stats = rect_of(output, &format!("{} Stats", kind.label()));
        assert!(
            (stats.left() - base.left()).abs() < 1.0
                && sockets.left() > icon.right()
                && (sockets.top() - stats.top()).abs() < 2.0,
            "the {kind:?} page keeps its sockets beside its stats: stats {stats:?}, sockets \
             {sockets:?}"
        );
    }
    // The base's card names its slot, class and generation, as its row in the list does.
    let named = ["Armor 1.0", "Armor 2.0"]
        .into_iter()
        .filter(|generation| {
            drawn
                .iter()
                .any(|text| text.ends_with(&format!(" · {generation}")))
        })
        .collect::<Vec<_>>();
    if kind == ItemKind::Armor {
        assert_eq!(
            named,
            [armor_generation(armor_2)],
            "armor names its generation"
        );
    } else {
        assert!(named.is_empty(), "only armor names a generation");
    }
    match origin {
        Origin::Legacy => assert!(!armor_2, "a legacy base is Armor 1.0"),
        Origin::Collections | Origin::Reissue if kind == ItemKind::Armor => {
            assert!(armor_2, "{origin:?} armor is Armor 2.0");
        }
        _ => {}
    }
}

fn author(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    kind: ItemKind,
    origin: Origin,
) -> Authored {
    new_from_menu(ctx, app, kind);
    let exotic = origin == Origin::Exotic;
    let legacy = origin == Origin::Legacy;
    // Armor from Collections or a reissue is Armor 2.0, with energy.
    let energized = kind == ItemKind::Armor && !exotic && !legacy;
    let (labels, stat, perk_stats): (&[&str], Option<&str>, &[(&str, i32)]) = match kind {
        ItemKind::Armor if exotic => (&["Intrinsic"], None, &[("Recovery", 10)]),
        ItemKind::Armor if legacy => (&["Trait"], Some("Mobility"), &[("Recovery", 10)]),
        ItemKind::Armor => (
            &["General Armor Mod", "Armor Perk"],
            Some("Mobility"),
            &[("Recovery", 10)],
        ),
        ItemKind::Sparrow => (&["Sparrow Perk"], Some("Boost"), &[("Speed", 20)]),
        ItemKind::Ship => (&["Transmat Effect"], None, &[]),
        ItemKind::GhostShell => (&["Ghost Perk"], None, &[]),
        ItemKind::Weapon
        | ItemKind::Shader
        | ItemKind::Subclass
        | ItemKind::Emblem
        | ItemKind::Mod => {
            unreachable!()
        }
    };
    let (perk_socket, collections_base) = choose_base(app, kind, labels, origin, energized);
    let name = format!("Parhelion Test {}{}", origin.prefix(), kind.label());
    app.recipe.rename_authored_item(name).unwrap();
    let base = app.recipe.donor.item_hash.parse_u32().unwrap();
    let donor = app.current_gear_donor().unwrap();

    assert_page_fits(ctx, app, kind.label());
    let output = settle_page(ctx, app);
    check_gear_page(&output, kind, origin, &donor);
    capture::write(ctx, &output, &format!("gear-{}", slug(kind, origin)));
    // A card's buttons take their own clicks under the card's right-click region: Change Base and
    // Change Icon each open their item picker, and a click on the page's empty corner closes it.
    for button in ["Change Base", "Change Icon"] {
        click(ctx, app, find(&output, button, |text, _| text == button));
        let opened = settle(ctx, app);
        assert!(
            accessible(&opened, "Search items").is_some(),
            "{button} opens the {kind:?} item picker"
        );
        click(ctx, app, egui::pos2(WIDTH - 4.0, HEIGHT - 4.0));
        let closed = settle(ctx, app);
        assert!(
            accessible(&closed, "Search items").is_none(),
            "a click outside closes the {kind:?} item picker"
        );
    }
    // Hovering the preview shows the corner that opens it in the model viewer. The last kind opens
    // it, so the other pages' captures show the corner as a reader first sees it.
    let preview = accessible(&output, "Model Preview").unwrap();
    frame(ctx, app, vec![egui::Event::PointerMoved(preview.center())]);
    let hovered = settle(ctx, app);
    let corner = accessible(&hovered, "Open in Window")
        .unwrap_or_else(|| panic!("hovering the {kind:?} preview shows no Open in Window"));
    capture::write(ctx, &hovered, &format!("gear-{}-hover", slug(kind, origin)));
    if kind == ItemKind::GhostShell {
        click(ctx, app, corner.center());
        assert!(
            sundial::ui::model_preview::popped_out(ctx, egui::Id::new("gear-preview")),
            "Open in Window opens the {kind:?} preview in the model viewer"
        );
    }
    frame(ctx, app, vec![egui::Event::PointerGone]);
    let output = settle(ctx, app);
    if energized {
        assert_eq!(
            app.catalog.as_ref().unwrap().item_stat_maximum(base),
            Some(42),
            "Armor 2.0 stats top out at 42"
        );
    }

    // A lore tab of its own, started from the lore section in Text Presentation, which then folds
    // away again.
    toggle_text_presentation(ctx, app, &output);
    let opened = settle_lore(ctx, app);
    capture::write(ctx, &opened, &format!("gear-{}-text", slug(kind, origin)));
    click(
        ctx,
        app,
        find(&opened, "Custom Lore Tab", |text, _| {
            text == "Custom Lore Tab"
        }),
    );
    assert!(
        app.recipe.overrides.lore.is_some(),
        "the {kind:?} page starts a lore tab"
    );
    app.recipe.overrides.lore = Some(format!(
        "A story written for the {} in Parhelion.",
        app.recipe.name
    ));
    let output = settle(ctx, app);
    toggle_text_presentation(ctx, app, &output);

    // Energy Capacity 10 and one stat, through the page's own edits.
    let energy = energized.then(|| {
        let sets = app.gear_plug_sets(base).unwrap();
        let catalog = app.catalog.as_ref().unwrap();
        let energy = energy_socket(catalog, &app.recipe, &donor, &sets).expect("energy socket");
        let (element, _) = energy.current.expect("a current energy plug");
        let plug = energy.plugs[&(element, 10)];
        set_socket_plug(&mut app.recipe, &donor, &donor.sockets[energy.index], plug);
        assert_eq!(
            socket_choices(app, &donor, energy.index),
            [plug],
            "the energy socket starts on capacity 10"
        );
        (energy.index, plug)
    });
    if let Some(stat) = stat {
        let catalog = app.catalog.as_ref().unwrap();
        let names: &[&'static str] = if kind == ItemKind::Sparrow {
            &SPARROW_STATS
        } else {
            &ARMOR_STATS
        };
        let row = stat_rows(catalog, &app.recipe, &donor, names)
            .into_iter()
            .find(|row| row.name == stat)
            .unwrap();
        set_item_stat(&mut app.recipe, row.definition_index, Some(row.item + 10));
    }

    // Armor 1.0's socket makes its second choice the default from the choice's menu, as a
    // weapon's does, and keeps the first beside it.
    let alternative = legacy.then(|| {
        let choices = socket_choices(app, &donor, perk_socket);
        let second = app.catalog.as_ref().unwrap().plug_label(choices[1], false);
        let output = settle(ctx, app);
        right_click(ctx, app, choice_tile(&output, &donor, perk_socket, &second));
        let output = settle(ctx, app);
        click(
            ctx,
            app,
            find(&output, "Make Default", |text, _| text == "Make Default"),
        );
        assert_eq!(
            socket_choices(app, &donor, perk_socket),
            [choices[1], choices[0]],
            "Make Default leads the socket with its second choice"
        );
        choices[0]
    });
    concrete_plug(app, &donor, perk_socket);
    // An Armor 2.0 general mod is offered in every armor piece's general mod socket.
    let offered = energized;
    let (perk_name, perk_stats) = custom_perk(
        ctx,
        app,
        kind,
        (&slug(kind, origin), perk_socket),
        perk_stats,
        offered,
    );
    let output = settle(ctx, app);
    find(&output, &perk_name, |text, _| text == perk_name);
    capture::write(
        ctx,
        &output,
        &format!("gear-{}-custom-perk", slug(kind, origin)),
    );
    // The reader saves each item before starting the next, so New starts without a prompt.
    app.recipe_baseline = app.recipe.clone();
    app.recipe_dirty = false;
    Authored {
        kind,
        recipe: app.recipe.clone(),
        base,
        collections_base,
        perk_socket,
        perk_name,
        perk_stats,
        offered,
        energy,
        alternative,
    }
}

/// The installed packages with Parhelion's own files swapped for the staged ones.
fn staged_view(packages: &Path, build: &BuildReport, view: &Path) {
    let artifacts = build
        .artifacts
        .iter()
        .map(|artifact| {
            (
                build.run_directory.join(&artifact.file_name),
                artifact.file_name.as_str(),
            )
        })
        .collect::<Vec<_>>();
    package_view(packages, &artifacts, view);
}

/// The installed packages with Parhelion's own files replaced by `artifacts`, in a folder a
/// catalog scan accepts. With none it is the stock game every build starts from.
fn package_view(packages: &Path, artifacts: &[(PathBuf, &str)], view: &Path) {
    if view.exists() {
        fs::remove_dir_all(view).unwrap();
    }
    let view_packages = view.join("packages");
    fs::create_dir_all(&view_packages).unwrap();
    for entry in fs::read_dir(packages).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name();
        if entry
            .path()
            .extension()
            .is_some_and(|extension| extension == "pkg")
            && !crate::package_profile::CANONICAL_ARTIFACT_FILE_NAMES
                .iter()
                .any(|canonical| name == *canonical)
            && !artifacts.iter().any(|(_, file_name)| name == **file_name)
        {
            fs::hard_link(entry.path(), view_packages.join(&name)).unwrap();
        }
    }
    for (source, file_name) in artifacts {
        fs::hard_link(source, view_packages.join(file_name)).unwrap();
    }
    let install = packages.parent().unwrap();
    let bin = view.join("bin/x64");
    fs::create_dir_all(&bin).unwrap();
    fs::hard_link(
        install.join("bin/x64/oo2core_3_win64.dll"),
        bin.join("oo2core_3_win64.dll"),
    )
    .unwrap();
    // A catalog scan only accepts a folder that looks like an install.
    fs::hard_link(install.join("destiny2.exe"), view.join("destiny2.exe")).unwrap();
}

fn has_stat(stats: &[WeaponInvestmentStat], stat: &WeaponStatOverride) -> bool {
    stats.iter().any(|native| {
        native.definition_index == stat.definition_index && native.value == stat.value
    })
}

/// The staged item keeps its base's kind, class and rarity and carries the page's edits.
fn check_item(
    staged: &InvestmentCatalog,
    item: &Authored,
    donor: &WeaponDonor,
    base: &WeaponDonor,
) {
    let name = &item.recipe.name;
    assert_eq!(
        ItemKind::from_bucket_hash(donor.summary.bucket_hash),
        Some(item.kind)
    );
    assert_eq!(&donor.summary.name, name);
    assert_eq!(donor.summary.bucket_hash, base.summary.bucket_hash);
    assert_eq!(donor.summary.rarity, base.summary.rarity);
    assert_eq!(
        staged.item_class_type(donor.summary.hash),
        staged.item_class_type(item.base)
    );
    for stat in &item.recipe.overrides.investment_stats {
        assert!(
            has_stat(&donor.investment_stats, stat),
            "{name} carries stat {stat:?}"
        );
    }
    if let Some((socket, plug)) = item.energy {
        assert_eq!(donor.sockets[socket].native_default, Some(plug));
        assert_eq!(
            donor.sockets[socket].reusable_plug_set_index,
            base.sockets[socket].reusable_plug_set_index,
            "{name} keeps its energy upgrades"
        );
    }
    // An edited socket keeps its base's plugs beside its own choices, so mods still apply.
    assert_eq!(
        donor.sockets[item.perk_socket].reusable_plug_set_index,
        base.sockets[item.perk_socket].reusable_plug_set_index,
        "{name} keeps its perk socket's plug set"
    );
}

/// The custom perk starts in its socket as its own plug, with its name and stats.
fn check_perk(
    staged: &InvestmentCatalog,
    item: &Authored,
    report: &crate::workflow::WeaponBuildReport,
    donor: &WeaponDonor,
) -> u32 {
    let [plug] = report.custom_plugs.as_slice() else {
        panic!("{} has one custom perk", item.recipe.name);
    };
    assert_eq!(plug.socket_index, item.perk_socket);
    let socket = &donor.sockets[item.perk_socket];
    assert_eq!(socket.native_default, Some(plug.item_hash));
    if let Some(alternative) = item.alternative {
        assert!(
            socket.ordered_embedded_choices.contains(&alternative),
            "{} keeps its socket's other choice",
            item.recipe.name
        );
    }
    assert_eq!(
        staged.item_display_name(plug.item_hash),
        Some(item.perk_name.as_str())
    );
    let stats = staged.item_stat_contributions(plug.item_hash);
    for stat in &item.perk_stats {
        assert!(
            has_stat(&stats, stat),
            "{} carries perk stat {stat:?}",
            item.perk_name
        );
    }
    // Offered everywhere, the perk joins its own socket's shared set, and the catalog finds it in
    // every set the build reports.
    if item.offered {
        let set = socket
            .reusable_plug_set_index
            .expect("the perk's socket keeps its shared plug set");
        assert!(
            plug.offered_sets.contains(&set),
            "{} is offered in its own socket's set {set}: {:?}",
            item.perk_name,
            plug.offered_sets
        );
        assert_eq!(
            staged.reusable_set_counts().get(&plug.item_hash),
            Some(&plug.offered_sets.len()),
            "{} reads back in every set it joined",
            item.perk_name
        );
    } else {
        assert!(plug.offered_sets.is_empty());
    }
    plug.item_hash
}

/// Ordinary armor joins runtime sets, Exotic armor joins only Exotics, and both join class badges. Other gear joins the runtime page in its base's category and every class.
fn check_collections(
    staged: &InvestmentCatalog,
    kind: ItemKind,
    base: u32,
    name: &str,
    item_hash: u32,
    brand: &str,
) {
    let base_parents = staged.item_collection_parents(base);
    let parents = staged.item_collection_parents(item_hash);
    assert!(!base_parents.is_empty());
    let paths = staged.item_collection_paths(item_hash);
    if kind == ItemKind::Armor {
        let exotic = staged.gear_donor(item_hash).unwrap().summary.rarity == WeaponRarity::Exotic;
        let supported = staged.item_class_type(item_hash).unwrap();
        for class in (0..3).filter(|class| supported == 3 || *class == supported) {
            let class_name = class_label(class).unwrap();
            if exotic {
                assert!(
                    paths.iter().any(|p| p
                        .iter()
                        .map(String::as_str)
                        .eq([class_name, "Armor", "Exotic", "Items"])),
                    "{name}: {paths:?}"
                );
                assert!(!paths.iter().any(|p| p.len() == 5 && p[1] == brand));
            } else {
                assert!(
                    paths.iter().any(|p| p.len() == 5
                        && p[0].starts_with(&format!("{brand} Armor Set "))
                        && p[1..]
                            .iter()
                            .map(String::as_str)
                            .eq([brand, class_name, "Armor", "Items"])),
                    "{name}: {paths:?}"
                );
            }
        }
    } else if crate::collection::GearPage::for_kind(kind).is_some() {
        let base_paths = staged.item_collection_paths(base);
        assert!(
            paths.iter().any(|path| {
                path.first().map(String::as_str) == Some(brand)
                    && base_paths
                        .iter()
                        .any(|base| base.len() == path.len() && base[1..] == path[1..])
            }),
            "{name} sits on the {brand} page in its base's category"
        );
        assert!(
            parents.iter().all(|parent| !base_parents.contains(parent)),
            "{name} leaves its base's season page"
        );
    } else {
        assert!(
            parents.iter().any(|parent| base_parents.contains(parent)),
            "{name} sits beside its base in Collections"
        );
    }
    for (class, hash) in [0x5355_4E54, 0x5355_4E48, 0x5355_4E57]
        .into_iter()
        .enumerate()
    {
        assert_eq!(
            parents.contains(&hash),
            kind != ItemKind::Armor
                || staged.item_class_type(item_hash) == Some(3)
                || staged.item_class_type(item_hash) == Some(class as u8),
            "{name} Sunrise badge class {class}"
        );
    }
}

/// Checks one staged item against what was authored and returns its read-back record.
fn read_back(
    staged: &InvestmentCatalog,
    build: &BuildReport,
    item: &Authored,
    (brand, packages): (&str, &Path),
) -> serde_json::Value {
    let report = build
        .weapons
        .iter()
        .find(|report| report.name == item.recipe.name)
        .unwrap();
    assert_eq!(report.kind, item.kind);
    let donor = staged
        .gear_donor(report.item_hash)
        .unwrap_or_else(|| panic!("{} reads back as gear", item.recipe.name));
    let base = staged.gear_donor(item.base).unwrap();
    check_item(staged, item, &donor, &base);
    let plug = check_perk(staged, item, report, &donor);
    check_collections(
        staged,
        item.kind,
        item.collections_base,
        &item.recipe.name,
        report.item_hash,
        brand,
    );
    // The lore tab reads back under the item's name with its own story.
    let lore = sundial::investment::load_item_lore(packages, report.item_hash)
        .unwrap()
        .unwrap_or_else(|| panic!("{} has a lore tab", item.recipe.name));
    assert_eq!(
        Some(lore.text.as_str()),
        item.recipe.overrides.lore.as_deref(),
        "{} carries its own lore",
        item.recipe.name
    );
    assert_eq!(lore.title, item.recipe.name);
    let stats = |stats: &[WeaponInvestmentStat]| {
        stats
            .iter()
            .map(|stat| format!("{} {}", stat.name.trim(), stat.value))
            .collect::<Vec<_>>()
    };
    serde_json::json!({
        "kind": item.kind,
        "name": donor.summary.name,
        "lore": lore.text,
        "item_hash": format!("0x{:08X}", report.item_hash),
        "base": format!("{} 0x{:08X}", base.summary.name, item.base),
        "type": donor.summary.type_name,
        "rarity": format!("{:?}", donor.summary.rarity),
        "class": staged.item_class_type(report.item_hash),
        "stats": stats(&donor.investment_stats),
        "sockets": donor
            .sockets
            .iter()
            .filter(|socket| {
                perk_destination(socket) || ENERGY_SOCKET_TYPES.contains(&socket.socket_type)
            })
            .map(|socket| {
                let plug = socket
                    .native_default
                    .map_or_else(String::new, |plug| staged.plug_label(plug, false));
                format!("{}: {plug}", socket.label)
            })
            .collect::<Vec<_>>(),
        "custom_perk": {
            "name": item.perk_name,
            "item_hash": format!("0x{plug:08X}"),
            "stats": stats(&staged.item_stat_contributions(plug)),
            "offered_sets": report.custom_plugs[0].offered_sets,
        },
        "collections": staged.item_collection_paths(report.item_hash),
    })
}

#[test]
#[ignore = "requires SUNDIAL_INSTALL and SUNDIAL_TEST_ARTIFACTS; stages subclasses from every stock donor"]
fn every_class_subclasses_remove_the_native_equip_requirement() {
    let packages = crate::test_support::install().join("packages");
    let artifacts = crate::test_support::artifact_dir("gear").join("class-requirements");
    fs::create_dir_all(&artifacts).unwrap();
    let stock = crate::test_support::catalog(packages.parent().unwrap()).unwrap();
    let bases = stock.subclasses(crate::package_profile::is_stock_item_definition);
    let recipes = bases
        .iter()
        .flat_map(|base| {
            [false, true].map(|every| {
                let mut recipe = WeaponRecipe::new_unbound_kind(ItemKind::Subclass).unwrap();
                recipe.set_donor(base.hash, base.name.clone());
                recipe
                    .rename_authored_item(format!(
                        "{} {}",
                        base.name,
                        if every { "Every Class" } else { "Base Class" }
                    ))
                    .unwrap();
                recipe.overrides.subclass_every_class = every;
                if every && base.hash == bases[0].hash {
                    recipe.type_name = Some("Subclass".into());
                }
                recipe
            })
        })
        .collect::<Vec<_>>();
    let snapshot = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
        package_directory: packages.clone(),
        staging_root: artifacts.join("staging"),
        ignore_installed_authored_overlays: true,
        recipes: recipes.clone(),
    })
    .unwrap();
    let build = crate::build_and_stage_snapshot_with_progress(&snapshot, |_| {}).unwrap();
    let view = artifacts.join("view");
    staged_view(&packages, &build, &view);
    let staged = InvestmentCatalog::load_with_cache_path(
        &view,
        &artifacts.join("catalog.json"),
        true,
        |_| {},
    )
    .unwrap();
    let manager = open_shadowkeep_package_manager(&view.join("packages")).unwrap();
    // Read the reachable equipment conditions independently of the authoring helper.
    let requirements = |hash| {
        let definition = manager
            .read_tag(tiger_pkg::TagHash(
                staged.item_definition_tag(hash).unwrap(),
            ))
            .unwrap();
        let equipment = crate::tag_payload::relative_target(&definition, 0x10).unwrap();
        let (count, _, groups, class) =
            crate::tag_payload::array_at(&definition, equipment).unwrap();
        assert_eq!(class, 0x8080_7D2F);
        (0..count)
            .map(|index| {
                let (count, _, rows, class) =
                    crate::tag_payload::array_at(&definition, groups + index * 16).unwrap();
                assert_eq!(class, 0x8080_7D31);
                (0..count)
                    .map(|index| {
                        let row = rows + index * 8;
                        (
                            crate::tag_payload::read_u16(&definition, row).unwrap(),
                            crate::tag_payload::read_u16(&definition, row + 4).unwrap(),
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    };
    let mut readback = Vec::new();
    for (base, pair) in bases.iter().zip(recipes.chunks_exact(2)) {
        let stock_requirements = requirements(base.hash);
        assert_eq!(stock_requirements.len(), 1);
        for recipe in pair {
            let hash = recipe.identity.item_hash.parse_u32().unwrap();
            let every = recipe.overrides.subclass_every_class;
            let actual = requirements(hash);
            assert_eq!(
                actual,
                if every {
                    vec![]
                } else {
                    stock_requirements.clone()
                },
                "{} equip conditions",
                recipe.name
            );
            assert_eq!(
                staged.item_class_type(hash),
                Some(if every { 3 } else { base.class_type })
            );
            let expected_type = recipe.type_name.clone().unwrap_or_else(|| {
                if every {
                    "Guardian Subclass".into()
                } else {
                    stock.item_type_name(base.hash).unwrap()
                }
            });
            assert_eq!(staged.item_type_name(hash), Some(expected_type.clone()));
            let own = staged
                .subclasses(|_| true)
                .into_iter()
                .find(|item| item.hash == hash)
                .unwrap();
            assert_eq!(
                own.entry_entities, base.entry_entities,
                "{} keeps its abilities",
                recipe.name
            );
            readback.push(serde_json::json!({"name": recipe.name, "item_hash": hash, "donor": base.hash, "class": own.class_type, "type_name": expected_type, "requirements": actual}));
        }
        assert_eq!(staged.item_class_type(base.hash), Some(base.class_type));
        assert_eq!(requirements(base.hash), stock_requirements);
    }
    fs::write(
        artifacts.join("readback.json"),
        serde_json::to_vec_pretty(&readback).unwrap(),
    )
    .unwrap();
    fs::remove_dir_all(&view).unwrap();
}

#[test]
#[ignore = "requires SUNDIAL_INSTALL and SUNDIAL_TEST_ARTIFACTS; builds and stages gear"]
fn gear_of_every_kind_goes_from_the_new_menu_to_staged_packages() {
    let packages = crate::test_support::install().join("packages");
    let artifacts = crate::test_support::artifact_dir("gear");
    fs::create_dir_all(artifacts.join("recipes")).unwrap();
    let catalog = crate::test_support::catalog(packages.parent().unwrap()).unwrap();
    let mut app = PackageAuthoringApp {
        packages: packages.clone(),
        perk_workbench: Workbench::offline(),
        ..Default::default()
    };
    app.install_catalog(catalog);
    let ctx = context();
    let kinds = std::env::var("PARHELION_GEAR_KINDS").ok();
    let wanted = |kind: ItemKind| {
        kinds.as_deref().is_none_or(|kinds| {
            kinds
                .split(',')
                .any(|name| name.trim().eq_ignore_ascii_case(kind.label()))
        })
    };
    let authored = [
        (ItemKind::Armor, Origin::Collections),
        (ItemKind::Armor, Origin::Exotic),
        (ItemKind::Armor, Origin::Reissue),
        (ItemKind::Armor, Origin::Legacy),
        (ItemKind::Sparrow, Origin::Collections),
        (ItemKind::Ship, Origin::Collections),
        (ItemKind::GhostShell, Origin::Collections),
    ]
    .into_iter()
    .filter(|(kind, _)| wanted(*kind))
    .map(|(kind, origin)| author(&ctx, &mut app, kind, origin))
    .collect::<Vec<_>>();
    let emblem = wanted(ItemKind::Emblem).then(|| author_emblem(&ctx, &mut app));
    let shader = wanted(ItemKind::Shader).then(|| author_shader(&ctx, &mut app));
    let subclass = wanted(ItemKind::Subclass).then(|| author_subclass(&ctx, &mut app));
    let retarget = wanted(ItemKind::Subclass).then(|| author_retarget(&ctx, &mut app));
    let recipes = authored
        .iter()
        .map(|item| item.recipe.clone())
        .chain(emblem.iter().map(|emblem| emblem.recipe.clone()))
        .chain(shader.iter().map(|shader| shader.recipe.clone()))
        .chain(subclass.iter().map(|subclass| subclass.recipe.clone()))
        .chain(retarget.iter().map(|retarget| retarget.recipe.clone()))
        .collect::<Vec<_>>();
    assert!(!recipes.is_empty(), "PARHELION_GEAR_KINDS names no kind");
    for recipe in &recipes {
        let json = recipe.to_json_pretty().unwrap();
        assert_eq!(WeaponRecipe::from_json_str(&json).unwrap(), *recipe);
        fs::write(
            artifacts
                .join("recipes")
                .join(format!("{}.parhelion.json", recipe.namespace)),
            json,
        )
        .unwrap();
    }

    let snapshot = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
        package_directory: packages.clone(),
        staging_root: artifacts.join("staging"),
        ignore_installed_authored_overlays: true,
        recipes: recipes.clone(),
    })
    .unwrap();
    let build = crate::build_and_stage_snapshot_with_progress(&snapshot, |progress| {
        eprintln!(
            "Gear build: {} {}/{}",
            progress.phase.label(),
            progress.completed,
            progress.total
        );
    })
    .unwrap();
    assert_eq!(build.weapons.len(), recipes.len());

    let brand = crate::branding::Branding::detect(packages.parent().unwrap()).name();
    let view = artifacts.join("view");
    staged_view(&packages, &build, &view);
    let staged = InvestmentCatalog::load_with_cache_path(
        &view,
        &artifacts.join("staged-catalog.json"),
        true,
        |_| {},
    )
    .unwrap();
    let readback =
        authored
            .iter()
            .map(|item| read_back(&staged, &build, item, (brand, &view.join("packages"))))
            .chain(emblem.iter().map(|emblem| {
                read_back_emblem(
                    &staged,
                    &build,
                    emblem,
                    (brand, &view.join("packages")),
                    &artifacts,
                )
            }))
            .chain(shader.iter().map(|shader| {
                read_back_shader(
                    &staged,
                    &build,
                    shader,
                    brand,
                    &view.join("packages"),
                    &artifacts,
                )
            }))
            .chain(subclass.iter().map(|subclass| {
                // The live install may already carry authored lists, so the stock baseline is the
                // packages the staged view started from, with no build in them.
                let stock_view = artifacts.join("stock-view");
                package_view(&packages, &[], &stock_view);
                let stock = InvestmentCatalog::load_with_cache_path(
                    &stock_view,
                    &artifacts.join("stock-catalog.json"),
                    true,
                    |_| {},
                )
                .unwrap();
                read_back_subclass((&stock, &staged), &build, subclass, &view.join("packages"))
            }))
            .chain(retarget.iter().map(|retarget| {
                read_back_retarget(&staged, &build, retarget, &view.join("packages"))
            }))
            .collect::<Vec<_>>();
    fs::write(
        artifacts.join("readback.json"),
        serde_json::to_vec_pretty(&readback).unwrap(),
    )
    .unwrap();
    fs::remove_dir_all(&view).unwrap();
}

fn tuning_parameter(bank: &sundial::investment::AbilityRowSummary) -> (u32, f32) {
    assert!(bank.charges, "a grenade's bank takes charge rows");
    let stock = bank
        .parameters
        .iter()
        .find(|parameter| {
            sundial::package_authoring::ability_bank::parameter_label(parameter.name).is_some()
        })
        .or(bank.parameters.first())
        .expect("a grenade's bank has script parameters a row can set");
    (
        stock.name,
        if stock.reset == 0.0 {
            1.0
        } else {
            stock.reset * 1.5
        },
    )
}
