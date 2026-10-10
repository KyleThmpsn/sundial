//! Properties: the values of an ability and of the graphs it spawns whose meaning is
//! established. The ability's own join its Ability card, and each graph it spawns has a card, in
//! the order the ability spawns them. A projectile's
//! card picks the projectile it fires and how it flies. Every card holds the graph's timers that
//! run out, the changes its effects make to the player and weapons, each named by the input it
//! changes, its invisibility, and a movement ability's traced controller values, such as a
//! Blink's distance and a jump's airborne jumps. Graphs that hold the same values share a card, and an edit
//! there changes each of them. A value change is the same override Raw Values writes,
//! and a projectile another stock ability's, so the build changes copies only this
//! ability uses.
use super::tuning::SPAWN_DEPTH;
use super::*;
use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver};
use sundial::package_authoring::PackageManager;
use sundial::package_authoring::ability_damage;
use sundial::package_authoring::ability_movement::{self, MovementValue, RowLane, Unit};
use sundial::package_authoring::ability_settings::{
    self, Kind as SettingKind, Setting, Unit as SettingUnit,
};
use sundial::package_authoring::ability_spawns::{describe, spawns, table_graphs, tables};
use sundial::package_authoring::runtime::modifiers;
use sundial::package_authoring::runtime::{WeaponRuntimeField, WeaponRuntimeValue};
use sundial::package_authoring::sandbox_perk::entity::effect_length::{
    self, EffectLength, UNLIMITED,
};
use sundial::package_authoring::sandbox_perk::entity::projectile::parameters::{
    self, Curve, CurveKind, Kind, Parameter,
};

mod property;
mod readings;
mod search;
mod swap;
mod tree;
use property::*;
use readings::*;
pub(super) use search::{matches, search_bar};
use swap::*;
use tree::*;
pub(super) use tree::{Properties, Tree};

/// Where an entity graph keeps the client's object type, and the type of a projectile.
const OBJECT_TYPE: usize = 0x96;
const PROJECTILE: u8 = 18;
/// Component Modifiers: records that add to or multiply an input of a component of the player
/// or a weapon.
const MODIFIERS: u32 = 0x8080_3B00;
/// Invisibility Attachment, whose named fields set how strong the invisibility is and how it
/// fades.
const INVISIBILITY: u32 = 0x8080_43DF;
/// The settings record's ability slot, read when the record changes the Abilities component.
const ABILITY_OFFSET: u32 = 0x48;
const ABILITIES: i64 = 9;

/// A closed section of a card: `title`, with what it holds on hover, and its tiles once opened. It
/// is marked once one of them holds an edit.
fn fold(
    ui: &mut egui::Ui,
    (salt, title, hover): (impl std::hash::Hash + std::fmt::Debug, &str, &str),
    edited: bool,
    tiles: impl FnOnce(&mut egui::Ui, f32),
) {
    let marked = format!("{title} •");
    let section = egui::CollapsingHeader::new(if edited { marked.as_str() } else { title })
        .id_salt(salt)
        .default_open(false)
        .show(ui, |ui| style::tiles(ui, tiles));
    let header = section.header_response.on_hover_text(hover);
    // A screen reader hears the edit dot as "Changed".
    if edited {
        style::named_control(header, format!("{title}, Changed"));
    }
}

/// What a card's Unconfirmed section says it holds.
const UNCONFIRMED: &str = "Values whose role is not yet traced";

/// A card's More section, for the values most authors leave alone.
fn more_fold(
    ui: &mut egui::Ui,
    salt: impl std::hash::Hash + std::fmt::Debug,
    (properties, lanes): (&[Property], &[&RowLane]),
    values: &mut Vec<WeaponRuntimeValueOverride>,
) {
    let tucked = properties
        .iter()
        .enumerate()
        .filter(|(_, property)| {
            (more(property) || idle(property, properties, values)) && !shadowed(lanes, property)
        })
        .collect::<Vec<_>>();
    if tucked.is_empty() {
        return;
    }
    let edited = tucked
        .iter()
        .any(|(_, property)| property.is_modified(values));
    let name = (salt, "More", "Values most abilities keep as they are");
    fold(ui, name, edited, |ui, width| {
        for (index, property) in tucked {
            let inactive = idle(property, properties, values);
            paired_tile(ui, width, index, (property, properties), values, inactive);
        }
    });
}

