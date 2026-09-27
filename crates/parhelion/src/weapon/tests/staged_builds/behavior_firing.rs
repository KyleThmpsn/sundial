//! Firing patterns and type-fitted amounts for grafted behaviors whose plugs change the barrel or
//! the magazine, built, staged and read back from the staged packages the game would load.
//!
//! Arc Lance, Graviton Lance's behavior on Arc Logic, could not fire: Black Hole takes one round
//! from every burst, and an auto rifle fires one. Other borrowed perks cut fire rate and magazine
//! size by amounts sized for their own weapon type. The ways this could still be wrong, each checked
//! against the staged packages rather than the recipe:
//!
//! - no private plug replaces the stock plug in the intrinsic socket, so the stock amounts apply
//! - the private plug drops the stock plug's other perks, or keeps the stock perk beside its clone
//! - the clone's burst lanes are not the source's burst (+1 on an auto rifle) or the host's (0)
//! - the base weapon's firing leaves Black Hole's slower shot timing in place
//! - a host of the source's own type gets a private plug it does not need
//! - Bastion's Saint's Fists on an auto rifle is not rebased by its own figures, or keeps its 25
//!   fewer shots a second and its 0.35 magazine
//! - Sleeper Simulant's Dornröschen on an auto rifle keeps its smaller magazine
//! - Bastion on another fusion rifle loses what it keeps on its own type
//! - the stock perk or its stock entity changes in the staged packages
use super::*;
use crate::recipe::{AdditionalBehaviorRecipe, RecipeBehaviorFiring};
use crate::weapon_behavior::{FiringRecord, firing_records};
use std::collections::BTreeMap;
use std::fmt::Write as _;

const ARC_LOGIC: u32 = 0xA25B_8F8F;
const BLACK_HOLE: u32 = 0xE8C9_DED3;
const SAINTS_FISTS: u32 = 0x46B8_4272;
const DORNROSCHEN: u32 = 0xE783_140A;
const INTRINSIC_SOCKET: usize = 0;
const BARREL: i64 = 2;
const MAGAZINE: i64 = 1;
const ROUNDS_PER_BURST: &[i64] = &[22, 23, 24, 25];
const RATE_OF_FIRE: &[i64] = &[0, 1, 2, 3];
const TIME_BETWEEN_SHOTS: &[i64] = &[4, 5];

/// One weapon of the batch and what its intrinsic socket must hold once staged.
struct Case {
    recipe: crate::WeaponRecipe,
    stock_plug: u32,
    /// Whether a private copy must replace the stock plug.
    private: bool,
    /// The amount each record on these component inputs must carry in the private clone.
    lanes: Vec<(i64, &'static [i64], f32)>,
    /// Whether every magazine record in the private clone must leave its input alone.
    ammo_left_alone: bool,
}

fn recipe(
    namespace: &str,
    name: &str,
    host: (u32, &str),
    behavior: &str,
    firing: Option<RecipeBehaviorFiring>,
) -> crate::WeaponRecipe {
    let mut recipe = crate::WeaponRecipe::new_weapon_for_donor(namespace, host.0, host.1).unwrap();
    recipe.name = name.to_owned();
    recipe.overrides.additional_behaviors = vec![AdditionalBehaviorRecipe {
        behavior: behavior.to_owned(),
    }];
    recipe.overrides.behavior_firing = firing;
    recipe
}

fn item_table(manager: &sundial::package_authoring::PackageManager) -> Vec<u8> {
    let globals = manager
        .read_tag(resolve_live_named_tag(manager, "investment_globals", None).unwrap())
        .unwrap();
    let root = manager
        .read_tag(TagHash(read_u32(&globals, 16).unwrap()))
        .unwrap();
    manager
        .read_tag(root_child_tag(&root, ROOT_ITEM_DEFINITION_TABLE_SLOT).unwrap())
        .unwrap()
}

fn item_definition(
    manager: &sundial::package_authoring::PackageManager,
    items: &[u8],
    hash: u32,
) -> Vec<u8> {
    let (count, _, rows, _) = array_at(items, 8).unwrap();
    let index = find_u32_row_key(items, rows, count, ITEM_ROW_SIZE, hash)
        .unwrap()
        .unwrap_or_else(|| panic!("item 0x{hash:08X} is staged"));
    manager
        .read_tag(TagHash(
            read_u32(items, rows + index * ITEM_ROW_SIZE + 16).unwrap(),
        ))
        .unwrap()
}

fn socket_plug(items: &[u8], definition: &[u8], socket: usize) -> u32 {
    let (_, _, rows, _) = array_at(items, 8).unwrap();
    let index = weapon_default_plug_indices(definition).unwrap()[socket];
    read_u32(items, rows + usize::from(index) * ITEM_ROW_SIZE).unwrap()
}

/// Every barrel firing and magazine record a perk's attached entities carry.
fn perk_records(
    manager: &sundial::package_authoring::PackageManager,
    globals: &[u8],
    perk: u16,
) -> Vec<FiringRecord> {
    let Ok(runtime) = load_sandbox_perk_runtime_action(manager, globals, usize::from(perk)) else {
        return Vec::new();
    };
    runtime
        .graphs
        .iter()
        .filter_map(|graph| firing_records(manager, graph.tag.0).ok())
        .flatten()
        .collect()
}

/// A readable amount for the report, without the locator.
fn describe(record: &FiringRecord) -> String {
    let component = if record.component == MAGAZINE {
        "magazine"
    } else {
        "barrel"
    };
    let operation = if record.operation == 1 { "x" } else { "+" };
    format!(
        "- {component} input {}: {operation}{}",
        record.input, record.value
    )
}

/// The staged packages and the stock perk lists every case compares against.
struct Staged<'a> {
    manager: &'a sundial::package_authoring::PackageManager,
    globals: &'a [u8],
    items: &'a [u8],
    stock: &'a BTreeMap<u32, Vec<u16>>,
}

