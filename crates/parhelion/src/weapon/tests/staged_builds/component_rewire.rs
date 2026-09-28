//! Advanced Gameplay component donors from another weapon type, applied to the whole group the
//! picker changes together, with connections the compiler rebuilds for the host. Built, staged
//! and read back from the packages the game would load. Package checks are not gameplay proof.
//! The ways this could still be wrong, each checked against the staged packages rather than the
//! recipe:
//!
//! - the build refuses a donor the rewire accepts on the stock entities, for a host that builds
//!   as a plain recipe
//! - a binding of the moved group does not resolve to the donor's owner once staged, or to a new
//!   private clone of it such as the HUD content owner that carries the weapon's HUD key
//! - the staged event table is not the one the rewire builds from the stock entities, apart from
//!   those clones' tags, so an edit after the graft or the packing changed it
//! - the authored weapon shares a stock runtime entity instead of its own
//! - a stock runtime entity or owner the rewire read changes in the staged packages
use super::*;
use std::{collections::BTreeMap, fmt::Write as _};
use sundial::package_authoring::{
    weapon_entity::{
        ComponentWiring, WEAPON_TRIGGER_COMPONENT_KEY, coupled_weapon_component_bindings,
        graft_weapon_component_bindings_or_rewire, weapon_component_bindings,
    },
    weapon_runtime::{
        WeaponRuntimeEntitySource, load_weapon_runtime_entity_at_pattern_index_with_manager,
        load_weapon_runtime_entity_with_manager,
    },
};

/// One authored weapon and the table its staged entity must carry.
struct Case {
    recipe: crate::WeaponRecipe,
    host: WeaponRuntimeEntitySource,
    donor: WeaponRuntimeEntitySource,
    bindings: Vec<u32>,
    expected: Vec<u8>,
}

/// The event rows of a weapon runtime entity, in stored order.
fn event_rows(entity: &[u8]) -> Vec<Vec<u8>> {
    let count = read_u64(entity, 0x20).unwrap() as usize;
    if count == 0 {
        return Vec::new();
    }
    let header = (0x28 + read_u64(entity, 0x28).unwrap() as i64) as usize;
    (0..count)
        .map(|index| entity[header + 16 + index * 0x48..header + 16 + (index + 1) * 0x48].to_vec())
        .collect()
}

fn component_owners(entity: &[u8]) -> Vec<u32> {
    let count = read_u64(entity, 0x10).unwrap() as usize;
    let header = (0x18 + read_u64(entity, 0x18).unwrap() as i64) as usize;
    (0..count)
        .map(|index| read_u32(entity, header + 16 + index * 0x0C).unwrap())
        .collect()
}

/// Weapons of a type that have a runtime row, as (hash, name, pattern row), legendary first.
fn weapons_of(catalog: &InvestmentCatalog, type_name: &str) -> Vec<(u32, String, u16)> {
    let mut weapons = catalog
        .weapon_donors()
        .into_iter()
        .filter(|donor| donor.type_name == type_name && !donor.name.is_empty())
        .filter_map(|donor| {
            let legendary = donor.rarity == sundial::investment::WeaponRarity::Legendary;
            Some((
                !legendary,
                donor.hash,
                donor.name,
                donor.weapon_pattern_index?,
            ))
        })
        .collect::<Vec<_>>();
    weapons.sort();
    weapons
        .into_iter()
        .map(|(_, hash, name, pattern)| (hash, name, pattern))
        .collect()
}