/// A card's folds after its tiles: More for the values most authors leave alone, then
/// Unconfirmed for the values whose role no native consumer establishes.
fn card_folds(
    ui: &mut egui::Ui,
    salt: impl std::hash::Hash + std::fmt::Debug + Copy,
    (properties, lanes): (&[Property], &[&RowLane]),
    values: &mut Vec<WeaponRuntimeValueOverride>,
) {
    more_fold(ui, (salt, "more"), (properties, lanes), values);
    let unconfirmed = properties
        .iter()
        .enumerate()
        .filter(|(_, property)| !confirmed(property))
        .collect::<Vec<_>>();
    if unconfirmed.is_empty() {
        return;
    }
    let edited = unconfirmed
        .iter()
        .any(|(_, property)| property.is_modified(values));
    let name = ((salt, "unconfirmed"), "Unconfirmed", UNCONFIRMED);
    fold(ui, name, edited, |ui, width| {
        for (index, property) in unconfirmed {
            property_tile(ui, width, index, property, values);
        }
    });
}

/// A movement value's bits as a number: a distance's float, or a count.
fn movement_value(movement: &MovementValue, bits: u32) -> f32 {
    movement.unit.value(bits)
}

/// An ability's loaded tree, with the keys its entry applies to its own row.
pub(super) struct Loaded {
    entity: u32,
    tree: Arc<Tree>,
    keys: Vec<u32>,
}

impl Loaded {
    /// The traced lanes of the bank rows the entry's own keys apply.
    fn lanes(&self) -> Vec<&RowLane> {
        self.tree
            .lanes
            .iter()
            .filter(|lane| self.keys.contains(&lane.key))
            .collect()
    }

    /// Whether a card stands for the ability's own graph.
    fn is_own(&self, card: &Card) -> bool {
        card.graphs.iter().any(|(tag, _)| *tag == self.entity)
    }

    /// The damage types its graphs' damage profiles deal, as the client encodes them.
    pub(super) fn damage_types(&self) -> &BTreeSet<u8> {
        &self.tree.damage
    }

    /// Whether the ability's own graph has values for its Ability card.
    pub(super) fn has_own_values(&self) -> bool {
        !self.lanes().is_empty()
            || self
                .tree
                .cards
                .iter()
                .any(|card| self.is_own(card) && !card.properties.is_empty())
    }
}

/// Whether a value a lane of the entry's keys sets stands for `property`, which then shows only
/// as the lane.
fn shadowed(lanes: &[&RowLane], property: &Property) -> bool {
    lanes.iter().any(|lane| {
        lane.label == property.label
            && matches!(
                lane.label,
                "Airborne Jumps"
                    | "Impulse Height Limit"
                    | "Active Energy Rate"
                    | "Velocity Blending"
            )
    })
}

/// The values of the ability's own graph.
fn own_properties(loaded: &Loaded) -> &[Property] {
    loaded
        .tree
        .cards
        .iter()
        .find(|card| loaded.is_own(card))
        .map_or(&[][..], |card| card.properties.as_slice())
}

/// The ability's own values as tiles of its Ability card: its graph's values, then the lanes of
/// the bank rows its entry's own keys apply. Those kept under More and those whose role is
/// unconfirmed follow the card's tiles in `own_folds`.
pub(super) fn own_tiles(ui: &mut egui::Ui, width: f32, loaded: &Loaded, edits: &mut EntryEdits) {
    let lanes = loaded.lanes();
    let properties = own_properties(loaded);
    for (index, property) in properties.iter().enumerate() {
        if shown(property, properties, &lanes, &edits.ability_values) {
            paired_tile(
                ui,
                width,
                index,
                (property, properties),
                &mut edits.ability_values,
                false,
            );
        }
    }
    for (index, lane) in lanes.iter().enumerate() {
        if lane.traced {
            lane_tile(ui, width, properties.len() + index, lane, edits);
        }
    }
}

