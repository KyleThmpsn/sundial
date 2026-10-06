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
use sundial::package_authoring::ability_movement::{self, MovementValue, RowLane, Unit};
use sundial::package_authoring::ability_spawns::{describe, spawns, table_graphs, tables};
use sundial::package_authoring::runtime::modifiers;
use sundial::package_authoring::runtime::{WeaponRuntimeField, WeaponRuntimeValue};
use sundial::package_authoring::sandbox_perk::entity::effect_length::{self, EffectLength};
use sundial::package_authoring::sandbox_perk::entity::projectile::parameters::{
    self, Kind, Parameter,
};

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

/// What a tile changes, on every graph its card stands for.
enum Value {
    Duration(Vec<EffectLength>),
    Flight(Vec<Parameter>),
    /// A modifier's amount: a factor when it multiplies, else an amount it adds.
    Amount {
        fields: Vec<WeaponRuntimeField>,
        multiply: bool,
    },
    /// A named float of a component whose meaning is established.
    Field(Vec<WeaponRuntimeField>),
    /// A movement controller value inside an opaque field.
    Movement(Vec<MovementValue>),
}

/// One tile: its name, what its tooltip adds, the stock value and the values it changes.
struct Property {
    label: String,
    hint: String,
    stock: f32,
    value: Value,
}

impl Property {
    /// What two properties share when one tile stands for both.
    fn signature(&self) -> (String, u32, u8) {
        let kind = match &self.value {
            Value::Duration(_) => 0,
            Value::Flight(_) => 1,
            Value::Amount {
                multiply: false, ..
            } => 2,
            Value::Amount { multiply: true, .. } => 3,
            Value::Field(_) => 4,
            Value::Movement(_) => 5,
        };
        (self.label.clone(), self.stock.to_bits(), kind)
    }

    /// Takes in the values of `other`, which has this one's signature.
    fn absorb(&mut self, other: Self) {
        match (&mut self.value, other.value) {
            (Value::Duration(own), Value::Duration(more)) => own.extend(more),
            (Value::Flight(own), Value::Flight(more)) => own.extend(more),
            (Value::Amount { fields: own, .. }, Value::Amount { fields: more, .. })
            | (Value::Field(own), Value::Field(more)) => own.extend(more),
            (Value::Movement(own), Value::Movement(more)) => own.extend(more),
            _ => {}
        }
    }

    /// The value it shows: its first value's own, else the stock one.
    fn current(&self, values: &[WeaponRuntimeValueOverride]) -> f32 {
        let edited = match &self.value {
            Value::Duration(lengths) => lengths
                .first()
                .and_then(|length| own(values, &length.field))
                .and_then(|value| effect_length::seconds(&value)),
            Value::Flight(parameters) => parameters
                .first()
                .and_then(|parameter| parameter.value(values).ok()),
            Value::Amount { fields, .. } | Value::Field(fields) => fields
                .first()
                .and_then(|field| own(values, field))
                .as_ref()
                .and_then(float),
            Value::Movement(movement) => movement
                .first()
                .map(|movement| movement_value(movement, movement.bits(values))),
        };
        edited.unwrap_or(self.stock)
    }

    fn is_modified(&self, values: &[WeaponRuntimeValueOverride]) -> bool {
        match &self.value {
            Value::Duration(lengths) => lengths
                .iter()
                .any(|length| own(values, &length.field).is_some()),
            Value::Flight(parameters) => parameters
                .iter()
                .any(|parameter| parameter.is_modified(values)),
            Value::Amount { fields, .. } | Value::Field(fields) => {
                fields.iter().any(|field| own(values, field).is_some())
            }
            Value::Movement(movement) => {
                movement.iter().any(|movement| movement.is_modified(values))
            }
        }
    }

    /// Sets every value it stands for to `value`.
    fn set(&self, values: &mut Vec<WeaponRuntimeValueOverride>, value: f32) {
        match &self.value {
            Value::Duration(lengths) => {
                for length in lengths {
                    put(values, &length.field, EffectLength::encode(value));
                }
            }
            Value::Flight(parameters) => {
                if parameters
                    .iter()
                    .all(|parameter| parameter.kind.validate(value).is_ok())
                {
                    for parameter in parameters {
                        let _ = parameter.set(values, value);
                    }
                }
            }
            Value::Amount { fields, .. } | Value::Field(fields) => {
                for field in fields {
                    put(
                        values,
                        field,
                        WeaponRuntimeValue::Float32Bits(value.to_bits()),
                    );
                }
            }
            Value::Movement(movement) => {
                for movement in movement {
                    let bits = movement.unit.bits(value);
                    movement.write(values, bits);
                }
            }
        }
    }

