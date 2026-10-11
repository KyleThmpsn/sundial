//! Persisted recipes through compilation, packaging and independent native readback.
//! Failure cases written before the compiler change:
//! - a copied spread pointer reaches stock or unrelated data, or loses its 20-byte ring rows
//! - instance and definition pool counts disagree, or a trajectory loses its reciprocal handle
//! - capacity follows the old barrel, misses an unchanged firing graph, or shrinks a larger pool
//! - pool growth drops explicit projectile values or changes a stock donor
//! - clearing spread is ignored, or a malformed final spread is treated as a single projectile
//! - a component donor sharing another item's pattern is rejected or supplies the wrong spread
//! - a grown row shares, loses or points outside its instance region for the object it owns, as a
//!   fusion rifle's or launcher's row owns one
//! - Bullets per Shot writes the stock translator, misses a tier, changes another output or table,
//!   or leaves a Barrel bullet input reading the stock column
//! - growth refuses, or moves as a pointer, a row's data word that reads like one, as a rocket
//!   launcher's distance curve flag at +0x1C1 does
use super::*;
use sundial::package_authoring::{
    entity::{WEAPON_BARREL_COMPONENT_KEY as BARREL, WEAPON_STAT_TRANSLATOR_COMPONENT_KEY},
    runtime::{
        load_weapon_runtime_entity_at_pattern_index_with_manager,
        load_weapon_runtime_entity_with_manager, load_weapon_runtime_graph_for_entity,
        modifiers::{BARREL_BULLETS_PER_SHOT, BARREL_COMPONENT},
    },
    sandbox_perk::entity::projectile::parameters::{self, Kind},
};

const MOVEMENT: u32 = 0x0437_756D;
/// More than the fusion rifle's one trajectory, so its pool must grow.
const FUSION_PELLETS: usize = 5;
/// One round of those bolts per pull, where the stock fusion rifle fires several.
const FUSION_BULLETS: u16 = 1;
/// More than a rocket launcher's one trajectory.
const ROCKET_PELLETS: usize = 3;
/// A row's declared pointers and references: its handle reference, its instance pointer and the
/// objects it owns. Every other byte of a row is data a grown row keeps as the stock row has it.
const ROW_POINTERS: [std::ops::Range<usize>; 3] = [0x00..0x18, 0x50..0x58, 0x60..0x68];

mod controls;

fn relative(bytes: &[u8], at: usize) -> usize {
    let delta = i64::from_le_bytes(bytes[at..at + 8].try_into().unwrap());
    assert_ne!(delta, 0, "a required native pointer is present");
    at.checked_add_signed(isize::try_from(delta).unwrap())
        .unwrap()
}

/// Whether `graph` names a graph with one Projectile Movement, the graphs pellet growth fits.
fn moving(manager: &PackageManager, graph: u32) -> bool {
    !matches!(graph, 0 | u32::MAX)
        && manager.read_tag(TagHash(graph)).is_ok_and(|payload| {
            weapon_component_bindings(&payload, MOVEMENT).is_ok_and(|found| found.len() == 1)
        })
}

/// The graphs the Barrel definition's projectile slots name at +0xB80 and +0xB90, which a weapon
/// whose content block names none fires, read without the compiler's reader.
fn barrel_graphs(manager: &PackageManager, entity: &[u8]) -> [u32; 2] {
    let binding = weapon_component_bindings(entity, BARREL).unwrap()[0];
    let owner = manager.read_tag(TagHash(binding.owner_tag)).unwrap();
    let definition = read_u64(&owner, binding.resource_offset as usize + 8).unwrap() as usize;
    [0xB80, 0xB90].map(|slot| read_u32(&owner, definition + slot).unwrap())
}

/// The byte after the Barrel definition's spread pointer, which turns each bullet's pattern by a
/// random angle when set, read without the compiler's spread reader.
fn rotation_byte(manager: &PackageManager, entity: &[u8]) -> u8 {
    let binding = weapon_component_bindings(entity, BARREL).unwrap()[0];
    let owner = manager.read_tag(TagHash(binding.owner_tag)).unwrap();
    let definition = read_u64(&owner, binding.resource_offset as usize + 8).unwrap() as usize;
    owner[definition + 0xE68]
}