/// The Ability card's closed sections after its tiles: More, then the values and bank lanes whose
/// role is unconfirmed.
pub(super) fn own_folds(ui: &mut egui::Ui, loaded: &Loaded, edits: &mut EntryEdits) {
    let lanes = loaded.lanes();
    let properties = own_properties(loaded);
    let salt = ("subclass-own", loaded.entity);
    more_fold(
        ui,
        (salt, "more"),
        (properties, &lanes),
        &mut edits.ability_values,
    );
    let hidden = properties
        .iter()
        .enumerate()
        .filter(|(_, property)| !confirmed(property) && !shadowed(&lanes, property))
        .collect::<Vec<_>>();
    let hidden_lanes = lanes
        .iter()
        .enumerate()
        .filter(|(_, lane)| !lane.traced)
        .collect::<Vec<_>>();
    if hidden.is_empty() && hidden_lanes.is_empty() {
        return;
    }
    let edited = hidden
        .iter()
        .any(|(_, property)| property.is_modified(&edits.ability_values))
        || hidden_lanes
            .iter()
            .any(|(_, lane)| edits.bank_value(lane.key, lane.row, lane.lane).is_some());
    let name = ((salt, "unconfirmed"), "Unconfirmed", UNCONFIRMED);
    fold(ui, name, edited, |ui, width| {
        for (index, property) in hidden {
            property_tile(ui, width, index, property, &mut edits.ability_values);
        }
        for (index, lane) in hidden_lanes {
            lane_tile(ui, width, properties.len() + index, lane, edits);
        }
    });
}

impl PackageAuthoringApp {
    /// The ability's tree, starting its load on first use. `None` while it loads.
    pub(super) fn load_properties(
        &self,
        ctx: &egui::Context,
        (entity, keys): (u32, Vec<u32>),
        page: &mut PageState,
    ) -> Option<Result<Loaded, String>> {
        let tree = page.properties.poll(ctx, &self.packages, entity)?;
        Some(tree.map(|tree| Loaded { entity, tree, keys }))
    }

    /// The Gameplay section's parts: a card for each graph the ability spawns with values whose
    /// meaning is established. The ability's own values sit on its Ability card. Returns the
    /// entry's edits once one changes.
    pub(super) fn draw_properties(
        &self,
        ui: &mut egui::Ui,
        loaded: &Loaded,
        edits: &EntryEdits,
        page: &mut PageState,
    ) -> Option<EntryEdits> {
        let tree = &loaded.tree;
        let donors = if tree.cards.iter().any(|card| card.projectile) {
            page.properties
                .donors(ui.ctx(), &self.packages, || self.stock_abilities())
        } else {
            None
        };
        let workbench = &self.perk_workbench;
        let catalog_name = |graph: u32| {
            let name = workbench.asset_name(graph, "");
            (!name.is_empty()).then_some(name)
        };
        let sources = Sources {
            donors: donors.as_ref(),
            catalog: workbench.asset_catalog(),
            name: &catalog_name,
        };
        // A swapped projectile shows what it fires now, and the graphs below it, which nothing
        // spawns any more, show nothing.
        let replaced = |tag: u32| {
            tree.parents
                .get(&tag)
                .is_some_and(|parent| edits.swap(*parent, tag).is_some())
        };
        let gone = |tag: u32| {
            let mut at = tag;
            while let Some(&parent) = tree.parents.get(&at) {
                if replaced(parent) {
                    return true;
                }
                at = parent;
            }
            false
        };
        let own_graphs = tree_graphs(tree);
        let mut next = edits.clone();
        let show_alternatives = page.properties.show_alternatives;
        for (index, card) in tree.cards.iter().enumerate() {
            if loaded.is_own(card) || card.graphs.iter().all(|(tag, _)| gone(*tag)) {
                continue;
            }
            // An alternative shows on request, or once it holds an edit.
            if card.alternative && !show_alternatives && !card_edited(card, edits) {
                continue;
            }
            let swapped = card.graphs.first().is_some_and(|(tag, _)| replaced(*tag));
            // The projectile swapped in shows its own tiles, which go to its private copy.
            let swapped_in = card
                .graphs
                .first()
                .and_then(|(tag, parent)| edits.swap((*parent)?, *tag))
                .map(|replacement| {
                    let tree = page.properties.poll(ui.ctx(), &self.packages, replacement);
                    (replacement, tree)
                });
            ui.push_id(("subclass-property-card", index), |ui| {
                nested(ui, card.depth, |ui| {
                    style::card(ui, |ui| {
                        card_header(ui, card);
                        if card.projectile {
                            swap_row(
                                ui,
                                (card, &own_graphs),
                                &sources,
                                &mut page.properties,
                                &mut next,
                            );
                        }
                        match &swapped_in {
                            Some((replacement, Some(Ok(tree)))) => {
                                if let Some(root) = root_card(tree, *replacement) {
                                    card_tiles(
                                        ui,
                                        ("swapped-in", index),
                                        root,
                                        &mut next.ability_values,
                                    );
                                }
                            }
                            Some((_, Some(Err(error)))) => {
                                ui.colored_label(
                                    ui.visuals().warn_fg_color,
                                    "Properties unavailable.",
                                )
                                .on_hover_text(error.as_str());
                            }
                            Some((_, None)) => {
                                ui.weak("Loading…");
                            }
                            None => {}
                        }
                        if !swapped && !card.properties.is_empty() {
                            let properties = &card.properties;
                            style::tiles(ui, |ui, width| {
                                for (index, property) in properties.iter().enumerate() {
                                    if shown(property, properties, &[], &next.ability_values) {
                                        paired_tile(
                                            ui,
                                            width,
                                            index,
                                            (property, properties),
                                            &mut next.ability_values,
                                            false,
                                        );
                                    }
                                }
                            });
                            card_folds(
                                ui,
                                ("subclass-card", index),
                                (properties, &[]),
                                &mut next.ability_values,
                            );
                        }
                    })
                });
            });
            // What the projectile swapped in spawns follows its card, as the ability's own do.
            if let Some((replacement, Some(Ok(tree)))) = &swapped_in {
                draw_tree_cards(
                    ui,
                    ("subclass-swapped-in", index),
                    tree,
                    (card.depth, Some(*replacement)),
                    &mut next.ability_values,
                );
            }
        }
        let alternatives = tree.cards.iter().filter(|card| card.alternative).count();
        if alternatives > 0 {
            ui.checkbox(
                &mut page.properties.show_alternatives,
                format!("Show Alternatives ({alternatives})"),
            )
            .on_hover_text("Parts that spawn only when a perk or node selects them");
        }
        (next != *edits).then_some(next)
    }
}