    fn reset(&self, values: &mut Vec<WeaponRuntimeValueOverride>) {
        match &self.value {
            Value::Duration(lengths) => values.retain(|each| {
                lengths
                    .iter()
                    .all(|length| each.locator != length.field.locator)
            }),
            Value::Flight(parameters) => {
                for parameter in parameters {
                    let _ = parameter.reset(values);
                }
            }
            Value::Amount { fields, .. } | Value::Field(fields) => {
                values.retain(|each| fields.iter().all(|field| each.locator != field.locator));
            }
            Value::Movement(movement) => {
                for movement in movement {
                    movement.reset(values);
                }
            }
        }
    }

    /// `value` as this property reads: seconds, a factor, a signed amount or a distance.
    fn reading(&self, value: f64) -> String {
        match &self.value {
            Value::Duration(_) => seconds_text(value),
            Value::Flight(parameters) if parameters[0].kind == Kind::TravelDistance => {
                travel_text(value)
            }
            Value::Flight(_) | Value::Amount { multiply: true, .. } => {
                format!("×{}", number(value))
            }
            Value::Amount {
                multiply: false, ..
            } => signed(value),
            Value::Field(_) | Value::Movement(_) => number(value),
        }
    }
}

/// A movement value's bits as a number: a distance's float, or a count.
fn movement_value(movement: &MovementValue, bits: u32) -> f32 {
    movement.unit.value(bits)
}

/// One graph of the ability's tree and what it shows.
struct Node {
    tag: u32,
    parent: Option<u32>,
    projectile: bool,
    /// Whether only its spawner's ability bank names it, so it spawns only when a key selects it.
    alternative: bool,
    /// Its kind in plain words.
    name: String,
    properties: Vec<Property>,
}

/// Graphs that show the same values, under one title.
struct Card {
    name: String,
    /// Its name, numbered where names repeat.
    title: String,
    /// How many cards above it spawn its first graph, which it nests under.
    depth: usize,
    /// Each graph it stands for, with the graph that spawns it.
    graphs: Vec<(u32, Option<u32>)>,
    /// Whether its graphs are projectiles another graph's components name, which a swap can
    /// replace.
    projectile: bool,
    /// Whether only a bank names its graphs, or those of a card above it, so they spawn only when
    /// a perk or node selects them.
    alternative: bool,
    properties: Vec<Property>,
}

impl Card {
    fn signature(&self) -> (String, bool, bool, Vec<(String, u32, u8)>) {
        (
            self.name.clone(),
            self.projectile,
            self.alternative,
            self.properties.iter().map(Property::signature).collect(),
        )
    }
}

/// An ability's cards, the graph above each graph of its tree, and the traced lanes of its bank's
/// rows, which show for the keys its entry applies.
pub(super) struct Tree {
    cards: Vec<Card>,
    parents: BTreeMap<u32, u32>,
    lanes: Vec<RowLane>,
}

type Load = Result<Tree, String>;

/// Every projectile a stock ability fires, with the ability and the projectile's name.
type Donors = Result<Arc<Vec<(u32, String)>>, String>;

/// The trees loaded so far, by ability entity, and the projectiles a swap can choose.
#[derive(Default)]
pub(super) struct Properties {
    trees: BTreeMap<u32, Result<Arc<Tree>, String>>,
    loading: Option<(u32, Receiver<Load>)>,
    donors: Option<Donors>,
    donor_loading: Option<Receiver<Donors>>,
    /// Whether the cards that spawn only when a perk or node selects them show.
    show_alternatives: bool,
}

impl Properties {
    /// Takes a finished load, and starts one for `entity` when it has none. Returns the tree once
    /// it is loaded.
    fn poll(
        &mut self,
        ctx: &egui::Context,
        packages: &Path,
        entity: u32,
    ) -> Option<Result<Arc<Tree>, String>> {
        if let Some((loading, receiver)) = &self.loading {
            let loading = *loading;
            let finished = match receiver.try_recv() {
                Ok(result) => Some(result.map(Arc::new)),
                Err(mpsc::TryRecvError::Disconnected) => Some(Err("The loader stopped.".into())),
                Err(mpsc::TryRecvError::Empty) => None,
            };
            if let Some(finished) = finished {
                self.trees.insert(loading, finished);
                self.loading = None;
            }
        }
        if let Some(tree) = self.trees.get(&entity) {
            return Some(tree.clone());
        }
        if self.loading.is_none() {
            let (sender, receiver) = mpsc::channel();
            let packages = packages.to_path_buf();
            std::thread::spawn(move || {
                let _ = sender.send(load(&packages, entity));
            });
            self.loading = Some((entity, receiver));
        }
        ctx.request_repaint_after(std::time::Duration::from_millis(100));
        None
    }