/// The first host and donor of these types whose group for `binding` the compiler must rewire,
/// with the table the rewire builds from the stock entities. A host that cannot build as a plain
/// recipe is skipped, since its own sockets say nothing about the runtime.
fn find_case(
    packages: &Path,
    manager: &sundial::package_authoring::PackageManager,
    catalog: &InvestmentCatalog,
    (host_type, donor_type): (&str, &str),
    binding: u32,
    name: &str,
) -> Case {
    let read = |tag: u32| manager.read_tag(tag);
    let load = |weapons: Vec<(u32, String, u16)>| {
        weapons
            .into_iter()
            .map(|weapon| {
                let entity =
                    load_weapon_runtime_entity_at_pattern_index_with_manager(manager, weapon.2)
                        .unwrap();
                (weapon, entity)
            })
            .collect::<Vec<_>>()
    };
    let donors = load(weapons_of(catalog, donor_type));
    let namespace = format!(
        "parhelion.component-rewire.{}",
        name.to_ascii_lowercase().replace(' ', "-")
    );
    for (host, host_entity) in load(weapons_of(catalog, host_type)) {
        let bindings = coupled_weapon_component_bindings(&host_entity.payload, binding).unwrap();
        let Some((donor, donor_entity, expected)) =
            donors.iter().find_map(|(donor, donor_entity)| {
                if donor_entity.entity_tag == host_entity.entity_tag {
                    return None;
                }
                let grafts = bindings
                    .iter()
                    .map(|&binding| (binding, donor_entity.payload.as_slice()))
                    .collect::<Vec<_>>();
                let mut expected = host_entity.payload.clone();
                (graft_weapon_component_bindings_or_rewire(&mut expected, &grafts, &read)
                    == Ok(ComponentWiring::Rewired))
                .then_some((donor, donor_entity, expected))
            })
        else {
            continue;
        };
        let mut recipe =
            crate::WeaponRecipe::new_weapon_for_donor(&namespace, host.0, &host.1).unwrap();
        recipe.name = name.to_owned();
        let plain = WeaponProjectSpec {
            weapons: vec![recipe.to_spec().unwrap()],
        };
        if build_weapon_project_after_catalog_validation(packages, &plain).is_err() {
            continue;
        }
        for &binding in &bindings {
            recipe.set_runtime_component_donor(
                binding,
                Some(crate::WeaponDonorReference {
                    item_hash: donor.0.into(),
                    expected_name: Some(donor.1.clone()),
                }),
            );
        }
        return Case {
            recipe,
            host: host_entity,
            donor: donor_entity.clone(),
            bindings,
            expected,
        };
    }
    panic!("no {donor_type} {binding:08X} group rewires onto a buildable {host_type}");
}

/// Owners the build cloned privately after the graft, such as the HUD content owner that carries
/// the weapon's own HUD key, mapped back to the owner each was cloned from. Every clone must be a
/// new tag in the place its original held.
fn private_clones(
    staged: &[u8],
    expected: &[u8],
    source: &sundial::package_authoring::PackageManager,
    name: &str,
) -> BTreeMap<u32, u32> {
    let staged = component_owners(staged);
    let expected = component_owners(expected);
    assert_eq!(staged.len(), expected.len(), "{name}: component count");
    let mut clones = BTreeMap::new();
    for (clone, original) in staged.into_iter().zip(expected) {
        if clone != original {
            assert!(
                source.read_tag(clone).is_err(),
                "{name}: owner 0x{original:08X} became another stock owner 0x{clone:08X}"
            );
            clones.insert(clone, original);
        }
    }
    clones
}

/// Event rows with every private clone named by the owner it was cloned from.
fn unclone(rows: Vec<Vec<u8>>, clones: &BTreeMap<u32, u32>) -> Vec<Vec<u8>> {
    rows.into_iter()
        .map(|mut row| {
            for at in [0x08, 0x28] {
                let tag = read_u32(&row, at).unwrap();
                if let Some(original) = clones.get(&tag) {
                    row[at..at + 4].copy_from_slice(&original.to_le_bytes());
                }
            }
            row
        })
        .collect()
}