/// How far a card nests under the card that spawns it, per level.
const NEST_INDENT: f32 = 18.0;
/// Levels past this one nest no further, so a deep tree keeps its cards wide.
const DEEPEST_NEST: usize = 3;

/// Draws `content` nested `depth` levels in.
fn nested<R>(ui: &mut egui::Ui, depth: usize, content: impl FnOnce(&mut egui::Ui) -> R) -> R {
    if depth == 0 {
        return content(ui);
    }
    ui.horizontal_top(|ui| {
        ui.add_space(NEST_INDENT * depth.min(DEEPEST_NEST) as f32);
        ui.vertical(content).inner
    })
    .inner
}

/// Whether a card holds an edit: a changed value, or a projectile it fires instead.
fn card_edited(card: &Card, edits: &EntryEdits) -> bool {
    card.properties
        .iter()
        .any(|property| property.is_modified(&edits.ability_values))
        || card
            .graphs
            .iter()
            .any(|(tag, parent)| parent.is_some_and(|parent| edits.swap(parent, *tag).is_some()))
}

/// One lane of a bank row the entry's key applies: its name over its field, and the stock value
/// beside the name once it changes. The edit changes the ability's private copy of the bank.
fn lane_tile(ui: &mut egui::Ui, width: f32, index: usize, lane: &RowLane, edits: &mut EntryEdits) {
    let own = edits.bank_value(lane.key, lane.row, lane.lane);
    let unit = lane.unit;
    let stock = f64::from(unit.value(lane.stock));
    let stock_text = movement_text(unit, stock);
    let hint = if lane.traced {
        movement_hint(lane.label)
    } else {
        "Role unconfirmed"
    };
    let original = own.is_some().then_some(stock_text.as_str());
    let (edited, reset) =
        style::stock_tile(ui, (width, index), (lane.label, hint), original, |ui| {
            if unit == Unit::Mode {
                return movement_mode(
                    ui,
                    ("bank-mode", index),
                    width,
                    lane.label,
                    unit.value(own.unwrap_or(lane.stock)),
                )
                .map(|value| unit.bits(value));
            }
            let size = egui::vec2(width, ui.spacing().interact_size.y);
            let mut value = f64::from(unit.value(own.unwrap_or(lane.stock)));
            let field = movement_field(&mut value, unit, lane.label);
            let response = style::named_control(ui.add_sized(size, field), lane.label);
            (response.changed() && value.is_finite()).then(|| unit.bits(value as f32))
        });
    if let Some(bits) = edited {
        edits.set_bank_value(
            lane.key,
            lane.row,
            lane.lane,
            (bits != lane.stock).then_some(bits),
        );
    } else if reset {
        edits.set_bank_value(lane.key, lane.row, lane.lane, None);
    }
}