    /// Every stock ability's projectiles, loading them on first use from `abilities`: each
    /// ability entity with its name.
    fn donors(
        &mut self,
        ctx: &egui::Context,
        packages: &Path,
        abilities: impl FnOnce() -> Vec<(u32, String)>,
    ) -> Option<Donors> {
        if let Some(receiver) = &self.donor_loading {
            match receiver.try_recv() {
                Ok(result) => {
                    self.donors = Some(result);
                    self.donor_loading = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.donors = Some(Err("The loader stopped.".into()));
                    self.donor_loading = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.donors.is_none() && self.donor_loading.is_none() {
            let (sender, receiver) = mpsc::channel();
            let packages = packages.to_path_buf();
            let abilities = abilities();
            std::thread::spawn(move || {
                let _ = sender.send(load_donors(&packages, &abilities).map(Arc::new));
            });
            self.donor_loading = Some(receiver);
        }
        if self.donors.is_none() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        self.donors.clone()
    }
}

/// The projectiles the stock abilities fire, each once, named by the first ability that fires it.
/// Native names are left out because projectiles share folders with enemy assets, so a grenade's
/// could read as an enemy's. An ability's projectiles are numbered in the order it spawns them,
/// those that always spawn apart from the alternatives, as its cards are.
fn load_donors(packages: &Path, abilities: &[(u32, String)]) -> Result<Vec<(u32, String)>, String> {
    let manager = open_shadowkeep_package_manager(packages)?;
    let mut found = BTreeMap::<u32, String>::new();
    for (entity, ability) in abilities {
        let mut fired = Vec::<(u32, bool)>::new();
        let mut seen = BTreeSet::new();
        let mut queue = VecDeque::from([(*entity, false, 0usize)]);
        while let Some((tag, alternative, depth)) = queue.pop_front() {
            if depth >= SPAWN_DEPTH || !seen.insert(tag) {
                continue;
            }
            let Ok(payload) = manager.read_tag(tiger_pkg::TagHash(tag)) else {
                continue;
            };
            let Ok(children) = children(&manager, tag, &payload) else {
                continue;
            };
            for (child, banked, _) in children {
                let alternative = alternative || banked;
                let projectile = manager
                    .read_tag(tiger_pkg::TagHash(child))
                    .is_ok_and(|payload| payload.get(OBJECT_TYPE) == Some(&PROJECTILE));
                if projectile
                    && !found.contains_key(&child)
                    && fired.iter().all(|(each, _)| *each != child)
                {
                    fired.push((child, alternative));
                }
                queue.push_back((child, alternative, depth + 1));
            }
        }
        for (child, alternative) in &fired {
            let count = fired.iter().filter(|(_, each)| each == alternative).count();
            let number = fired
                .iter()
                .take_while(|(each, _)| each != child)
                .filter(|(_, each)| each == alternative)
                .count()
                + 1;
            let mut label = format!("{ability} Projectile");
            if count > 1 {
                label = format!("{label} {number}");
            }
            if *alternative {
                label.push_str(" · Alternative");
            }
            found.insert(*child, label);
        }
    }
    let mut donors = found.into_iter().collect::<Vec<_>>();
    donors.sort_by(|(a_tag, a), (b_tag, b)| a.cmp(b).then(a_tag.cmp(b_tag)));
    Ok(donors)
}

fn load(packages: &Path, entity: u32) -> Load {
    let manager = open_shadowkeep_package_manager(packages)?;
    let mut nodes = Vec::new();
    let mut parents = BTreeMap::new();
    let mut seen = BTreeSet::new();
    let mut queue = VecDeque::from([(entity, None, false, true, 0usize)]);
    while let Some((tag, parent, alternative, direct, depth)) = queue.pop_front() {
        if !seen.insert(tag) {
            continue;
        }
        if let Some(parent) = parent {
            parents.insert(tag, parent);
        }
        let payload = manager
            .read_tag(tiger_pkg::TagHash(tag))
            .map_err(|error| error.to_string())?;
        let name = match parent {
            Some(_) => graph_name(&payload),
            None => "Ability".to_owned(),
        };
        nodes.push(Node {
            tag,
            parent,
            projectile: direct && payload.get(OBJECT_TYPE) == Some(&PROJECTILE),
            alternative,
            name,
            properties: properties_of(&manager, tag, &payload),
        });
        if depth < SPAWN_DEPTH {
            for (child, banked, direct) in children(&manager, tag, &payload)? {
                queue.push_back((child, Some(tag), alternative || banked, direct, depth + 1));
            }
        }
    }
    Ok(Tree {
        cards: cards(nodes, &parents),
        parents,
        lanes: bank_lanes(&manager, entity),
    })
}

/// Every traced lane of the rows of `entity`'s bank, for any key. A bank that does not read has
/// none.
fn bank_lanes(manager: &PackageManager, entity: u32) -> Vec<RowLane> {
    (|| -> Option<Vec<RowLane>> {
        let payload = manager.read_tag(tiger_pkg::TagHash(entity)).ok()?;
        let bank = sundial::package_authoring::ability_modifier::entity_bank(&payload)
            .ok()
            .flatten()?;
        let bank = manager.read_tag(tiger_pkg::TagHash(bank)).ok()?;
        let keys = sundial::package_authoring::ability_bank::property_rows(&bank)
            .ok()?
            .iter()
            .map(|row| row.key)
            .collect::<Vec<_>>();
        ability_movement::row_lanes(&bank, &keys).ok()
    })()
    .unwrap_or_default()
}

/// A multiplier's field.
fn factor_field(value: &mut f64) -> egui::DragValue<'_> {
    egui::DragValue::new(value)
        .speed(0.01)
        .range(0.0..=20.0)
        .clamp_existing_to_range(false)
        .custom_formatter(|value, _| format!("×{}", number(value)))
        .custom_parser(parse_amount)
}

/// The graphs `tag` spawns, directly or through its impact tables, each once, those its own
/// components name first, with whether only its ability bank names one and whether a component
/// names it directly. A graph only the bank names is one the bank's rows spawn when a perk or
/// node applies their key, such as Axion Bolt's alternative seekers, rather than the one it
/// throws. Fires swaps only a projectile named directly.
fn children(
    manager: &PackageManager,
    tag: u32,
    payload: &[u8],
) -> Result<Vec<(u32, bool, bool)>, String> {
    let is_bank = sundial::package_authoring::ability_modifier::is_bank;
    let mut found = Vec::<(u32, bool, bool)>::new();
    let mut add = |graph: u32, banked: bool, direct: bool| match found
        .iter_mut()
        .find(|(each, ..)| *each == graph)
    {
        Some((_, alternative, named)) => {
            *alternative &= banked;
            *named |= direct;
        }
        None => found.push((graph, banked, direct)),
    };
    for spawn in spawns(manager, tag, payload)? {
        add(spawn.graph, is_bank(spawn.owner), true);
    }
    for place in tables(manager, tag, payload)? {
        for graph in table_graphs(manager, place.graph)? {
            if graph != tag {
                add(graph, is_bank(place.owner), false);
            }
        }
    }
    found.sort_by_key(|(_, alternative, _)| *alternative);
    Ok(found)
}

/// A graph's kind in plain words: an Object for a placed prop, an Effect for one that rides on a
/// player. The card sits on its ability's page, so the kind is enough, and native names can read
/// as an enemy's. The component that sets it apart is left to its tiles, which show what it
/// changes.
fn graph_name(payload: &[u8]) -> String {
    let described = describe(payload).unwrap_or_else(|_| "Part".to_owned());
    let kind = described.split(" · ").next().unwrap_or(&described);
    match kind {
        "Prop" => "Object",
        "Hop-On" => "Effect",
        other => other,
    }
    .to_owned()
}

/// What a graph shows: how it flies, its timers that run out, its effects' modifiers whose input
/// is named, and its invisibility. Its bank's values are the Ability card's parameters.
fn properties_of(manager: &PackageManager, tag: u32, payload: &[u8]) -> Vec<Property> {
    let Ok(mut graph) = load_weapon_runtime_graph_for_entity(manager, 0, 0, tag, payload) else {
        return Vec::new();
    };
    graph.scope_fields();
    let is_bank = sundial::package_authoring::ability_modifier::is_bank;
    graph
        .resources
        .retain(|resource| !is_bank(resource.owner_tag));
    let mut found = Vec::new();
    for parameter in parameters::discover(&graph) {
        found.push(Property {
            label: flight_label(parameter.kind).to_owned(),
            hint: flight_hint(parameter.kind).to_owned(),
            stock: parameter.original(),
            value: Value::Flight(vec![parameter]),
        });
    }
    // A timer that never runs out, or runs for no time, has no length to change. It is the
    // longest the graph lasts: a Rift whose timers were tripled ended at its stock time in game.
    for length in effect_length::discover(&graph) {
        let stock = length.stock();
        if stock.is_finite() && stock > 0.0 {
            found.push(Property {
                label: "Duration".to_owned(),
                hint: "Longest it lasts. It may end sooner".to_owned(),
                stock,
                value: Value::Duration(vec![length]),
            });
        }
    }
    for movement in ability_movement::discover(&graph) {
        let hint = if !movement.traced {
            "Role unconfirmed"
        } else if movement.unit == Unit::Distance {
            "Native units"
        } else {
            ""
        };
        found.push(Property {
            label: movement.label.to_owned(),
            hint: hint.to_owned(),
            stock: movement_value(&movement, movement.stock()),
            value: Value::Movement(vec![movement]),
        });
    }
    for resource in &graph.resources {
        let roots = std::iter::once(&resource.instance).chain(resource.definition.iter());
        match resource.concrete_class {
            MODIFIERS => {
                for root in roots {
                    found.extend(modifiers_in(&root.fields));
                }
            }
            INVISIBILITY => {
                for field in roots.flat_map(|root| &root.fields) {
                    if let Some(stock) = float(&field.value)
                        && stock != 0.0
                        && named(field)
                    {
                        found.push(Property {
                            label: field.name.clone(),
                            hint: "Invisibility".to_owned(),
                            stock,
                            value: Value::Field(vec![field.clone()]),
                        });
                    }
                }
            }
            _ => {}
        }
    }
    merge(found)
}

/// A flight value's name on its projectile's card.
const fn flight_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Speed => "Speed",
        Kind::Gravity => "Gravity",
        Kind::TravelDistance => "Travel Limit",
    }
}

