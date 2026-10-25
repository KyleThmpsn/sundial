//! Whether a weapon's firing, charge and other gameplay components can move between weapon
//! families through the component splice: a trace rifle's or a fusion rifle's Firing Behavior
//! onto an auto rifle, a hand cannon or a sidearm, and the Trigger Charge component onto a
//! weapon with or without one. The splice is what the Gameplay page offers, so its planner is
//! the oracle: a plan is a splice the build would write, and a refusal is the reason the page
//! would show. The ways the page's promise could be wrong, each checked against the packages:
//!
//! - a trace or fusion rifle's Firing Behavior, Barrel or Magazine does not plan onto a weapon
//!   of another family, so the three parts the page offers from any weapon are not from any
//! - Trigger Charge plans onto a host that has no such component, which no writer could place
//! - a host that has the component takes a charge weapon's, which the page could then offer
use super::*;
use std::fmt::Write as _;
use sundial::package_authoring::{
    entity::{
        WEAPON_BARREL_COMPONENT_KEY, WEAPON_CONTROLLER_COMPONENT_KEY, WEAPON_INPUT_COMPONENT_KEY,
        WEAPON_MAGAZINE_COMPONENT_KEY, WEAPON_TRIGGER_CHARGE_COMPONENT_KEY,
        WEAPON_TRIGGER_COMPONENT_KEY, plan_component_splice, weapon_component_bindings,
    },
    runtime::load_weapon_runtime_entity_with_manager,
};

const COMPONENTS: [(u32, &str); 6] = [
    (WEAPON_TRIGGER_COMPONENT_KEY, "Firing Behavior"),
    (WEAPON_TRIGGER_CHARGE_COMPONENT_KEY, "Trigger Charge"),
    (WEAPON_BARREL_COMPONENT_KEY, "Barrel"),
    (WEAPON_MAGAZINE_COMPONENT_KEY, "Magazine"),
    (WEAPON_CONTROLLER_COMPONENT_KEY, "Weapon Controller"),
    (WEAPON_INPUT_COMPONENT_KEY, "Input"),
];

/// The parts the Gameplay page offers from any weapon.
const OFFERED: [u32; 3] = [
    WEAPON_TRIGGER_COMPONENT_KEY,
    WEAPON_BARREL_COMPONENT_KEY,
    WEAPON_MAGAZINE_COMPONENT_KEY,
];

struct Weapon {
    name: String,
    entity: u32,
    payload: Vec<u8>,
}

fn plain(name: &str) -> String {
    name.to_lowercase().replace('\u{2019}', "'")
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES and SUNDIAL_TEST_ARTIFACTS"]
fn firing_and_charge_components_across_weapon_families() {
    let packages = crate::test_support::stock_packages();
    let catalog = crate::test_support::catalog(packages.parent().unwrap()).unwrap();
    let manager = open_shadowkeep_package_manager(&packages).unwrap();
    let donors = catalog.weapon_donors();
    let weapon = |hash: u32, name: &str| {
        load_weapon_runtime_entity_with_manager(&manager, hash)
            .ok()
            .map(|source| Weapon {
                name: name.to_owned(),
                entity: source.entity_tag,
                payload: source.payload,
            })
    };
    let named = |name: &str| {
        donors
            .iter()
            .filter(|donor| plain(&donor.name) == plain(name))
            .find_map(|donor| weapon(donor.hash, &donor.name))
            .unwrap_or_else(|| panic!("{name} with a runtime"))
    };
    let legendary = |type_name: &str| {
        donors
            .iter()
            .filter(|donor| {
                donor.type_name == type_name
                    && donor.rarity == sundial::investment::WeaponRarity::Legendary
            })
            .find_map(|donor| weapon(donor.hash, &format!("{} ({type_name})", donor.name)))
            .unwrap_or_else(|| panic!("a legendary {type_name} with a runtime"))
    };
    let sources = [
        named("Prometheus Lens"),
        named("Coldheart"),
        named("Jötunn"),
        named("Devil's Ruin"),
        legendary("Fusion Rifle"),
    ];
    let hosts = [
        legendary("Auto Rifle"),
        legendary("Hand Cannon"),
        legendary("Sidearm"),
        legendary("Fusion Rifle"),
    ];
    let count = |weapon: &Weapon, key: u32| {
        weapon_component_bindings(&weapon.payload, key).map_or(0, |bindings| bindings.len())
    };
    let mut report = String::from("# Gameplay components across weapon families\n\n");
    writeln!(
        report,
        "| Weapon | Entity | {} |",
        COMPONENTS.map(|(_, name)| name).join(" | ")
    )
    .unwrap();
    writeln!(report, "|{}", "---|".repeat(COMPONENTS.len() + 2)).unwrap();
    for weapon in sources.iter().chain(&hosts) {
        let counts = COMPONENTS
            .map(|(key, _)| count(weapon, key).to_string())
            .join(" | ");
        writeln!(
            report,
            "| {} | 0x{:08X} | {counts} |",
            weapon.name, weapon.entity
        )
        .unwrap();
    }
    report.push_str("\nComponent bindings per weapon, then what the splice planner says for each donor on each host.\n\n");
    writeln!(report, "| Donor | Host | Component | Plan |").unwrap();
    writeln!(report, "|---|---|---|---|").unwrap();
    let mut failures = Vec::new();
    for host in &hosts {
        for source in &sources {
            if host.entity == source.entity {
                continue;
            }
            for (key, name) in COMPONENTS {
                let plan = plan_component_splice(&manager, &host.payload, &source.payload, key);
                let line = match &plan {
                    Ok(plan) => format!(
                        "plans: {} writes, {} arrays appended",
                        plan.writes.len(),
                        plan.arrays.len()
                    ),
                    Err(error) => format!("refused: {error}"),
                };
                writeln!(
                    report,
                    "| {} | {} | {name} | {line} |",
                    source.name, host.name
                )
                .unwrap();
                let host_has = count(host, key) == 1;
                let source_has = count(source, key) == 1;
                if OFFERED.contains(&key) && host_has && source_has && plan.is_err() {
                    failures.push(format!(
                        "{name} from {} onto {}: {line}",
                        source.name, host.name
                    ));
                }
                if key == WEAPON_TRIGGER_CHARGE_COMPONENT_KEY && !host_has && plan.is_ok() {
                    failures.push(format!(
                        "Trigger Charge from {} planned onto {}, which has none",
                        source.name, host.name
                    ));
                }
            }
        }
    }
    let out = crate::test_support::artifact_dir("component-shapes");
    fs::create_dir_all(&out).unwrap();
    fs::write(out.join("report.md"), &report).unwrap();
    println!("{report}\nReport: {}", out.join("report.md").display());
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