/// A card's title, its graphs in the title's tooltip, and Alternative on a card that spawns only
/// when a perk or node selects it. Where it comes from shows by its nesting.
fn card_header(ui: &mut egui::Ui, card: &Card) {
    ui.horizontal(|ui| {
        let tags = card
            .graphs
            .iter()
            .map(|(tag, _)| format!("0x{tag:08X}"))
            .collect::<Vec<_>>()
            .join(", ");
        ui.label(egui::RichText::new(&card.title).strong())
            .on_hover_text(tags);
        if card.graphs.len() > 1 {
            ui.weak(format!("{} Matching Parts", card.graphs.len()))
                .on_hover_text(
                    "These controls change every matching part. Technical edits individual parts.",
                );
        }
        if card.alternative {
            ui.label(quiet(ui, "Alternative"))
                .on_hover_text("Spawns only when a perk or node selects it");
        }
    });
}

/// Every graph a tree's cards stand for.
fn tree_graphs(tree: &Tree) -> BTreeSet<u32> {
    tree.cards
        .iter()
        .flat_map(|card| card.graphs.iter().map(|(tag, _)| *tag))
        .collect()
}

/// The card that stands for `graph` in `tree`.
fn root_card(tree: &Tree, graph: u32) -> Option<&Card> {
    tree.cards
        .iter()
        .find(|card| card.graphs.iter().any(|(tag, _)| *tag == graph))
}

/// A card's tiles and closed sections, for a graph outside the ability's own tree.
fn card_tiles(
    ui: &mut egui::Ui,
    salt: impl std::hash::Hash + std::fmt::Debug + Copy,
    card: &Card,
    values: &mut Vec<WeaponRuntimeValueOverride>,
) {
    let properties = &card.properties;
    style::tiles(ui, |ui, width| {
        for (index, property) in properties.iter().enumerate() {
            if shown(property, properties, &[], values) {
                paired_tile(ui, width, index, (property, properties), values, false);
            }
        }
    });
    card_folds(ui, salt, (properties, &[]), values);
}

/// The cards of `tree`, a graph outside the ability's own tree, such as a projectile swapped in or
/// the one a weapon fires, nested `depth` levels past their own nesting, without `skip`'s card.
/// Alternatives and swaps stay with an ability's own cards.
fn draw_tree_cards(
    ui: &mut egui::Ui,
    salt: impl std::hash::Hash + std::fmt::Debug + Copy,
    tree: &Tree,
    (depth, skip): (usize, Option<u32>),
    values: &mut Vec<WeaponRuntimeValueOverride>,
) {
    for (index, card) in tree.cards.iter().enumerate() {
        let skipped = skip.is_some_and(|graph| card.graphs.iter().any(|(tag, _)| *tag == graph));
        if skipped || card.alternative || card.properties.is_empty() {
            continue;
        }
        ui.push_id((salt, index), |ui| {
            nested(ui, depth + card.depth, |ui| {
                style::card(ui, |ui| {
                    card_header(ui, card);
                    card_tiles(ui, (salt, index), card, values);
                });
            });
        });
    }
}

/// Property cards for a graph outside any ability, such as the projectile a weapon fires: its
/// tree, loaded once, with the tiles an ability's Gameplay cards show.
#[derive(Default)]
pub(in crate::app) struct GraphCards(Properties);

impl GraphCards {
    /// Whether `graph`'s tree holds a card with a tile, once it has loaded.
    pub(in crate::app) fn has_cards(
        &mut self,
        ctx: &egui::Context,
        packages: &Path,
        graph: u32,
    ) -> Option<bool> {
        match self.0.poll(ctx, packages, graph)? {
            Ok(tree) => Some(
                tree.cards
                    .iter()
                    .any(|card| !card.alternative && !card.properties.is_empty()),
            ),
            Err(_) => Some(true),
        }
    }