/// What a flight value scales, for its tile's tooltip.
const fn flight_hint(kind: Kind) -> &'static str {
    match kind {
        Kind::Speed => "Times the speed it is launched at",
        Kind::Gravity => "Times normal gravity",
        Kind::TravelDistance => "How far it flies before it ends",
    }
}

/// One settings record of a Component Modifiers root: its amount and its other numbers.
struct Record {
    amount: WeaponRuntimeField,
    numbers: Vec<(u32, i64)>,
}

impl Record {
    fn number(&self, offset: u32) -> Option<i64> {
        self.numbers
            .iter()
            .find(|(at, _)| *at == offset)
            .map(|(_, number)| *number)
    }

    /// Its tile, named by the input it changes, and by the ability for an ability's input. A
    /// record whose input has no established name, or whose operation changes nothing, has none.
    fn property(self) -> Option<Property> {
        let stock = float(&self.amount.value)?;
        let component = self.number(modifiers::COMPONENT_OFFSET)?;
        let input = self.number(modifiers::INPUT_OFFSET)?;
        let multiply = match self.number(modifiers::OPERATION_OFFSET)? {
            modifiers::OPERATION_ADD => false,
            modifiers::OPERATION_MULTIPLY => true,
            _ => return None,
        };
        let input_name = choice(modifiers::input_choices(component), input)?;
        let ability = (component == ABILITIES)
            .then(|| self.number(ABILITY_OFFSET))
            .flatten()
            .filter(|slot| *slot >= 0)
            .and_then(|slot| choice(meaning(ABILITY_OFFSET), slot));
        let label = match ability {
            Some(ability) => format!("{ability} {input_name}"),
            None => input_name.to_owned(),
        };
        let hint = choice(meaning(modifiers::COMPONENT_OFFSET), component)
            .unwrap_or_default()
            .to_owned();
        Some(Property {
            label,
            hint,
            stock,
            value: Value::Amount {
                fields: vec![self.amount],
                multiply,
            },
        })
    }
}