/// Decode the native pattern and complete ring rows without the compiler's spread reader.
fn spread(manager: &PackageManager, entity: &[u8]) -> (u32, Vec<u8>) {
    let bindings = weapon_component_bindings(entity, BARREL).unwrap();
    let [binding] = bindings.as_slice() else {
        panic!("one barrel")
    };
    let owner = manager.read_tag(TagHash(binding.owner_tag)).unwrap();
    let instance = binding.resource_offset as usize;
    assert_eq!(binding.concrete_class, 0x8080_3889);
    assert_eq!(read_u32(&owner, instance + 4).unwrap(), 0x8080_3865);
    let definition = read_u64(&owner, instance + 8).unwrap() as usize;
    let slot = definition + 0xE60;
    if read_u64(&owner, slot).unwrap() == 0 {
        return (1, Vec::new());
    }
    let pattern = relative(&owner, slot);
    assert_eq!(read_u32(&owner, pattern - 4).unwrap(), 0x8080_888D);
    for ordinal in 0..3 {
        let at = pattern + ordinal * 0x18;
        assert_eq!(read_u32(&owner, at).unwrap(), 0x8080_888D);
        assert_eq!(read_u32(&owner, at + 4).unwrap(), ordinal as u32);
        assert_eq!(read_u32(&owner, at + 8).unwrap(), binding.owner_tag);
        assert_eq!(read_u32(&owner, at + 12).unwrap(), 0x8080_888D);
        assert_eq!(read_u64(&owner, at + 16).unwrap(), pattern as u64);
    }
    let count = read_u64(&owner, pattern + 0x48).unwrap() as usize;
    let header = relative(&owner, pattern + 0x50);
    assert_eq!(read_u32(&owner, header - 4).unwrap(), 0x8080_9FBD);
    assert_eq!(read_u64(&owner, header).unwrap(), count as u64);
    assert_eq!(read_u32(&owner, header + 8).unwrap(), 0x8080_888F);
    let rows = owner[header + 16..header + 16 + count * 20].to_vec();
    let pellets = read_u32(&owner, pattern + 0x60).unwrap();
    assert_eq!(
        rows.chunks_exact(20)
            .map(|row| read_u32(row, 12).unwrap())
            .sum::<u32>(),
        pellets
    );
    (pellets, rows)
}

fn fired(manager: &PackageManager, entity: &[u8], group: u32) -> u32 {
    let binding = weapon_component_bindings(entity, 0x5F0D_D954).unwrap()[0];
    let owner = manager.read_tag(TagHash(binding.owner_tag)).unwrap();
    let definition = read_u64(&owner, binding.resource_offset as usize + 8).unwrap() as usize;
    let blocks = crate::weapon::ammo::property_offsets(&owner, definition).unwrap();
    let block = blocks
        .iter()
        .copied()
        .find(|at| read_u32(&owner, at + 0x10).unwrap() == group)
        .unwrap_or(blocks[0]);
    let named = |at: usize| {
        Some(read_u32(&owner, at + 0xF0).unwrap()).filter(|graph| !matches!(*graph, 0 | u32::MAX))
    };
    // A block that names no graph, as Bellowing Giant's group's does, leaves the graph to another
    // block. The build fits every block's graph, so the first that names one stands for them.
    named(block)
        .or_else(|| blocks.iter().find_map(|&at| named(at)))
        .expect("a content block names the fired graph")
}

/// Every moving-projectile graph the weapon's content blocks name, each once in block order. The
/// build fits each of them, since a perk can select any block.
fn block_graphs(manager: &PackageManager, entity: &[u8]) -> Vec<u32> {
    let binding = weapon_component_bindings(entity, 0x5F0D_D954).unwrap()[0];
    let owner = manager.read_tag(TagHash(binding.owner_tag)).unwrap();
    let definition = read_u64(&owner, binding.resource_offset as usize + 8).unwrap() as usize;
    let mut graphs = Vec::new();
    for at in crate::weapon::ammo::property_offsets(&owner, definition).unwrap() {
        let graph = read_u32(&owner, at + 0xF0).unwrap();
        let moving = || {
            let payload = manager.read_tag(TagHash(graph)).unwrap();
            weapon_component_bindings(&payload, MOVEMENT).is_ok_and(|found| found.len() == 1)
        };
        if !matches!(graph, 0 | u32::MAX) && !graphs.contains(&graph) && moving() {
            graphs.push(graph);
        }
    }
    graphs
}

