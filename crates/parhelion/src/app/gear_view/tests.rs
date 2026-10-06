//! Gear from the toolbar to staged packages. Each non-weapon kind is created from the New menu,
//! edited on its page, given a lore tab of its own and a custom perk from its socket's menu, built
//! the way Build & Stage builds it and read back from the staged packages through Sundial's
//! catalog. Armor 1.0 keeps two choices in one socket, one made the default from its menu. A shader
//! remixes another shader's dyes and gives two surfaces custom colors and iridescence, and its
//! preview of those unbuilt edits must match the built shader drawn the same way. A subclass
//! takes a grenade from another class and an attunement from another subclass of its own, and
//! authors its middle path: a name of its own, and a second node taken from another path with
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
//! Nameplate section, keeps its base's overlay and takes a picture of its own as its background,
//! with each image's tile naming its size. It must show all three from a nameplate container of its
//! own, named by its strings and by the presentation row the game draws it from, with the picture
//! painted into the background's texture, and the background must export as that picture.
//!
//! `PARHELION_DEFAULT_WEAPONS_PACKAGES` names the installed packages and
//! `PARHELION_GEAR_ARTIFACTS` a folder on the same drive. The folder keeps the recipes, the staged
//! run and `readback.json`. `PARHELION_UI_CAPTURE_DIR` adds page captures, and
//! `PARHELION_GEAR_KINDS` (such as `Shader` or `Armor,Ship`) authors only the kinds it names.
//! Nothing installed is changed: the read-back view is hard links to the packages plus the
//! staged files.
use super::*;
mod armor_class;
mod armor_collections;
mod emblem_trackers;
mod shader_socket;
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
use std::fs;
use std::sync::Arc;
use std::time::{Duration, Instant};
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
    let imported = recipient.import_files(&[bundle.clone()]);
    assert!(imported.errors.is_empty(), "{:?}", imported.errors);
    assert_eq!(imported.paths.len(), 2);
    let edited = app.recipe.to_json_pretty().unwrap();
    if let Some(output) = std::env::var_os("PARHELION_UI_CAPTURE_DIR") {
        fs::copy(
            bundle,
            PathBuf::from(&output).join("source-variants.parhelion-bundle.json"),
        )
        .unwrap();
        fs::write(
            PathBuf::from(output).join("imported-shader-editable.parhelion.json"),
            edited,
        )
        .unwrap();
    }
}
use sundial::ui::model_preview::{Appearance, SurfaceOverride, still};

const WIDTH: f32 = 1320.0;

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
    energy: Option<(usize, u32)>,
    /// The plug the perk's socket keeps as a second choice.
    alternative: Option<u32>,
}