/// The tiles of a Component Modifiers root's records. A record is the settings record's fields
/// in order, starting at its amount.
fn modifiers_in(fields: &[WeaponRuntimeField]) -> Vec<Property> {
    let mut records: Vec<Record> = Vec::new();
    for field in fields
        .iter()
        .filter(|field| field.locator.type_handle.get() == modifiers::SETTINGS_SCHEMA)
    {
        let offset = field.locator.value_offset;
        if offset == modifiers::AMOUNT_OFFSET {
            records.push(Record {
                amount: field.clone(),
                numbers: Vec::new(),
            });
        } else if let Some(record) = records.last_mut()
            && let Some(number) = integer(&field.value)
        {
            record.numbers.push((offset, number));
        }
    }
    records.into_iter().filter_map(Record::property).collect()
}

/// The names the settings record gives the values at `offset`.
fn meaning(offset: u32) -> &'static [(i64, &'static str)] {
    match modifiers::field_meaning(modifiers::SETTINGS_SCHEMA, offset) {
        Some(meaning) => meaning.choices,
        None => &[],
    }
}

fn choice(choices: &'static [(i64, &'static str)], value: i64) -> Option<&'static str> {
    choices
        .iter()
        .find(|(each, _)| *each == value)
        .map(|(_, name)| *name)
}

/// Properties that share a name and stock value as one tile, then repeated names numbered.
fn merge(found: Vec<Property>) -> Vec<Property> {
    let mut merged: Vec<Property> = Vec::new();
    for property in found {
        let signature = property.signature();
        match merged.iter().position(|each| each.signature() == signature) {
            Some(index) => merged[index].absorb(property),
            None => merged.push(property),
        }
    }
    let mut counts = BTreeMap::<String, usize>::new();
    for property in &merged {
        *counts.entry(property.label.clone()).or_default() += 1;
    }
    let mut seen = BTreeMap::<String, usize>::new();
    for property in &mut merged {
        if counts.get(&property.label).is_some_and(|count| *count > 1) {
            let number = seen.entry(property.label.clone()).or_default();
            *number += 1;
            if *number > 1 {
                property.label = format!("{} {number}", property.label);
            }
        }
    }
    merged
}