/// A row's data: its bytes with its declared pointers and references cleared.
fn row_data(owner: &[u8], row: usize) -> Vec<u8> {
    let mut data = owner[row..row + 0x210].to_vec();
    for range in ROW_POINTERS {
        data[range].fill(0);
    }
    data
}

/// Follow every trajectory and handle in the packed owner, including both reset counters, and the
/// objects rows own. Returns how many rows own one, and the data every row shares.
#[allow(clippy::cognitive_complexity)]
fn pool(manager: &PackageManager, graph: u32) -> (usize, [u32; 2], usize, Vec<u8>) {
    let entity = manager.read_tag(TagHash(graph)).unwrap();
    let bindings = weapon_component_bindings(&entity, MOVEMENT).unwrap();
    let [binding] = bindings.as_slice() else {
        panic!("one moving projectile")
    };
    let owner = manager.read_tag(TagHash(binding.owner_tag)).unwrap();
    let instance = binding.resource_offset as usize;
    let definition = read_u64(&owner, instance + 8).unwrap() as usize;
    let count = read_u64(&owner, instance + 0x1A8).unwrap() as usize;
    let header = relative(&owner, instance + 0x1B0);
    assert_eq!(read_u32(&owner, header - 4).unwrap(), 0x8080_9FBD);
    assert_eq!(read_u64(&owner, header).unwrap(), count as u64);
    assert_eq!(read_u32(&owner, header + 8).unwrap(), 0x8080_37BA);
    assert_eq!(read_u32(&owner, instance + 0x1C0).unwrap() as usize, count);
    assert_eq!(read_u64(&owner, definition + 0x80).unwrap() as usize, count);
    let mut handles = BTreeSet::new();
    for i in 0..count {
        let row = header + 16 + i * 0x210;
        assert_eq!(read_u32(&owner, row).unwrap(), binding.owner_tag);
        assert_eq!(read_u32(&owner, row + 4).unwrap(), 0x8080_37BB);
        let handle = read_u64(&owner, row + 8).unwrap() as usize;
        assert!(handles.insert(handle));
        assert_eq!(relative(&owner, row + 0x10), instance);
        assert_eq!(read_u32(&owner, handle).unwrap(), binding.owner_tag);
        assert_eq!(read_u32(&owner, handle + 4).unwrap(), 0x8080_37BA);
        assert_eq!(read_u64(&owner, handle + 8).unwrap(), row as u64);
        assert_eq!(
            row_data(&owner, row),
            row_data(&owner, header + 16),
            "row {i} keeps the template row's data"
        );
    }
    // Each owning row names a copy of its own, inside the instance region, with the first row's
    // bytes.
    let objects = (0..count)
        .map(|i| header + 16 + i * 0x210 + 0x50)
        .filter(|slot| read_u64(&owner, *slot).unwrap() != 0)
        .map(|slot| relative(&owner, slot))
        .collect::<Vec<_>>();
    if let Some(&first) = objects.first() {
        assert_eq!(objects.len(), count, "every row owns an object");
        assert_eq!(objects.iter().collect::<BTreeSet<_>>().len(), count);
        for &object in &objects {
            assert_eq!(read_u32(&owner, object - 4).unwrap(), 0x8080_37BD);
            assert!(object > header && object + 0x100 <= definition);
            assert_eq!(owner[object..object + 0x100], owner[first..first + 0x100]);
        }
    }
    (
        count,
        [
            read_u32(&owner, instance + 0x144).unwrap(),
            read_u32(&owner, definition + 0x88).unwrap(),
        ],
        objects.len(),
        row_data(&owner, header + 16),
    )
}

