//! Subclass and ability authoring through the actual workbench UI.

use super::*;
mod attached;
mod icons;
mod readback;
pub(super) use readback::*;

/// A subclass from the New menu whose first grenade comes from another class and whose top
/// attunement comes from another subclass of its class.
pub(super) struct AuthoredSubclass {
    pub(super) recipe: WeaponRecipe,
    pub(super) base: SubclassSummary,
    pub(super) grenade: SubclassSummary,
    pub(super) attunement: SubclassSummary,
    /// The perk the authored middle-path node and the authored grenade add to the ones they
    /// start with.
    pub(super) extra_perk: u16,
    /// The stock perk the authored grenade's custom perk copies.
    pub(super) custom_effect: u16,
    /// The value of its entity the authored grenade changes.
    pub(super) ability_value: AbilityValue,
    /// The script parameter of its bank the authored grenade sets, and the value.
    pub(super) parameter: (u32, f32),
    /// The value of a graph the authored grenade spawns it changes.
    pub(super) spawn_value: AbilityValue,
    /// The palette of its effects the authored grenade recolors, and how.
    pub(super) palette: PaletteEdit,
    /// The color its materials hold that the authored grenade recolors, and how.
    pub(super) tint: TintEdit,
    /// The first grenade's stock entity and bank, and its value of a graph only that bank names,
    /// which gives its copy a private copy of the bank.
    pub(super) bank_value: (u32, u32, AbilityValue),
    /// The first grenade's palette that only graphs behind an impact table draw, and how it is
    /// recolored, which gives its copy a private copy of the table.
    pub(super) table_palette: PaletteEdit,
    /// The projectile the base's third grenade fires in place of the one it spawns.
    pub(super) swap: SpawnSwap,
    /// The damage type the first grenade deals, which none of its stock profiles does.
    pub(super) damage_type: crate::recipe::RecipeDamageType,
    /// The base's first movement ability's stock entity and its count of airborne jumps.
    pub(super) movement: (u32, ability_movement::MovementValue),
    /// The base's third movement ability's stock entity and the airborne jumps row its key
    /// applies in its bank.
    pub(super) jump_row: (u32, ability_movement::RowLane),
}

/// One value of a stock ability's entity, and the edit an ability authors to it.
pub(super) struct AbilityValue {
    pub(super) entity: u32,
    pub(super) name: String,
    pub(super) stock: WeaponRuntimeValue,
    pub(super) edit: WeaponRuntimeValueOverride,
}

/// A 32-bit float of `entity` outside its bank, doubled, as the Values tab would write it. The
/// graph loads the way the page loads it.
pub(super) fn ability_value(packages: &Path, entity: u32) -> AbilityValue {
    let manager = open_shadowkeep_package_manager(packages).unwrap();
    graph_value(&manager, entity)
        .unwrap_or_else(|| panic!("ability entity 0x{entity:08X} has a float outside its bank"))
}

/// The first graph `entity` spawns with a float, and that float doubled, as the Spawns tab would
/// write it.
pub(super) fn spawn_value(packages: &Path, entity: u32) -> AbilityValue {
    let manager = open_shadowkeep_package_manager(packages).unwrap();
    let payload = manager.read_tag(tiger_pkg::TagHash(entity)).unwrap();
    ability_spawns::spawned_graphs(&manager, entity, &payload)
        .unwrap()
        .into_iter()
        .find_map(|graph| graph_value(&manager, graph))
        .unwrap_or_else(|| panic!("ability entity 0x{entity:08X} spawns no graph with a float"))
}

/// Whether `graph` is a projectile, by the client's object type at `+0x96`.
pub(super) fn is_projectile(manager: &PackageManager, graph: u32) -> bool {
    manager
        .read_tag(tiger_pkg::TagHash(graph))
        .is_ok_and(|payload| payload.get(0x96) == Some(&18))
}