/// The graphs with something to show as cards, in the order the ability spawns them. Graphs with
/// the same name and values share one. Names that repeat are numbered after their kind, and each
/// card names the card of the nearest graph above it that has one.
fn cards(nodes: Vec<Node>, parents: &BTreeMap<u32, u32>) -> Vec<Card> {
    let mut cards: Vec<Card> = Vec::new();
    for node in nodes {
        let projectile = node.projectile && node.parent.is_some();
        if !projectile && node.properties.is_empty() {
            continue;
        }
        let card = Card {
            title: String::new(),
            depth: 0,
            name: node.name,
            graphs: vec![(node.tag, node.parent)],
            projectile,
            alternative: node.alternative,
            properties: node.properties,
        };
        let signature = card.signature();
        match cards.iter().position(|each| each.signature() == signature) {
            Some(index) => {
                let shared = &mut cards[index];
                shared.graphs.extend(card.graphs);
                for (own, more) in shared.properties.iter_mut().zip(card.properties) {
                    own.absorb(more);
                }
            }
            None => cards.push(card),
        }
    }
    let owners = cards
        .iter()
        .enumerate()
        .flat_map(|(index, card)| card.graphs.iter().map(move |(tag, _)| (*tag, index)))
        .collect::<BTreeMap<_, _>>();
    // The card above each one. Everything comes from the ability, so its own card is above none.
    let above = cards
        .iter()
        .map(|card| {
            let mut at = card.graphs.first()?.0;
            while let Some(&parent) = parents.get(&at) {
                if let Some(&index) = owners.get(&parent) {
                    return parents.contains_key(&parent).then_some(index);
                }
                at = parent;
            }
            None
        })
        .collect::<Vec<_>>();
    // Cards come in the order the ability spawns them, so a card's parent is already placed.
    for (index, parent) in above.into_iter().enumerate() {
        if let Some(parent) = parent.filter(|parent| *parent < index) {
            let (depth, alternative) = (cards[parent].depth, cards[parent].alternative);
            cards[index].depth = depth + 1;
            cards[index].alternative |= alternative;
        }
    }
    // Names that repeat are numbered, the cards that always spawn apart from the alternatives.
    let key = |card: &Card| (card.name.clone(), card.alternative);
    let mut counts = BTreeMap::<(String, bool), usize>::new();
    for card in &cards {
        *counts.entry(key(card)).or_default() += 1;
    }
    let mut seen = BTreeMap::<(String, bool), usize>::new();
    for card in &mut cards {
        card.title = if counts.get(&key(card)).is_some_and(|count| *count > 1) {
            let number = seen.entry(key(card)).or_default();
            *number += 1;
            match card.name.split_once(" · ") {
                Some((kind, rest)) => format!("{kind} {number} · {rest}"),
                None => format!("{} {number}", card.name),
            }
        } else {
            card.name.clone()
        };
    }
    cards
}

/// Sets the override for one field, or removes it when the value is stock.
fn put(
    values: &mut Vec<WeaponRuntimeValueOverride>,
    field: &WeaponRuntimeField,
    value: WeaponRuntimeValue,
) {
    values.retain(|each| each.locator != field.locator);
    if value != field.value {
        values.push(WeaponRuntimeValueOverride {
            locator: field.locator.clone(),
            value,
        });
    }
}

fn own(
    values: &[WeaponRuntimeValueOverride],
    field: &WeaponRuntimeField,
) -> Option<WeaponRuntimeValue> {
    values
        .iter()
        .find(|each| each.locator == field.locator)
        .map(|each| each.value.clone())
}

fn float(value: &WeaponRuntimeValue) -> Option<f32> {
    match value {
        WeaponRuntimeValue::Float32Bits(bits) => Some(f32::from_bits(*bits)),
        _ => None,
    }
}

fn integer(value: &WeaponRuntimeValue) -> Option<i64> {
    match value {
        WeaponRuntimeValue::Signed(number) => Some(*number),
        WeaponRuntimeValue::Unsigned(number) => i64::try_from(*number).ok(),
        _ => None,
    }
}

/// A field whose name came from the game or a traced reader, not a placeholder.
fn named(field: &WeaponRuntimeField) -> bool {
    !["Unnamed", "Member 0x", "Unreflected", "Value 0x", "M "]
        .iter()
        .any(|prefix| field.name.starts_with(prefix))
}

/// A number with up to three decimals.
fn number(value: f64) -> String {
    egui::emath::format_with_decimals_in_range(value, 0..=3)
}

/// An amount a modifier adds, with its sign.
fn signed(value: f64) -> String {
    if value < 0.0 {
        number(value)
    } else {
        format!("+{}", number(value))
    }
}

/// Seconds as a duration field reads them: a negative length never runs out.
fn seconds_text(seconds: f64) -> String {
    if seconds < 0.0 {
        "Unlimited".to_owned()
    } else {
        format!(
            "{} s",
            egui::emath::format_with_decimals_in_range(seconds, 0..=2)
        )
    }
}

fn parse_seconds(text: &str) -> Option<f64> {
    let text = text.trim().to_lowercase();
    if text.starts_with("unl") {
        return Some(-1.0);
    }
    text.trim_end_matches('s').trim().parse().ok()
}

/// A travel distance limit: zero sets none.
fn travel_text(distance: f64) -> String {
    if distance <= 0.0 {
        "No Limit".to_owned()
    } else {
        format!("{} Units", number(distance))
    }
}

fn parse_travel(text: &str) -> Option<f64> {
    let text = text.trim().to_lowercase();
    if text.starts_with("no") {
        return Some(0.0);
    }
    text.trim_end_matches("units").trim().parse().ok()
}