/// Reads one case back from the staged packages and checks it, adding it to the report.
fn verify(staged: &Staged<'_>, case: &Case, report: &mut String) {
    let name = &case.recipe.name;
    let hash = case.recipe.identity.item_hash.parse_u32().unwrap();
    let definition = item_definition(staged.manager, staged.items, hash);
    let plug = socket_plug(staged.items, &definition, INTRINSIC_SOCKET);
    let _ = writeln!(report, "## {name}\n\nIntrinsic plug: 0x{plug:08X}");
    if !case.private {
        assert_eq!(plug, case.stock_plug, "{name}: the stock plug stays");
        let _ = writeln!(report, "Stock plug kept, no private copy.\n");
        return;
    }
    assert_ne!(plug, case.stock_plug, "{name}: a private plug replaces it");
    let stock = &staged.stock[&case.stock_plug];
    let perks = weapon_sandbox_perks(&item_definition(staged.manager, staged.items, plug)).unwrap();
    assert_eq!(perks.len(), stock.len(), "{name}: every perk stays");
    let private = perks
        .iter()
        .copied()
        .filter(|perk| !stock.contains(perk))
        .collect::<Vec<_>>();
    let [private] = private.as_slice() else {
        panic!("{name}: exactly one perk is a private clone, got {private:?}");
    };
    let records = perk_records(staged.manager, staged.globals, *private);
    let _ = writeln!(report, "Private perk {private}:");
    for record in &records {
        let _ = writeln!(report, "{}", describe(record));
    }
    let _ = writeln!(report);
    for (component, inputs, value) in &case.lanes {
        for input in *inputs {
            let found = records
                .iter()
                .filter(|record| record.component == *component && record.input == *input)
                .map(|record| record.value)
                .collect::<Vec<_>>();
            assert_eq!(
                found,
                [*value],
                "{name}: component {component} input {input}"
            );
        }
    }
    if case.ammo_left_alone {
        for record in records.iter().filter(|record| record.component == MAGAZINE) {
            let neutral = if record.operation == 1 { 1.0 } else { 0.0 };
            assert_eq!(record.value, neutral, "{name}: {}", describe(record));
        }
    }
}

/// The first legendary weapon of a type with a runtime row, as a recipe host.
fn legendary(catalog: &InvestmentCatalog, type_name: &str) -> (u32, String) {
    catalog
        .weapon_donors()
        .into_iter()
        .find(|donor| {
            donor.type_name == type_name
                && donor.rarity == sundial::investment::WeaponRarity::Legendary
                && donor.weapon_pattern_index.is_some()
        })
        .map(|donor| (donor.hash, donor.name))
        .unwrap_or_else(|| panic!("a legendary {type_name} with a runtime"))
}