    /// Draws `graph`'s cards, editing `values`.
    pub(in crate::app) fn draw(
        &mut self,
        ui: &mut egui::Ui,
        packages: &Path,
        graph: u32,
        values: &mut Vec<WeaponRuntimeValueOverride>,
    ) {
        match self.0.poll(ui.ctx(), packages, graph) {
            Some(Ok(tree)) => {
                draw_tree_cards(ui, ("graph-cards", graph), &tree, (0, None), values);
            }
            Some(Err(error)) => {
                ui.colored_label(ui.visuals().warn_fg_color, "Properties unavailable.")
                    .on_hover_text(error);
            }
            None => {
                ui.weak("Loading…");
            }
        }
    }
}

/// One property's tile: its name over its field, and the stock value beside the name once it
/// changes.
fn property_tile(
    ui: &mut egui::Ui,
    width: f32,
    index: usize,
    property: &Property,
    values: &mut Vec<WeaponRuntimeValueOverride>,
) {
    property_tile_state(ui, width, index, property, values, false);
}

fn paired_tile(
    ui: &mut egui::Ui,
    width: f32,
    index: usize,
    (property, siblings): (&Property, &[Property]),
    values: &mut Vec<WeaponRuntimeValueOverride>,
    inactive: bool,
) {
    let before = property.current(values);
    property_tile_state(ui, width, index, property, values, inactive);
    keep_ordered(property, before, siblings, values);
}