/// Every output of the stat translator table for `group`, by translation, tier and column, and the
/// one output all of the Barrel's burst inputs copy, decoded without the compiler's burst reader.
fn burst(
    manager: &PackageManager,
    entity: &[u8],
    group: u32,
) -> (Vec<Vec<Vec<u32>>>, (usize, usize)) {
    let bindings = weapon_component_bindings(entity, WEAPON_STAT_TRANSLATOR_COMPONENT_KEY).unwrap();
    let [binding] = bindings.as_slice() else {
        panic!("one stat translator")
    };
    let owner = manager.read_tag(TagHash(binding.owner_tag)).unwrap();
    // A descriptor is a count and a relative pointer to a marked header that names the row class.
    let array = |at: usize| -> Option<(usize, u32, usize)> {
        let count = usize::try_from(read_u64(&owner, at).ok()?).ok()?;
        if count == 0 {
            return Some((0, 0, 0));
        }
        let delta = i64::from_le_bytes(owner.get(at + 8..at + 16)?.try_into().ok()?);
        let header = (at + 8).checked_add_signed(isize::try_from(delta).ok()?)?;
        if header < 4
            || read_u32(&owner, header - 4).ok()? != 0x8080_9FBD
            || read_u64(&owner, header).ok()? != count as u64
        {
            return None;
        }
        Some((count, read_u32(&owner, header + 8).ok()?, header + 16))
    };
    let of = |at: usize, class: u32| {
        let (count, found, rows) = array(at).unwrap();
        assert!(
            count == 0 || found == class,
            "class 0x{class:08X} at 0x{at:X}"
        );
        (count, rows)
    };
    let only = |class: u32| {
        let found = (0..owner.len().saturating_sub(16))
            .step_by(8)
            .filter_map(array)
            .filter(|found| found.0 > 0 && found.1 == class)
            .map(|(count, _, rows)| (count, rows))
            .collect::<BTreeSet<_>>();
        assert_eq!(found.len(), 1, "one array of class 0x{class:08X}");
        found.into_iter().next().unwrap()
    };
    let keys = only(0x8080_38C7);
    let tables = only(0x8080_3975);
    assert_eq!(keys.0, tables.0);
    let index = (0..keys.0)
        .find(|key| read_u32(&owner, keys.1 + key * 0x28 + 0x10).unwrap() == group)
        .unwrap_or(0);
    let table = tables.1 + index * 0x30;
    let (count, rows) = of(table, 0x8080_3979);
    let translations: Vec<Vec<Vec<u32>>> = (0..count)
        .map(|translation| {
            let (tiers, rows) = of(rows + translation * 0x38 + 0x10, 0x8080_9F2A);
            (0..tiers)
                .map(|tier| {
                    let (columns, values) = of(rows + tier * 0x10, 0x8080_000F);
                    (0..columns)
                        .map(|column| read_u32(&owner, values + column * 4).unwrap())
                        .collect()
                })
                .collect()
        })
        .collect();
    let (routes, rows) = of(table + 0x10, 0x8080_3981);
    let sources = BARREL_BULLETS_PER_SHOT.map(|input| {
        let found = (0..routes)
            .map(|route| rows + route * 0x50)
            .filter(|&route| {
                i64::from(read_u32(&owner, route).unwrap()) == BARREL_COMPONENT
                    && i64::from(read_u32(&owner, route + 4).unwrap()) == input
            })
            .collect::<Vec<_>>();
        let [route] = found[..] else {
            panic!("one route for Barrel input {input}")
        };
        assert_eq!(read_u32(&owner, route + 0xC).unwrap(), 4, "a direct copy");
        read_u32(&owner, route + 0x1C).unwrap()
    });
    assert!(sources.iter().all(|&source| source == sources[0]));
    // Outputs are numbered from 0x1E00, 0x20 for each translation.
    let output = sources[0] - 0x1E00;
    (
        translations,
        ((output / 0x20) as usize, (output % 0x20) as usize),
    )
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES and a fresh SUNDIAL_TEST_ARTIFACTS directory"]
#[allow(
    clippy::cognitive_complexity,
    reason = "The staged workflow keeps its independent native readback and source-preservation checks together"
)]
fn composed_pellet_barrels_and_final_projectiles_survive_packaging() {
    let packages = crate::test_support::stock_packages();
    let output = std::path::PathBuf::from(
        std::env::var_os("SUNDIAL_TEST_ARTIFACTS").expect("artifact directory"),
    )
    .join("pellet-barrels");
    fs::create_dir_all(output.parent().unwrap()).unwrap();
    fs::create_dir(&output).expect("use a fresh artifact directory");
    let revision = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(revision.status.success());
    let changes = std::process::Command::new("git")
        .args(["diff", "--binary", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(changes.status.success());
    fs::write(output.join("source.diff"), changes.stdout).unwrap();
    // Preserve new source files too, which a working-tree diff omits until they are tracked.
    for (name, source) in [
        (
            "spread.rs",
            include_str!(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../src/entity/spread.rs"
            )),
        ),
        ("firing.rs", include_str!("../../custom_runtime/firing.rs")),
        ("burst.rs", include_str!("../../../weapon/burst.rs")),
        ("pellet_barrels.rs", include_str!("pellet_barrels.rs")),
    ] {
        fs::write(output.join(format!("source-{name}")), source).unwrap();
    }
    let stock = open_manager(&packages).unwrap();
    let catalog = crate::test_support::catalog(packages.parent().unwrap()).unwrap();
    let donors = catalog.weapon_donors();
    let named = |name: &str| {
        donors
            .iter()
            .find(|donor| {
                donor.name.replace('\u{2019}', "'") == name && donor.weapon_pattern_index.is_some()
            })
            .unwrap_or_else(|| panic!("required donor {name}"))
    };
    let hand = named("IKELOS_HC_v1.0.2");
    let shotgun = named("Felwinter's Lie");
    let shared = named("Baligant XU7743");
    let thorn = named("Thorn");
    let wolves = named("Lord of Wolves");
    let fusion = named("Cartesian Coordinate");
    let rocket = named("Bellowing Giant");
    let runtime = |donor: &sundial::investment::WeaponDonorSummary| {
        load_weapon_runtime_entity_at_pattern_index_with_manager(
            &stock,
            donor.weapon_pattern_index.unwrap(),
        )
        .unwrap()
    };
    let hand_runtime = runtime(hand);
    let shotgun_runtime = runtime(shotgun);
    let shared_runtime = runtime(shared);
    assert_ne!(
        shared.hash, shared_runtime.item_hash,
        "this donor must exercise an item whose selector names another pattern identity"
    );
    let thorn_runtime = runtime(thorn);
    let wolves_runtime = runtime(wolves);
    let fusion_runtime = runtime(fusion);
    let rocket_runtime = runtime(rocket);
    let expected_spread = spread(&stock, &shotgun_runtime.payload);
    let shared_spread = spread(&stock, &shared_runtime.payload);
    assert!(expected_spread.0 > 1 && !expected_spread.1.is_empty());
    assert!(shared_spread.0 > 1 && !shared_spread.1.is_empty());
    let thorn_graph = fired(
        &stock,
        &thorn_runtime.payload,
        thorn_runtime.weapon_content_group_hash,
    );
    let wolves_graph = fired(
        &stock,
        &wolves_runtime.payload,
        wolves_runtime.weapon_content_group_hash,
    );
    assert!(pool(&stock, thorn_graph).0 < expected_spread.0 as usize);
    // Each of these weapons must have a graph whose rows own objects and must grow.
    for (label, source, pellets) in [
        ("fusion", &fusion_runtime, FUSION_PELLETS),
        ("rocket", &rocket_runtime, ROCKET_PELLETS),
    ] {
        assert!(
            block_graphs(&stock, &source.payload).iter().any(|&graph| {
                let (count, _, owned, _) = pool(&stock, graph);
                count < pellets && owned == count
            }),
            "a {label} graph whose rows own objects grows"
        );
    }
    let stock_burst = burst(
        &stock,
        &fusion_runtime.payload,
        fusion_runtime.weapon_translation_group_hash,
    );
    let (translation, column) = stock_burst.1;
    let stock_bullets = stock_burst.0[translation]
        .iter()
        .map(|tier| tier[column])
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let [stock_bullets] = stock_bullets[..] else {
        panic!("one stock bullets per shot at every tier")
    };
    assert_ne!(stock_bullets, f32::from(FUSION_BULLETS).to_bits());
    let wolves_capacity = pool(&stock, wolves_graph).0;
    assert!(wolves_capacity >= expected_spread.0 as usize);
    let graph_payload = stock.read_tag(TagHash(thorn_graph)).unwrap();
    let graph =
        load_weapon_runtime_graph_for_entity(&stock, 0, 0, thorn_graph, &graph_payload).unwrap();
    let mut speed = Vec::new();
    parameters::discover(&graph)
        .iter()
        .find(|parameter| parameter.kind == Kind::Speed)
        .unwrap()
        .set(&mut speed, 3.25)
        .unwrap();
    let make = |label: &str,
                host: &sundial::investment::WeaponDonorSummary,
                barrel: &sundial::investment::WeaponDonorSummary,
                behavior: Option<&str>| {
        let mut recipe = crate::WeaponRecipe::new_weapon_for_donor(
            format!("parhelion.pellets.{label}"),
            host.hash,
            &host.name,
        )
        .unwrap();
        recipe.set_component_splice(
            BARREL,
            Some(crate::WeaponDonorReference {
                item_hash: barrel.hash.into(),
                expected_name: Some(barrel.name.clone()),
            }),
        );
        recipe.overrides.skip_behavior_perks = true;
        recipe.overrides.additional_behaviors = behavior
            .into_iter()
            .map(|behavior| crate::recipe::AdditionalBehaviorRecipe {
                behavior: behavior.into(),
            })
            .collect();
        recipe
    };
    let mut edited = make("typed", hand, shotgun, Some("thorn-graph"));
    edited.overrides.projectile = Some(crate::weapon::projectile::Edits {
        graph: thorn_graph,
        values: speed,
    });
    let mut cleared = make("cleared", hand, shotgun, Some("thorn-graph"));
    let binding = weapon_component_bindings(&hand_runtime.payload, BARREL).unwrap()[0];
    let owner = stock.read_tag(TagHash(binding.owner_tag)).unwrap();
    let slot = read_u64(&owner, binding.resource_offset as usize + 8).unwrap() as usize + 0xE60;
    cleared.overrides.runtime_resource_patches.push(
        crate::recipe::WeaponRuntimeResourcePatchRecipe {
            binding_hash: BARREL.into(),
            resource_index: 0,
            offset: (slot - binding.resource_offset as usize) as u32,
            bytes: "00 00 00 00 00 00 00 00".into(),
            graph_values: Vec::new(),
        },
    );
    let recipes = vec![
        edited,
        make("sufficient", hand, shotgun, Some("lord-of-wolves-graph")),
        make("base", thorn, shotgun, None),
        make("removed", shotgun, hand, Some("thorn-graph")),
        cleared,
        make("shared-pattern", hand, shared, Some("thorn-graph")),
        {
            // As an author sets Pellets per Bullet on the fusion rifle's own Barrel.
            let mut recipe = crate::WeaponRecipe::new_weapon_for_donor(
                "parhelion.pellets.fusion",
                fusion.hash,
                &fusion.name,
            )
            .unwrap();
            recipe.overrides.barrel = Some(crate::weapon::barrel::Edits {
                pellets: Some(FUSION_PELLETS as u16),
                bullets_per_shot: Some(FUSION_BULLETS),
                random_rotation: Some(false),
                ..Default::default()
            });
            recipe
        },
        {
            // A rocket launcher's rows own objects and hold a data word that reads like a pointer.
            let mut recipe = crate::WeaponRecipe::new_weapon_for_donor(
                "parhelion.pellets.rocket",
                rocket.hash,
                &rocket.name,
            )
            .unwrap();
            recipe.overrides.barrel = Some(crate::weapon::barrel::Edits {
                pellets: Some(ROCKET_PELLETS as u16),
                ..Default::default()
            });
            recipe
        },
    ];
    let recipes = recipes
        .into_iter()
        .enumerate()
        .map(|(i, recipe)| {
            let path = output.join(format!("recipe-{i}.json"));
            recipe.save_json(&path).unwrap();
            crate::WeaponRecipe::load_json(path).unwrap()
        })
        .collect::<Vec<_>>();
    let snapshot = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
        package_directory: packages.clone(),
        staging_root: output.join("staged"),
        ignore_installed_authored_overlays: true,
        recipes: recipes.clone(),
    })
    .unwrap();
    let build = crate::build_and_stage_snapshot_with_progress(&snapshot, |_| {}).unwrap();
    let view = crate::workflow::FilteredPackageView::create(&packages, &[]).unwrap();
    for artifact in &build.artifacts {
        view.add_overlay(&build.run_directory.join(&artifact.file_name))
            .unwrap();
    }
    let manager = open_manager(view.path()).unwrap();
    let mut readback = Vec::new();
    for (i, recipe) in recipes.iter().enumerate() {
        let item = recipe.identity.item_hash.parse_u32().unwrap();
        let authored = load_weapon_runtime_entity_with_manager(&manager, item).unwrap();
        let actual = spread(&manager, &authored.payload);
        let rotation = rotation_byte(&manager, &authored.payload);
        if i == 6 {
            assert_eq!(actual.0 as usize, FUSION_PELLETS);
            // The stock fusion Barrel turns each bullet's pattern, so the authored 0 is the edit.
            assert_eq!(rotation_byte(&stock, &fusion_runtime.payload), 1);
            assert_eq!(
                rotation, 0,
                "the fusion Barrel keeps each bullet's pattern level"
            );
        } else if i == 7 {
            assert_eq!(actual.0 as usize, ROCKET_PELLETS);
        } else {
            assert_eq!(
                actual,
                if i < 3 {
                    expected_spread.clone()
                } else if i == 5 {
                    shared_spread.clone()
                } else {
                    (1, Vec::new())
                }
            );
        }
        let graph = fired(
            &manager,
            &authored.payload,
            authored.weapon_content_group_hash,
        );
        let (capacity, speeds, owned, _) = pool(&manager, graph);
        let expected_capacity = if i == 1 {
            wolves_capacity
        } else if i == 6 {
            FUSION_PELLETS
        } else if i == 7 {
            ROCKET_PELLETS
        } else if i < 3 {
            expected_spread.0 as usize
        } else if i == 5 {
            (shared_spread.0 as usize).max(pool(&stock, thorn_graph).0)
        } else {
            pool(&stock, thorn_graph).0
        };
        assert_eq!(capacity, expected_capacity);
        if i == 0 {
            assert_eq!(speeds, [3.25_f32.to_bits(); 2]);
        }
        // Every graph a block names grows, each row keeping the stock row's data and owning a copy
        // of its own of what the stock row owns.
        if let Some((source, pellets)) = match i {
            6 => Some((&fusion_runtime, FUSION_PELLETS)),
            7 => Some((&rocket_runtime, ROCKET_PELLETS)),
            _ => None,
        } {
            let before = block_graphs(&stock, &source.payload);
            let after = block_graphs(&manager, &authored.payload);
            assert_eq!(
                before.len(),
                after.len(),
                "every content block keeps its graph"
            );
            // The rocket's selected block names no graph, so it fires its Barrel's, which grows
            // the same way with both slots naming the grown copy. The fusion rifle's block fires
            // its own, so its Barrel's graph need not grow.
            let (before, after) = if i == 7 {
                let [stock_barrel, repeated] = barrel_graphs(&stock, &source.payload);
                assert_eq!(stock_barrel, repeated, "the stock Barrel's slots agree");
                assert!(
                    moving(&stock, stock_barrel),
                    "the rocket fires its Barrel's graph"
                );
                let [grown_barrel, grown_repeat] = barrel_graphs(&manager, &authored.payload);
                assert_eq!(
                    grown_barrel, grown_repeat,
                    "both Barrel slots name the grown graph"
                );
                (
                    before.into_iter().chain([stock_barrel]).collect::<Vec<_>>(),
                    after.into_iter().chain([grown_barrel]).collect::<Vec<_>>(),
                )
            } else {
                (before, after)
            };
            for (&stock_graph, &grown) in before.iter().zip(&after) {
                let (count, _, owned_before, data_before) = pool(&stock, stock_graph);
                let (capacity, _, owned_after, data_after) = pool(&manager, grown);
                assert_eq!(capacity, pellets.max(count));
                assert_eq!(data_after, data_before, "rows keep the stock row's data");
                if owned_before > 0 {
                    assert_eq!(owned_after, capacity, "every grown row owns its own object");
                }
            }
        }
        let mut bullets = serde_json::Value::Null;
        if i == 6 {
            let mut expected = stock_burst.clone();
            for tier in &mut expected.0[translation] {
                tier[column] = f32::from(FUSION_BULLETS).to_bits();
            }
            assert_eq!(
                burst(
                    &manager,
                    &authored.payload,
                    authored.weapon_translation_group_hash
                ),
                expected,
                "only the bullet column changes, at every tier"
            );
            bullets = serde_json::json!({"stock": f32::from_bits(stock_bullets), "authored": FUSION_BULLETS, "translation": translation, "column": column, "tiers": expected.0[translation].len()});
        }
        if i == 0 || i == 2 || i == 5 || i == 6 || i == 7 {
            assert!(
                stock.get_entry(TagHash(graph)).is_none(),
                "growth uses a private graph"
            );
        }
        readback.push(serde_json::json!({"item": item, "entity": authored.entity_tag, "graph": graph, "pellets": actual.0, "rings": actual.1, "capacity": capacity, "owned_objects": owned, "speed_bits": speeds, "bullets_per_shot": bullets, "random_rotation_byte": rotation}));
    }
    let mut inputs = Vec::new();
    for (label, source) in [
        ("hand", &hand_runtime),
        ("shotgun", &shotgun_runtime),
        ("shared-pattern", &shared_runtime),
        ("thorn", &thorn_runtime),
        ("wolves", &wolves_runtime),
        ("fusion", &fusion_runtime),
        ("rocket", &rocket_runtime),
    ] {
        let mut tags = BTreeSet::from([source.entity_tag]);
        for key in [BARREL, 0x5F0D_D954] {
            tags.extend(
                weapon_component_bindings(&source.payload, key)
                    .unwrap()
                    .iter()
                    .map(|binding| binding.owner_tag),
            );
        }
        if label == "fusion" {
            tags.extend(
                weapon_component_bindings(&source.payload, WEAPON_STAT_TRANSLATOR_COMPONENT_KEY)
                    .unwrap()
                    .iter()
                    .map(|binding| binding.owner_tag),
            );
        }
        let graph = fired(&stock, &source.payload, source.weapon_content_group_hash);
        tags.insert(graph);
        let payload = stock.read_tag(TagHash(graph)).unwrap();
        tags.extend(
            weapon_component_bindings(&payload, MOVEMENT)
                .unwrap()
                .iter()
                .map(|binding| binding.owner_tag),
        );
        for tag in tags {
            let payload = stock.read_tag(TagHash(tag)).unwrap();
            assert_eq!(
                manager.read_tag(TagHash(tag)).unwrap(),
                payload,
                "stock {label} 0x{tag:08X}"
            );
            let path = output.join(format!("{label}-{tag:08X}.bin"));
            fs::write(&path, payload).unwrap();
            inputs.push(serde_json::json!({"file": path.file_name().unwrap().to_string_lossy(), "sha256": crate::artifact::digest_file(&path).unwrap().sha256}));
        }
    }
    let mut malformed = recipes[4].clone();
    malformed.overrides.runtime_resource_patches[0].bytes = "FF FF FF FF FF FF FF 7F".into();
    malformed.save_json(output.join("malformed.json")).unwrap();
    let bad = crate::BatchBuildSnapshot::new(crate::BatchBuildRequest {
        package_directory: packages.clone(),
        staging_root: output.join("rejected"),
        ignore_installed_authored_overlays: true,
        recipes: vec![malformed],
    })
    .unwrap();
    let rejection = crate::build_and_stage_snapshot_with_progress(&bad, |_| {})
        .expect_err("an unreadable final spread must fail")
        .to_string();
    let executable = std::env::current_exe().unwrap();
    let artifacts = build.artifacts.iter().map(|artifact| {
        let path = build.run_directory.join(&artifact.file_name);
        serde_json::json!({"file": path, "sha256": crate::artifact::digest_file(&path).unwrap().sha256})
    }).collect::<Vec<_>>();
    fs::write(output.join("readback.json"), serde_json::to_vec_pretty(&serde_json::json!({
        "native_build": "86657.20.08.23.1800.d2_rc", "packages": packages, "executable": executable,
        "revision_at_run": String::from_utf8_lossy(&revision.stdout).trim(), "working_changes": "source.diff",
        "repeat_command": "cargo test --release -p parhelion composed_pellet_barrels_and_final_projectiles_survive_packaging -- --ignored --nocapture",
        "executable_sha256": crate::artifact::digest_file(&executable).unwrap().sha256,
        "inputs": inputs, "artifacts": artifacts, "readback": readback, "malformed_rejection": rejection,
        "shared_pattern_donor": {"item": shared.hash, "pattern_index": shared.weapon_pattern_index, "pattern_item": shared_runtime.item_hash, "entity": shared_runtime.entity_tag},
        "limits": "Package readback only. Pellet damage, rendered bolts, bullets fired per pull and repeated firing require gameplay verification."
    })).unwrap()).unwrap();
}