/// `entity`'s bank, and a value of a graph that bank names and nothing else of `entity` does,
/// doubled, as the Spawns tab would write it. None when the bank names no such graph with a
/// float.
pub(super) fn bank_value(manager: &PackageManager, entity: u32) -> Option<(u32, AbilityValue)> {
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
pub(super) fn projectile_swap(manager: &PackageManager, entity: u32, donor: u32) -> SpawnSwap {
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
        damage_type: None,
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
/// movement ability's key applies, put in through the recipe as the Gameplay tab writes them,
/// then shown on that tab with the first one's bank rows. Returns each stock entity and value.
pub(super) fn author_movement(
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
    // Its Gameplay tab, marked for the edit, shows the count as a tile of its own.
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
        find(&output, "the Gameplay tab", |text, _| text == "Gameplay •"),
    );
    let output = settle_loaded(ctx, app, "the movement ability's properties");
    // Its bank rows its key applies show beside it.
    for text in ["Airborne Jumps", "Vertical Impulse", "Directional Impulse"] {
        find(&output, text, |drawn, _| drawn == text);
    }
    capture::write(ctx, &output, "gear-subclass-movement-properties");
    // A source with the same native entity must keep its authored bank through a real source
    // selection and a persisted recipe reload. Hunter subclasses share these jump entities.
    let shared = app
        .subclasses
        .iter()
        .find(|other| {
            other.hash != base.hash && other.entry_entities.get(&third) == Some(&third_entity)
        })
        .expect("another stock subclass shares the movement entity")
        .clone();
    let output = settle(ctx, app);
    click(
        ctx,
        app,
        find(&output, "the third movement choice", |text, _| {
            text == base.entry_names[&third]
        }),
    );
    let output = settle(ctx, app);
    click(ctx, app, section_tab(&output, "Ability"));
    choose(ctx, app, &shared.name, &shared.entry_names[&third]);
    let saved = WeaponRecipe::from_json_str(&app.recipe.to_json_pretty().unwrap()).unwrap();
    assert_eq!(
        saved
            .overrides
            .subclass_abilities
            .as_ref()
            .unwrap()
            .edits(base.hash, Place::Ability(third))
            .bank_value(row.key, row.row, row.lane),
        Some(ROW_AIRBORNE_JUMPS),
        "a shared source retains the private bank edit after reload"
    );
    choose(ctx, app, &base.name, &base.entry_names[&third]);
    let output = settle(ctx, app);
    click(ctx, app, section_tab(&output, "Gameplay"));
    let output = settle_loaded(ctx, app, "the shared movement source");
    capture::write(ctx, &output, "gear-subclass-shared-movement-source");
    ((entity, jumps), (third_entity, row))
}

/// The traced lanes of the rows of `entry`'s bank its own pool's keys apply to its row, as the
/// Gameplay tab finds them.
pub(super) fn own_lanes(
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

/// The movement value `label` names in `entity`'s graph, as the Gameplay tab finds it.
pub(super) fn movement_value(
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

/// The first palette of ability `entity`'s effects that only graphs behind an impact table draw:
/// no graph its components name directly, at any level, draws it.
pub(super) fn table_palette(manager: &PackageManager, entity: u32) -> Option<u32> {
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
/// an impact table reaches, a green grade over every effect and a damage type none of its damage
/// profiles deals, the third grenade's swap of its projectile for one the first grenade fires,
/// and the class ability's faster recharge, put in through the recipe as the Spawns, Effect
/// Colors, Damage Type, Properties and Recharge fields write them. Returns the first grenade's
/// entity, bank and value, the swap, the palette change and the damage type.
pub(super) fn author_bank_and_swap(
    app: &mut PackageAuthoringApp,
    (base, grenade): (&SubclassSummary, &SubclassSummary),
) -> (
    (u32, u32, AbilityValue),
    SpawnSwap,
    PaletteEdit,
    crate::recipe::RecipeDamageType,
) {
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
    let mut swap = projectile_swap(&manager, swapped, entity);
    // The projectile swapped in deals a type of its own, one neither its profiles nor the third
    // grenade's deal, while the third grenade itself keeps its damage type.
    let dealt = [swapped, swap.replacement]
        .into_iter()
        .flat_map(|root| damage_profiles(&manager, root))
        .map(|(.., mode)| mode)
        .collect::<BTreeSet<_>>();
    swap.damage_type = Some(
        [
            crate::recipe::RecipeDamageType::Arc,
            crate::recipe::RecipeDamageType::Solar,
            crate::recipe::RecipeDamageType::Void,
            crate::recipe::RecipeDamageType::Kinetic,
        ]
        .into_iter()
        .find(|damage| !dealt.contains(&crate::subclass::damage_mode(*damage)))
        .expect("a damage type neither the third grenade nor the projectile swapped in deals"),
    );
    let damage = other_damage_type(&manager, entity);
    let mut abilities = app.recipe.overrides.subclass_abilities.clone().unwrap();
    let mut edits = abilities.edits(base.hash, Place::Ability(first));
    edits.ability_values.push(value.edit.clone());
    edits.set_palette(palette);
    // And every effect's final color takes the palette's new hue, as Overall sets it.
    edits.set_grade(GRADE);
    edits.damage_type = Some(damage);
    abilities.set_edits(base.hash, Place::Ability(first), edits);
    let place = Place::Ability(third);
    let mut edits = abilities.edits(base.hash, place);
    edits.set_swap(swap.graph, swap.replaced, Some(swap.replacement));
    edits.set_swap_damage(swap.graph, swap.replaced, swap.damage_type);
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
    ((entity, bank, value), swap, palette, damage)
}

/// Each damage profile the graphs below ability `entity` name, with the graph naming it, found
/// the way the Damage Type tile and the build find them: graph, profile and damage type.
pub(super) fn damage_profiles(manager: &PackageManager, entity: u32) -> Vec<(u32, u32, u8)> {
    ability_palette::ability_graphs(manager, entity, crate::subclass::SPAWN_DEPTH)
        .unwrap()
        .into_iter()
        .flat_map(|(graph, payload)| {
            sundial::package_authoring::ability_damage::references(manager, graph, &payload)
                .unwrap()
                .into_iter()
                .map(move |(place, profile)| (graph, place.graph, profile.mode))
        })
        .collect()
}

/// A damage type none of ability `entity`'s damage profiles deals, so the build copies every
/// profile its graphs name.
pub(super) fn other_damage_type(
    manager: &PackageManager,
    entity: u32,
) -> crate::recipe::RecipeDamageType {
    use crate::recipe::RecipeDamageType;
    let stock = damage_profiles(manager, entity)
        .into_iter()
        .map(|(.., mode)| mode)
        .collect::<std::collections::BTreeSet<_>>();
    assert!(
        !stock.is_empty(),
        "ability entity 0x{entity:08X}'s graphs name a damage profile"
    );
    [
        RecipeDamageType::Arc,
        RecipeDamageType::Solar,
        RecipeDamageType::Void,
        RecipeDamageType::Kinetic,
    ]
    .into_iter()
    .find(|damage| {
        let edits = crate::subclass::EntryEdits {
            damage_type: Some(*damage),
            ..Default::default()
        };
        edits
            .damage_mode()
            .is_some_and(|mode| !stock.contains(&mode))
    })
    .expect("a damage type the ability's profiles do not deal")
}

/// The palettes the effects of ability `entity` draw with, found the way the Effect Colors field
/// and the build find them.
pub(super) fn stock_palettes(packages: &Path, entity: u32) -> Vec<ability_palette::Palette> {
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
pub(super) fn twins(manager: &PackageManager, stock: u32, copied: u32) -> BTreeMap<u32, u32> {
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
pub(super) fn table_twins(
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

/// The grade the first grenade gives every effect: green, as Colorize sets it.
const GRADE: EffectGrade = EffectGrade {
    hue: 120,
    colorize: true,
    ..EffectGrade::STOCK
};

/// The pixel programs ability `entity`'s effects draw with, each once: those of its particle
/// systems' materials and of the materials its models, lights and other resources name.
pub(super) fn effect_programs(manager: &PackageManager, entity: u32) -> BTreeSet<u32> {
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
pub(super) fn program_code(manager: &PackageManager, program: u32) -> Option<Vec<u8>> {
    let data = manager
        .get_entry(tiger_pkg::TagHash(program))
        .filter(|entry| entry.file_type == 33)?
        .reference;
    manager.read_tag(tiger_pkg::TagHash(data)).ok()
}

/// The name the Spawns tab gives `graph` among the graphs `entity` spawns, read the way the page
/// loads it.
pub(super) fn spawn_name(packages: &Path, entity: u32, graph: u32) -> String {
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
pub(super) fn graph_value(manager: &PackageManager, entity: u32) -> Option<AbilityValue> {
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
pub(super) fn investment_globals(manager: &PackageManager) -> Vec<u8> {
    let globals = resolve_live_named_tag(manager, "investment_globals", None).unwrap();
    manager.read_tag(globals).unwrap()
}

/// The ability entities a perk's On a Specific Ability and Ends on a Specific Ability conditions
/// name. A row with no action names none.
pub(super) fn named_abilities(manager: &PackageManager, globals: &[u8], perk: u16) -> Vec<u32> {
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
pub(super) fn check_retargeted(
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
pub(super) struct RetargetBase {
    pub(super) base: SubclassSummary,
    pub(super) grenade: u8,
    pub(super) entity: u32,
    pub(super) perk: u16,
    pub(super) holders: Vec<u8>,
}

/// A subclass on such a base whose grenade has a value of its own, and whose grenade and second
/// middle node add the perk that names it.
pub(super) struct AuthoredRetarget {
    pub(super) recipe: WeaponRecipe,
    pub(super) target: RetargetBase,
    pub(super) value: AbilityValue,
}

/// The first sandbox perk that names each ability entity in an On a Specific Ability or Ends on a
/// Specific Ability condition, across every perk row with a runtime action of its own.
pub(super) fn perks_naming_abilities(
    manager: &PackageManager,
    globals: &[u8],
) -> BTreeMap<u32, u16> {
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
pub(super) fn find_retarget(packages: &Path, subclasses: &[SubclassSummary]) -> RetargetBase {
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
pub(super) fn author_retarget(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
) -> AuthoredRetarget {
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

/// The authored middle path's name, and its passive third node's.
const PATH_NAME: &str = "Way of the Sundial";
const NODE_NAME: &str = "Sundial Strike";
const NODE_POSITION: u8 = 2;
/// The authored second grenade's name and description, and its custom perk's name.
const ABILITY_NAME: &str = "Sundial Burst";
const ABILITY_DESCRIPTION: &str = "Authored in Parhelion.";
const CUSTOM_PERK_NAME: &str = "Sundial Spark";
/// The authored grenade takes the icon of this entry of the top attunement's source: its lead.
const ABILITY_ICON_ENTRY: u8 = AttunementPath::Top.entries()[0];

/// An ability's or node's section tab, marked or not.
pub(super) fn section_tab(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
    let marked = format!("{label} •");
    find(output, label, |text, _| text == label || text == marked)
}

/// Clicks `choice` on the line of the detail panel's choices that `subclass` leads. An ability's or
/// node's choices open over the page from its Based On button and close on a pick. An attunement
/// lists its own.
pub(super) fn choose(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    subclass: &str,
    choice: &str,
) {
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
    let drawn = texts(&output);
    let line = find(&output, subclass, |text, rect| {
        text == subclass
            && drawn.iter().any(|(label, bounds)| {
                label == choice
                    && bounds.min.x > rect.max.x
                    && (bounds.center().y - rect.center().y).abs() < 12.0
            })
    });
    click(
        ctx,
        app,
        find(&output, choice, |text, rect| {
            text == choice && rect.min.x > line.x && (rect.center().y - line.y).abs() < 12.0
        }),
    );
}

/// Runs frames until every stock subclass node's icon has loaded, since package icons load on a
/// worker, so a capture shows them.
pub(super) fn settle_icons(ctx: &egui::Context, app: &mut PackageAuthoringApp) -> egui::FullOutput {
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
pub(super) fn frame_at(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
    width: f32,
) -> egui::FullOutput {
    let mut output = None;
    for _ in 0..2 {
        let frame = ctx.run_ui(
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
pub(super) fn author_subclass(
    ctx: &egui::Context,
    app: &mut PackageAuthoringApp,
) -> AuthoredSubclass {
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
    // Its Damage Type Icon starts on the base's damage type, and another one goes in its strings.
    let shown = base
        .damage_type
        .expect("the catalog reads the base's damage type");
    find(&output, "the Damage Type Icon picker", |text, _| {
        text == shown.label()
    });
    app.recipe.overrides.subclass_damage_type =
        Some(if shown == sundial::investment::WeaponDamageType::Void {
            crate::recipe::RecipeDamageType::Solar
        } else {
            crate::recipe::RecipeDamageType::Void
        });

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
    // The middle path of its own: renamed, with its third node from the base's bottom path
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
    let mut node = SubclassPathNode::stock(NODE_POSITION, base.hash, AttunementPath::Bottom, 1);
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
        SubclassSelection::Entry(Place::Node(AttunementPath::Middle, NODE_POSITION))
    );
    // Its Ability section holds its text under a header naming what it is based on. Its Perks
    // tab holds the added perk, and its Gameplay tab the added modifier among its Ability
    // Changes.
    let output = settle_icons(ctx, app);
    for text in ["Middle Path · Node 3", "Description", "Based On"] {
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
    for text in ["Changes While Equipped", "Add Change"] {
        find(&output, text, |drawn, _| {
            drawn.strip_suffix(" •").unwrap_or(drawn) == text
        });
    }
    capture::write(ctx, &output, "gear-subclass-path-gameplay");
    // Add Change opens its form under the node's Changes While Equipped chips, which Cancel closes.
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
    let (bank_value, swap, table_palette, damage_type) =
        author_bank_and_swap(app, (&base, &grenade));
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
        damage_type,
        movement,
        jump_row,
    }
}

/// The color the Screen Art test paints the top attunement's picture with.
const SCREEN_ART_COLOR: [u8; 4] = [255, 0, 255, 255];

/// Gives the subclass screen art of its own, as the Appearance tab writes it: a picture for the
/// top attunement and the attunement subclass's top picture for the middle one, the bottom kept.
/// The tab shows a tile for each attunement.
pub(super) fn author_screen_art(
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

/// The colors of ability `entity`'s materials that Effect Colors lists, found the way the field
/// and the build find them.
pub(super) fn stock_tints(packages: &Path, entity: u32) -> Vec<ability_tint::Tint> {
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
pub(super) fn author_ability(
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
    let default_row = u16::try_from(
        bank.parameters
            .iter()
            .position(|p| p.name == parameter.0)
            .expect("the authored parameter has a table row"),
    )
    .unwrap();
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
    edits.set_parameter_default(parameter.0, default_row, Some(0.375));
    edits.ability_values = vec![value.edit.clone(), spawned.edit.clone()];
    edits.set_palette(palette);
    edits.set_tint(tint);
    // And every effect's final color takes the same hue, after the palette and tint, as Overall
    // sets it.
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
    for text in ["Ability •", "Perks •", "Gameplay •", "Visuals •"] {
        find(&output, text, |drawn, _| drawn == text);
    }
    click(ctx, app, section_tab(&output, "Ability"));
    let output = settle_icons(ctx, app);
    for text in ["Grenade 2", "Icon", "Change Icon…", "Based On"] {
        find(&output, text, |drawn, _| drawn == text);
    }
    capture::write(ctx, &output, "gear-subclass-ability-text");
    // Effect Colors, in the Visuals section, loads the palettes the grenade's effects draw with
    // and shows the turned hue, then every stock palette, which names the one the colors come
    // from.
    click(ctx, app, section_tab(&output, "Visuals"));
    let output = settle_loaded(ctx, app, "the grenade's effect colors");
    find(&output, "Effect Colors", |text, _| text == "Effect Colors");
    // The field draws the degrees as one label, or as the number beside a degree sign.
    let hue = format!("{}°", palette.hue);
    let degrees = palette.hue.to_string();
    find(&output, "the palette's turned hue", |text, _| {
        text == hue || text == degrees
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
    for text in [
        "Extra Charges",
        "Projectile",
        "Speed",
        "Gravity",
        "Travel Limit",
    ] {
        find(&output, text, |drawn, _| drawn == text);
    }
    capture::write(ctx, &output, "gear-subclass-ability-gameplay");
    click(
        ctx,
        app,
        accessible(&output, "Find Properties").unwrap().center(),
    );
    frame(ctx, app, vec![egui::Event::Text("collision".into())]);
    let output = settle_loaded(ctx, app, "property search");
    find(&output, "a control inside More", |text, _| {
        text == "Collision Mode"
    });
    capture::write(ctx, &output, "gear-subclass-property-search");
    click(
        ctx,
        app,
        accessible(&output, "Clear Property Search")
            .unwrap()
            .center(),
    );
    let output = settle_loaded(ctx, app, "all properties");
    find(
        &output,
        "active abilities can change other abilities",
        |text, _| text == "Changes While Equipped",
    );
    // Its Technical part, closed until opened and marked for the grenade's values, loads the
    // grenade's entity, counts the edit among its fields, then lists the graphs the grenade
    // spawns and opens the one it changes.
    click(ctx, app, tab(&output, "Technical •"));
    let output = settle_loaded(ctx, app, "the grenade's entity");
    find(&output, "the editable script fallback values", |text, _| {
        text.starts_with("Parameter Defaults")
    });
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
    click(
        ctx,
        app,
        accessible(&output, "Stat Passives").unwrap().center(),
    );
    let output = settle_icons(ctx, app);
    click(ctx, app, section_tab(&output, "Perks"));
    let output = settle_icons(ctx, app);
    find(&output, "the stat passives", |text, _| {
        text == "Stat Passives"
    });
    find(&output, "private stat perk authoring", |text, _| {
        text == "New Custom Perk"
    });
    capture::write(ctx, &output, "gear-subclass-stat-passives");
    (custom_effect, (value, spawned), parameter, (palette, tint))
}

/// A stock perk as the Subclass page labels its chip: the first stock entry that grants it,
/// numbered when that entry grants several.
pub(super) fn perk_label(subclasses: &[SubclassSummary], perk: u16) -> String {
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
