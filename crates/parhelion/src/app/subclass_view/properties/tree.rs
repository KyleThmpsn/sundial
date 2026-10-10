//! An ability's tree of graphs and the cards that stand for them, loaded off the UI
//! thread with the projectiles a swap can choose.
use super::*;

/// One graph of the ability's tree and what it shows.
pub(super) struct Node {
    pub(super) tag: u32,
    pub(super) parent: Option<u32>,
    pub(super) projectile: bool,
    /// Whether only its spawner's ability bank names it, so it spawns only when a key selects it.
    pub(super) alternative: bool,
    /// Its kind in plain words.
    pub(super) name: String,
    pub(super) properties: Vec<Property>,
}

/// Graphs that show the same values, under one title.
pub(super) struct Card {
    pub(super) name: String,
    /// Its name, numbered where names repeat.
    pub(super) title: String,
    /// How many cards above it spawn its first graph, which it nests under.
    pub(super) depth: usize,
    /// Each graph it stands for, with the graph that spawns it.
    pub(super) graphs: Vec<(u32, Option<u32>)>,
    /// Whether its graphs are projectiles another graph's components name, which a swap can
    /// replace.
    pub(super) projectile: bool,
    /// Whether only a bank names its graphs, or those of a card above it, so they spawn only when
    /// a perk or node selects them.
    pub(super) alternative: bool,
    pub(super) properties: Vec<Property>,
}

impl Card {
    pub(super) fn signature(&self) -> (String, bool, bool, Vec<PropertySignature>) {
        (
            self.name.clone(),
            self.projectile,
            self.alternative,
            self.properties.iter().map(Property::signature).collect(),
        )
    }
}

/// An ability's cards, the graph above each graph of its tree, the traced lanes of its bank's
/// rows, which show for the keys its entry applies, and the damage types its graphs' damage
/// profiles deal, as the client encodes them.
pub(in crate::app::subclass_view) struct Tree {
    pub(super) cards: Vec<Card>,
    pub(super) parents: BTreeMap<u32, u32>,
    pub(super) lanes: Vec<RowLane>,
    pub(super) damage: BTreeSet<u8>,
}

pub(super) type Load = Result<Tree, String>;

/// Every projectile a stock ability fires, with the ability and the projectile's name.
pub(super) type Donors = Result<Arc<Vec<(u32, String)>>, String>;

/// The trees loaded so far, by ability entity, and the projectiles a swap can choose.
#[derive(Default)]
pub(in crate::app::subclass_view) struct Properties {
    pub(super) trees: BTreeMap<u32, Result<Arc<Tree>, String>>,
    pub(super) loading: Option<(u32, Receiver<Load>)>,
    pub(super) donors: Option<Donors>,
    pub(super) donor_loading: Option<Receiver<Donors>>,
    /// Whether the cards that spawn only when a perk or node selects them show.
    pub(super) show_alternatives: bool,
    /// Whether the Projectile browser lists every projectile in the engine catalog, weapons',
    /// vehicles' and enemies' included, rather than only the stock abilities' own.
    pub(super) all_projectiles: bool,
    /// Whether the page needs the engine catalog the perk workbench loads: for that list, or to
    /// name a swap to a projectile no stock ability fires.
    pub(in crate::app::subclass_view) wants_catalog: bool,
}