fn context() -> egui::Context {
    let ctx = egui::Context::default();
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
    let output = ctx.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(WIDTH, 1400.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::TopBottomPanel::top("gear-flow-toolbar").show(ctx, |ui| {
                let mut replaced = false;
                ui.horizontal(|ui| app.draw_new_item_button(ui, &mut replaced));
            });
            egui::CentralPanel::default().show(ctx, |ui| {
                workbench_style(ui);
                // The page the app's dispatch draws: a subclass's Appearance, else the editor.
                if app.recipe.kind == ItemKind::Subclass
                    && app.workbench_page == WorkbenchPage::Appearance
                {
                    app.draw_subclass_appearance(ui);
                } else if !app.recipe.kind.is_weapon() {
                    app.draw_gear_editor(ui);
                }
            });
            app.draw_perk_workbench(ctx);
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
            output = Some(ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 1400.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
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

/// New Weapon stays one click, and the caret lists every kind. Picks `kind` from it.
fn new_from_menu(ctx: &egui::Context, app: &mut PackageAuthoringApp, kind: ItemKind) {
    let output = settle(ctx, app);
    find(&output, "New Weapon", |text, _| text == "New Weapon");
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

/// Right-clicks the socket's first choice, picks Edit as Custom Perk, edits the perk and applies
/// it. The socket keeps its other choices.
fn custom_perk(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    kind: ItemKind,
    item_slug: &str,
    socket_index: usize,
    stats: &[(&str, i32)],
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
    let output = settle(ctx, app);
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
        ItemKind::Weapon | ItemKind::Shader | ItemKind::Subclass | ItemKind::Emblem => {
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
    let (perk_name, perk_stats) =
        custom_perk(ctx, app, kind, &slug(kind, origin), perk_socket, perk_stats);
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
        },
        "collections": staged.item_collection_paths(report.item_hash),
    })
}

/// An emblem from the New menu with another emblem's banner picked in its page's Nameplate
/// section, the base's own overlay and a picture of its own as its background. A file dialog
/// cannot be driven here, so the picture goes into the recipe the way an import puts it.
struct AuthoredEmblem {
    recipe: WeaponRecipe,
    base: u32,
    banner: u32,
    background: EmbeddedImage,
}

/// The emblem page: base, icon and rarity as on other gear, then a tile for each nameplate image
/// naming its size, `sizes`, and the editor of the selected one, the banner at first, in place of
/// sockets and stats. No model preview.
fn check_emblem_page(output: &egui::FullOutput, sizes: [(u32, u32); 3]) {
    let drawn = texts(output)
        .into_iter()
        .map(|(text, _)| text)
        .collect::<Vec<_>>();
    for label in [
        "Base Emblem",
        "Inventory Icon",
        "Rarity",
        "Text Presentation",
        "Nameplate",
        "Banner",
        "Overlay",
        "Background",
        "Import Image…",
        "Export PNG…",
    ] {
        assert!(
            drawn.iter().any(|text| text == label),
            "Emblem page lacks {label}"
        );
    }
    for (part, (width, height)) in NameplatePart::ALL.into_iter().zip(sizes) {
        let size = format!("({width} × {height} px)");
        assert!(
            drawn.contains(&size),
            "the {} tile names its size, {size}",
            part.label()
        );
    }
    assert_eq!(
        drawn.iter().filter(|text| *text == "Banner").count(),
        2,
        "the banner's tile starts selected, with its editor below"
    );
    assert!(
        !drawn.iter().any(|text| text == "Perks & Sockets"),
        "Emblem page shows sockets"
    );
    assert!(
        accessible(output, "Model Preview").is_none(),
        "Emblem page shows a model preview"
    );
}

/// A nameplate container's banner, overlay and background layers, where it has them.
fn nameplate_layers(manager: &PackageManager, container: u32) -> [Option<u32>; 3] {
    let payload = manager.read_tag(container).unwrap();
    NameplatePart::ALL.map(|part| {
        let offset = part.layer_offset();
        let layer = u32::from_le_bytes(payload[offset..offset + 4].try_into().unwrap());
        (layer != u32::MAX).then_some(layer)
    })
}

/// A layer's texture header and size.
fn layer_texture(manager: &PackageManager, layer: u32) -> (TagHash, (u32, u32)) {
    let layer_tag = TagHash(layer);
    let payload = manager.read_tag(layer_tag).unwrap();
    let header = crate::icon_edit::texture_reference_offsets(&payload, layer_tag).unwrap()[0].1;
    let header_payload = manager.read_tag(header).unwrap();
    let side = |offset: usize| {
        u32::from(u16::from_le_bytes([
            header_payload[offset],
            header_payload[offset + 1],
        ]))
    };
    (header, (side(0x0E), side(0x10)))
}

/// Frames until the page has loaded each of these emblems' images.
fn settle_nameplate(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    images: [(u32, NameplatePart); 3],
) -> egui::FullOutput {
    let start = Instant::now();
    loop {
        frame(ctx, app, Vec::new());
        let catalog = app.catalog.as_ref().unwrap();
        let loaded = images.iter().all(|&(emblem, part)| {
            catalog
                .nameplate_texture(ctx, emblem, part.layer_offset())
                .is_some()
        });
        if loaded {
            return settle(ctx, app);
        }
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "the nameplate images did not load"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// A picture with detail everywhere, so a crop or a resize shows in the built texture.
fn background_picture() -> EmbeddedImage {
    EmbeddedImage::from_rgba(image::RgbaImage::from_fn(1200, 120, |x, y| {
        image::Rgba([(x % 256) as u8, (y * 2) as u8, ((x / 5) % 256) as u8, 255])
    }))
    .unwrap()
}

fn author_emblem(ctx: &egui::Context, app: &mut PackageAuthoringApp) -> AuthoredEmblem {
    new_from_menu(ctx, app, ItemKind::Emblem);
    let manager = open_shadowkeep_package_manager(&app.packages).unwrap();
    let catalog = app.catalog.as_ref().unwrap();
    // A base with all three images, so a picture has a background to take the place of.
    let emblems = app.gear_donors[&ItemKind::Emblem]
        .iter()
        .filter(|emblem| {
            emblem.collection_backed
                && catalog
                    .nameplate_container(emblem.hash)
                    .is_some_and(|container| {
                        nameplate_layers(&manager, container)
                            .iter()
                            .all(Option::is_some)
                    })
        })
        .collect::<Vec<_>>();
    let base = emblems
        .first()
        .expect("a Collections emblem with all three images");
    // A banner that differs from the base's, so the one the build carries is the one picked.
    let target = emblems
        .iter()
        .find(|emblem| {
            catalog.nameplate_container(emblem.hash) != catalog.nameplate_container(base.hash)
        })
        .expect("two emblems with different nameplates");
    let (base_hash, base_name) = (base.hash, base.name.clone());
    let (target_hash, target_name) = (target.hash, target.name.clone());
    let base_layers = nameplate_layers(&manager, catalog.nameplate_container(base_hash).unwrap());
    let target_layers =
        nameplate_layers(&manager, catalog.nameplate_container(target_hash).unwrap());
    let size = |layer: Option<u32>| layer_texture(&manager, layer.unwrap()).1;
    app.recipe.set_donor(base_hash, base_name);
    app.recipe
        .rename_authored_item("Parhelion Test Emblem")
        .unwrap();

    assert_page_fits(ctx, app, "Emblem");
    let output = settle_nameplate(ctx, app, NameplatePart::ALL.map(|part| (base_hash, part)));
    check_emblem_page(&output, base_layers.map(size));
    capture::write(ctx, &output, "gear-emblem");

    // An emblem has no lore tab, so its text offers none.
    let opened = toggle_text_presentation(ctx, app, &output);
    find(&opened, "Custom Item-Type Label", |text, _| {
        text == "Custom Item-Type Label"
    });
    assert!(
        !texts(&opened)
            .iter()
            .any(|(text, _)| text.contains("Lore Tab")),
        "an emblem page offers no lore tab"
    );
    let output = toggle_text_presentation(ctx, app, &opened);

    // Every image starts on its base's. The selected banner's picker reads Base after the three
    // tiles that read it too, and its search takes focus as it opens, so the reader types the
    // emblem's name.
    let picker = texts(&output)
        .into_iter()
        .rev()
        .find(|(text, _)| text == "Base")
        .map(|(_, rect)| rect.center())
        .expect("the banner's picker");
    click(ctx, app, picker);
    frame(ctx, app, vec![egui::Event::Text(target_name.clone())]);
    let output = settle(ctx, app);
    capture::write(ctx, &output, "gear-emblem-picker");
    // The search box holds the bare name. A row names the emblem with its hash after it.
    let row_label = format!("{target_name}  (");
    let row = texts(&output)
        .into_iter()
        .find(|(text, _)| text.starts_with(&row_label))
        .map(|(_, rect)| rect.center())
        .unwrap_or_else(|| panic!("{target_name} is not in the nameplate list"));
    click(ctx, app, row);
    assert_eq!(
        app.recipe
            .overrides
            .nameplate
            .as_ref()
            .and_then(|nameplate| nameplate.part(NameplatePart::Banner)),
        Some(&NameplateImage::Emblem {
            item_hash: target_hash.into(),
        }),
        "the banner's picker picks {target_name}"
    );

    // The background's tile selects it, and the picture goes in as an import puts it.
    let output = settle(ctx, app);
    click(
        ctx,
        app,
        find(&output, "Background tile", |text, _| text == "Background"),
    );
    settle(ctx, app);
    assert_eq!(
        app.emblem_page.selected,
        NameplatePart::Background,
        "the background's tile selects it"
    );
    let background = background_picture();
    app.recipe
        .overrides
        .nameplate
        .get_or_insert_with(Default::default)
        .set(
            NameplatePart::Background,
            Some(NameplateImage::Image {
                image: background.clone(),
            }),
        );
    // The banner's tile names the picked emblem and its size, and the picture's the size of the
    // base's background, which it builds at.
    let output = settle_nameplate(
        ctx,
        app,
        [
            (target_hash, NameplatePart::Banner),
            (base_hash, NameplatePart::Overlay),
            (base_hash, NameplatePart::Background),
        ],
    );
    let drawn = texts(&output)
        .into_iter()
        .map(|(text, _)| text)
        .collect::<Vec<_>>();
    for label in [target_name.as_str(), "Base", "Picture"] {
        assert!(
            drawn.iter().any(|text| text == label),
            "the nameplate shows {label}"
        );
    }
    for (part, layer) in [
        (NameplatePart::Banner, target_layers[0]),
        (NameplatePart::Overlay, base_layers[1]),
        (NameplatePart::Background, base_layers[2]),
    ] {
        let (width, height) = size(layer);
        let label = format!("({width} × {height} px)");
        assert!(
            drawn.contains(&label),
            "the {} tile names {label}",
            part.label()
        );
    }
    capture::write(ctx, &output, "gear-emblem-nameplate");
    // The reader saves before starting the next item, so New starts without a prompt.
    app.recipe_baseline = app.recipe.clone();
    app.recipe_dirty = false;
    AuthoredEmblem {
        recipe: app.recipe.clone(),
        base: base_hash,
        banner: target_hash,
        background,
    }
}

/// The staged emblem keeps its base's bucket and rarity, and sits on the runtime's page under
/// Emblems and in its badge. Its strings and its presentation row name a nameplate container of
/// its own, whose banner is the picked emblem's layer, whose overlay is the base's and whose
/// background is a layer of its own, painted with the picture. The page's export writes that
/// background into `artifacts` as the picture.
fn read_back_emblem(
    staged: &InvestmentCatalog,
    build: &BuildReport,
    emblem: &AuthoredEmblem,
    (brand, packages): (&str, &Path),
    artifacts: &Path,
) -> serde_json::Value {
    let name = &emblem.recipe.name;
    let report = build
        .weapons
        .iter()
        .find(|report| report.name == *name)
        .unwrap();
    assert_eq!(report.kind, ItemKind::Emblem);
    let donor = staged
        .gear_donor(report.item_hash)
        .unwrap_or_else(|| panic!("{name} reads back as an emblem"));
    let base = staged.gear_donor(emblem.base).unwrap();
    assert_eq!(
        ItemKind::from_bucket_hash(donor.summary.bucket_hash),
        Some(ItemKind::Emblem)
    );
    assert_eq!(&donor.summary.name, name);
    assert_eq!(donor.summary.rarity, base.summary.rarity);
    assert!(donor.sockets.is_empty(), "{name} has no sockets");
    check_collections(
        staged,
        ItemKind::Emblem,
        emblem.base,
        name,
        report.item_hash,
        brand,
    );

    let container = staged
        .nameplate_container(report.item_hash)
        .unwrap_or_else(|| panic!("{name} names a nameplate in its strings"));
    let base_container = staged.nameplate_container(emblem.base).unwrap();
    assert_ne!(
        container, base_container,
        "{name} has a nameplate container of its own"
    );
    let manager = open_shadowkeep_package_manager(packages).unwrap();
    let globals = investment_globals(&manager);
    let dense = manager
        .read_tag(
            investment_globals_table_tag(&globals, GLOBALS_ITEM_DENSE_PRESENTATION_TABLE_SLOT)
                .unwrap(),
        )
        .unwrap();
    let fields = crate::item::dense_field_tags(&dense, usize::from(report.item_index)).unwrap();
    assert_eq!(
        fields
            .iter()
            .find(|(kind, _)| *kind == crate::emblem::NAMEPLATE_FIELD_TYPE)
            .map(|(_, tags)| tags.clone()),
        Some(vec![container]),
        "{name}'s presentation row shows its own nameplate"
    );
    let layers = nameplate_layers(&manager, container);
    let base_layers = nameplate_layers(&manager, base_container);
    let picked = nameplate_layers(&manager, staged.nameplate_container(emblem.banner).unwrap());
    assert_eq!(layers[0], picked[0], "{name} shows the picked banner");
    assert_eq!(layers[1], base_layers[1], "{name} keeps the base's overlay");
    let background = layers[2].expect("a background layer");
    assert_ne!(
        Some(background),
        base_layers[2],
        "{name}'s background is a layer of its own"
    );
    let (header, (width, height)) = layer_texture(&manager, background);
    let data = manager
        .read_tag(TagHash(manager.get_entry(header).unwrap().reference))
        .unwrap();
    let picture = crate::image_import::cover(emblem.background.pixels(), width, height);
    assert!(
        data == *picture.as_raw(),
        "{name}'s background texture holds the picture covering {width}x{height}"
    );
    let export = artifacts.join("emblem-background.png");
    emblem_view::write_png(
        &export,
        packages,
        NameplatePart::Background,
        emblem_view::Export::Layer(container),
    )
    .unwrap();
    assert!(
        image::open(&export).unwrap().into_rgba8() == picture,
        "{name}'s background exports as the picture covering {width}x{height}"
    );
    let tag = |layer: Option<u32>| layer.map(|layer| format!("0x{layer:08X}"));
    serde_json::json!({
        "kind": ItemKind::Emblem,
        "name": donor.summary.name,
        "item_hash": format!("0x{:08X}", report.item_hash),
        "base": format!("{} 0x{:08X}", base.summary.name, emblem.base),
        "rarity": format!("{:?}", donor.summary.rarity),
        "nameplate": {
            "container": format!("0x{container:08X}"),
            "banner": tag(layers[0]),
            "banner_from": format!("0x{:08X}", emblem.banner),
            "overlay": tag(layers[1]),
            "background": tag(layers[2]),
            "background_size": format!("{width}x{height}"),
            "background_export": "emblem-background.png",
        },
        "collections": staged.item_collection_paths(report.item_hash),
    })
}

/// A shader from the New menu that takes its cloth dyes from another stock shader, gives two
/// surfaces custom colors and iridescence on every gear type and one weapon surface more values of
/// its own, and gives the weapons' suit dye another shader's textures.
struct AuthoredShader {
    recipe: WeaponRecipe,
    base: u32,
    source: u32,
    /// The armor piece the page's preview showed the unbuilt shader on, and its image.
    preview_item: u32,
    preview: Arc<egui::ColorImage>,
    /// The weapon the page's preview showed first, and its image.
    weapon_item: u32,
    weapon_preview: Arc<egui::ColorImage>,
}

/// The dye a shader's rows give one key.
fn dye(rows: &shader_view::DyeRows, key: i8) -> Option<u16> {
    rows.iter()
        .flatten()
        .find(|row| row.channel_index == key)
        .map(|row| row.dye_reference_index)
}

/// Whether `rows` carry `source`'s dyes for one channel on every gear type.
fn channel_matches(
    rows: &shader_view::DyeRows,
    source: &shader_view::DyeRows,
    channel: i8,
) -> bool {
    crate::dye::GearType::ALL.iter().all(|gear| {
        let first = gear.first_key();
        dye(rows, first + channel) == dye(source, first + channel)
    })
}

/// A shader's rows as a model preview composes them.
fn preview_rows(rows: &shader_view::DyeRows) -> [Vec<(i8, u16)>; 3] {
    rows.clone().map(|rows| {
        rows.iter()
            .map(|row| (row.channel_index, row.dye_reference_index))
            .collect()
    })
}

/// Pixels of two same-sized previews that differ by more than one level in a channel. The unbuilt
/// preview applies its edits as surface overrides while the built shader reads the same values
/// from its dye records. Once the preview framed models closer (2026-09-30), the two drew 172 of
/// 147,076 pixels one level apart in one channel and none further, which no eye sees. Anything
/// more is a real difference.
fn differing_pixels(a: &egui::ColorImage, b: &egui::ColorImage) -> usize {
    assert_eq!(a.size, b.size, "the previews compared are the same size");
    a.pixels
        .iter()
        .zip(&b.pixels)
        .filter(|(a, b)| {
            a.to_array()
                .into_iter()
                .zip(b.to_array())
                .any(|(a, b)| a.abs_diff(b) > 1)
        })
        .count()
}

/// The model preview's image among a frame's texture uploads. The font atlas is the only other
/// upload this large, and it is not a color image.
fn preview_image(output: &egui::FullOutput) -> Option<Arc<egui::ColorImage>> {
    output
        .textures_delta
        .set
        .iter()
        .filter(|(_, delta)| delta.pos.is_none())
        .filter_map(|(_, delta)| match &delta.image {
            egui::ImageData::Color(image) => Some(image.clone()),
            egui::ImageData::Font(_) => None,
        })
        .filter(|image| image.width() >= 200 && image.height() >= 200)
        .max_by_key(|image| image.width() * image.height())
}

/// Runs frames until the preview has read what it was asked for. Returns the last image it drew.
fn await_preview(mut frame: impl FnMut() -> egui::FullOutput, what: &str) -> Arc<egui::ColorImage> {
    let start = Instant::now();
    let mut image = None;
    loop {
        let output = frame();
        image = preview_image(&output).or(image);
        let drawn = texts(&output);
        if !drawn.iter().any(|(text, _)| text == "Loading Model") {
            return image.unwrap_or_else(|| {
                panic!(
                    "{what} drew no preview. Text drawn: {:?}",
                    drawn.iter().map(|(text, _)| text).collect::<Vec<_>>()
                )
            });
        }
        assert!(
            start.elapsed() < Duration::from_secs(180),
            "{what} did not load"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The model preview alone, drawing `appearance` at the size of an earlier image.
fn draw_preview(
    packages: &Path,
    appearance: &Appearance,
    overrides: &[SurfaceOverride],
    like: &egui::ColorImage,
    what: &str,
) -> Arc<egui::ColorImage> {
    let ctx = context();
    let size = egui::vec2(like.width() as f32, like.height() as f32);
    await_preview(
        || {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        size + egui::vec2(64.0, 64.0),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        still::show(
                            ui,
                            egui::Id::new("read-back-preview"),
                            packages,
                            Some(appearance.clone()),
                            overrides,
                            size,
                        );
                    });
                },
            )
        },
        what,
    )
}

fn save_image(image: &egui::ColorImage, path: &Path) {
    let bytes = image
        .pixels
        .iter()
        .flat_map(|pixel| pixel.to_array())
        .collect::<Vec<_>>();
    ::image::RgbaImage::from_raw(image.width() as u32, image.height() as u32, bytes)
        .unwrap()
        .save(path)
        .unwrap();
}

/// The custom surfaces: armor primary takes a color and an iridescence row that tints the
/// color, and cloth secondary one that tints the highlight, on every gear type. The weapons' suit
/// primary takes a color, metalness, smoothness, glow and worn color of its own.
fn custom_surfaces(packages: &Path) -> Vec<DyeEdit> {
    let lookup = sundial::package_authoring::load_iridescence_rows(packages).unwrap();
    let row = |parity: i16| {
        lookup
            .iter()
            .map(|row| row.id)
            .find(|id| id % 2 == parity)
            .expect("an authored iridescence row of each parity")
    };
    let value = |value: f32| DyeValue::new(value).unwrap();
    vec![
        DyeEdit {
            color: Some([0xC8, 0x32, 0x14]),
            iridescence: Some(row(0)),
            ..DyeEdit::new(None, DyeChannel::Armor, DyeSurface::Primary)
        },
        DyeEdit {
            iridescence: Some(row(1)),
            ..DyeEdit::new(None, DyeChannel::Cloth, DyeSurface::Secondary)
        },
        DyeEdit {
            color: Some([0x14, 0x64, 0xC8]),
            metalness: Some(value(1.0)),
            smoothness: Some([value(0.6), value(0.9)]),
            glow: Some([0x10, 0x20, 0x30]),
            worn_color: Some([0xEE, 0xDD, 0xCC]),
            ..DyeEdit::new(
                Some(GearType::Weapon),
                DyeChannel::Suit,
                DyeSurface::Primary,
            )
        },
    ]
}

/// The weapons' suit dye takes the detail textures of another stock shader's weapon suit dye,
/// repeated twice as often.
fn custom_textures(app: &PackageAuthoringApp, base: &shader_view::DyeRows) -> Vec<DyeTextureEdit> {
    let key = GearType::Weapon.key(DyeChannel::Suit);
    let own = dye(base, key).expect("the base shader has a weapon suit dye");
    let catalog = app.catalog.as_ref().unwrap();
    let materials =
        |dyes: &[u16]| sundial::package_authoring::load_dye_materials(&app.packages, dyes).unwrap();
    let own_detail = materials(&[own])[&own].as_ref().unwrap().detail_tag;
    let mut candidates = app.gear_donors[&ItemKind::Shader]
        .iter()
        .filter_map(|shader| dye(&shader_view::stock_rows(catalog, shader.hash), key))
        .filter(|dye| *dye != own)
        .collect::<Vec<_>>();
    candidates.sort_unstable();
    candidates.dedup();
    // Read a few at a time, since most stock shaders share a handful of textures.
    let (detail, normal) = candidates
        .chunks(16)
        .find_map(|chunk| {
            materials(chunk)
                .into_values()
                .flatten()
                .find_map(|material| {
                    (material.detail_tag.is_some() && material.detail_tag != own_detail)
                        .then_some((material.detail_tag, material.normal_tag))
                })
        })
        .expect("another shader's weapon suit dye with other textures");
    let value = |value: f32| DyeValue::new(value).unwrap();
    vec![DyeTextureEdit {
        detail,
        normal,
        detail_tiling: Some([value(2.0), value(2.0), value(0.0), value(0.0)]),
        ..DyeTextureEdit::new(Some(GearType::Weapon), DyeChannel::Suit)
    }]
}

/// Cloth is the middle channel of each gear type's three.
const CLOTH: i8 = 1;

fn author_shader(ctx: &egui::Context, app: &mut PackageAuthoringApp) -> AuthoredShader {
    new_from_menu(ctx, app, ItemKind::Shader);
    app.recipe
        .rename_authored_item("Parhelion Test Shader")
        .unwrap();
    let base = app.recipe.donor.item_hash.parse_u32().unwrap();
    assert_page_fits(ctx, app, "Shader");
    let source = remix_cloth(ctx, app, base);
    edit_surfaces(ctx, app, base);
    let (weapon_item, weapon_preview) = preview_weapon(ctx, app);
    let (preview_item, preview) = preview_armor(ctx, app);
    check_weapon_view(ctx, app, weapon_item);
    app.recipe_baseline = app.recipe.clone();
    app.recipe_dirty = false;
    AuthoredShader {
        recipe: app.recipe.clone(),
        base,
        source,
        preview_item,
        preview,
        weapon_item,
        weapon_preview,
    }
}

/// Runs frames until nothing on the page is still loading.
fn settle_loaded(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    what: &str,
) -> egui::FullOutput {
    let start = Instant::now();
    loop {
        let output = settle(ctx, app);
        if !texts(&output).iter().any(|(text, _)| text == "Loading…") {
            return output;
        }
        assert!(
            start.elapsed() < Duration::from_secs(120),
            "{what} did not load"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The new shader's page, then its cloth taken from another stock shader through the page's own
/// edit, once every surface has loaded. Returns the source shader.
fn remix_cloth(ctx: &egui::Context, app: &mut PackageAuthoringApp, base: u32) -> u32 {
    let output = settle(ctx, app);
    for label in [
        "Dyes",
        "All Gear",
        "Armor Primary",
        "Cloth Secondary",
        "Suit Primary",
        "Copy from Shader…",
        "Preview",
        "Rarity",
    ] {
        find(&output, label, |text, _| text == label);
    }
    capture::write(ctx, &output, "gear-shader");
    // A shader has no lore tab, so its text offers none.
    let opened = toggle_text_presentation(ctx, app, &output);
    find(&opened, "Custom Item-Type Label", |text, _| {
        text == "Custom Item-Type Label"
    });
    assert!(
        !texts(&opened)
            .iter()
            .any(|(text, _)| text.contains("Lore Tab")),
        "a shader page offers no lore tab"
    );
    toggle_text_presentation(ctx, app, &opened);
    // The channel's shader list sets the channel through the page's own edit.
    let catalog = app.catalog.as_ref().unwrap();
    let base_rows = shader_view::stock_rows(catalog, base);
    let source = app.gear_donors[&ItemKind::Shader]
        .iter()
        .map(|shader| shader.hash)
        .find(|hash| {
            *hash != base
                && !channel_matches(&base_rows, &shader_view::stock_rows(catalog, *hash), CLOTH)
        })
        .expect("another shader with other cloth dyes");
    let source_rows = shader_view::stock_rows(catalog, source);
    shader_view::set_shader_channel(&mut app.recipe, &base_rows, &source_rows, CLOTH, None);
    let output = settle(ctx, app);
    find(&output, "Restore Base Dyes", |text, _| {
        text == "Restore Base Dyes"
    });
    // Each surface loads from the installed dyes in the background, then the inspector shows the
    // chosen one's values.
    let output = settle_loaded(ctx, app, "The dyes");
    assert!(
        !texts(&output).iter().any(|(text, _)| text == "Unavailable"),
        "every surface loads"
    );
    for label in [
        "Paint",
        "Color",
        "Iridescence",
        "Detail",
        "Textures",
        "Worn",
        "Glow",
    ] {
        find(&output, label, |text, _| text == label);
    }
    capture::write(ctx, &output, "gear-shader-remixed");
    source
}

/// Two surfaces on every gear type and one weapon surface take custom values, and the weapons'
/// suit dye another shader's textures, through the page's own edits. The values show in the
/// surfaces' tiles and inspector, and the icon draws from them once the dyes and iridescence ramps
/// have loaded.
fn edit_surfaces(ctx: &egui::Context, app: &mut PackageAuthoringApp, base: u32) {
    let edits = custom_surfaces(&app.packages);
    for edit in &edits {
        shader_view::set_dye_edit(
            &mut app.recipe.overrides.dye_edits,
            edit.gear,
            edit.channel,
            edit.surface,
            |surface| *surface = *edit,
        );
    }
    assert_eq!(app.recipe.overrides.dye_edits, edits);
    let base_rows = shader_view::stock_rows(app.catalog.as_ref().unwrap(), base);
    let textures = custom_textures(app, &base_rows);
    for texture in &textures {
        shader_view::set_texture_edit(
            &mut app.recipe.overrides.dye_texture_edits,
            texture.gear,
            texture.channel,
            |each| *each = *texture,
        );
    }
    assert_eq!(app.recipe.overrides.dye_texture_edits, textures);
    let output = settle(ctx, app);
    find(&output, "the custom color", |text, _| text == "#C83214");
    // A surface's iridescence shows in the inspector once its tile is chosen.
    for edit in edits.iter().filter(|edit| edit.gear.is_none()) {
        let Some(id) = edit.iridescence else {
            continue;
        };
        let name = format!(
            "{} {}",
            edit.channel.name_on(edit.gear),
            edit.surface.label()
        );
        let output = settle(ctx, app);
        click(ctx, app, find(&output, &name, |text, _| text == name));
        assert_eq!(app.shader_surface, (edit.channel, edit.surface));
        let id = id.to_string();
        find(&settle(ctx, app), "the custom iridescence", |text, _| {
            text == id
        });
    }
    // A new shader draws its icon from its dyes, custom ones included, once the stock dyes and
    // iridescence ramps have loaded.
    assert!(
        app.recipe.overrides.icon_from_dyes,
        "a new shader draws its icon from its dyes"
    );
    let start = Instant::now();
    while app.recipe.overrides.icon_edit.imported_image.is_none() {
        assert!(
            start.elapsed() < Duration::from_secs(60),
            "the page drew the shader's icon from its dyes"
        );
        std::thread::sleep(Duration::from_millis(50));
        let _output = settle(ctx, app);
    }
}

/// Until another item is chosen the page previews the first weapon, which the weapons' own surface
/// and textures change. Returns the weapon and the page's preview of it.
fn preview_weapon(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
) -> (u32, Arc<egui::ColorImage>) {
    let weapon = app.donor_summaries[0].hash;
    assert_eq!(app.shader_preview_item, None);
    let weapon_preview = await_preview(|| frame(ctx, app, Vec::new()), "The shader page");
    let catalog = app.catalog.as_ref().unwrap();
    let rows = preview_rows(app.recipe.overrides.render_dye_rows.as_ref().unwrap());
    let weapon_stock = draw_preview(
        &app.packages,
        &catalog.shader_preview_appearance(weapon, &rows).unwrap(),
        &[],
        &weapon_preview,
        "The weapon without custom dyes",
    );
    assert_ne!(
        weapon_stock.pixels, weapon_preview.pixels,
        "the custom dyes change the weapon's preview"
    );
    (weapon, weapon_preview)
}

/// The preview's item picker finds an armor piece by its hash, and the custom surfaces change what
/// the page draws on it. Returns the armor piece and the page's preview of it.
fn preview_armor(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
) -> (u32, Arc<egui::ColorImage>) {
    let catalog = app.catalog.as_ref().unwrap();
    let rows = preview_rows(app.recipe.overrides.render_dye_rows.as_ref().unwrap());
    let armor = app.gear_donors[&ItemKind::Armor]
        .iter()
        .map(|donor| donor.hash)
        .find(|hash| {
            catalog
                .shader_preview_appearance(*hash, &rows)
                .is_some_and(|appearance| {
                    !appearance.dyes.is_empty()
                        && appearance.dyes.iter().all(|(key, _)| (0..=2).contains(key))
                })
        })
        .expect("an armor piece with its own dyes");
    let appearance = catalog.shader_preview_appearance(armor, &rows).unwrap();
    let first = app.donor_summaries[0].name.clone();
    let output = settle(ctx, app);
    find(&output, "Preview", |text, _| text == "Preview");
    click(
        ctx,
        app,
        find(&output, "the preview's item", |text, _| text == first),
    );
    let hash = format!("0x{armor:08X}");
    frame(ctx, app, vec![egui::Event::Text(hash.clone())]);
    let output = settle(ctx, app);
    let row = format!("({hash})");
    click(
        ctx,
        app,
        find(&output, "the armor piece in the picker", |text, _| {
            text.ends_with(&row)
        }),
    );
    assert_eq!(app.shader_preview_item, Some(armor));
    let preview = await_preview(|| frame(ctx, app, Vec::new()), "The shader page");
    capture::write(ctx, &settle(ctx, app), "gear-shader-custom");
    // The custom surfaces change what the preview draws.
    let stock = draw_preview(
        &app.packages,
        &appearance,
        &[],
        &preview,
        "The preview without custom surfaces",
    );
    assert_ne!(
        stock.pixels, preview.pixels,
        "the custom surfaces change the preview"
    );
    (armor, preview)
}

/// The Weapons tab, marked for the weapons' own edits, shows those edits over the ones for every
/// gear type, names the weapons' channels by number as Bungie does, and moves the preview to a
/// weapon.
fn check_weapon_view(ctx: &egui::Context, app: &mut PackageAuthoringApp, weapon: u32) {
    let output = settle(ctx, app);
    click(
        ctx,
        app,
        find(&output, "the Weapons tab", |text, _| text == "Weapons •"),
    );
    assert_eq!(app.shader_dye_gear, Some(GearType::Weapon));
    assert_eq!(app.shader_preview_item, Some(weapon));
    await_preview(|| frame(ctx, app, Vec::new()), "The weapons' view");
    let output = settle_loaded(ctx, app, "The weapons' dyes");
    find(&output, "the weapons' suit color", |text, _| {
        text == "#1464C8"
    });
    find(&output, "the weapons' third channel", |text, _| {
        text == "Channel 3 Primary"
    });
    capture::write(ctx, &output, "gear-shader-weapons");
}

/// Each gear type's dye for `channel` in the staged shader. An edited dye is a custom one whose
/// vectors are the stock dye's with that gear type's edits written over them, every value, and
/// which binds the edits' textures. Any other dye keeps the stock one.
fn check_custom_dyes(
    packages: &Path,
    name: &str,
    rows: &shader_view::DyeRows,
    stock: &shader_view::DyeRows,
    channel: DyeChannel,
    (edits, textures): (&[DyeEdit], &[DyeTextureEdit]),
) -> Vec<String> {
    let pairs = GearType::ALL
        .into_iter()
        .filter_map(|gear| {
            let key = gear.key(channel);
            Some((gear, key, dye(rows, key)?, dye(stock, key)?))
        })
        .collect::<Vec<_>>();
    assert!(!pairs.is_empty(), "{name} has no {channel:?} dyes");
    let indices = pairs
        .iter()
        .flat_map(|&(_, _, built, original)| [built, original])
        .collect::<Vec<_>>();
    let materials = sundial::package_authoring::load_dye_materials(packages, &indices).unwrap();
    pairs
        .into_iter()
        .map(|(gear, key, built, original)| {
            let surfaces = DyeSurface::ALL
                .into_iter()
                .filter_map(|surface| surface_edit(edits, gear, channel, surface))
                .collect::<Vec<_>>();
            let texture = texture_edit(textures, gear, channel);
            if surfaces.is_empty() && texture.is_none() {
                assert_eq!(built, original, "{name} keeps the stock dye for key {key}");
                return format!("key {key}: stock dye {built}");
            }
            assert_ne!(built, original, "{name} has a custom dye for key {key}");
            let built_material = materials[&built].as_ref().unwrap();
            let stock_material = materials[&original].as_ref().unwrap();
            let mut expected = stock_material.vectors;
            for edit in &surfaces {
                write_vectors(&mut expected, &edit.writes());
            }
            if let Some(texture) = texture {
                write_vectors(&mut expected, &texture.writes());
            }
            assert_eq!(
                built_material.vectors, expected,
                "{name} key {key} carries every edited value"
            );
            let bound = (
                texture
                    .and_then(|texture| texture.detail)
                    .or(stock_material.detail_tag),
                texture
                    .and_then(|texture| texture.normal)
                    .or(stock_material.normal_tag),
            );
            assert_eq!(
                (built_material.detail_tag, built_material.normal_tag),
                bound,
                "{name} key {key} binds its textures"
            );
            format!("key {key}: dye {built} from {original}, textures {bound:?}")
        })
        .collect()
}

/// The staged shader keeps the stock shape and its base's dyes, with cloth from the source and
/// custom dyes for the edited surfaces. Its preview matches the page's preview of the unbuilt
/// edits pixel for pixel.
fn read_back_shader(
    staged: &InvestmentCatalog,
    build: &BuildReport,
    shader: &AuthoredShader,
    brand: &str,
    packages: &Path,
    artifacts: &Path,
) -> serde_json::Value {
    let name = &shader.recipe.name;
    let report = build
        .weapons
        .iter()
        .find(|report| report.name == *name)
        .unwrap();
    assert_eq!(report.kind, ItemKind::Shader);
    assert_eq!(
        staged.item_display_name(report.item_hash),
        Some(name.as_str())
    );
    assert!(
        staged.is_shader(report.item_hash),
        "{name} keeps the stock shader shape"
    );
    let rows = shader_view::stock_rows(staged, report.item_hash);
    let base = shader_view::stock_rows(staged, shader.base);
    let source = shader_view::stock_rows(staged, shader.source);
    let edits = &shader.recipe.overrides.dye_edits;
    let textures = &shader.recipe.overrides.dye_texture_edits;
    let custom = DyeChannel::ALL
        .into_iter()
        .flat_map(|channel| {
            // Cloth came from the source shader, and the other channels from the base.
            let stock = if channel.offset() == CLOTH {
                &source
            } else {
                &base
            };
            check_custom_dyes(packages, name, &rows, stock, channel, (edits, textures))
        })
        .collect::<Vec<_>>();
    let built = draw_preview(
        packages,
        &staged
            .shader_preview_appearance(shader.preview_item, &preview_rows(&rows))
            .unwrap(),
        &[],
        &shader.preview,
        "The built shader",
    );
    save_image(
        &shader.preview,
        &artifacts.join("shader-preview-unbuilt.png"),
    );
    save_image(&built, &artifacts.join("shader-preview-built.png"));
    assert_eq!(
        differing_pixels(&built, &shader.preview),
        0,
        "{name} built draws like its unbuilt preview on 0x{:08X}",
        shader.preview_item
    );
    // On a weapon too, with the weapons' own surface and textures.
    let built_weapon = draw_preview(
        packages,
        &staged
            .shader_preview_appearance(shader.weapon_item, &preview_rows(&rows))
            .unwrap(),
        &[],
        &shader.weapon_preview,
        "The built shader on a weapon",
    );
    save_image(
        &shader.weapon_preview,
        &artifacts.join("shader-weapon-unbuilt.png"),
    );
    save_image(&built_weapon, &artifacts.join("shader-weapon-built.png"));
    assert_eq!(
        differing_pixels(&built_weapon, &shader.weapon_preview),
        0,
        "{name} built draws like its unbuilt preview on the weapon 0x{:08X}",
        shader.weapon_item
    );
    check_collections(
        staged,
        ItemKind::Shader,
        shader.base,
        name,
        report.item_hash,
        brand,
    );
    serde_json::json!({
        "kind": ItemKind::Shader,
        "name": name,
        "item_hash": format!("0x{:08X}", report.item_hash),
        "base": format!(
            "{} 0x{:08X}",
            staged.item_display_name(shader.base).unwrap_or_default(),
            shader.base
        ),
        "cloth_from": format!(
            "{} 0x{:08X}",
            staged.item_display_name(shader.source).unwrap_or_default(),
            shader.source
        ),
        "dye_rows": rows[0]
            .iter()
            .map(|row| format!("{}: {}", row.channel_index, row.dye_reference_index))
            .collect::<Vec<_>>(),
        "dye_edits": edits,
        "dye_texture_edits": textures,
        "custom_dyes": custom,
        "preview": {
            "item": format!(
                "{} 0x{:08X}",
                staged.item_display_name(shader.preview_item).unwrap_or_default(),
                shader.preview_item
            ),
            "size": shader.preview.size,
            "matches_built": true,
        },
        "weapon_preview": {
            "item": format!(
                "{} 0x{:08X}",
                staged.item_display_name(shader.weapon_item).unwrap_or_default(),
                shader.weapon_item
            ),
            "matches_built": true,
        },
        "collections": staged.item_collection_paths(report.item_hash),
    })
}

/// A subclass from the New menu whose first grenade comes from another class and whose top
/// attunement comes from another subclass of its class.
struct AuthoredSubclass {
    recipe: WeaponRecipe,
    base: SubclassSummary,
    grenade: SubclassSummary,
    attunement: SubclassSummary,
    /// The perk the authored middle-path node and the authored grenade add to the ones they
    /// start with.
    extra_perk: u16,
    /// The stock perk the authored grenade's custom perk copies.
    custom_effect: u16,
    /// The value of its entity the authored grenade changes.
    ability_value: AbilityValue,
    /// The script parameter of its bank the authored grenade sets, and the value.
    parameter: (u32, f32),
    /// The value of a graph the authored grenade spawns it changes.
    spawn_value: AbilityValue,
    /// The palette of its effects the authored grenade recolors, and how.
    palette: PaletteEdit,
    /// The color its materials hold that the authored grenade recolors, and how.
    tint: TintEdit,
    /// The first grenade's stock entity and bank, and its value of a graph only that bank names,
    /// which gives its copy a private copy of the bank.
    bank_value: (u32, u32, AbilityValue),
    /// The first grenade's palette that only graphs behind an impact table draw, and how it is
    /// recolored, which gives its copy a private copy of the table.
    table_palette: PaletteEdit,
    /// The projectile the base's third grenade fires in place of the one it spawns.
    swap: SpawnSwap,
    /// The base's first movement ability's stock entity and its count of airborne jumps.
    movement: (u32, ability_movement::MovementValue),
    /// The base's third movement ability's stock entity and the airborne jumps row its key
    /// applies in its bank.
    jump_row: (u32, ability_movement::RowLane),
}

/// One value of a stock ability's entity, and the edit an ability authors to it.
struct AbilityValue {
    entity: u32,
    name: String,
    stock: WeaponRuntimeValue,
    edit: WeaponRuntimeValueOverride,
}

/// A 32-bit float of `entity` outside its bank, doubled, as the Values tab would write it. The
/// graph loads the way the page loads it.
fn ability_value(packages: &Path, entity: u32) -> AbilityValue {
    let manager = open_shadowkeep_package_manager(packages).unwrap();
    graph_value(&manager, entity)
        .unwrap_or_else(|| panic!("ability entity 0x{entity:08X} has a float outside its bank"))
}

/// The first graph `entity` spawns with a float, and that float doubled, as the Spawns tab would
/// write it.
fn spawn_value(packages: &Path, entity: u32) -> AbilityValue {
    let manager = open_shadowkeep_package_manager(packages).unwrap();
    let payload = manager.read_tag(tiger_pkg::TagHash(entity)).unwrap();
    ability_spawns::spawned_graphs(&manager, entity, &payload)
        .unwrap()
        .into_iter()
        .find_map(|graph| graph_value(&manager, graph))
        .unwrap_or_else(|| panic!("ability entity 0x{entity:08X} spawns no graph with a float"))
}

/// Whether `graph` is a projectile, by the client's object type at `+0x96`.
fn is_projectile(manager: &PackageManager, graph: u32) -> bool {
    manager
        .read_tag(tiger_pkg::TagHash(graph))
        .is_ok_and(|payload| payload.get(0x96) == Some(&18))
}

/// `entity`'s bank, and a value of a graph that bank names and nothing else of `entity` does,
/// doubled, as the Spawns tab would write it. None when the bank names no such graph with a
/// float.
fn bank_value(manager: &PackageManager, entity: u32) -> Option<(u32, AbilityValue)> {
    let payload = manager.read_tag(tiger_pkg::TagHash(entity)).ok()?;
    let bank = ability_modifier::entity_bank(&payload).ok()??;
    let spawns = ability_spawns::spawns(manager, entity, &payload).ok()?;
    let elsewhere = spawns
        .iter()
        .filter(|spawn| spawn.owner != bank)
        .map(|spawn| spawn.graph)
        .collect::<BTreeSet<_>>();
    let value = spawns
        .iter()
        .filter(|spawn| spawn.owner == bank && !elsewhere.contains(&spawn.graph))
        .find_map(|spawn| graph_value(manager, spawn.graph))?;
    Some((bank, value))
}

/// The first projectile `entity` names, in place of which it fires the first projectile
/// `donor`'s graphs name.
fn projectile_swap(manager: &PackageManager, entity: u32, donor: u32) -> SpawnSwap {
    let payload = manager.read_tag(tiger_pkg::TagHash(entity)).unwrap();
    let replaced = ability_spawns::spawned_graphs(manager, entity, &payload)
        .unwrap()
        .into_iter()
        .find(|graph| is_projectile(manager, *graph))
        .unwrap_or_else(|| panic!("ability entity 0x{entity:08X} names a projectile to swap"));
    let mut seen = BTreeSet::new();
    let mut queue = std::collections::VecDeque::from([(donor, 0usize)]);
    let replacement = loop {
        let (graph, depth) = queue
            .pop_front()
            .unwrap_or_else(|| panic!("ability entity 0x{donor:08X} fires a projectile"));
        if !seen.insert(graph) {
            continue;
        }
        if graph != donor && graph != replaced && is_projectile(manager, graph) {
            break graph;
        }
        if depth < 3 {
            let payload = manager.read_tag(tiger_pkg::TagHash(graph)).unwrap();
            for child in ability_spawns::spawned_graphs(manager, graph, &payload).unwrap() {
                queue.push_back((child, depth + 1));
            }
        }
    };
    SpawnSwap {
        graph: entity,
        replaced,
        replacement,
    }
}

/// How much faster the authored class ability recharges.
const RECHARGE: f32 = 1.5;

/// The airborne jumps the base's first movement ability allows once authored.
const AIRBORNE_JUMPS: u32 = 3;
/// The airborne jumps the row the third movement ability's key applies sets once authored.
const ROW_AIRBORNE_JUMPS: u32 = 4;

/// The base's first movement ability's airborne jumps, and the airborne jumps row its third
/// movement ability's key applies, put in through the recipe as the Properties tab writes them,
/// then shown on that tab with the first one's bank rows. Returns each stock entity and value.
fn author_movement(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    base: &SubclassSummary,
) -> (
    (u32, ability_movement::MovementValue),
    (u32, ability_movement::RowLane),
) {
    let entry = layout::MOVEMENT[0];
    let entity = *base
        .entry_entities
        .get(&entry)
        .expect("the base's first movement ability has an entity");
    let manager = open_shadowkeep_package_manager(&app.packages).unwrap();
    let jumps = movement_value(&manager, entity, "Airborne Jumps")
        .expect("the base's first movement ability holds its airborne jumps");
    let mut abilities = app.recipe.overrides.subclass_abilities.clone().unwrap();
    let mut edits = abilities.edits(base.hash, Place::Ability(entry));
    jumps.write(&mut edits.ability_values, AIRBORNE_JUMPS);
    abilities.set_edits(base.hash, Place::Ability(entry), edits);
    let third = layout::MOVEMENT[2];
    let third_entity = *base
        .entry_entities
        .get(&third)
        .expect("the base's third movement ability has an entity");
    let row = own_lanes(&manager, base, third)
        .into_iter()
        .find(|lane| lane.label == "Airborne Jumps")
        .expect("the third movement ability's key applies an airborne jumps row");
    let mut edits = abilities.edits(base.hash, Place::Ability(third));
    edits.set_bank_value(row.key, row.row, row.lane, Some(ROW_AIRBORNE_JUMPS));
    abilities.set_edits(base.hash, Place::Ability(third), edits);
    app.recipe.overrides.subclass_abilities = Some(abilities);
    // Its Properties tab, marked for the edit, shows the count as a tile of its own.
    let name = base.entry_names[&entry].as_str();
    let output = settle(ctx, app);
    click(ctx, app, find(&output, name, |text, _| text == name));
    assert_eq!(
        app.subclass_page.selection,
        SubclassSelection::Entry(Place::Ability(entry))
    );
    let output = settle(ctx, app);
    click(
        ctx,
        app,
        find(&output, "the Properties tab", |text, _| {
            text == "Properties •"
        }),
    );
    let output = settle_loaded(ctx, app, "the movement ability's properties");
    // Its bank rows its key applies show beside it.
    for text in ["Airborne Jumps", "Vertical Impulse", "Directional Impulse"] {
        find(&output, text, |drawn, _| drawn == text);
    }
    capture::write(ctx, &output, "gear-subclass-movement-properties");
    ((entity, jumps), (third_entity, row))
}

/// The traced lanes of the rows of `entry`'s bank its own pool's keys apply to its row, as the
/// Properties tab finds them.
fn own_lanes(
    manager: &PackageManager,
    subclass: &SubclassSummary,
    entry: u8,
) -> Vec<ability_movement::RowLane> {
    let entity = subclass.entry_entities[&entry];
    let row = subclass.entry_rows[&entry];
    let keys = subclass.entry_modifiers[&entry]
        .iter()
        .filter(|(_, target)| *target == row)
        .map(|(key, _)| *key)
        .collect::<Vec<_>>();
    let payload = manager.read_tag(tiger_pkg::TagHash(entity)).unwrap();
    let bank = ability_modifier::entity_bank(&payload).unwrap().unwrap();
    let bank = manager.read_tag(tiger_pkg::TagHash(bank)).unwrap();
    ability_movement::row_lanes(&bank, &keys).unwrap()
}

/// The third movement ability's airborne jumps row read back: it names a copy of its entity,
/// which binds a private copy of its bank holding the authored count in that row, and the stock
/// bank keeps its own. Returns what `readback.json` records of it.
fn read_back_jump_row(
    manager: &PackageManager,
    authored: &SubclassSummary,
    (entity, row): &(u32, ability_movement::RowLane),
    name: &str,
) -> serde_json::Value {
    let copied = *authored
        .entry_entities
        .get(&layout::MOVEMENT[2])
        .unwrap_or_else(|| panic!("{name}'s third movement ability names an entity"));
    assert_ne!(
        copied, *entity,
        "{name}'s third movement ability names a copy of its entity"
    );
    // The copy binds its bank where the stock entity binds the stock one.
    use sundial::package_authoring::entity::{
        weapon_component_binding_hashes, weapon_component_bindings,
    };
    let stock_entity = manager.read_tag(tiger_pkg::TagHash(*entity)).unwrap();
    let copied_entity = manager.read_tag(tiger_pkg::TagHash(copied)).unwrap();
    let stock_bank = ability_modifier::entity_bank(&stock_entity)
        .unwrap()
        .unwrap_or_else(|| panic!("0x{entity:08X} binds a bank"));
    let mut banks = BTreeSet::new();
    for binding in weapon_component_binding_hashes(&stock_entity).unwrap() {
        let stock = weapon_component_bindings(&stock_entity, binding).unwrap();
        let copy = weapon_component_bindings(&copied_entity, binding).unwrap();
        for (stock, copy) in stock.iter().zip(&copy) {
            if stock.owner_tag == stock_bank {
                banks.insert(copy.owner_tag);
            }
        }
    }
    assert_eq!(
        banks.len(),
        1,
        "{name}'s third movement ability binds one bank: {banks:X?}"
    );
    let private = *banks.first().unwrap();
    assert_ne!(
        private, stock_bank,
        "{name}'s third movement ability binds a private copy of its bank"
    );
    let count = |bank: u32| {
        let payload = manager.read_tag(tiger_pkg::TagHash(bank)).unwrap();
        ability_movement::row_lane(&payload, (row.key, row.row, row.lane))
            .unwrap_or_else(|error| panic!("bank 0x{bank:08X}: {error}"))
            .stock
    };
    assert_eq!(
        count(private),
        ROW_AIRBORNE_JUMPS,
        "{name}'s private bank's row sets the authored airborne jumps"
    );
    assert_eq!(
        count(stock_bank),
        row.stock,
        "the stock bank's row keeps its airborne jumps"
    );
    serde_json::json!({
        "stock_entity": format!("0x{entity:08X}"),
        "copy_entity": format!("0x{copied:08X}"),
        "stock_bank": format!("0x{stock_bank:08X}"),
        "private_bank": format!("0x{private:08X}"),
        "key": format!("0x{:08X}", row.key),
        "row": row.row,
        "stock": row.stock,
        "authored": ROW_AIRBORNE_JUMPS,
    })
}

/// The movement value `label` names in `entity`'s graph, as the Properties tab finds it.
fn movement_value(
    manager: &PackageManager,
    entity: u32,
    label: &str,
) -> Option<ability_movement::MovementValue> {
    let payload = manager.read_tag(tiger_pkg::TagHash(entity)).ok()?;
    let mut graph = load_weapon_runtime_graph_for_entity(manager, 0, 0, entity, &payload).ok()?;
    graph.scope_fields();
    ability_movement::discover(&graph)
        .into_iter()
        .find(|value| value.label == label)
}

/// The first movement ability's airborne jumps read back: it names a copy of its entity, which
/// holds the authored count, and the stock entity keeps its own. Returns what `readback.json`
/// records of it.
fn read_back_movement(
    manager: &PackageManager,
    authored: &SubclassSummary,
    (entity, jumps): &(u32, ability_movement::MovementValue),
    name: &str,
) -> serde_json::Value {
    let copied = *authored
        .entry_entities
        .get(&layout::MOVEMENT[0])
        .unwrap_or_else(|| panic!("{name}'s first movement ability names an entity"));
    assert_ne!(
        copied, *entity,
        "{name}'s first movement ability names a copy of its entity"
    );
    let count = |graph: u32| {
        movement_value(manager, graph, jumps.label)
            .unwrap_or_else(|| panic!("0x{graph:08X} holds its airborne jumps"))
            .stock()
    };
    assert_eq!(
        count(copied),
        AIRBORNE_JUMPS,
        "{name}'s copy allows the authored airborne jumps"
    );
    assert_eq!(
        count(*entity),
        jumps.stock(),
        "the stock movement ability keeps its airborne jumps"
    );
    serde_json::json!({
        "stock_entity": format!("0x{entity:08X}"),
        "copy_entity": format!("0x{copied:08X}"),
        "stock": jumps.stock(),
        "authored": AIRBORNE_JUMPS,
    })
}

/// The first palette of ability `entity`'s effects that only graphs behind an impact table draw:
/// no graph its components name directly, at any level, draws it.
fn table_palette(manager: &PackageManager, entity: u32) -> Option<u32> {
    let mut direct = BTreeSet::from([entity]);
    let mut level = vec![entity];
    for _ in 0..crate::subclass::SPAWN_DEPTH {
        let mut next = Vec::new();
        for graph in level {
            let payload = manager.read_tag(tiger_pkg::TagHash(graph)).ok()?;
            for child in ability_spawns::spawned_graphs(manager, graph, &payload).ok()? {
                if direct.insert(child) {
                    next.push(child);
                }
            }
        }
        level = next;
    }
    ability_palette::ability_palettes(manager, entity, crate::subclass::SPAWN_DEPTH)
        .ok()?
        .into_iter()
        .find(|palette| {
            palette
                .uses
                .iter()
                .all(|each| !direct.contains(&each.graph))
        })
        .map(|palette| palette.header)
}

/// The first grenade's value of a graph only its bank names, a turn of the hue of a palette only
/// an impact table reaches and a green grade over every effect, the third grenade's swap of its projectile for one the first
/// grenade fires, and the class ability's faster recharge, put in through the recipe as the
/// Spawns, Effect Colors, Properties and Recharge fields write them. Returns the first grenade's
/// entity, bank and value, the swap and the palette change.
fn author_bank_and_swap(
    app: &mut PackageAuthoringApp,
    (base, grenade): (&SubclassSummary, &SubclassSummary),
) -> ((u32, u32, AbilityValue), SpawnSwap, PaletteEdit) {
    let manager = open_shadowkeep_package_manager(&app.packages).unwrap();
    let first = layout::GRENADES[0];
    let entity = *grenade
        .entry_entities
        .get(&first)
        .expect("the taken grenade has an entity");
    let (bank, value) =
        bank_value(&manager, entity).expect("the taken grenade's bank names a graph of its own");
    let palette = PaletteEdit {
        hue: 120,
        ..PaletteEdit::new(
            table_palette(&manager, entity)
                .expect("an impact table reaches the only graphs drawing a taken grenade palette"),
        )
    };
    let third = layout::GRENADES[2];
    let swapped = *base
        .entry_entities
        .get(&third)
        .expect("the base's third grenade has an entity");
    let swap = projectile_swap(&manager, swapped, entity);
    let mut abilities = app.recipe.overrides.subclass_abilities.clone().unwrap();
    let mut edits = abilities.edits(base.hash, Place::Ability(first));
    edits.ability_values.push(value.edit.clone());
    edits.set_palette(palette);
    // And every effect's final color takes the palette's new hue, as Overall sets it.
    edits.set_grade(GRADE);
    abilities.set_edits(base.hash, Place::Ability(first), edits);
    let place = Place::Ability(third);
    let mut edits = abilities.edits(base.hash, place);
    edits.set_swap(swap.graph, swap.replaced, Some(swap.replacement));
    // The projectile it fires in place of its own takes the grade too.
    edits.set_grade(GRADE);
    abilities.set_edits(base.hash, place, edits);
    let class = layout::CLASS_ABILITIES[0];
    let row = *base
        .entry_rows
        .get(&class)
        .expect("the base's class ability equips a row");
    assert!(
        app.catalog
            .as_ref()
            .unwrap()
            .ability_row(row)
            .is_some_and(|row| row.recharge),
        "the base's class ability takes a recharge rate"
    );
    let mut edits = abilities.edits(base.hash, Place::Ability(class));
    edits.set_recharge(Some(RECHARGE));
    abilities.set_edits(base.hash, Place::Ability(class), edits);
    app.recipe.overrides.subclass_abilities = Some(abilities);
    ((entity, bank, value), swap, palette)
}

/// The class ability's recharge read back: its pool applies the recharge key to its row, and its
/// bank has a numeric input row under that key. Returns what `readback.json` records of it.
fn read_back_recharge(
    manager: &PackageManager,
    staged: &InvestmentCatalog,
    authored: &SubclassSummary,
    name: &str,
) -> serde_json::Value {
    let class = layout::CLASS_ABILITIES[0];
    let row = *authored
        .entry_rows
        .get(&class)
        .unwrap_or_else(|| panic!("{name}'s class ability equips a row"));
    let key = ability_modifier::recharge_key(RECHARGE.to_bits());
    assert!(
        authored
            .entry_modifiers
            .get(&class)
            .is_some_and(|applied| applied.contains(&(key, row))),
        "{name}'s class ability applies its recharge key 0x{key:08X} to row {row}"
    );
    let bank = staged
        .ability_row(row)
        .and_then(|row| row.bank)
        .unwrap_or_else(|| panic!("{name}'s class ability row {row} reads a bank"));
    let payload = manager.read_tag(tiger_pkg::TagHash(bank)).unwrap();
    let rows = sundial::package_authoring::ability_bank::property_rows(&payload).unwrap();
    let found = rows
        .iter()
        .find(|each| each.key == key)
        .unwrap_or_else(|| panic!("bank 0x{bank:08X} has a row for the recharge key"));
    assert_eq!(
        found.modifier_class, 0x8080_451B,
        "the recharge row changes a numeric input"
    );
    serde_json::json!({
        "row": row,
        "bank": format!("0x{bank:08X}"),
        "key": format!("0x{key:08X}"),
        "multiplier": RECHARGE,
    })
}

/// The palettes the effects of ability `entity` draw with, found the way the Effect Colors field
/// and the build find them.
fn stock_palettes(packages: &Path, entity: u32) -> Vec<ability_palette::Palette> {
    let manager = open_shadowkeep_package_manager(packages).unwrap();
    let palettes =
        ability_palette::ability_palettes(&manager, entity, crate::subclass::SPAWN_DEPTH).unwrap();
    assert!(
        !palettes.is_empty(),
        "ability entity 0x{entity:08X}'s effects draw with a palette"
    );
    palettes
}

/// Each graph of stock ability `stock`'s tree, with the graph at the same place of `copied`'s
/// tree: a private copy, or the stock graph where the copy keeps it. Graphs pair by the places
/// their parents name them, through impact tables too.
fn twins(manager: &PackageManager, stock: u32, copied: u32) -> BTreeMap<u32, u32> {
    type Place = (u32, u16, u32);
    let place = |spawn: &ability_spawns::Spawn| -> Place {
        (spawn.binding_hash, spawn.resource_index, spawn.offset)
    };
    let read = |tag: u32| manager.read_tag(tiger_pkg::TagHash(tag)).unwrap();
    let mut twins = BTreeMap::from([(stock, copied)]);
    let mut queue = std::collections::VecDeque::from([(stock, copied, 0)]);
    while let Some((own, copy, depth)) = queue.pop_front() {
        if depth >= crate::subclass::SPAWN_DEPTH {
            continue;
        }
        let (own_payload, copy_payload) = (read(own), read(copy));
        let mut pairs = Vec::new();
        let named = ability_spawns::spawns(manager, copy, &copy_payload)
            .unwrap()
            .into_iter()
            .map(|spawn| (place(&spawn), spawn.graph))
            .collect::<BTreeMap<_, _>>();
        for spawn in ability_spawns::spawns(manager, own, &own_payload).unwrap() {
            if let Some(&graph) = named.get(&place(&spawn)) {
                pairs.push((spawn.graph, graph));
            }
        }
        let tables = ability_spawns::tables(manager, copy, &copy_payload)
            .unwrap()
            .into_iter()
            .map(|table| (place(&table), table.graph))
            .collect::<BTreeMap<_, _>>();
        for table in ability_spawns::tables(manager, own, &own_payload).unwrap() {
            if let Some(&copy_table) = tables.get(&place(&table)) {
                table_twins(
                    manager,
                    (table.graph, copy_table),
                    &mut BTreeSet::new(),
                    &mut pairs,
                );
            }
        }
        for (own, copy) in pairs {
            if twins.insert(own, copy).is_none() {
                queue.push_back((own, copy, depth + 1));
            }
        }
    }
    twins
}

/// The graphs of a stock impact table paired with those of its twin, by the fields naming them,
/// through the tables they name.
fn table_twins(
    manager: &PackageManager,
    (own, copy): (u32, u32),
    seen: &mut BTreeSet<u32>,
    pairs: &mut Vec<(u32, u32)>,
) {
    if !seen.insert(own) {
        return;
    }
    let named = ability_spawns::table_entries(manager, copy)
        .unwrap()
        .into_iter()
        .map(|(tag, offsets)| (offsets, tag))
        .collect::<BTreeMap<_, _>>();
    for (tag, offsets) in ability_spawns::table_entries(manager, own).unwrap() {
        let Some(&twin) = named.get(&offsets) else {
            continue;
        };
        if ability_spawns::is_table(manager, tag) {
            table_twins(manager, (tag, twin), seen, pairs);
        } else {
            pairs.push((tag, twin));
        }
    }
}

/// The authored grenade's tint read back: at every place the stock grenade's effects draw the
/// stock color, the copy draws a private material whose constant holds the edited color, read
/// from the material or the constant buffer it names. Returns what `readback.json` records of
/// it.
fn read_back_tint(
    manager: &PackageManager,
    (stock, copied): (u32, u32),
    edit: TintEdit,
    name: &str,
) -> serde_json::Value {
    let stock_tints =
        ability_tint::ability_tints(manager, stock, crate::subclass::SPAWN_DEPTH).unwrap();
    let original = stock_tints
        .iter()
        .find(|tint| edit.starts_from(tint.rgb))
        .unwrap_or_else(|| panic!("the stock grenade still draws with its tint"));
    let expected = edit.apply(original.rgb);
    let twins = twins(manager, stock, copied);
    let read = |tag: u32| manager.read_tag(tiger_pkg::TagHash(tag)).unwrap();
    let float = |payload: &[u8], at: usize| {
        f32::from_le_bytes(payload[at..at + 4].try_into().unwrap()).to_bits()
    };
    for tint_use in &original.uses {
        let graph = *twins.get(&tint_use.graph).unwrap_or_else(|| {
            panic!("{name}'s copy has a twin of graph 0x{:08X}", tint_use.graph)
        });
        let site = ability_palette::particle_sites(manager, &read(graph))
            .unwrap()
            .into_iter()
            .find(|site| {
                (site.binding_hash, site.resource_index, site.offset)
                    == (
                        tint_use.site.binding_hash,
                        tint_use.site.resource_index,
                        tint_use.site.offset,
                    )
            })
            .unwrap_or_else(|| panic!("{name}'s graph 0x{graph:08X} keeps the tinted effect"));
        let system = read(site.system);
        let material = u32::from_le_bytes(system[0x14..0x18].try_into().unwrap());
        assert_ne!(
            material, tint_use.material,
            "{name}'s copy draws the tinted effect with a private material"
        );
        let material = read(material);
        let (payload, at) = match tint_use.store {
            ability_tint::ConstantStore::Inline => (material, tint_use.constant.offset),
            ability_tint::ConstantStore::External { .. } => {
                let header = u32::from_le_bytes(material[0x34C..0x350].try_into().unwrap());
                let data = manager
                    .get_entry(tiger_pkg::TagHash(header))
                    .unwrap_or_else(|| panic!("constant buffer 0x{header:08X} is live"))
                    .reference;
                (read(data), tint_use.constant.offset)
            }
        };
        let found = [0, 4, 8].map(|lane| float(&payload, at + lane));
        assert_eq!(
            found,
            expected.map(f32::to_bits),
            "{name}'s copy holds the recolored tint where the stock one was"
        );
    }
    serde_json::json!({
        "stock": original.rgb,
        "copy": expected,
        "uses": original.uses.len(),
        "hue": edit.hue,
        "colorize": edit.colorize,
    })
}

/// The grade the first grenade gives every effect: green, as Colorize sets it.
const GRADE: EffectGrade = EffectGrade {
    hue: 120,
    colorize: true,
    ..EffectGrade::STOCK
};

/// The pixel programs ability `entity`'s effects draw with, each once: those of its particle
/// systems' materials and of the materials its models, lights and other resources name.
fn effect_programs(manager: &PackageManager, entity: u32) -> BTreeSet<u32> {
    let mut materials = BTreeSet::new();
    for (graph, payload) in
        ability_palette::ability_graphs(manager, entity, crate::subclass::SPAWN_DEPTH).unwrap()
    {
        for site in ability_palette::particle_sites(manager, &payload).unwrap() {
            let system = manager.read_tag(tiger_pkg::TagHash(site.system)).unwrap();
            materials.insert(u32::from_le_bytes(system[0x14..0x18].try_into().unwrap()));
        }
        for route in material_routes(manager, graph, &payload).unwrap() {
            materials.insert(route.material());
        }
    }
    materials
        .into_iter()
        .filter(|material| {
            manager
                .get_entry(tiger_pkg::TagHash(*material))
                .is_some_and(|entry| entry.reference == ability_palette::MATERIAL_CLASS)
        })
        .map(|material| {
            let material = manager.read_tag(tiger_pkg::TagHash(material)).unwrap();
            u32::from_le_bytes(material[0x2C8..0x2CC].try_into().unwrap())
        })
        .collect()
}

/// A pixel program's bytecode, which its header names by package reference.
fn program_code(manager: &PackageManager, program: u32) -> Option<Vec<u8>> {
    let data = manager
        .get_entry(tiger_pkg::TagHash(program))
        .filter(|entry| entry.file_type == 33)?
        .reference;
    manager.read_tag(tiger_pkg::TagHash(data)).ok()
}

/// A grenade's grade read back: its copy's effects draw with none of the pixel programs the grade
/// applies to among those of `stocks`, the stock grenade and any projectile its copy fires in
/// place of a stock one, and with private ones that read as valid SM5 programs and write the
/// grade's color. Returns what `readback.json` records of it.
fn read_back_grade(
    manager: &PackageManager,
    (stocks, copied): (&[u32], u32),
    name: &str,
) -> serde_json::Value {
    let program = GRADE.program();
    let crate::dxbc::grade::Grade::Colorize(color) = program else {
        unreachable!("the test grade colorizes");
    };
    let stock_programs = stocks
        .iter()
        .flat_map(|stock| effect_programs(manager, *stock))
        .collect::<BTreeSet<_>>();
    let gradable = stock_programs
        .iter()
        .copied()
        .filter(|tag| {
            program_code(manager, *tag)
                .is_some_and(|code| crate::dxbc::grade::grade(&code, program).unwrap().is_some())
        })
        .collect::<BTreeSet<_>>();
    assert!(
        !gradable.is_empty(),
        "{name}'s stock effects draw with a program the grade applies to"
    );
    let copy_programs = effect_programs(manager, copied);
    assert!(
        copy_programs.is_disjoint(&gradable),
        "{name}'s copy draws with no stock program the grade applies to"
    );
    let private = copy_programs
        .difference(&stock_programs)
        .copied()
        .collect::<Vec<_>>();
    // A material that multiplies its color takes only the hue, written as one minus it.
    let crate::dxbc::grade::Grade::Hue { hue, .. } = GRADE.neutral_program() else {
        unreachable!("the test grade colorizes");
    };
    let literal = |channels: [f32; 3]| {
        channels
            .iter()
            .map(|channel| channel.to_bits())
            .chain([0])
            .collect::<Vec<_>>()
    };
    let words = [literal(color), literal(hue.map(|channel| 1.0 - channel))];
    for tag in &private {
        let code = program_code(manager, *tag)
            .unwrap_or_else(|| panic!("{name}'s private program 0x{tag:08X} names its bytecode"));
        crate::dxbc::Program::read(&code)
            .unwrap_or_else(|error| panic!("private program 0x{tag:08X}: {error}"));
        let tokens = code
            .chunks_exact(4)
            .map(|word| u32::from_le_bytes(word.try_into().unwrap()))
            .collect::<Vec<_>>();
        assert!(
            words
                .iter()
                .any(|words| tokens.windows(words.len()).any(|window| window == words)),
            "private program 0x{tag:08X} writes the grade's color"
        );
    }
    assert!(
        !private.is_empty(),
        "{name}'s copy draws with private graded programs"
    );
    serde_json::json!({
        "stock_programs": stock_programs.len(),
        "gradable": gradable.len(),
        "private_programs": private.len(),
        "hue": GRADE.hue,
        "colorize": GRADE.colorize,
    })
}

/// The authored grenade's effect colors read back: its copy draws every use of the stock palette
/// with a private palette of the taken palette's colors, edited, and the stock entity still draws
/// the stock one. Returns what `readback.json` records of it.
fn read_back_palette(
    manager: &PackageManager,
    (stock, copied): (u32, u32),
    edit: PaletteEdit,
    name: &str,
) -> serde_json::Value {
    let stock_palettes =
        ability_palette::ability_palettes(manager, stock, crate::subclass::SPAWN_DEPTH).unwrap();
    let copy_palettes =
        ability_palette::ability_palettes(manager, copied, crate::subclass::SPAWN_DEPTH).unwrap();
    let original = stock_palettes
        .iter()
        .find(|palette| palette.header == edit.palette)
        .unwrap_or_else(|| panic!("the stock grenade still draws with 0x{:08X}", edit.palette));
    let pixels = ability_palette::palette_pixels(manager, edit.palette).unwrap();
    let mut expected = ability_palette::palette_pixels(manager, edit.source()).unwrap();
    edit.apply(&mut expected);
    assert_ne!(expected, pixels, "the edit changes the palette's colors");
    assert!(
        copy_palettes
            .iter()
            .all(|palette| palette.header != edit.palette),
        "{name}'s copy no longer draws with the stock palette"
    );
    let recolored = copy_palettes
        .iter()
        .find(|palette| {
            ability_palette::palette_pixels(manager, palette.header).unwrap() == expected
        })
        .unwrap_or_else(|| panic!("{name}'s copy draws with the recolored palette"));
    assert_eq!(
        recolored.uses.len(),
        original.uses.len(),
        "{name}'s copy reaches the recolored palette everywhere the stock one was"
    );
    assert!(
        recolored
            .uses
            .iter()
            .all(|each| original.uses.iter().all(|stock| {
                stock.material != each.material && stock.site.system != each.site.system
            })),
        "{name}'s copy draws through private materials and particle systems"
    );
    serde_json::json!({
        "stock_palette": format!("0x{:08X}", edit.palette),
        "colors_from": edit.from.map(|from| format!("0x{from:08X}")),
        "copy_palette": format!("0x{:08X}", recolored.header),
        "uses": recolored.uses.len(),
        "hue": edit.hue,
        "saturation": edit.saturation,
        "brightness": edit.brightness,
    })
}

/// The name the Spawns tab gives `graph` among the graphs `entity` spawns, read the way the page
/// loads it.
fn spawn_name(packages: &Path, entity: u32, graph: u32) -> String {
    let manager = open_shadowkeep_package_manager(packages).unwrap();
    let payload = manager.read_tag(tiger_pkg::TagHash(entity)).unwrap();
    let graphs = ability_spawns::reached_graphs(&manager, entity, &payload).unwrap();
    let objects = sundial::package_authoring::sandbox_perk::entity::catalog::cached_only(packages)
        .ok()
        .flatten();
    let names = ability_spawns::names(&manager, &graphs, objects.as_deref());
    graphs
        .iter()
        .position(|each| *each == graph)
        .map(|index| names[index].clone())
        .unwrap_or_else(|| panic!("ability entity 0x{entity:08X} does not spawn 0x{graph:08X}"))
}

/// A graph's first 32-bit float outside any ability bank, doubled: a named one when it has one,
/// since some abilities, such as Skip Grenade, declare every value natively and name none.
fn graph_value(manager: &PackageManager, entity: u32) -> Option<AbilityValue> {
    let payload = manager.read_tag(tiger_pkg::TagHash(entity)).ok()?;
    let mut graph = load_weapon_runtime_graph_for_entity(manager, 0, 0, entity, &payload).ok()?;
    graph.scope_fields();
    // The page leaves the ability's bank out, since the build keeps banks stock.
    graph
        .resources
        .retain(|resource| !ability_modifier::is_bank(resource.owner_tag));
    graph
        .owners
        .retain(|owner| !ability_modifier::is_bank(owner.owner_tag));
    let floats = graph
        .resources
        .iter()
        .flat_map(|resource| std::iter::once(&resource.instance).chain(resource.definition.iter()))
        .chain(graph.owners.iter().flat_map(|owner| &owner.roots))
        .flat_map(|root| &root.fields)
        .filter_map(|field| match field.value {
            WeaponRuntimeValue::Float32Bits(bits)
                if field.kind == WeaponRuntimeValueKind::Float32
                    && f32::from_bits(bits).is_normal()
                    && f32::from_bits(bits).abs() < 1.0e30 =>
            {
                Some((field, f32::from_bits(bits)))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let (field, value) = floats
        .iter()
        .find(|(field, _)| field.source == WeaponRuntimeFieldSource::GeneratedSchema)
        .or(floats.first())
        .copied()?;
    Some(AbilityValue {
        entity,
        name: field.name.clone(),
        stock: field.value.clone(),
        edit: WeaponRuntimeValueOverride {
            locator: field.locator.clone(),
            value: WeaponRuntimeValue::Float32Bits((value * 2.0).to_bits()),
        },
    })
}

/// The investment globals, which lead to each finished perk's runtime action.
fn investment_globals(manager: &PackageManager) -> Vec<u8> {
    let globals = resolve_live_named_tag(manager, "investment_globals", None).unwrap();
    manager.read_tag(globals).unwrap()
}

/// The ability entities a perk's On a Specific Ability and Ends on a Specific Ability conditions
/// name. A row with no action names none.
fn named_abilities(manager: &PackageManager, globals: &[u8], perk: u16) -> Vec<u32> {
    match load_sandbox_perk_runtime_action(manager, globals, usize::from(perk)) {
        Ok(action) => ability_reference::references(&action.action_payload)
            .unwrap_or_else(|error| panic!("perk {perk}: {error}"))
            .into_iter()
            .map(|(_, entity)| entity)
            .collect(),
        Err(error) if error.contains("is not assigned") => Vec::new(),
        Err(error) => panic!("perk {perk}: {error}"),
    }
}

/// Checks the perks an authored entry grants against the `stock` perks it starts from: each the
/// same, except that one naming the copied ability's stock entity is replaced where it stands by
/// a private copy naming the copy. Returns each replaced perk and its copy.
fn check_retargeted(
    (manager, globals): (&PackageManager, &[u8]),
    (stock, granted): (&[u16], &[u16]),
    (entity, copy): (u32, u32),
    what: &str,
) -> Vec<(u16, u16)> {
    assert_eq!(
        granted.len(),
        stock.len(),
        "{what} grants as many of these perks as it starts with: {granted:?} for {stock:?}"
    );
    let mut replaced = Vec::new();
    for (&from, &to) in stock.iter().zip(granted) {
        if !named_abilities(manager, globals, from).contains(&entity) {
            assert_eq!(to, from, "{what} keeps perk {from}");
            continue;
        }
        let names = named_abilities(manager, globals, to);
        assert!(
            to != from && names.contains(&copy) && !names.contains(&entity),
            "{what} replaces perk {from}, which names 0x{entity:08X}, with a copy naming 0x{copy:08X}, but perk {to} names {names:X?}"
        );
        replaced.push((from, to));
    }
    replaced
}

/// A stock subclass one of whose grenades a sandbox perk names by the entity its pool equips,
/// that grenade and entity, the perk, and the entries the scenario adds the perk to.
struct RetargetBase {
    base: SubclassSummary,
    grenade: u8,
    entity: u32,
    perk: u16,
    holders: Vec<u8>,
}

/// A subclass on such a base whose grenade has a value of its own, and whose grenade and second
/// middle node add the perk that names it.
struct AuthoredRetarget {
    recipe: WeaponRecipe,
    target: RetargetBase,
    value: AbilityValue,
}

/// The first sandbox perk that names each ability entity in an On a Specific Ability or Ends on a
/// Specific Ability condition, across every perk row with a runtime action of its own.
fn perks_naming_abilities(manager: &PackageManager, globals: &[u8]) -> BTreeMap<u32, u16> {
    use sundial::package_authoring::investment_schema::{
        GLOBALS_FINISHED_SANDBOX_PERK_TABLE_SLOT, investment_globals_table_tag,
    };
    let tag =
        investment_globals_table_tag(globals, GLOBALS_FINISHED_SANDBOX_PERK_TABLE_SLOT).unwrap();
    let catalog = manager.read_tag(tiger_pkg::TagHash(tag)).unwrap();
    let count =
        sundial::package_authoring::sandbox_perk::finished_sandbox_perk_count(&catalog).unwrap();
    let mut naming = BTreeMap::new();
    for perk in 0..count {
        // Rows with no runtime action of their own name nothing.
        let Ok(action) = load_sandbox_perk_runtime_action(manager, globals, perk) else {
            continue;
        };
        let perk = u16::try_from(perk).unwrap();
        for (_, entity) in ability_reference::references(&action.action_payload).unwrap() {
            naming.entry(entity).or_insert(perk);
        }
    }
    naming
}

/// The first stock subclass one of whose grenades a sandbox perk names, and the entries the
/// scenario adds that perk to: the grenade and its second middle node. No stock subclass's own
/// perks name one of its abilities, so the perk comes from elsewhere, as Sunbracers' Helium
/// Spirals names Solar Grenade. None found means stock conditions do not name abilities by the
/// entities the ability tables lead to.
fn find_retarget(packages: &Path, subclasses: &[SubclassSummary]) -> RetargetBase {
    let manager = open_shadowkeep_package_manager(packages).unwrap();
    let globals = investment_globals(&manager);
    let naming = perks_naming_abilities(&manager, &globals);
    subclasses
        .iter()
        .find_map(|subclass| {
            layout::GRENADES.into_iter().find_map(|grenade| {
                let entity = *subclass.entry_entities.get(&grenade)?;
                let perk = *naming.get(&entity)?;
                Some(RetargetBase {
                    base: subclass.clone(),
                    grenade,
                    entity,
                    perk,
                    holders: vec![grenade, AttunementPath::Middle.entries()[1]],
                })
            })
        })
        .expect("a sandbox perk names a stock subclass's grenade by the entity it equips")
}

/// A new subclass on the retarget base, whose grenade takes one value of its own and whose grenade
/// and second middle node add the perk that names it, through the recipe, as typing does.
fn author_retarget(ctx: &egui::Context, app: &mut PackageAuthoringApp) -> AuthoredRetarget {
    let target = find_retarget(&app.packages, &app.subclasses);
    new_from_menu(ctx, app, ItemKind::Subclass);
    app.recipe
        .rename_authored_item("Parhelion Retarget Subclass")
        .unwrap();
    app.recipe
        .set_donor(target.base.hash, target.base.name.clone());
    let grenade = Place::Ability(target.grenade);
    let node = Place::Node(AttunementPath::Middle, 1);
    let value = ability_value(&app.packages, target.entity);
    let mut abilities = SubclassAbilities::default();
    let mut edits = abilities.edits(target.base.hash, grenade);
    edits.ability_values = vec![value.edit.clone()];
    edits.added_perks = vec![target.perk];
    abilities.set_edits(target.base.hash, grenade, edits);
    let mut edits = abilities.edits(target.base.hash, node);
    edits.added_perks = vec![target.perk];
    abilities.set_edits(target.base.hash, node, edits);
    app.recipe.overrides.subclass_abilities = Some(abilities);
    settle_icons(ctx, app);
    app.recipe_baseline = app.recipe.clone();
    app.recipe_dirty = false;
    AuthoredRetarget {
        recipe: app.recipe.clone(),
        target,
        value,
    }
}

/// The retarget subclass's grenade names a copy of its entity, and every entry grants the perks it
/// starts with, its stock ones and the added one, with each naming the grenade replaced by a
/// private copy naming the copy. The stock subclass keeps its perks.
fn read_back_retarget(
    staged: &InvestmentCatalog,
    build: &BuildReport,
    authored: &AuthoredRetarget,
    packages: &Path,
) -> serde_json::Value {
    let name = &authored.recipe.name;
    let target = &authored.target;
    let report = build
        .weapons
        .iter()
        .find(|report| report.name == *name)
        .unwrap();
    let subclasses = staged.subclasses(|_| true);
    let own = subclasses
        .iter()
        .find(|subclass| subclass.hash == report.item_hash)
        .unwrap_or_else(|| panic!("{name} reads back as a subclass"));
    let stock = subclasses
        .iter()
        .find(|subclass| subclass.hash == target.base.hash)
        .expect("the retarget base stays installed");
    assert_eq!(
        own.class_type, target.base.class_type,
        "{name} keeps its class"
    );
    let copy = *own
        .entry_entities
        .get(&target.grenade)
        .unwrap_or_else(|| panic!("{name}'s grenade names an entity"));
    assert_ne!(
        copy, target.entity,
        "{name}'s grenade names a copy of its entity"
    );
    assert_eq!(
        stock.entry_entities.get(&target.grenade),
        Some(&target.entity),
        "the stock grenade keeps its entity"
    );
    let manager = open_shadowkeep_package_manager(packages).unwrap();
    let globals = investment_globals(&manager);
    let mut replaced = Vec::new();
    let entries = target
        .base
        .entry_perks
        .keys()
        .chain(&target.holders)
        .copied()
        .collect::<BTreeSet<_>>();
    for entry in entries {
        let mut perks = target
            .base
            .entry_perks
            .get(&entry)
            .cloned()
            .unwrap_or_default();
        assert_eq!(
            stock.entry_perks.get(&entry).cloned().unwrap_or_default(),
            perks,
            "the stock subclass keeps entry {entry}'s perks"
        );
        if target.holders.contains(&entry) {
            perks.push(target.perk);
        }
        let granted = own.entry_perks.get(&entry).cloned().unwrap_or_default();
        replaced.extend(
            check_retargeted(
                (&manager, &globals),
                (&perks, &granted),
                (target.entity, copy),
                &format!("{name}'s entry {entry}"),
            )
            .into_iter()
            .map(|(from, to)| (entry, from, to)),
        );
    }
    let mut expected = target
        .holders
        .iter()
        .map(|entry| (*entry, target.perk))
        .collect::<Vec<_>>();
    expected.sort_unstable();
    let mut found = replaced
        .iter()
        .map(|(entry, from, _)| (*entry, *from))
        .collect::<Vec<_>>();
    found.sort_unstable();
    assert_eq!(
        found, expected,
        "{name} replaces every perk that names its grenade"
    );
    serde_json::json!({
        "kind": ItemKind::Subclass,
        "name": name,
        "item_hash": format!("0x{:08X}", report.item_hash),
        "base": format!("{} 0x{:08X}", target.base.name, target.base.hash),
        "grenade": target.base.entry_names.get(&target.grenade),
        "value": authored.value.name,
        "naming_perk": target.perk,
        "stock_entity": format!("0x{:08X}", target.entity),
        "copy_entity": format!("0x{copy:08X}"),
        "retargeted": replaced
            .iter()
            .map(|(entry, from, to)| serde_json::json!({
                "entry": entry,
                "stock_perk": from,
                "private_perk": to,
            }))
            .collect::<Vec<_>>(),
    })
}

/// The authored middle path's name, and its second node's.
const PATH_NAME: &str = "Way of the Sundial";
const NODE_NAME: &str = "Sundial Strike";
/// The authored second grenade's name and description, and its custom perk's name.
const ABILITY_NAME: &str = "Sundial Burst";
const ABILITY_DESCRIPTION: &str = "Authored in Parhelion.";
const CUSTOM_PERK_NAME: &str = "Sundial Spark";
/// The authored grenade takes the icon of this entry of the top attunement's source: its lead.
const ABILITY_ICON_ENTRY: u8 = AttunementPath::Top.entries()[0];

/// An ability's or node's section tab, marked or not.
fn section_tab(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
    let marked = format!("{label} •");
    find(output, label, |text, _| text == label || text == marked)
}

/// Clicks `choice` on the line of the detail panel's choices that `subclass` leads. An ability's or
/// node's choices open over the page from its Based On button and close on a pick. An attunement
/// lists its own.
fn choose(ctx: &egui::Context, app: &mut PackageAuthoringApp, subclass: &str, choice: &str) {
    let output = settle(ctx, app);
    if let Some(row) = accessible(&output, "Change Based On") {
        click(ctx, app, row.center());
        assert_eq!(
            app.subclass_page.choosing,
            Some(app.subclass_page.selection),
            "the Based On row opens its choices"
        );
    }
    let output = settle(ctx, app);
    let line = find(&output, subclass, |text, _| text == subclass);
    click(
        ctx,
        app,
        find(&output, choice, |text, rect| {
            text == choice && (rect.center().y - line.y).abs() < 12.0
        }),
    );
}

/// Runs frames until every stock subclass node's icon has loaded, since package icons load on a
/// worker, so a capture shows them.
fn settle_icons(ctx: &egui::Context, app: &mut PackageAuthoringApp) -> egui::FullOutput {
    let start = Instant::now();
    loop {
        frame(ctx, app, Vec::new());
        let catalog = app.catalog.as_ref().unwrap();
        let pending = app
            .subclasses
            .iter()
            .flat_map(|subclass| subclass.entry_icons.values())
            .any(|&container| catalog.subclass_icon(ctx, container).is_none());
        if !pending || start.elapsed() > Duration::from_secs(30) {
            return settle(ctx, app);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The gear page alone in a window `width` wide.
fn frame_at(ctx: &egui::Context, app: &mut PackageAuthoringApp, width: f32) -> egui::FullOutput {
    let mut output = None;
    for _ in 0..2 {
        let frame = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 1400.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    workbench_style(ui);
                    app.draw_gear_editor(ui);
                });
            },
        );
        capture::record(&frame);
        output = Some(frame);
    }
    output.unwrap()
}

#[allow(
    clippy::cognitive_complexity,
    reason = "End-to-end verification keeps the ordered workflow and its independent assertions together"
)]
fn author_subclass(ctx: &egui::Context, app: &mut PackageAuthoringApp) -> AuthoredSubclass {
    new_from_menu(ctx, app, ItemKind::Subclass);
    app.recipe
        .rename_authored_item("Parhelion Test Subclass")
        .unwrap();
    let hash = app.recipe.donor.item_hash.parse_u32().unwrap();
    let base = app
        .subclasses
        .iter()
        .find(|subclass| subclass.hash == hash)
        .expect("the New menu starts on a stock subclass")
        .clone();
    // The catalog reads each node's icon and description from its display record.
    assert!(
        !base.entry_icons.is_empty() && !base.entry_descriptions.is_empty(),
        "{} has no node icons or descriptions",
        base.name
    );
    assert_page_fits(ctx, app, "Subclass");
    let output = settle_icons(ctx, app);
    for label in [
        "Abilities",
        "Attunements",
        "Class Ability",
        "Movement",
        "Grenade",
        "Super",
        "Top",
        "Middle",
        "Restore Base Abilities",
    ] {
        find(&output, label, |text, _| text == label);
    }
    for absent in ["Rarity", "Perks & Sockets", "Stats"] {
        assert!(
            !texts(&output).iter().any(|(text, _)| text == absent),
            "a subclass page has no {absent}"
        );
    }
    capture::write(ctx, &output, "gear-subclass");
    // A new subclass is for every class, which its Class picker shows and the manifest carries
    // to the install.
    find(&output, "the Class picker", |text, _| text == "Any Class");
    assert!(
        app.recipe.overrides.subclass_every_class,
        "a new subclass is for every class"
    );

    // A grenade from another class: its row shows it beside the list, where its subclass's line
    // offers it. Its bank takes the authored node's charge and names a graph of its own, whose
    // value gives the copy a private bank, and an impact table names the only graphs drawing one
    // of its palettes, whose recolor gives the copy a private table.
    let entry = layout::GRENADES[0];
    let grenade = {
        let manager = open_shadowkeep_package_manager(&app.packages).unwrap();
        let catalog = app.catalog.as_ref().unwrap();
        app.subclasses
            .iter()
            .find(|subclass| {
                subclass.class_type != base.class_type
                    && subclass
                        .entry_rows
                        .get(&entry)
                        .and_then(|row| catalog.ability_row(*row))
                        .is_some_and(|row| row.charges)
                    && subclass.entry_entities.get(&entry).is_some_and(|entity| {
                        bank_value(&manager, *entity).is_some()
                            && table_palette(&manager, *entity).is_some()
                    })
            })
            .expect(
                "a subclass of another class whose grenade's bank names a graph of its own and \
                 whose impact tables name the only graphs drawing a palette",
            )
            .clone()
    };
    let row = base.entry_names[&entry].as_str();
    let output = settle(ctx, app);
    click(ctx, app, find(&output, row, |text, _| text == row));
    assert_eq!(
        app.subclass_page.selection,
        SubclassSelection::Entry(Place::Ability(entry))
    );
    choose(ctx, app, &grenade.name, &grenade.entry_names[&entry]);
    let abilities = app.recipe.overrides.subclass_abilities.clone().unwrap();
    assert_eq!(
        abilities
            .choice(entry)
            .map(|choice| (choice.source, choice.source_entry)),
        Some((grenade.hash, entry)),
        "the page took the grenade from {}",
        grenade.name
    );
    // An attunement from the third class, where a bottom one can fill the top place.
    let attunement = app
        .subclasses
        .iter()
        .find(|subclass| {
            subclass.class_type != base.class_type && subclass.class_type != grenade.class_type
        })
        .expect("a subclass of the third class")
        .clone();
    let top = base.attunement_names[AttunementPath::Top.index()].as_str();
    let output = settle(ctx, app);
    click(ctx, app, find(&output, top, |text, _| text == top));
    assert_eq!(
        app.subclass_page.selection,
        SubclassSelection::Path(AttunementPath::Top)
    );
    choose(
        ctx,
        app,
        &attunement.name,
        &attunement.attunement_names[AttunementPath::Bottom.index()],
    );
    let abilities = app.recipe.overrides.subclass_abilities.clone().unwrap();
    assert_eq!(
        abilities
            .attunement(AttunementPath::Top)
            .map(|choice| (choice.source, choice.source_path)),
        Some((attunement.hash, AttunementPath::Bottom)),
        "the page took the top attunement from {}",
        attunement.name
    );
    // The middle path of its own: renamed, with its second node from the base's bottom path
    // under its own name and description, and with one more of the base's perks.
    let from_node = AttunementPath::Bottom.entries()[1];
    let node_perks = base
        .entry_perks
        .get(&from_node)
        .cloned()
        .unwrap_or_default();
    let extra_perk = base
        .entry_perks
        .values()
        .flatten()
        .copied()
        .find(|perk| !node_perks.contains(perk))
        .expect("the base subclass has another perk");
    let mut abilities = app.recipe.overrides.subclass_abilities.clone().unwrap();
    abilities.set_path_name(
        AttunementPath::Middle,
        base.hash,
        Some(PATH_NAME.to_owned()),
    );
    let mut node = SubclassPathNode::stock(1, base.hash, AttunementPath::Bottom, 1);
    node.edits.name = Some(NODE_NAME.to_owned());
    node.edits.description = Some("Built node by node in Parhelion.".to_owned());
    node.edits.added_perks = vec![extra_perk];
    node.edits.modifiers = vec![AbilityModifier {
        target: layout::GRENADES[0],
        effect: ModifierEffect::Charges { count: 1 },
    }];
    abilities.set_path_node(AttunementPath::Middle, base.hash, node);
    app.recipe.overrides.subclass_abilities = Some(abilities);
    // Restore sits in the header's menu, named for what the header shows.
    let output = settle(ctx, app);
    let shown = &attunement.attunement_names[AttunementPath::Bottom.index()];
    let menu = accessible(&output, &format!("More {shown} Options"))
        .expect("the top attunement's header has a menu");
    click(ctx, app, menu.center());
    let output = settle(ctx, app);
    let restore = format!("Restore {top}");
    find(&output, "the top attunement's restore", |text, _| {
        text == restore
    });
    click(ctx, app, menu.center());
    let output = settle(ctx, app);
    // The Middle tab, marked for its edits, shows the middle attunement and its nodes.
    click(
        ctx,
        app,
        find(&output, "the Middle tab", |text, _| text == "Middle •"),
    );
    assert_eq!(
        app.subclass_page.selection,
        SubclassSelection::Path(AttunementPath::Middle)
    );
    let output = settle(ctx, app);
    find(&output, "the middle path's name", |text, _| {
        text == PATH_NAME
    });
    let output = settle_icons(ctx, app);
    capture::write(ctx, &output, "gear-subclass-edited");
    // The authored node shows beside the list with its own name, description and perks.
    click(
        ctx,
        app,
        find(&output, "the authored node", |text, _| text == NODE_NAME),
    );
    assert_eq!(
        app.subclass_page.selection,
        SubclassSelection::Entry(Place::Node(AttunementPath::Middle, 1))
    );
    // Its Ability section holds its text under a header naming what it is based on. Its Perks
    // tab holds the added perk, and its Gameplay tab the added modifier among its Ability
    // Changes.
    let output = settle_icons(ctx, app);
    for text in ["Middle Path · Node 2", "Description", "Based On"] {
        find(&output, text, |drawn, _| drawn == text);
    }
    click(ctx, app, section_tab(&output, "Perks"));
    let output = settle_icons(ctx, app);
    for text in ["Perks", "Add Perk"] {
        find(&output, text, |drawn, _| drawn == text);
    }
    capture::write(ctx, &output, "gear-subclass-path");
    click(ctx, app, section_tab(&output, "Gameplay"));
    let output = settle_icons(ctx, app);
    for text in ["Ability Changes", "Add Change"] {
        find(&output, text, |drawn, _| drawn == text);
    }
    capture::write(ctx, &output, "gear-subclass-path-gameplay");
    // Add Change opens its form under the node's Ability Changes chips, which Cancel closes.
    click(
        ctx,
        app,
        find(&output, "Add Change", |text, _| text == "Add Change"),
    );
    let output = settle(ctx, app);
    for text in ["Add", "Ability", "Change", "Cancel"] {
        find(&output, text, |drawn, _| drawn == text);
    }
    capture::write(ctx, &output, "gear-subclass-add-modifier");
    click(
        ctx,
        app,
        find(&output, "Cancel", |text, _| text == "Cancel"),
    );
    let output = settle_icons(ctx, app);
    assert!(
        !app.subclass_page.adding_modifier(),
        "Cancel closes the Add Change form"
    );
    // Hovering a row shows the house tooltip: the ability's icon, its slot and subclass, and its
    // node's own description.
    let grenade_name = grenade.entry_names[&entry].as_str();
    let row = find(&output, "the taken grenade's row", |text, _| {
        text == grenade_name
    });
    frame(ctx, app, vec![egui::Event::PointerMoved(row)]);
    let mut output = frame(ctx, app, Vec::new());
    for _ in 0..40 {
        output = frame(ctx, app, Vec::new());
    }
    let subtitle = format!("Grenade · {}", grenade.name);
    find(&output, "the tooltip's slot and subclass", |text, _| {
        text == subtitle
    });
    let description = grenade
        .entry_descriptions
        .get(&entry)
        .expect("the taken grenade has a description");
    find(&output, "the tooltip's description", |text, _| {
        text == description.as_str()
    });
    capture::write(ctx, &output, "gear-subclass-tooltip");
    frame(ctx, app, vec![egui::Event::PointerGone]);
    let (custom_effect, (ability_value, spawn_value), parameter, (palette, tint)) =
        author_ability(ctx, app, (&base, &attunement), extra_perk);
    let (bank_value, swap, table_palette) = author_bank_and_swap(app, (&base, &grenade));
    let (movement, jump_row) = author_movement(ctx, app, &base);
    // A narrow window stacks the detail under the list.
    capture::write(ctx, &frame_at(ctx, app, 640.0), "gear-subclass-narrow");
    author_screen_art(ctx, app, &attunement);
    app.recipe_baseline = app.recipe.clone();
    app.recipe_dirty = false;
    AuthoredSubclass {
        recipe: app.recipe.clone(),
        base,
        grenade,
        attunement,
        extra_perk,
        custom_effect,
        ability_value,
        parameter,
        spawn_value,
        palette,
        tint,
        bank_value,
        table_palette,
        swap,
        movement,
        jump_row,
    }
}

/// The color the Screen Art test paints the top attunement's picture with.
const SCREEN_ART_COLOR: [u8; 4] = [255, 0, 255, 255];

/// Gives the subclass screen art of its own, as the Appearance tab writes it: a picture for the
/// top attunement and the attunement subclass's top picture for the middle one, the bottom kept.
/// The tab shows a tile for each attunement.
fn author_screen_art(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    attunement: &SubclassSummary,
) {
    let picture = crate::image_import::EmbeddedImage::from_rgba(image::RgbaImage::from_pixel(
        32,
        32,
        image::Rgba(SCREEN_ART_COLOR),
    ))
    .unwrap();
    app.recipe.overrides.screen_art = Some(ScreenArt {
        top: Some(ArtImage::Image { image: picture }),
        bottom: None,
        middle: Some(ArtImage::Subclass {
            item_hash: attunement.hash.into(),
            part: ArtPart::Top,
        }),
    });
    // The subclass's pages include Appearance, which the tab bar opens.
    assert!(WorkbenchPage::for_kind(ItemKind::Subclass).contains(&WorkbenchPage::Appearance));
    app.workbench_page = WorkbenchPage::Appearance;
    let start = Instant::now();
    let output = loop {
        let output = settle(ctx, app);
        // The base's bottom picture loads on a worker.
        if accessible(&output, "Bottom").is_some() && app.subclass_page_art_loaded() {
            break output;
        }
        assert!(
            start.elapsed() < Duration::from_secs(120),
            "the screen art pictures did not load: {:?}",
            texts(&output)
                .iter()
                .map(|(text, _)| text)
                .collect::<Vec<_>>()
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    for text in ["Screen Art", "Top", "Bottom", "Middle", "Picture", "Base"] {
        find(&output, text, |drawn, _| drawn == text);
    }
    capture::write(ctx, &output, "gear-subclass-screen-art");
    app.workbench_page = WorkbenchPage::Weapon;
}

/// The authored subclass's screen art read back: its strings name a container of its own, whose
/// top picture is the painted one, whose bottom is the base's own and whose middle is the
/// attunement subclass's top picture. Returns what `readback.json` records of it.
fn read_back_screen_art(
    staged: &InvestmentCatalog,
    (item, subclass): (u32, &AuthoredSubclass),
    packages: &Path,
) -> serde_json::Value {
    let name = &subclass.recipe.name;
    let manager = open_shadowkeep_package_manager(packages).unwrap();
    let pictures = |container: u32| {
        let payload =
            crate::icon_edit::read_icon_container(&manager, tiger_pkg::TagHash(container)).unwrap();
        let layer = tiger_pkg::TagHash(u32::from_le_bytes(payload[0x14..0x18].try_into().unwrap()));
        let layer_payload = manager.read_tag(layer).unwrap();
        crate::icon_edit::texture_reference_offsets(&layer_payload, layer)
            .unwrap()
            .into_iter()
            .map(|(_, header)| header)
            .collect::<Vec<_>>()
    };
    let container = staged
        .nameplate_container(item)
        .unwrap_or_else(|| panic!("{name} names screen art in its strings"));
    let base_container = staged.nameplate_container(subclass.base.hash).unwrap();
    assert_ne!(
        container, base_container,
        "{name} has screen art of its own"
    );
    let own = pictures(container);
    let base = pictures(base_container);
    let taken = pictures(
        staged
            .nameplate_container(subclass.attunement.hash)
            .unwrap(),
    );
    assert_eq!(
        own.len(),
        3,
        "{name}'s screen art keeps one picture per attunement"
    );
    assert_ne!(own[0], base[0], "{name}'s top picture is a painted copy");
    let painted = crate::icon_edit::decode_texture(&manager, own[0]).unwrap();
    assert_eq!(
        painted
            .get_pixel(painted.width() / 2, painted.height() / 2)
            .0,
        SCREEN_ART_COLOR,
        "{name}'s top picture holds the picture"
    );
    assert_eq!(own[1], base[1], "{name} keeps its base's bottom picture");
    assert_eq!(
        own[2], taken[0],
        "{name}'s middle picture is the attunement subclass's top one"
    );
    serde_json::json!({
        "container": format!("0x{container:08X}"),
        "base_container": format!("0x{base_container:08X}"),
        "pictures": own.iter().map(|tag| format!("{tag}")).collect::<Vec<_>>(),
    })
}

/// The colors of ability `entity`'s materials that Effect Colors lists, found the way the field
/// and the build find them.
fn stock_tints(packages: &Path, entity: u32) -> Vec<ability_tint::Tint> {
    let manager = open_shadowkeep_package_manager(packages).unwrap();
    let tints =
        ability_tint::ability_tints(&manager, entity, crate::subclass::SPAWN_DEPTH).unwrap();
    assert!(
        !tints.is_empty(),
        "ability entity 0x{entity:08X}'s materials hold a tint"
    );
    tints
}

/// Authors the second grenade as an ability of its own. Its text, icon, extra perk, two extra
/// charges, a value of its first named script parameter, a value of its entity and a turn of its
/// first effect palette's hue go in through the recipe, as typing does, with a value of a graph
/// it spawns, and its custom perk goes through the page's New Custom Perk, the workbench and Apply
/// to Subclass. Returns the stock perk the custom perk copies, the entity's and the spawned
/// graph's values, the parameter with its value, and the palette change.
#[allow(
    clippy::cognitive_complexity,
    clippy::type_complexity,
    reason = "End-to-end verification keeps the ordered workflow and its independent assertions together"
)]
fn author_ability(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    (base, attunement): (&SubclassSummary, &SubclassSummary),
    extra_perk: u16,
) -> (
    u16,
    (AbilityValue, AbilityValue),
    (u32, f32),
    (PaletteEdit, TintEdit),
) {
    let place = Place::Ability(layout::GRENADES[1]);
    let entity = *base
        .entry_entities
        .get(&layout::GRENADES[1])
        .expect("the base's second grenade has an entity");
    let value = ability_value(&app.packages, entity);
    let spawned = spawn_value(&app.packages, entity);
    let row = *base
        .entry_rows
        .get(&layout::GRENADES[1])
        .expect("the base's second grenade equips a row");
    let bank = app
        .catalog
        .as_ref()
        .unwrap()
        .ability_row(row)
        .expect("the catalog reads the grenade's row");
    let parameter = tuning_parameter(bank);
    // Its first palette takes the colors of another palette the attunement's abilities draw
    // with, then turns.
    let own = stock_palettes(&app.packages, entity)[0].header;
    let from = attunement
        .entry_entities
        .values()
        .find_map(|other| {
            let manager = open_shadowkeep_package_manager(&app.packages).unwrap();
            ability_palette::ability_palettes(&manager, *other, crate::subclass::SPAWN_DEPTH)
                .ok()?
                .into_iter()
                .map(|palette| palette.header)
                .find(|header| *header != own)
        })
        .expect("an ability of the attunement draws with another palette");
    let palette = PaletteEdit {
        from: Some(from),
        hue: 120,
        ..PaletteEdit::new(own)
    };
    // And its first tint takes that hue outright, as Colorize sets it.
    let tint = TintEdit {
        hue: 120,
        colorize: true,
        ..TintEdit::new(stock_tints(&app.packages, entity)[0].rgb).unwrap()
    };
    let mut abilities = app.recipe.overrides.subclass_abilities.clone().unwrap();
    let mut edits = abilities.edits(base.hash, place);
    edits.name = Some(ABILITY_NAME.to_owned());
    edits.description = Some(ABILITY_DESCRIPTION.to_owned());
    edits.icon = Some(EntryIcon::Ability {
        subclass: attunement.hash,
        entry: ABILITY_ICON_ENTRY,
    });
    edits.added_perks = vec![extra_perk];
    edits.extra_charges = 2;
    edits.set_parameter(parameter.0, Some(parameter.1));
    edits.ability_values = vec![value.edit.clone(), spawned.edit.clone()];
    edits.set_palette(palette);
    edits.set_tint(tint);
    // And every effect's final color takes the same hue, after the palette and tint, as Every
    // Effect sets it.
    edits.set_grade(GRADE);
    abilities.set_edits(base.hash, place, edits);
    app.recipe.overrides.subclass_abilities = Some(abilities);
    let output = settle_icons(ctx, app);
    click(
        ctx,
        app,
        find(&output, "the authored grenade", |text, _| {
            text == ABILITY_NAME
        }),
    );
    assert_eq!(app.subclass_page.selection, SubclassSelection::Entry(place));
    // Every section the grenade edits is marked. The Ability section holds its text and icon.
    let output = settle_icons(ctx, app);
    for text in ["Ability •", "Perks •", "Gameplay •", "Colors •"] {
        find(&output, text, |drawn, _| drawn == text);
    }
    click(ctx, app, section_tab(&output, "Ability"));
    let output = settle_icons(ctx, app);
    for text in ["Grenade 2", "Icon", "Ability Icon", "Artwork…", "Based On"] {
        find(&output, text, |drawn, _| drawn == text);
    }
    capture::write(ctx, &output, "gear-subclass-ability-text");
    // Effect Colors, in the Visuals section, loads the palettes the grenade's effects draw with
    // and shows the turned hue, then every stock palette, which names the one the colors come
    // from.
    click(ctx, app, section_tab(&output, "Visuals"));
    let output = settle_loaded(ctx, app, "the grenade's effect colors");
    find(&output, "Effect Colors", |text, _| text == "Effect Colors");
    let hue = format!("{}°", palette.hue);
    find(&output, "the palette's turned hue", |text, _| text == hue);
    find(&output, "the row that sets every color", |text, _| {
        text == "Set All"
    });
    find(&output, "the row that grades every effect", |text, _| {
        text == "Overall"
    });
    let start = Instant::now();
    let output = loop {
        let output = settle(ctx, app);
        if !texts(&output).iter().any(|(text, _)| text == "Own Colors") {
            break output;
        }
        assert!(
            start.elapsed() < Duration::from_secs(600),
            "the stock palettes did not load"
        );
        std::thread::sleep(Duration::from_millis(100));
    };
    assert!(
        accessible(&output, "Colors From").is_some(),
        "the palette names where its colors come from"
    );
    capture::write(ctx, &output, "gear-subclass-ability-colors");
    // The Gameplay section's Ability card holds the charges, the grenade's named parameters and
    // its own entity's values as tiles. A card follows for each part the grenade spawns with
    // values whose meaning is established: its projectiles, each with what it fires and how it
    // flies.
    let tab = |output: &egui::FullOutput, label: &str| find(output, label, |text, _| text == label);
    click(ctx, app, section_tab(&output, "Gameplay"));
    let output = settle_loaded(ctx, app, "the grenade's properties");
    for text in ["Charges", "+2", "Fires", "Speed", "Gravity", "Travel Limit"] {
        find(&output, text, |drawn, _| drawn == text);
    }
    capture::write(ctx, &output, "gear-subclass-ability-gameplay");
    // Its Technical part, closed until opened and marked for the grenade's values, loads the
    // grenade's entity, counts the edit among its fields, then lists the graphs the grenade
    // spawns and opens the one it changes.
    click(ctx, app, tab(&output, "Technical •"));
    let output = settle_loaded(ctx, app, "the grenade's entity");
    find(&output, "the customized value count", |text, _| {
        text.ends_with(" Values · 1 Changed")
    });
    capture::write(ctx, &output, "gear-subclass-ability-values");
    // The page names a spawned graph by its native name or its kind, as the loader does.
    let spawn_label = format!("{} •", spawn_name(&app.packages, entity, spawned.entity));
    let changed = find(&output, "the changed spawned graph", |text, _| {
        text == spawn_label
    });
    capture::write(ctx, &output, "gear-subclass-ability-spawns");
    click(ctx, app, changed);
    let output = settle_loaded(ctx, app, "the spawned graph");
    find(
        &output,
        "the spawned graph's customized value count",
        |text, _| text.ends_with(" Values · 1 Changed"),
    );
    // A projectile's plainly named values lead, outside their groups.
    find(&output, "the projectile's named values", |text, _| {
        text.starts_with("Initial Speed")
    });
    capture::write(ctx, &output, "gear-subclass-ability-spawn-values");
    // The trail's root shares the Ability card's name and sits below the Technical header.
    let below = find(&output, "Technical •", |text, _| text == "Technical •").y;
    click(
        ctx,
        app,
        find(&output, "the trail's root", |text, rect| {
            text == "Ability" && rect.min.y > below
        }),
    );
    let output = settle_loaded(ctx, app, "the grenade's spawns");
    // The Perks section holds the grenade's perks.
    click(ctx, app, section_tab(&output, "Perks"));
    let output = settle_icons(ctx, app);
    for text in ["Add Perk", "New Custom Perk"] {
        find(&output, text, |drawn, _| drawn == text);
    }
    capture::write(ctx, &output, "gear-subclass-ability-perks");
    click(
        ctx,
        app,
        find(&output, "New Custom Perk", |text, _| {
            text == "New Custom Perk"
        }),
    );
    settle(ctx, app);
    assert!(
        app.perk_workbench.open,
        "New Custom Perk opens the workbench"
    );
    // Effect checks read the workbench's perk discovery, so wait for it before applying.
    let start = Instant::now();
    while !app.perk_workbench.discovery_settled() {
        assert!(
            start.elapsed() < Duration::from_secs(900),
            "perk discovery did not finish"
        );
        frame(ctx, app, Vec::new());
        std::thread::sleep(Duration::from_millis(20));
    }
    // A copy of one of the base's perks, the first the workbench takes.
    let mut candidates = base
        .entry_perks
        .values()
        .flatten()
        .copied()
        .filter(|perk| *perk != extra_perk)
        .collect::<Vec<_>>();
    candidates.sort_unstable();
    candidates.dedup();
    let perk = app
        .perk_workbench
        .open_perk_mut()
        .expect("New Custom Perk opens a perk");
    perk.name = CUSTOM_PERK_NAME.to_owned();
    perk.effects = candidates
        .into_iter()
        .map(crate::perk::PerkRecipe::effect)
        .collect();
    app.perk_workbench.remove_flagged_effects();
    let perk = app.perk_workbench.open_perk_mut().unwrap();
    perk.effects.truncate(1);
    let custom_effect = perk
        .effects
        .first()
        .expect("the workbench takes one of the base's perks")
        .source_perk_index;
    let output = settle(ctx, app);
    let destination = format!("Grenade 2 · {ABILITY_NAME}");
    find(&output, "the perk's destination", |text, _| {
        text == destination
    });
    capture::write(ctx, &output, "gear-subclass-ability-workbench");
    click(
        ctx,
        app,
        find(&output, "Apply to Subclass", |text, _| {
            text == "Apply to Subclass"
        }),
    );
    let edits = app
        .recipe
        .overrides
        .subclass_abilities
        .as_ref()
        .unwrap()
        .edits(base.hash, place);
    assert_eq!(
        edits
            .custom_perks
            .iter()
            .map(|perk| perk.name.as_str())
            .collect::<Vec<_>>(),
        [CUSTOM_PERK_NAME],
        "Apply to Subclass puts the custom perk on the grenade"
    );
    // The workbench window stays open over the page, so it closes before the page's own
    // controls are used. Edit as Custom Perk opens it again.
    app.perk_workbench.open = false;
    // The added perk, edited as a custom perk from its chip's menu, takes the stock perk's place
    // once applied.
    let label = perk_label(&app.subclasses, extra_perk);
    let copy_name = format!("Custom {label}");
    let output = settle_icons(ctx, app);
    let perks = find(&output, "the Perks field", |text, _| text == "Perks");
    let chip = find(&output, "the added perk's chip", |text, rect| {
        text == label
            && rect.center().x > perks.x
            && (perks.y - 12.0..perks.y + 60.0).contains(&rect.center().y)
    });
    right_click(ctx, app, chip);
    let output = settle(ctx, app);
    click(
        ctx,
        app,
        find(&output, "Edit as Custom Perk…", |text, _| {
            text == "Edit as Custom Perk…"
        }),
    );
    settle(ctx, app);
    app.perk_workbench.remove_flagged_effects();
    let copy = app
        .perk_workbench
        .open_perk_mut()
        .expect("Edit as Custom Perk opens a copy");
    assert_eq!(copy.name, copy_name);
    assert_eq!(
        copy.effects
            .iter()
            .map(|effect| effect.source_perk_index)
            .collect::<Vec<_>>(),
        [extra_perk],
        "the copy carries the stock perk's effect, which the workbench takes"
    );
    let output = settle(ctx, app);
    click(
        ctx,
        app,
        find(&output, "Apply to Subclass", |text, _| {
            text == "Apply to Subclass"
        }),
    );
    let edits = app
        .recipe
        .overrides
        .subclass_abilities
        .as_ref()
        .unwrap()
        .edits(base.hash, place);
    assert_eq!(
        edits
            .custom_perks
            .iter()
            .map(|perk| perk.name.as_str())
            .collect::<Vec<_>>(),
        [CUSTOM_PERK_NAME, copy_name.as_str()],
        "Apply to Subclass adds the copy beside the first custom perk"
    );
    let stock = base
        .entry_perks
        .get(&layout::GRENADES[1])
        .cloned()
        .unwrap_or_default();
    assert!(
        !edits.perks(&stock).contains(&extra_perk),
        "the copy takes the stock perk's place"
    );
    // The reader closes the workbench, which otherwise sits over the page.
    app.perk_workbench.open = false;
    let output = settle_icons(ctx, app);
    for chip in [CUSTOM_PERK_NAME, copy_name.as_str()] {
        find(&output, "a custom perk's chip", |text, _| text == chip);
    }
    capture::write(ctx, &output, "gear-subclass-ability");
    (custom_effect, (value, spawned), parameter, (palette, tint))
}

/// A stock perk as the Subclass page labels its chip: the first stock entry that grants it,
/// numbered when that entry grants several.
fn perk_label(subclasses: &[SubclassSummary], perk: u16) -> String {
    subclasses
        .iter()
        .find_map(|subclass| {
            subclass.entry_perks.iter().find_map(|(entry, perks)| {
                let ordinal = perks.iter().position(|each| *each == perk)?;
                let name = subclass
                    .entry_names
                    .get(entry)
                    .map_or("Unknown Ability", String::as_str);
                Some(if perks.len() > 1 {
                    format!("{name} {}", ordinal + 1)
                } else {
                    name.to_owned()
                })
            })
        })
        .unwrap_or_else(|| format!("Perk {perk}"))
}

/// The authored grenade read back: its own text and icon, its perks, its modifiers, and its entity
/// copy with the edited value and the spawned graph it changes. Perks of the list that name the
/// grenade name the copy. Returns what `readback.json` records of it.
fn read_back_ability(
    (stock, staged): (&InvestmentCatalog, &InvestmentCatalog),
    authored: &SubclassSummary,
    subclass: &AuthoredSubclass,
    packages: &Path,
) -> serde_json::Value {
    let name = &subclass.recipe.name;
    let base = &subclass.base;
    let ability = layout::GRENADES[1];
    let from = AttunementPath::Bottom.entries();
    let authored_node = AttunementPath::Middle.entries()[1];
    // The authored grenade's pool equips an ability row of its own, whose pattern names a copy of
    // the stock entity with the edited value. A perk of the list that names the stock entity
    // names the copy through a private copy of its own.
    let value = &subclass.ability_value;
    let copied = *authored
        .entry_entities
        .get(&ability)
        .unwrap_or_else(|| panic!("{name}'s authored grenade names an entity"));
    assert_ne!(
        copied, value.entity,
        "{name}'s authored grenade names a copy of its entity"
    );
    let manager = open_shadowkeep_package_manager(packages).unwrap();
    let globals = investment_globals(&manager);
    let moved = (value.entity, copied);
    let mut node_perks = base.entry_perks.get(&from[1]).cloned().unwrap_or_default();
    node_perks.push(subclass.extra_perk);
    let mut retargeted = check_retargeted(
        (&manager, &globals),
        (
            &node_perks,
            &authored
                .entry_perks
                .get(&authored_node)
                .cloned()
                .unwrap_or_default(),
        ),
        moved,
        &format!("{name}'s authored node, from its source's perks and the added one,"),
    );
    // The authored grenade: its own text and the chosen icon, its stock perks less the one its
    // copy replaced, then its two custom perks. The first custom perk is a sandbox perk of its
    // own. The copy is one too, or the stock row again when that perk has no runtime of its own.
    assert_eq!(
        authored
            .entry_descriptions
            .get(&ability)
            .map(String::as_str),
        Some(ABILITY_DESCRIPTION),
        "{name}'s authored grenade has its own description"
    );
    assert_eq!(
        authored.entry_icons.get(&ability),
        subclass.attunement.entry_icons.get(&ABILITY_ICON_ENTRY),
        "{name}'s authored grenade shows the icon it takes"
    );
    let ability_perks = base
        .entry_perks
        .get(&ability)
        .into_iter()
        .flatten()
        .copied()
        .filter(|perk| *perk != subclass.extra_perk)
        .collect::<Vec<_>>();
    let granted = authored
        .entry_perks
        .get(&ability)
        .cloned()
        .unwrap_or_default();
    assert_eq!(
        granted.len(),
        ability_perks.len() + 2,
        "{name}'s authored grenade grants its perks and its two custom perks: {granted:?}"
    );
    retargeted.extend(check_retargeted(
        (&manager, &globals),
        (&ability_perks, &granted[..ability_perks.len()]),
        moved,
        &format!("{name}'s authored grenade"),
    ));
    let own = |perk: u16| {
        !stock.subclasses(|_| true).iter().any(|stock| {
            stock
                .entry_perks
                .values()
                .flatten()
                .any(|each| *each == perk)
        })
    };
    let (custom, copy) = (
        granted[ability_perks.len()],
        granted[ability_perks.len() + 1],
    );
    assert!(
        custom != subclass.custom_effect && own(custom),
        "{name}'s custom perk {custom} is a sandbox perk of its own, not stock {}",
        subclass.custom_effect
    );
    assert!(
        copy == subclass.extra_perk || (copy != custom && own(copy)),
        "{name}'s copy of perk {} grants {copy}",
        subclass.extra_perk
    );
    let modifiers = read_back_modifiers(staged, authored, subclass);
    // The stock grenade keeps its entity and value, and the copy carries the edited value.
    let stock_base = staged
        .subclasses(|_| true)
        .into_iter()
        .find(|staged| staged.hash == base.hash)
        .expect("the base subclass stays installed");
    assert_eq!(
        stock_base.entry_entities.get(&ability),
        Some(&value.entity),
        "the stock grenade keeps its entity"
    );
    let mut locator = value.edit.locator.clone();
    locator.graph_tag = None;
    let read = |entity: u32| {
        let payload = manager.read_tag(tiger_pkg::TagHash(entity)).unwrap();
        resolve_weapon_runtime_field(&manager, &payload, &locator)
            .unwrap_or_else(|error| panic!("{} of 0x{entity:08X}: {error}", value.name))
            .field
            .value
    };
    assert_eq!(
        read(copied),
        value.edit.value,
        "{name}'s copy carries the edited {}",
        value.name
    );
    assert_eq!(
        read(value.entity),
        value.stock,
        "the stock entity keeps its {}",
        value.name
    );
    let spawn = read_back_spawn(&manager, copied, &subclass.spawn_value, name);
    let swap = read_back_swap(&manager, authored, subclass.swap, name);
    let recharge = read_back_recharge(&manager, staged, authored, name);
    let movement = read_back_movement(&manager, authored, &subclass.movement, name);
    let jump_row = read_back_jump_row(&manager, authored, &subclass.jump_row, name);
    let bank = read_back_bank(&manager, authored, &subclass.bank_value, name);
    let table = read_back_table_palette(&manager, authored, subclass, name);
    let palette = read_back_palette(&manager, (value.entity, copied), subclass.palette, name);
    let tint = read_back_tint(&manager, (value.entity, copied), subclass.tint, name);
    let first = *authored
        .entry_entities
        .get(&layout::GRENADES[0])
        .unwrap_or_else(|| panic!("{name}'s first grenade names an entity"));
    let grade = read_back_grade(&manager, (&[subclass.bank_value.0], first), name);
    let authored_grade = read_back_grade(&manager, (&[value.entity], copied), name);
    let third = *authored
        .entry_entities
        .get(&layout::GRENADES[2])
        .unwrap_or_else(|| panic!("{name}'s third grenade names an entity"));
    let swapped_grade = read_back_grade(
        &manager,
        (&[subclass.swap.graph, subclass.swap.replacement], third),
        name,
    );
    serde_json::json!({
        "name": ABILITY_NAME,
        "perks": granted,
        "custom_perk": custom,
        "custom_perk_copies": subclass.custom_effect,
        "edited_stock_perk": subclass.extra_perk,
        "edited_stock_perk_row": copy,
        "modifiers": modifiers,
        "retargeted": retargeted,
        "spawn_value": spawn,
        "swap": swap,
        "bank": bank,
        "table_palette": table,
        "recharge": recharge,
        "movement": movement,
        "jump_row": jump_row,
        "palette": palette,
        "tint": tint,
        "grades": {
            "first": grade,
            "authored": authored_grade,
            "swapped": swapped_grade,
        },
        "ability_value": {
            "field": value.name,
            "stock_entity": format!("0x{:08X}", value.entity),
            "copy_entity": format!("0x{copied:08X}"),
            "stock": format!("{:?}", value.stock),
            "authored": format!("{:?}", value.edit.value),
        },
    })
}

/// The authored grenade's extra charges and parameter value, and the node's charge on the other
/// grenade, read back: pool records on each target's row whose keys name the bank rows the build
/// added. Returns what `readback.json` records of them.
fn read_back_modifiers(
    staged: &InvestmentCatalog,
    authored: &SubclassSummary,
    subclass: &AuthoredSubclass,
) -> serde_json::Value {
    let name = &subclass.recipe.name;
    let ability = layout::GRENADES[1];
    let authored_node = AttunementPath::Middle.entries()[1];
    // Its extra charges and parameter value reach its own row, the copy's, as pool records whose
    // keys name bank rows the build added. The node's charge reaches the other grenade the same
    // way, on that grenade's copy row.
    let row = *authored
        .entry_rows
        .get(&ability)
        .unwrap_or_else(|| panic!("{name}'s authored grenade equips a row"));
    let bank = staged
        .ability_row(row)
        .unwrap_or_else(|| panic!("{name}'s grenade row {row} reads back"));
    let (parameter, parameter_value) = subclass.parameter;
    let bank_tag = bank
        .bank
        .unwrap_or_else(|| panic!("{name}'s grenade has a bank"));
    let charges_key = ability_modifier::charge_key(2);
    let parameter_key =
        ability_modifier::parameter_key(bank_tag, parameter, parameter_value.to_bits(), false);
    let applied = authored
        .entry_modifiers
        .get(&ability)
        .cloned()
        .unwrap_or_default();
    for key in [charges_key, parameter_key] {
        assert!(
            applied.contains(&(key, row)),
            "{name}'s authored grenade applies key 0x{key:08X} to its row {row}: {applied:08X?}"
        );
    }
    let key_row = |key: u32| {
        bank.keys
            .iter()
            .find(|each| each.key == key)
            .unwrap_or_else(|| panic!("bank 0x{bank_tag:08X} has a row for key 0x{key:08X}"))
    };
    assert_eq!(
        key_row(charges_key).charges,
        Some(2),
        "the charge row adds two"
    );
    assert_eq!(
        key_row(parameter_key)
            .parameters
            .iter()
            .map(|each| (each.name, each.applied, each.add))
            .collect::<Vec<_>>(),
        [(parameter, parameter_value, false)],
        "the parameter row sets the parameter"
    );
    let other_row = *subclass
        .grenade
        .entry_rows
        .get(&layout::GRENADES[0])
        .expect("the taken grenade equips a row");
    let node_key = ability_modifier::charge_key(1);
    // The taken grenade's value gives it an entity and a row of its own, which the node's charge
    // reaches in place of the stock row, or the copy the subclass equips would go without it.
    let copy_row = *authored
        .entry_rows
        .get(&layout::GRENADES[0])
        .expect("the taken grenade's copy equips a row");
    assert_ne!(
        copy_row, other_row,
        "the taken grenade equips a row of its own"
    );
    assert!(
        authored
            .entry_modifiers
            .get(&authored_node)
            .is_some_and(|applied| applied.contains(&(node_key, copy_row))),
        "{name}'s authored node gives the taken grenade's copy its charge on row {copy_row}"
    );
    assert!(
        staged.ability_row(other_row).is_some_and(|other| other
            .keys
            .iter()
            .any(|each| each.key == node_key && each.charges == Some(1))),
        "the taken grenade's bank has a row for one charge"
    );
    serde_json::json!({
        "extra_charges": {"row": row, "key": format!("0x{charges_key:08X}")},
        "parameter": {
            "name": format!("0x{parameter:08X}"),
            "value": parameter_value,
            "key": format!("0x{parameter_key:08X}"),
        },
    })
}

/// The third grenade's swap read back: it names a copy of its entity, which names no stock
/// replaced projectile and names a private copy of the replacement, byte equal to it under a tag
/// of its own. Returns what `readback.json` records of it.
fn read_back_swap(
    manager: &PackageManager,
    authored: &SubclassSummary,
    swap: SpawnSwap,
    name: &str,
) -> serde_json::Value {
    let copied = *authored
        .entry_entities
        .get(&layout::GRENADES[2])
        .unwrap_or_else(|| panic!("{name}'s third grenade names an entity"));
    assert_ne!(
        copied, swap.graph,
        "{name}'s third grenade names a copy of its entity"
    );
    let payload = manager.read_tag(tiger_pkg::TagHash(copied)).unwrap();
    let named = ability_spawns::spawned_graphs(manager, copied, &payload).unwrap();
    assert!(
        !named.contains(&swap.replaced),
        "{name}'s copy no longer names projectile 0x{:08X}",
        swap.replaced
    );
    // Every place the stock grenade named the replaced projectile names one private copy of the
    // replacement, which the grade may have recolored below it.
    let place =
        |spawn: &ability_spawns::Spawn| (spawn.binding_hash, spawn.resource_index, spawn.offset);
    let stock = manager.read_tag(tiger_pkg::TagHash(swap.graph)).unwrap();
    let places = ability_spawns::spawns(manager, swap.graph, &stock)
        .unwrap()
        .iter()
        .filter(|spawn| spawn.graph == swap.replaced)
        .map(place)
        .collect::<BTreeSet<_>>();
    assert!(
        !places.is_empty(),
        "the stock grenade names the replaced projectile"
    );
    let named_there = ability_spawns::spawns(manager, copied, &payload)
        .unwrap()
        .iter()
        .filter(|spawn| places.contains(&place(spawn)))
        .map(|spawn| spawn.graph)
        .collect::<BTreeSet<_>>();
    assert_eq!(
        named_there.len(),
        1,
        "{name}'s copy names one projectile where the stock one named 0x{:08X}",
        swap.replaced
    );
    let private = *named_there.first().unwrap();
    assert!(
        private != swap.replacement && private != swap.replaced,
        "{name}'s copy names a private copy of projectile 0x{:08X}",
        swap.replacement
    );
    let replacement = manager
        .read_tag(tiger_pkg::TagHash(swap.replacement))
        .unwrap();
    let copy = manager.read_tag(tiger_pkg::TagHash(private)).unwrap();
    assert!(
        copy.len() == replacement.len() && copy.get(0x96) == Some(&18),
        "{name}'s private projectile 0x{private:08X} is a copy of 0x{:08X}",
        swap.replacement
    );
    serde_json::json!({
        "stock_entity": format!("0x{:08X}", swap.graph),
        "copy_entity": format!("0x{copied:08X}"),
        "replaced": format!("0x{:08X}", swap.replaced),
        "replacement": format!("0x{:08X}", swap.replacement),
        "private_copy": format!("0x{private:08X}"),
    })
}

/// The first grenade's copy read back: it binds a private copy of its bank in place of the stock
/// one. The copy validates as a bank under its own tag, holds every row the build gave the stock
/// bank in the same order, the node's charge among them, lays out its blocks as the staged stock
/// bank does for every offset the entity names, and names a private copy of the graph only the
/// bank names, holding the edited value. Returns what `readback.json` records of it.
/// The first grenade's palette that only an impact table reaches, read back: as
/// `read_back_palette` checks, its copy draws a recolored palette everywhere the stock one was,
/// through private materials and particle systems, and the copy's graphs name an impact table the
/// stock grenade's do not, a private copy. Returns what `readback.json` records of it.
fn read_back_table_palette(
    manager: &PackageManager,
    authored: &SubclassSummary,
    subclass: &AuthoredSubclass,
    name: &str,
) -> serde_json::Value {
    let stock = subclass.bank_value.0;
    let copied = *authored
        .entry_entities
        .get(&layout::GRENADES[0])
        .unwrap_or_else(|| panic!("{name}'s first grenade names an entity"));
    let palette = read_back_palette(manager, (stock, copied), subclass.table_palette, name);
    let tables = |entity: u32| {
        ability_palette::ability_graphs(manager, entity, crate::subclass::SPAWN_DEPTH)
            .unwrap()
            .into_iter()
            .flat_map(|(graph, payload)| ability_spawns::tables(manager, graph, &payload).unwrap())
            .map(|place| place.graph)
            .collect::<BTreeSet<_>>()
    };
    let stock_tables = tables(stock);
    let private = tables(copied)
        .difference(&stock_tables)
        .copied()
        .collect::<Vec<_>>();
    assert!(
        !private.is_empty(),
        "{name}'s first grenade reaches its recolored graphs through a private impact table"
    );
    for table in &private {
        assert!(
            ability_spawns::is_table(manager, *table),
            "0x{table:08X} is an impact table"
        );
    }
    serde_json::json!({
        "palette": palette,
        "stock_tables": stock_tables.len(),
        "private_tables": private.iter().map(|tag| format!("0x{tag:08X}")).collect::<Vec<_>>(),
    })
}

fn read_back_bank(
    manager: &PackageManager,
    authored: &SubclassSummary,
    (entity, bank, value): &(u32, u32, AbilityValue),
    name: &str,
) -> serde_json::Value {
    use sundial::package_authoring::ability_bank::{bank_owner, property_rows, validate};
    use sundial::package_authoring::entity::{
        weapon_component_binding_hashes, weapon_component_bindings,
    };
    let copied = *authored
        .entry_entities
        .get(&layout::GRENADES[0])
        .unwrap_or_else(|| panic!("{name}'s first grenade names an entity"));
    assert_ne!(
        copied, *entity,
        "{name}'s first grenade names a copy of its entity"
    );
    let stock_entity = manager.read_tag(tiger_pkg::TagHash(*entity)).unwrap();
    let copied_entity = manager.read_tag(tiger_pkg::TagHash(copied)).unwrap();
    let mut private = BTreeSet::new();
    for binding in weapon_component_binding_hashes(&stock_entity).unwrap() {
        let stock = weapon_component_bindings(&stock_entity, binding).unwrap();
        let copy = weapon_component_bindings(&copied_entity, binding).unwrap();
        assert_eq!(
            stock.len(),
            copy.len(),
            "binding 0x{binding:08X} keeps its resources"
        );
        for (stock, copy) in stock.iter().zip(&copy) {
            if stock.owner_tag == *bank {
                private.insert(copy.owner_tag);
            }
        }
    }
    assert_eq!(
        private.len(),
        1,
        "{name}'s first grenade binds one bank: {private:X?}"
    );
    let private = *private.first().unwrap();
    assert_ne!(
        private, *bank,
        "{name}'s first grenade binds a private copy of its bank"
    );
    let copy = manager.read_tag(tiger_pkg::TagHash(private)).unwrap();
    validate(&copy).unwrap_or_else(|error| panic!("{name}'s private bank: {error}"));
    assert_eq!(
        bank_owner(&copy).unwrap(),
        private,
        "the private bank names itself"
    );
    let rows = |payload: &[u8]| {
        property_rows(payload)
            .unwrap()
            .iter()
            .map(|row| (row.key, row.handler.get(), row.modifier_class))
            .collect::<Vec<_>>()
    };
    let staged = manager.read_tag(tiger_pkg::TagHash(*bank)).unwrap();
    assert_eq!(
        rows(&copy),
        rows(&staged),
        "{name}'s private bank holds every row the build gave the stock bank"
    );
    // An entity names a bank's blocks by tag, class and offset. The copy names its bank's blocks
    // where the stock grenade names the stock bank's, both moved for the rows they took: the
    // same places, classes and offsets, and the same words at each, the bank's own tag aside.
    let word =
        |payload: &[u8], at: usize| u32::from_le_bytes(payload[at..at + 4].try_into().unwrap());
    let tuples = |payload: &[u8], owner: u32| {
        (0..payload.len().saturating_sub(15))
            .step_by(8)
            .filter(|at| {
                word(payload, *at) == owner
                    && (0x8080_0000..=0x8080_FFFF).contains(&word(payload, at + 4))
            })
            .map(|at| {
                let offset = u64::from_le_bytes(payload[at + 8..at + 16].try_into().unwrap());
                (at, word(payload, at + 4), offset)
            })
            .collect::<Vec<_>>()
    };
    let named = tuples(&copied_entity, private);
    assert!(
        !named.is_empty(),
        "{name}'s first grenade names its private bank's blocks"
    );
    assert_eq!(
        named,
        tuples(&stock_entity, *bank),
        "{name}'s first grenade names its private bank's blocks where the stock grenade names the stock bank's"
    );
    let block = |payload: &[u8], owner: u32, offset: u64| {
        usize::try_from(offset)
            .ok()
            .filter(|offset| offset + 8 <= payload.len())
            .map(|offset| {
                [word(payload, offset), word(payload, offset + 4)]
                    .map(|each| if each == owner { 0 } else { each })
            })
    };
    for (at, _, offset) in &named {
        assert_eq!(
            block(&copy, private, *offset),
            block(&staged, *bank, *offset),
            "{name}'s first grenade finds the stock block at 0x{offset:X}, named from +0x{at:X}"
        );
    }
    let graph = read_back_spawn(manager, copied, value, name);
    serde_json::json!({
        "stock_entity": format!("0x{entity:08X}"),
        "copy_entity": format!("0x{copied:08X}"),
        "stock_bank": format!("0x{bank:08X}"),
        "private_bank": format!("0x{private:08X}"),
        "rows": rows(&copy).len(),
        "named_blocks": named.len(),
        "graph_value": graph,
    })
}

/// The spawned graph's value read back: the ability's copy names a private copy of the graph,
/// holding the edited value, and the stock graph keeps its own. Returns what `readback.json`
/// records of it.
fn read_back_spawn(
    manager: &PackageManager,
    copied: u32,
    spawn: &AbilityValue,
    name: &str,
) -> serde_json::Value {
    let copied_payload = manager.read_tag(tiger_pkg::TagHash(copied)).unwrap();
    let spawned = ability_spawns::spawned_graphs(manager, copied, &copied_payload).unwrap();
    assert!(
        !spawned.contains(&spawn.entity),
        "{name}'s copy names a private copy of graph 0x{:08X}, not the stock one",
        spawn.entity
    );
    let mut spawn_locator = spawn.edit.locator.clone();
    spawn_locator.graph_tag = None;
    let read_spawn = |graph: u32| {
        let payload = manager.read_tag(tiger_pkg::TagHash(graph)).ok()?;
        resolve_weapon_runtime_field(manager, &payload, &spawn_locator)
            .ok()
            .map(|resolved| resolved.field.value)
    };
    let private = spawned
        .iter()
        .copied()
        .find(|graph| read_spawn(*graph).as_ref() == Some(&spawn.edit.value))
        .unwrap_or_else(|| {
            panic!(
                "{name}'s copy names no graph with the edited {} of 0x{:08X}",
                spawn.name, spawn.entity
            )
        });
    assert_eq!(
        read_spawn(spawn.entity),
        Some(spawn.stock.clone()),
        "the spawned stock graph keeps its {}",
        spawn.name
    );
    serde_json::json!({
        "field": spawn.name,
        "stock_graph": format!("0x{:08X}", spawn.entity),
        "copy_graph": format!("0x{private:08X}"),
        "authored": format!("{:?}", spawn.edit.value),
    })
}

/// The staged subclass is class neutral and keeps every ability it did not take elsewhere, has
/// the chosen grenade and attunement under their own names, a list of its own and no Collections
/// entry.
#[allow(
    clippy::cognitive_complexity,
    reason = "End-to-end verification keeps the ordered workflow and its independent assertions together"
)]
fn read_back_subclass(
    (stock, staged): (&InvestmentCatalog, &InvestmentCatalog),
    build: &BuildReport,
    subclass: &AuthoredSubclass,
    packages: &Path,
) -> serde_json::Value {
    let name = &subclass.recipe.name;
    let report = build
        .weapons
        .iter()
        .find(|report| report.name == *name)
        .unwrap();
    assert_eq!(report.kind, ItemKind::Subclass);
    assert!(
        report.collection.is_none(),
        "{name} has no collectible or unlock"
    );
    let authored = staged
        .subclasses(|_| true)
        .into_iter()
        .find(|staged| staged.hash == report.item_hash)
        .unwrap_or_else(|| panic!("{name} reads back as a subclass"));
    assert_eq!(&authored.name, name);
    assert_eq!(authored.class_type, 3, "Every Class is class neutral");
    assert_eq!(
        staged.item_type_name(report.item_hash).as_deref(),
        Some("Guardian Subclass")
    );
    // The manifest retains the donor class for default equipping and Every Class for grants.
    let manifest: serde_json::Value = serde_json::from_slice(
        &fs::read(
            build
                .run_directory
                .join(crate::manifest::MANIFEST_FILE_NAME),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(
        manifest["project"]["weapons"]
            .as_array()
            .unwrap()
            .iter()
            .find(|weapon| weapon["name"] == name.as_str())
            .map(|weapon| (weapon["class_type"].clone(), weapon["every_class"].clone())),
        Some((
            serde_json::json!(subclass.base.class_type),
            serde_json::json!(true)
        )),
        "{name} records its class and Every Class for the install"
    );
    assert!(
        staged.item_collection_parents(report.item_hash).is_empty(),
        "{name} stays out of Collections"
    );
    // Every subclass the build authors edits its abilities, the retarget one too, so each adds
    // one list.
    let authored_lists = build
        .weapons
        .iter()
        .filter(|report| report.kind == ItemKind::Subclass)
        .count();
    assert_eq!(
        staged.socket_entry_list_count(),
        stock.socket_entry_list_count() + authored_lists,
        "each authored subclass adds one socket-entry list"
    );
    let base = &subclass.base;
    let grenade = layout::GRENADES[0];
    let ability = layout::GRENADES[1];
    let top = AttunementPath::Top.entries();
    let from = AttunementPath::Bottom.entries();
    let authored_node = AttunementPath::Middle.entries()[1];
    for (&entry, expected) in &base.entry_names {
        let expected = if entry == grenade {
            subclass.grenade.entry_names[&entry].as_str()
        } else if entry == ability {
            ABILITY_NAME
        } else if let Some(position) = top.iter().position(|top| *top == entry) {
            subclass.attunement.entry_names[&from[position]].as_str()
        } else if entry == authored_node {
            NODE_NAME
        } else {
            expected.as_str()
        };
        assert_eq!(
            authored.entry_names.get(&entry).map(String::as_str),
            Some(expected),
            "{name} entry {entry}"
        );
    }
    assert_eq!(
        authored.attunement_names,
        [
            subclass.attunement.attunement_names[AttunementPath::Bottom.index()].clone(),
            base.attunement_names[AttunementPath::Bottom.index()].clone(),
            PATH_NAME.to_owned(),
        ],
        "{name} names each attunement for the subclass it comes from, or by its own name"
    );
    serde_json::json!({
        "kind": ItemKind::Subclass,
        "name": name,
        "item_hash": format!("0x{:08X}", report.item_hash),
        "base": format!("{} 0x{:08X}", base.name, base.hash),
        "class": authored.class_type,
        "every_class": subclass.recipe.overrides.subclass_every_class,
        "grenade_from": format!("{} 0x{:08X}", subclass.grenade.name, subclass.grenade.hash),
        "top_attunement_from": format!(
            "{} 0x{:08X}",
            subclass.attunement.name, subclass.attunement.hash
        ),
        "attunements": authored.attunement_names,
        "authored_grenade": read_back_ability((stock, staged), &authored, subclass, packages),
        "screen_art": read_back_screen_art(staged, (report.item_hash, subclass), packages),
        "abilities": authored
            .entry_names
            .iter()
            .map(|(entry, name)| format!("{entry}: {name}"))
            .collect::<Vec<_>>(),
        "collections": staged.item_collection_paths(report.item_hash),
    })
}

#[test]
#[ignore = "requires PARHELION_DEFAULT_WEAPONS_PACKAGES and PARHELION_GEAR_ARTIFACTS; stages subclasses from every stock donor"]
fn every_class_subclasses_remove_the_native_equip_requirement() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_DEFAULT_WEAPONS_PACKAGES").unwrap());
    let artifacts = PathBuf::from(std::env::var_os("PARHELION_GEAR_ARTIFACTS").unwrap())
        .join("class-requirements");
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
#[ignore = "requires PARHELION_DEFAULT_WEAPONS_PACKAGES and PARHELION_GEAR_ARTIFACTS; builds and stages gear"]
fn gear_of_every_kind_goes_from_the_new_menu_to_staged_packages() {
    let packages = PathBuf::from(
        std::env::var_os("PARHELION_DEFAULT_WEAPONS_PACKAGES")
            .expect("PARHELION_DEFAULT_WEAPONS_PACKAGES names the installed packages"),
    );
    let artifacts = PathBuf::from(
        std::env::var_os("PARHELION_GEAR_ARTIFACTS")
            .expect("PARHELION_GEAR_ARTIFACTS names an output folder on the packages' drive"),
    );
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