fn parse_amount(text: &str) -> Option<f64> {
    text.trim()
        .trim_start_matches(['×', 'x', '+'])
        .trim()
        .parse()
        .ok()
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
    lanes.iter().any(|lane| lane.label == property.label)
}

/// The ability's own values as tiles of its Ability card: its graph's values, then the lanes of
/// the bank rows its entry's own keys apply.
pub(super) fn own_tiles(ui: &mut egui::Ui, width: f32, loaded: &Loaded, edits: &mut EntryEdits) {
    let lanes = loaded.lanes();
    let properties = loaded
        .tree
        .cards
        .iter()
        .find(|card| loaded.is_own(card))
        .map_or(&[][..], |card| card.properties.as_slice());
    for (index, property) in properties.iter().enumerate() {
        if !shadowed(&lanes, property) {
            property_tile(ui, width, index, property, &mut edits.ability_values);
        }
    }
    for (index, lane) in lanes.iter().enumerate() {
        lane_tile(ui, width, properties.len() + index, lane, edits);
    }
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
        let lanes = loaded.lanes();
        let donors = if tree.cards.iter().any(|card| card.projectile) {
            page.properties
                .donors(ui.ctx(), &self.packages, || self.stock_abilities())
        } else {
            None
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
            ui.push_id(("subclass-property-card", index), |ui| {
                nested(ui, card.depth, |ui| {
                    style::card(ui, |ui| {
                        card_header(ui, card);
                        if card.projectile {
                            swap_row(ui, card, donors.as_ref(), &mut next);
                        }
                        if !swapped && !card.properties.is_empty() {
                            style::tiles(ui, |ui, width| {
                                for (index, property) in card.properties.iter().enumerate() {
                                    if !shadowed(&lanes, property) {
                                        property_tile(
                                            ui,
                                            width,
                                            index,
                                            property,
                                            &mut next.ability_values,
                                        );
                                    }
                                }
                            });
                        }
                    })
                });
            });
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

/// One traced lane of a bank row the entry's key applies: its name over its field, the stock
/// value in the name's tooltip. The edit changes the ability's private copy of the bank.
fn lane_tile(ui: &mut egui::Ui, width: f32, index: usize, lane: &RowLane, edits: &mut EntryEdits) {
    let own = edits.bank_value(lane.key, lane.row, lane.lane);
    let unit = lane.unit;
    let stock = f64::from(unit.value(lane.stock));
    let stock_text = match unit {
        Unit::Count => format!("Stock {}", number(stock)),
        Unit::Distance | Unit::Factor => format!("Stock ×{}", number(stock)),
    };
    let hint = if lane.traced {
        stock_text
    } else {
        format!(
            "Role unconfirmed
{stock_text}"
        )
    };
    let (edited, reset) = style::tile(ui, width, index, lane.label, &hint, own.is_some(), |ui| {
        let size = egui::vec2(width, ui.spacing().interact_size.y);
        let mut value = f64::from(unit.value(own.unwrap_or(lane.stock)));
        let field = match unit {
            Unit::Count => egui::DragValue::new(&mut value)
                .speed(0.1)
                .range(1.0..=10.0)
                .clamp_existing_to_range(false)
                .fixed_decimals(0),
            Unit::Distance | Unit::Factor => factor_field(&mut value),
        };
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
        if card.alternative {
            ui.label(quiet(ui, "Alternative"))
                .on_hover_text("Spawns only when a perk or node selects it");
        }
    });
}

/// The projectile a card's graphs fire: the original, or another stock ability's, chosen in the
/// browser the perk workbench picks projectiles with.
fn swap_row(ui: &mut egui::Ui, card: &Card, donors: Option<&Donors>, edits: &mut EntryEdits) {
    let swaps = card
        .graphs
        .iter()
        .filter_map(|(tag, parent)| Some(((*parent)?, *tag)))
        .collect::<Vec<_>>();
    let Some(&(parent, tag)) = swaps.first() else {
        return;
    };
    let current = edits.swap(parent, tag);
    let named = |each: u32| {
        donors
            .and_then(|donors| donors.as_ref().ok())
            .and_then(|donors| donors.iter().find(|(donor, _)| *donor == each))
            .map(|(_, name)| name.clone())
    };
    // The projectile it fires before any swap, by the ability that fires it.
    let original = named(tag).unwrap_or_else(|| card.title.clone());
    let shown = current.map_or_else(
        || original.clone(),
        |each| named(each).unwrap_or_else(|| format!("Projectile 0x{each:08X}")),
    );
    let mut chosen = current;
    ui.horizontal(|ui| {
        ui.label(quiet(ui, "Fires"));
        let picked = crate::app::pickers::browser(
            ui,
            ("subclass-swap", parent, tag),
            &shown,
            "Choose a Projectile",
            &mut String::new(),
            |ui, query, reset, height| {
                swap_choices(
                    ui,
                    (card, donors),
                    (&original, current),
                    (query, reset, height),
                )
            },
        );
        if let Some(picked) = picked {
            chosen = picked;
        }
        if current.is_some() && detail::reset_icon(ui) {
            chosen = None;
        }
    });
    if chosen != current {
        for &(parent, tag) in &swaps {
            edits.set_swap(parent, tag, chosen);
        }
    }
}

/// The Fires browser's listing: the original first, then each stock ability's projectile the
/// search matches. Names that repeat, from abilities two subclasses both name, are numbered as
/// the workbench numbers its variants. Returns the choice once used: `None` for the original.
fn swap_choices(
    ui: &mut egui::Ui,
    (card, donors): (&Card, Option<&Donors>),
    (original, current): (&str, Option<u32>),
    (query, reset, height): (&str, bool, f32),
) -> Option<Option<u32>> {
    let donors = match donors {
        Some(Ok(donors)) => donors,
        Some(Err(error)) => {
            ui.weak("Projectiles unavailable.")
                .on_hover_text(error.as_str());
            return None;
        }
        None => {
            ui.weak("Loading…");
            return None;
        }
    };
    let own = |donor: u32| card.graphs.iter().any(|(tag, _)| *tag == donor);
    let others = donors
        .iter()
        .filter(|(donor, _)| !own(*donor))
        .collect::<Vec<_>>();
    let mut counts = BTreeMap::<&str, usize>::new();
    for (_, label) in &others {
        *counts.entry(label.as_str()).or_default() += 1;
    }
    let mut seen = BTreeMap::<&str, usize>::new();
    let mut rows = vec![(None, original.to_owned())];
    for (donor, label) in others {
        let title = if counts.get(label.as_str()).is_some_and(|count| *count > 1) {
            let number = seen.entry(label.as_str()).or_default();
            *number += 1;
            format!("{label} · Variant {number}")
        } else {
            label.clone()
        };
        rows.push((Some(*donor), title));
    }
    rows.retain(|(_, title)| query.is_empty() || title.to_lowercase().contains(query));
    // Donor tags are never zero, so zero stands for the original.
    let key = |tag: Option<u32>| tag.map_or(0, u64::from);
    let keys = rows.iter().map(|(tag, _)| key(*tag)).collect::<Vec<_>>();
    crate::app::pickers::BrowserList {
        keys: &keys,
        height,
        reset,
        row_height: sundial::investment::authoring_choice_row_height(ui),
        select: reset.then(|| key(current)),
    }
    .draw_with_actions_activating(
        ui,
        |ui, index, selected| {
            let (tag, title) = &rows[index];
            let detail = tag.map_or_else(|| "Original".to_owned(), |tag| format!("0x{tag:08X}"));
            sundial::investment::draw_asset_choice_row(ui, title, &detail, selected)
        },
        |ui, index, activated| (ui.button("Use").clicked() || activated).then(|| rows[index].0),
    )
}

/// One property's tile: its name over its field, the stock value in the name's tooltip.
fn property_tile(
    ui: &mut egui::Ui,
    width: f32,
    index: usize,
    property: &Property,
    values: &mut Vec<WeaponRuntimeValueOverride>,
) {
    let stock = format!("Stock {}", property.reading(f64::from(property.stock)));
    let hint = if property.hint.is_empty() {
        stock
    } else {
        format!("{}\n{stock}", property.hint)
    };
    let current = property.current(values);
    let modified = property.is_modified(values);
    let (edited, reset) = style::tile(ui, width, index, &property.label, &hint, modified, |ui| {
        let size = egui::vec2(width, ui.spacing().interact_size.y);
        let mut value = f64::from(current);
        let field = match &property.value {
            Value::Duration(_) => egui::DragValue::new(&mut value)
                .speed(0.1)
                .range(-1.0..=3600.0)
                .clamp_existing_to_range(false)
                .custom_formatter(|value, _| seconds_text(value))
                .custom_parser(parse_seconds),
            Value::Flight(parameters) if parameters[0].kind == Kind::TravelDistance => {
                egui::DragValue::new(&mut value)
                    .speed(1.0)
                    .custom_formatter(|value, _| travel_text(value))
                    .custom_parser(parse_travel)
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
            Value::Movement(movement) => match movement[0].unit {
                Unit::Distance => egui::DragValue::new(&mut value)
                    .speed(0.05)
                    .range(0.5..=50.0)
                    .clamp_existing_to_range(false)
                    .max_decimals(2),
                Unit::Count => egui::DragValue::new(&mut value)
                    .speed(0.1)
                    .range(1.0..=10.0)
                    .clamp_existing_to_range(false)
                    .fixed_decimals(0),
                Unit::Factor => factor_field(&mut value),
            },
        };
        let response = style::named_control(ui.add_sized(size, field), &property.label);
        // Any negative length never runs out, as the stock ones written -1 do.
        let value = match property.value {
            Value::Duration(_) if value < 0.0 => -1.0,
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