/// Reads one weapon back from the staged packages and checks it, adding it to the report.
fn verify(
    staged: &sundial::package_authoring::PackageManager,
    source: &sundial::package_authoring::PackageManager,
    case: &Case,
    report: &mut String,
) {
    let name = &case.recipe.name;
    let hash = case.recipe.identity.item_hash.parse_u32().unwrap();
    let authored = load_weapon_runtime_entity_with_manager(staged, hash).unwrap();
    assert_ne!(
        authored.entity_tag, case.host.entity_tag,
        "{name}: the weapon has its own runtime entity"
    );
    let clones = private_clones(&authored.payload, &case.expected, source, name);
    for &binding in &case.bindings {
        let donor_owners = weapon_component_bindings(&case.donor.payload, binding)
            .unwrap()
            .into_iter()
            .map(|binding| binding.owner_tag)
            .collect::<Vec<_>>();
        let staged_owners = weapon_component_bindings(&authored.payload, binding)
            .unwrap()
            .into_iter()
            .map(|binding| *clones.get(&binding.owner_tag).unwrap_or(&binding.owner_tag))
            .collect::<Vec<_>>();
        assert_eq!(
            staged_owners, donor_owners,
            "{name}: binding 0x{binding:08X}"
        );
    }
    assert_eq!(
        unclone(event_rows(&authored.payload), &clones),
        event_rows(&case.expected),
        "{name}: staged event table"
    );
    // Everything the rewire read stays stock for every other weapon.
    for entity in [&case.host, &case.donor] {
        assert_eq!(
            staged.read_tag(entity.entity_tag).unwrap(),
            source.read_tag(entity.entity_tag).unwrap(),
            "{name}: stock entity 0x{:08X}",
            entity.entity_tag
        );
        for owner in component_owners(&entity.payload) {
            assert_eq!(
                staged.read_tag(owner).unwrap(),
                source.read_tag(owner).unwrap(),
                "{name}: stock owner 0x{owner:08X}"
            );
        }
    }
    let host_rows = event_rows(&case.host.payload).len();
    let _ = writeln!(
        report,
        "## {name}\n\nHost runtime 0x{:08X}, donor runtime 0x{:08X}, authored runtime 0x{:08X}.\n\
         {} bindings moved. Event rows: {host_rows} stock, {} rewired. Private owner clones: {}.\n",
        case.host.entity_tag,
        case.donor.entity_tag,
        authored.entity_tag,
        case.bindings.len(),
        event_rows(&authored.payload).len(),
        clones
            .iter()
            .map(|(clone, original)| format!("0x{clone:08X} from 0x{original:08X}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
}

#[test]
#[ignore = "requires PARHELION_CLEAN_STOCK_PACKAGES pointing to Shadowkeep packages"]
fn rewired_component_donors_stage_the_tables_the_rewire_builds() {
    let packages = PathBuf::from(std::env::var_os("PARHELION_CLEAN_STOCK_PACKAGES").unwrap());
    let catalog = InvestmentCatalog::load(packages.parent().unwrap(), false, |_| {}).unwrap();
    let source = open_manager(&packages).unwrap();
    // Host type, donor type: a hand cannon's Firing Behavior on a sidearm and the reverse, and an
    // auto rifle's on a scout rifle. Every component of these weapons shares one picker group.
    let cases = [
        (("Sidearm", "Hand Cannon"), "Cannon Sidearm"),
        (("Hand Cannon", "Sidearm"), "Sidearm Cannon"),
        (("Scout Rifle", "Auto Rifle"), "Auto Scout"),
    ]
    .into_iter()
    .map(|(types, name)| {
        find_case(
            &packages,
            &source,
            &catalog,
            types,
            WEAPON_TRIGGER_COMPONENT_KEY,
            name,
        )
    })
    .collect::<Vec<_>>();
    let specs = cases
        .iter()
        .map(|case| case.recipe.to_spec().unwrap())
        .collect::<Vec<_>>();
    let bundle = build_weapon_project_after_catalog_validation(
        &packages,
        &WeaponProjectSpec { weapons: specs },
    )
    .expect("the rewired component batch builds");
    let view = staged_view(&packages, ".parhelion-component-rewire-", &bundle);
    let staged = open_manager(&view.path().join("packages")).unwrap();
    let out = std::env::var_os("PARHELION_REWIRE_OUT").map_or_else(
        || std::env::temp_dir().join("parhelion-component-rewire"),
        PathBuf::from,
    );
    fs::create_dir_all(&out).unwrap();
    let mut report = String::from("# Component Rewire E2E\n\n");
    for case in &cases {
        verify(&staged, &source, case, &mut report);
        // The recipe, so the same weapon can be built and tried in game.
        case.recipe
            .save_json(out.join(format!("{}.parhelion.json", case.recipe.slug())))
            .unwrap();
    }
    fs::write(out.join("report.md"), &report).unwrap();
    eprintln!("{report}\nReport: {}", out.join("report.md").display());
}
