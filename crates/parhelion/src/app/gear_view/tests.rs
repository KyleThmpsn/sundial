//! Gear from the toolbar to staged packages. Each non-weapon kind is created from the New menu,
//! edited on its page, given a lore tab of its own and a custom perk from its socket's menu, built
//! the way Build & Stage builds it and read back from the staged packages through Sundial's
//! catalog. Armor 1.0 keeps two choices in one socket, one made the default from its menu. A shader
//! remixes another shader's dyes and gives two surfaces custom colors and iridescence, and its
//! preview of those unbuilt edits must match the built shader drawn the same way. A subclass
//! takes a grenade from another class and an attunement from another subclass of its own, and
//! authors its middle path: a name of its own, and a second node taken from another path with
//! its own name, description and one more perk.
//!
//! `PARHELION_DEFAULT_WEAPONS_PACKAGES` names the installed packages and
//! `PARHELION_GEAR_ARTIFACTS` a folder on the same drive. The folder keeps the recipes, the staged
//! run and `readback.json`. `PARHELION_UI_CAPTURE_DIR` adds page captures, and
//! `PARHELION_GEAR_KINDS` (such as `Shader` or `Armor,Ship`) authors only the kinds it names.
//! Nothing installed is changed: the read-back view is hard links to the packages plus the
//! staged files.
use super::*;
use crate::app::custom_perks::workbench::{Workbench, tests::capture};
use crate::app::shader_view;
use crate::app::subclass_view::SubclassSelection;
use crate::dye::{
    DyeChannel, DyeEdit, DyeSurface, DyeTextureEdit, DyeValue, GearType, surface_edit,
    texture_edit, write_vectors,
};
use crate::subclass::{AttunementPath, SubclassPathNode, layout};
use std::fs;
use std::sync::Arc;
use std::time::{Duration, Instant};
use sundial::investment::SubclassSummary;
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
                if !app.recipe.kind.is_weapon() {
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

fn texts(output: &egui::FullOutput) -> Vec<(String, egui::Rect)> {
    fn walk(shape: &egui::Shape, found: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::Shape::Text(text) => found.push((
                text.galley.job.text.clone(),
                text.galley.rect.translate(text.pos.to_vec2()),
            )),
            egui::Shape::Vec(shapes) => shapes.iter().for_each(|shape| walk(shape, found)),
            _ => {}
        }
    }
    let mut found = Vec::new();
    for clipped in &output.shapes {
        walk(&clipped.shape, &mut found);
    }
    found
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
    for pressed in [true, false] {
        frame(
            ctx,
            app,
            vec![
                egui::Event::PointerMoved(position),
                egui::Event::PointerButton {
                    pos: position,
                    button,
                    pressed,
                    modifiers: Default::default(),
                },
            ],
        );
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
            let sets = catalog.gear_supported_plug_sets(summary.hash).ok()?;
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

/// Runs frames until the base item's lore has loaded, so a capture shows the page as a reader
/// sees it.
fn settle_lore(ctx: &egui::Context, app: &mut PackageAuthoringApp) -> egui::FullOutput {
    let start = Instant::now();
    loop {
        let output = settle(ctx, app);
        if app.presentation_editor.lore_loaded() {
            return output;
        }
        assert!(
            start.elapsed() < Duration::from_secs(120),
            "the base item's lore did not load"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// The page's own layout: the icon under the base in one column, the lore tab in the other, and
/// armor named by its generation.
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
        "Lore Tab",
        "Custom Lore Tab",
    ]) {
        assert!(
            drawn.iter().any(|text| text == label),
            "{kind:?} page lacks {label}"
        );
    }
    let base = rect_of(output, &format!("Base {}", kind.label()));
    let icon = rect_of(output, "Inventory Icon");
    let lore = rect_of(output, "Lore Tab");
    let sockets = rect_of(output, "Perks & Sockets");
    assert!(
        icon.top() > base.bottom()
            && (icon.left() - base.left()).abs() < 1.0
            && icon.bottom() < sockets.top(),
        "the {kind:?} page puts the icon under its base: base {base:?}, icon {icon:?}"
    );
    assert!(
        lore.left() > icon.right() && lore.bottom() < sockets.top(),
        "the {kind:?} page puts the lore tab beside the base: lore {lore:?}, icon {icon:?}"
    );
    let named = ["Armor 1.0", "Armor 2.0"]
        .into_iter()
        .filter(|generation| drawn.iter().any(|text| text == generation))
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
        ItemKind::Weapon | ItemKind::Shader | ItemKind::Subclass => unreachable!(),
    };
    let (perk_socket, collections_base) = choose_base(app, kind, labels, origin, energized);
    let name = format!("Parhelion Test {}{}", origin.prefix(), kind.label());
    app.recipe.rename_authored_item(name).unwrap();
    let base = app.recipe.donor.item_hash.parse_u32().unwrap();
    let donor = app.current_gear_donor().unwrap();

    assert_page_fits(ctx, app, kind.label());
    let output = settle_lore(ctx, app);
    check_gear_page(&output, kind, origin, &donor);
    capture::write(ctx, &output, &format!("gear-{}", slug(kind, origin)));
    if energized {
        assert_eq!(
            app.catalog.as_ref().unwrap().item_stat_maximum(base),
            Some(42),
            "Armor 2.0 stats top out at 42"
        );
    }

    // A lore tab of its own, started from the page's lore section.
    click(
        ctx,
        app,
        find(&output, "Custom Lore Tab", |text, _| {
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

/// Sparrows, Ships, Ghost Shells and shaders sit on the runtime's own page in their base's
/// category. Armor sits beside its base. Everything but class armor also joins the runtime's
/// badge.
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
    if crate::collection::GearPage::for_kind(kind).is_some() {
        let base_paths = staged.item_collection_paths(base);
        let on_brand_page = staged.item_collection_paths(item_hash).iter().any(|path| {
            path.first().map(String::as_str) == Some(brand)
                && base_paths
                    .iter()
                    .any(|base| base.len() == path.len() && base[1..] == path[1..])
        });
        assert!(
            on_brand_page,
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
    // The badge adds its own parents beside the page.
    assert_eq!(
        parents.len() > 1,
        kind != ItemKind::Armor,
        "{name} Sunrise badge membership"
    );
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
    crate::dye::GEAR_TYPE_KEYS
        .iter()
        .all(|first| dye(rows, first + channel) == dye(source, first + channel))
}

/// A shader's rows as a model preview composes them.
fn preview_rows(rows: &shader_view::DyeRows) -> [Vec<(i8, u16)>; 3] {
    rows.clone().map(|rows| {
        rows.iter()
            .map(|row| (row.channel_index, row.dye_reference_index))
            .collect()
    })
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
    let toggle = |ctx: &egui::Context, app: &mut PackageAuthoringApp, output: &egui::FullOutput| {
        click(
            ctx,
            app,
            find(output, "Text Presentation", |text, _| {
                text == "Text Presentation"
            }),
        );
        // The section folds open over a few frames.
        for _ in 0..12 {
            frame(ctx, app, Vec::new());
        }
        settle(ctx, app)
    };
    let opened = toggle(ctx, app, &output);
    find(&opened, "Custom Item-Type Label", |text, _| {
        text == "Custom Item-Type Label"
    });
    assert!(
        !texts(&opened)
            .iter()
            .any(|(text, _)| text.contains("Lore Tab")),
        "a shader page offers no lore tab"
    );
    toggle(ctx, app, &opened);
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
    let differing = built
        .pixels
        .iter()
        .zip(&shader.preview.pixels)
        .filter(|(built, unbuilt)| built != unbuilt)
        .count();
    assert_eq!(
        differing, 0,
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
    let differing = built_weapon
        .pixels
        .iter()
        .zip(&shader.weapon_preview.pixels)
        .filter(|(built, unbuilt)| built != unbuilt)
        .count();
    assert_eq!(
        differing, 0,
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
    /// The perk the authored middle-path node adds to the ones it starts with.
    extra_perk: u16,
}

/// The authored middle path's name, and its second node's.
const PATH_NAME: &str = "Way of the Sundial";
const NODE_NAME: &str = "Sundial Strike";

/// Clicks `choice` on the line of the detail panel's choices that `subclass` leads.
fn choose(ctx: &egui::Context, app: &mut PackageAuthoringApp, subclass: &str, choice: &str) {
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

    // A grenade from another class: its row shows it beside the list, where its subclass's line
    // offers it.
    let grenade = app
        .subclasses
        .iter()
        .find(|subclass| subclass.class_type != base.class_type)
        .expect("a subclass of another class")
        .clone();
    let entry = layout::GRENADES[0];
    let row = base.entry_names[&entry].as_str();
    let output = settle(ctx, app);
    click(ctx, app, find(&output, row, |text, _| text == row));
    assert_eq!(app.subclass_selection, SubclassSelection::Ability(entry));
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
        app.subclass_selection,
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
    node.name = Some(NODE_NAME.to_owned());
    node.description = Some("Built node by node in Parhelion.".to_owned());
    node.added_perks = vec![extra_perk];
    abilities.set_path_node(AttunementPath::Middle, base.hash, 1, Some(node));
    app.recipe.overrides.subclass_abilities = Some(abilities);
    let output = settle(ctx, app);
    let restore = format!("Restore {top}");
    find(&output, "the top attunement's restore", |text, _| {
        text == restore
    });
    // The Middle tab, marked for its edits, shows the middle attunement and its nodes.
    click(
        ctx,
        app,
        find(&output, "the Middle tab", |text, _| text == "Middle •"),
    );
    assert_eq!(
        app.subclass_selection,
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
        app.subclass_selection,
        SubclassSelection::Node(AttunementPath::Middle, 1)
    );
    let output = settle_icons(ctx, app);
    for text in [
        "Middle Path · Node 2",
        "Description",
        "Perks",
        "Add Perk",
        "Based On",
    ] {
        find(&output, text, |drawn, _| drawn == text);
    }
    capture::write(ctx, &output, "gear-subclass-path");
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
    // A narrow window stacks the detail under the list.
    capture::write(ctx, &frame_at(ctx, app, 640.0), "gear-subclass-narrow");
    app.recipe_baseline = app.recipe.clone();
    app.recipe_dirty = false;
    AuthoredSubclass {
        recipe: app.recipe.clone(),
        base,
        grenade,
        attunement,
        extra_perk,
    }
}

/// The staged subclass keeps its base's class and every ability it did not take elsewhere, has
/// the chosen grenade and attunement under their own names, a list of its own and no Collections
/// entry.
fn read_back_subclass(
    stock: &InvestmentCatalog,
    staged: &InvestmentCatalog,
    build: &BuildReport,
    subclass: &AuthoredSubclass,
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
    assert_eq!(authored.class_type, subclass.base.class_type);
    // The install adds the subclass to each character of this class, which only the manifest
    // records.
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
            .map(|weapon| weapon["class_type"].clone()),
        Some(serde_json::json!(subclass.base.class_type)),
        "{name} records its class for the install"
    );
    assert!(
        staged.item_collection_parents(report.item_hash).is_empty(),
        "{name} stays out of Collections"
    );
    assert_eq!(
        staged.socket_entry_list_count(),
        stock.socket_entry_list_count() + 1,
        "{name} adds one socket-entry list"
    );
    let base = &subclass.base;
    let grenade = layout::GRENADES[0];
    let top = AttunementPath::Top.entries();
    let from = AttunementPath::Bottom.entries();
    let authored_node = AttunementPath::Middle.entries()[1];
    for (&entry, expected) in &base.entry_names {
        let expected = if entry == grenade {
            subclass.grenade.entry_names[&entry].as_str()
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
    let mut node_perks = base.entry_perks.get(&from[1]).cloned().unwrap_or_default();
    node_perks.push(subclass.extra_perk);
    assert_eq!(
        authored.entry_perks.get(&authored_node),
        Some(&node_perks),
        "{name}'s authored node grants its source's perks and the added one"
    );
    serde_json::json!({
        "kind": ItemKind::Subclass,
        "name": name,
        "item_hash": format!("0x{:08X}", report.item_hash),
        "base": format!("{} 0x{:08X}", base.name, base.hash),
        "class": authored.class_type,
        "grenade_from": format!("{} 0x{:08X}", subclass.grenade.name, subclass.grenade.hash),
        "top_attunement_from": format!(
            "{} 0x{:08X}",
            subclass.attunement.name, subclass.attunement.hash
        ),
        "attunements": authored.attunement_names,
        "abilities": authored
            .entry_names
            .iter()
            .map(|(entry, name)| format!("{entry}: {name}"))
            .collect::<Vec<_>>(),
        "collections": staged.item_collection_paths(report.item_hash),
    })
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
    let catalog = InvestmentCatalog::load(packages.parent().unwrap(), false, |_| {}).unwrap();
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
    let shader = wanted(ItemKind::Shader).then(|| author_shader(&ctx, &mut app));
    let subclass = wanted(ItemKind::Subclass).then(|| author_subclass(&ctx, &mut app));
    let recipes = authored
        .iter()
        .map(|item| item.recipe.clone())
        .chain(shader.iter().map(|shader| shader.recipe.clone()))
        .chain(subclass.iter().map(|subclass| subclass.recipe.clone()))
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
    let readback = authored
        .iter()
        .map(|item| read_back(&staged, &build, item, (brand, &view.join("packages"))))
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
            read_back_subclass(&stock, &staged, &build, subclass)
        }))
        .collect::<Vec<_>>();
    fs::write(
        artifacts.join("readback.json"),
        serde_json::to_vec_pretty(&readback).unwrap(),
    )
    .unwrap();
    fs::remove_dir_all(&view).unwrap();
}