fn cases(catalog: &InvestmentCatalog) -> Vec<Case> {
    let arc_logic = (ARC_LOGIC, "Arc Logic");
    let pulse = legendary(catalog, "Pulse Rifle");
    let fusion = legendary(catalog, "Fusion Rifle");
    let mut cases = vec![
        // Graviton fires two, an auto rifle one, so the clone adds one where the stock takes one.
        Case {
            recipe: recipe(
                "parhelion.behavior-firing.arc-lance",
                "Arc Lance",
                arc_logic,
                "graviton-lance-graph",
                None,
            ),
            stock_plug: BLACK_HOLE,
            private: true,
            lanes: vec![
                (BARREL, ROUNDS_PER_BURST, 1.0),
                (BARREL, TIME_BETWEEN_SHOTS, 1.25),
            ],
            ammo_left_alone: true,
        },
        Case {
            recipe: recipe(
                "parhelion.behavior-firing.arc-lance-base",
                "Arc Lance Base Firing",
                arc_logic,
                "graviton-lance-graph",
                Some(RecipeBehaviorFiring::Weapon),
            ),
            stock_plug: BLACK_HOLE,
            private: true,
            lanes: vec![
                (BARREL, ROUNDS_PER_BURST, 0.0),
                (BARREL, TIME_BETWEEN_SHOTS, 1.0),
            ],
            ammo_left_alone: true,
        },
        // A pulse rifle already starts from Graviton's three, so the stock plug is right as it is.
        Case {
            recipe: recipe(
                "parhelion.behavior-firing.pulse",
                "Graviton Pulse",
                (pulse.0, &pulse.1),
                "graviton-lance-graph",
                None,
            ),
            stock_plug: BLACK_HOLE,
            private: false,
            lanes: Vec::new(),
            ammo_left_alone: false,
        },
        // Bastion fires three spreads from a fusion rifle's seven, so an auto rifle adds two, and
        // keeps its own fire rate and magazine, since 25 fewer shots a second and a 0.35 magazine
        // were sized for a fusion rifle.
        Case {
            recipe: recipe(
                "parhelion.behavior-firing.bastion",
                "Arc Bastion",
                arc_logic,
                "bastion-graph",
                None,
            ),
            stock_plug: SAINTS_FISTS,
            private: true,
            lanes: vec![(BARREL, ROUNDS_PER_BURST, 2.0), (BARREL, RATE_OF_FIRE, 0.0)],
            ammo_left_alone: true,
        },
        // On another fusion rifle every one of those figures is the right size already.
        Case {
            recipe: recipe(
                "parhelion.behavior-firing.bastion-fusion",
                "Bastion Fusion",
                (fusion.0, &fusion.1),
                "bastion-graph",
                None,
            ),
            stock_plug: SAINTS_FISTS,
            private: false,
            lanes: Vec::new(),
            ammo_left_alone: false,
        },
        // Dornröschen's smaller magazine and reserves were sized for a linear fusion rifle.
        Case {
            recipe: recipe(
                "parhelion.behavior-firing.sleeper",
                "Arc Sleeper",
                arc_logic,
                "sleeper-simulant-graph",
                None,
            ),
            stock_plug: DORNROSCHEN,
            private: true,
            lanes: Vec::new(),
            ammo_left_alone: true,
        },
    ];
    // A saved recipe that borrows Graviton Lance onto a weapon firing one round per pull, such as
    // the library's own Arc Lance, is checked the same way as the Arc Lance built above.
    if let Some(path) = std::env::var_os("PARHELION_FIRING_RECIPE") {
        cases.push(Case {
            recipe: crate::WeaponRecipe::from_json_str(&fs::read_to_string(path).unwrap()).unwrap(),
            stock_plug: BLACK_HOLE,
            private: true,
            lanes: vec![
                (BARREL, ROUNDS_PER_BURST, 1.0),
                (BARREL, TIME_BETWEEN_SHOTS, 1.25),
            ],
            ammo_left_alone: true,
        });
    }
    cases
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES pointing to Shadowkeep packages"]
fn borrowed_burst_changes_follow_the_chosen_firing_pattern_in_staged_packages() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
    let catalog = InvestmentCatalog::load(packages.parent().unwrap(), false, |_| {}).unwrap();
    let source = open_manager(&packages).unwrap();
    let source_globals = source
        .read_tag(resolve_live_named_tag(&source, "investment_globals", None).unwrap())
        .unwrap();
    let source_items = item_table(&source);
    let stock = [BLACK_HOLE, SAINTS_FISTS, DORNROSCHEN]
        .into_iter()
        .map(|plug| {
            let definition = item_definition(&source, &source_items, plug);
            (plug, weapon_sandbox_perks(&definition).unwrap())
        })
        .collect::<BTreeMap<_, _>>();

    let cases = cases(&catalog);
    let specs = cases
        .iter()
        .map(|case| case.recipe.to_spec().unwrap())
        .collect::<Vec<_>>();
    let bundle = build_weapon_project_after_catalog_validation(
        &packages,
        &WeaponProjectSpec { weapons: specs },
    )
    .expect("the firing-pattern batch builds");
    let view = staged_view(&packages, ".parhelion-behavior-firing-", &bundle);
    let manager = open_manager(&view.path().join("packages")).unwrap();
    let globals = manager
        .read_tag(resolve_live_named_tag(&manager, "investment_globals", None).unwrap())
        .unwrap();
    let items = item_table(&manager);
    let staged = Staged {
        manager: &manager,
        globals: &globals,
        items: &items,
        stock: &stock,
    };

    let mut report = String::from("# Behavior Firing E2E\n\n");
    for case in &cases {
        verify(&staged, case, &mut report);
    }
    // The stock perks and the entities they attach are what every other weapon still uses.
    for perk in stock.values().flatten() {
        assert_eq!(
            perk_records(&manager, &globals, *perk),
            perk_records(&source, &source_globals, *perk),
            "stock perk {perk} is unchanged"
        );
    }
    let out = std::env::var_os("PARHELION_FIRING_OUT").map_or_else(
        || std::env::temp_dir().join("parhelion-behavior-firing"),
        PathBuf::from,
    );
    fs::create_dir_all(&out).unwrap();
    fs::write(out.join("report.md"), &report).unwrap();
    eprintln!("{report}\nReport: {}", out.join("report.md").display());
}