fn property_tile_state(
    ui: &mut egui::Ui,
    width: f32,
    index: usize,
    property: &Property,
    values: &mut Vec<WeaponRuntimeValueOverride>,
    inactive: bool,
) {
    property.keep_unlimited(values);
    let stock = property.reading(f64::from(property.stock));
    let current = property.current(values);
    let original = property.is_modified(values).then_some(stock.as_str());
    let name = (property.label.as_str(), property.hint.as_str());
    let (edited, reset) = style::stock_tile(ui, (width, index), name, original, |ui| {
        property_state_hint(ui, property, values, inactive);
        if let Value::Movement(movement) = &property.value
            && movement[0].unit == Unit::Mode
        {
            return movement_mode(
                ui,
                ("movement-mode", index),
                width,
                &property.label,
                current,
            );
        }
        // A flag is a checkbox, on as one and off as zero.
        if let Value::Setting(settings) = &property.value
            && settings[0].kind.unit() == SettingUnit::Flag
        {
            let mut on = current >= 0.5;
            let label = if on { "On" } else { "Off" };
            let response = style::named_control(ui.checkbox(&mut on, label), &property.label);
            return response.changed().then_some(if on { 1.0 } else { 0.0 });
        }
        if let Value::Setting(settings) = &property.value {
            let names = match settings[0].kind {
                SettingKind::ExpirationResponse => Some(&EXPIRATION_RESPONSES),
                SettingKind::CollisionMode => Some(&COLLISION_MODES),
                _ => None,
            };
            if let Some(names) = names {
                return setting_choice(
                    ui,
                    (index, width),
                    (settings[0].kind, &property.label),
                    current,
                    names,
                );
            }
        }
        let size = egui::vec2(width, ui.spacing().interact_size.y);
        let mut value = f64::from(current);
        // Below zero is Unlimited, for a timer that can be.
        let unlimited = property.can_be_unlimited();
        let floor = if unlimited { -1.0 } else { 0.0 };
        let field = match &property.value {
            Value::Duration(_) => egui::DragValue::new(&mut value)
                .speed(0.1)
                .range(floor..=3600.0)
                .clamp_existing_to_range(false)
                .custom_formatter(move |value, _| duration_text(value))
                .custom_parser(parse_duration),
            Value::Timer { scaling: false, .. } => egui::DragValue::new(&mut value)
                .speed(0.1)
                .range(-60.0..=60.0)
                .clamp_existing_to_range(false)
                .custom_formatter(|value, _| format!("{} s", number(value)))
                .custom_parser(parse_number),
            Value::Timer { scaling: true, .. } => egui::DragValue::new(&mut value)
                .speed(0.05)
                .range(-10.0..=10.0)
                .clamp_existing_to_range(false)
                .custom_formatter(|value, _| number(value))
                .custom_parser(parse_number),
            Value::Flight(parameters) if parameters[0].kind == Kind::TravelDistance => {
                egui::DragValue::new(&mut value)
                    .speed(1.0)
                    .custom_formatter(|value, _| travel_text(value))
                    .custom_parser(parse_travel)
            }
            Value::Flight(parameters) if parameters[0].kind == Kind::Speed => {
                egui::DragValue::new(&mut value)
                    .speed(0.01)
                    .custom_formatter(|value, _| speed_text(value))
                    .custom_parser(parse_speed)
            }
            Value::Flight(_) | Value::Amount { multiply: true, .. } => {
                egui::DragValue::new(&mut value)
                    .speed(0.01)
                    .custom_formatter(|value, _| format!("×{}", number(value)))
                    .custom_parser(parse_amount)
            }
            Value::Amount {
                multiply: false, ..
            } => egui::DragValue::new(&mut value)
                .speed(0.05)
                .custom_formatter(|value, _| signed(value))
                .custom_parser(parse_amount),
            Value::Field(_) => egui::DragValue::new(&mut value).speed(0.01).max_decimals(3),
            Value::Movement(movement) => {
                movement_field(&mut value, movement[0].unit, movement[0].label)
            }
            Value::Curve(curves) => {
                let (top, format): (f64, fn(f64) -> String) = match curves[0].kind {
                    CurveKind::FinalSpeed => (200.0, number),
                    CurveKind::FinalGravity => (10.0, |value| format!("×{}", number(value))),
                    CurveKind::Start | CurveKind::End => {
                        (200.0, |value| format!("{} Units", number(value)))
                    }
                };
                egui::DragValue::new(&mut value)
                    .speed(0.05)
                    .range(0.0..=top)
                    .clamp_existing_to_range(false)
                    .custom_formatter(move |value, _| format(value))
                    .custom_parser(parse_number)
            }
            Value::Setting(settings) => {
                let kind = settings[0].kind;
                let (low, high) = kind.range();
                // A count steps by whole numbers at a usable drag speed. Other values cross their
                // range in a few hundred points, so a wide one such as Turn Rate stays draggable.
                let speed = if kind.unit() == SettingUnit::Count {
                    0.05
                } else {
                    (f64::from(high - low) / 400.0).max(0.01)
                };
                egui::DragValue::new(&mut value)
                    .speed(speed)
                    .range(f64::from(low)..=f64::from(high))
                    .clamp_existing_to_range(false)
                    .custom_formatter(move |value, _| setting_text(kind, value))
                    .custom_parser(move |text| parse_setting(kind, text))
            }
        };
        let response = style::named_control(ui.add_sized(size, field), &property.label);
        let value = match property.value {
            Value::Duration(_) if value < 0.0 => UNLIMITED,
            Value::Duration(_) => value.max(0.0) as f32,
            _ => value as f32,
        };
        (response.changed() && value.is_finite()).then_some(value)
    });
    if let Some(value) = edited {
        property.set(values, value);
    } else if reset {
        property.reset(values);
    }
}

fn setting_choice(
    ui: &mut egui::Ui,
    (index, width): (usize, f32),
    (kind, label): (SettingKind, &str),
    current: f32,
    names: &[&str],
) -> Option<f32> {
    let mut value = current;
    let response = egui::ComboBox::from_id_salt(("setting-choice", index))
        .width(width)
        .selected_text(setting_text(kind, f64::from(value)))
        .show_ui(ui, |ui| {
            for (index, name) in names.iter().enumerate() {
                ui.selectable_value(&mut value, index as f32, *name);
            }
        });
    style::named_control(response.response, label);
    (value != current).then_some(value)
}

fn property_state_hint(
    ui: &mut egui::Ui,
    property: &Property,
    values: &[WeaponRuntimeValueOverride],
    inactive: bool,
) {
    if property.mixed(values) {
        ui.weak("Mixed Values").on_hover_text(
            "Changing this control updates every matching part. Use Technical to edit individual parts.",
        );
    }
    if inactive {
        ui.weak("Inactive")
            .on_hover_text("Another setting disables this value. You can still edit or reset it.");
    }
}