impl Properties {
    /// Takes a finished load, and starts one for `entity` when it has none. Returns the tree once
    /// it is loaded.
    pub(super) fn poll(
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
    pub(super) fn donors(
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
pub(super) fn load_donors(
    packages: &Path,
    abilities: &[(u32, String)],
) -> Result<Vec<(u32, String)>, String> {
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

pub(super) fn load(packages: &Path, entity: u32) -> Load {
    let manager = open_shadowkeep_package_manager(packages)?;
    let mut nodes = Vec::new();
    let mut parents = BTreeMap::new();
    let mut damage = BTreeSet::new();
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
        // Only what the tile shows, so a graph whose profiles do not read leaves none.
        if let Ok(profiles) = ability_damage::references(&manager, tag, &payload) {
            damage.extend(profiles.into_iter().map(|(_, profile)| profile.mode));
        }
        let name = match parent {
            Some(_) => graph_name(&payload),
            None if payload.get(OBJECT_TYPE) == Some(&PROJECTILE) => "Projectile".to_owned(),
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
        damage,
    })
}

/// Every traced lane of the rows of `entity`'s bank, for any key. A bank that does not read has
/// none.
pub(super) fn bank_lanes(manager: &PackageManager, entity: u32) -> Vec<RowLane> {
    (|| -> Option<Vec<RowLane>> {
        let payload = manager.read_tag(tiger_pkg::TagHash(entity)).ok()?;
        let bank = sundial::package_authoring::ability_modifier::entity_bank(&payload)
            .ok()
            .flatten()?;
        let bank = manager.read_tag(tiger_pkg::TagHash(bank)).ok()?;
        ability_movement::validate_bank_context(manager, &payload, &bank).ok()?;
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
pub(super) fn factor_field(value: &mut f64) -> egui::DragValue<'_> {
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
/// throws. The Projectile swap takes only a projectile named directly.
pub(super) fn children(
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
pub(super) fn graph_name(payload: &[u8]) -> String {
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
pub(super) fn properties_of(manager: &PackageManager, tag: u32, payload: &[u8]) -> Vec<Property> {
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
    for curve in parameters::curves(&graph) {
        found.push(Property {
            label: curve.kind.label().to_owned(),
            hint: curve_hint(curve.kind).to_owned(),
            stock: curve.original(),
            value: Value::Curve(vec![curve]),
        });
    }
    let settings = ability_settings::discover(manager, &graph);
    let claimed = settings
        .iter()
        .map(|setting| setting.field.locator.clone())
        .collect::<Vec<_>>();
    for setting in settings {
        found.push(Property {
            label: setting.kind.label().to_owned(),
            hint: setting.kind.hint().to_owned(),
            stock: setting.stock(),
            value: Value::Setting(vec![setting]),
        });
    }
    // A timer is an upper lifetime bound, including immediate and unlimited stock timers.
    // A timer that scales an input before adding seconds has no single length: its seconds are
    // an offset, which can be negative, and its input has a coefficient.
    for length in effect_length::discover(&graph) {
        let stock = length.stock();
        if let Some(scale) = &length.input_scale {
            found.push(Property {
                label: "Duration Offset".to_owned(),
                hint: "Seconds added to the timer".to_owned(),
                stock,
                value: Value::Timer {
                    fields: vec![length.field.clone()],
                    scaling: false,
                },
            });
            found.push(Property {
                label: "Duration Scaling".to_owned(),
                hint: "Scales the timer's named input".to_owned(),
                stock: length.per_input.unwrap_or(f32::NAN),
                value: Value::Timer {
                    fields: vec![scale.clone()],
                    scaling: true,
                },
            });
        } else if stock.is_finite() {
            found.push(Property {
                label: "Duration".to_owned(),
                hint: "This part's self-destruct timer. Other events can end it sooner".to_owned(),
                stock: length_reading(&[], &length).unwrap_or(stock),
                value: Value::Duration(vec![length]),
            });
        }
    }
    for movement in ability_movement::discover(&graph) {
        let hint = if movement.traced {
            movement_hint(movement.label)
        } else {
            "Role unconfirmed"
        };
        let label = match movement.label {
            "Directional Velocity Multiplier 1"
            | "Directional Velocity Multiplier 2"
            | "Vertical Velocity Multiplier" => {
                format!("Base {}", movement.label)
            }
            label => label.to_owned(),
        };
        found.push(Property {
            label,
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
                        && !claimed.contains(&field.locator)
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
pub(super) const fn flight_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Speed => "Speed",
        Kind::Gravity => "Gravity",
        Kind::TravelDistance => "Travel Limit",
    }
}

/// What a curve setting sets, for its tile's tooltip.
pub(super) const fn curve_hint(kind: CurveKind) -> &'static str {
    match kind {
        CurveKind::FinalSpeed => "Speed it reaches at Curve End",
        CurveKind::FinalGravity => "Gravity it reaches at Curve End",
        CurveKind::Start => "Distance before speed and gravity start to change",
        CurveKind::End => "Distance where they reach their final values",
    }
}

/// What a flight value scales, for its tile's tooltip.
pub(super) const fn flight_hint(kind: Kind) -> &'static str {
    match kind {
        Kind::Speed => "Times the speed it is launched at",
        Kind::Gravity => "Times normal gravity",
        Kind::TravelDistance => "How far it flies before it ends",
    }
}

/// One settings record of a Component Modifiers root: its amount and its other numbers.
pub(super) struct Record {
    pub(super) amount: WeaponRuntimeField,
    pub(super) numbers: Vec<(u32, i64)>,
}

impl Record {
    pub(super) fn number(&self, offset: u32) -> Option<i64> {
        self.numbers
            .iter()
            .find(|(at, _)| *at == offset)
            .map(|(_, number)| *number)
    }

    /// Its tile, named by the input it changes, and by the ability for an ability's input. A
    /// record whose input has no established name, or whose operation changes nothing, has none.
    pub(super) fn property(self) -> Option<Property> {
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
        let hint = modifiers::input_hint(component, input)
            .or_else(|| choice(meaning(modifiers::COMPONENT_OFFSET), component))
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
pub(super) fn modifiers_in(fields: &[WeaponRuntimeField]) -> Vec<Property> {
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
pub(super) fn meaning(offset: u32) -> &'static [(i64, &'static str)] {
    match modifiers::field_meaning(modifiers::SETTINGS_SCHEMA, offset) {
        Some(meaning) => meaning.choices,
        None => &[],
    }
}

pub(super) fn choice(choices: &'static [(i64, &'static str)], value: i64) -> Option<&'static str> {
    choices
        .iter()
        .find(|(each, _)| *each == value)
        .map(|(_, name)| *name)
}

/// Properties that share a name and stock value as one tile, then repeated names numbered.
pub(super) fn merge(found: Vec<Property>) -> Vec<Property> {
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
pub(super) fn cards(nodes: Vec<Node>, parents: &BTreeMap<u32, u32>) -> Vec<Card> {
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
pub(super) fn put(
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

pub(super) fn own(
    values: &[WeaponRuntimeValueOverride],
    field: &WeaponRuntimeField,
) -> Option<WeaponRuntimeValue> {
    values
        .iter()
        .find(|each| each.locator == field.locator)
        .map(|each| each.value.clone())
}
