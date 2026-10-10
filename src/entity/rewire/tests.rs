//! Rewires every Advanced Gameplay component of every stock weapon runtime onto every other, alone
//! and as the whole group the picker changes together, and checks each rebuilt event table against
//! the wiring stock weapons ship. Package checks are not gameplay proof. The ways a rewire could
//! still be wrong, each checked here:
//!
//! - a target row that never touched a replaced owner changes or disappears, other than an empty
//!   row whose source the donor now fills
//! - a row names an owner other than the one at its component index, or rows leave component order
//! - a row between the moved owner and a target owner differs from every stock weapon that carries
//!   both owners, which catches a wrong receiver, channel entry or variable
//! - a stock connection between the moved owner and a target owner is missing from the rewire
//! - the moved owner cannot be rewired back to exactly the target's own table
//! - the component list or bindings stop converging on the donor's owners
//! - a hand cannon's picker group on a sidearm, the first in-game candidate, is not rewired
use super::*;
use crate::package_authoring::{
    PackageManager, open_shadowkeep_package_manager,
    runtime::load_weapon_runtime_entity_at_pattern_index_with_manager,
};
use std::{collections::HashMap, fmt::Write as _};

const BINDINGS: [(&str, u32); 8] = [
    ("Trigger", WEAPON_TRIGGER_COMPONENT_KEY),
    ("Barrel", WEAPON_BARREL_COMPONENT_KEY),
    ("Magazine", WEAPON_MAGAZINE_COMPONENT_KEY),
    ("Reload", WEAPON_RELOAD_COMPONENT_KEY),
    ("Stat Translator", WEAPON_STAT_TRANSLATOR_COMPONENT_KEY),
    ("Controller", WEAPON_CONTROLLER_COMPONENT_KEY),
    ("Input", WEAPON_INPUT_COMPONENT_KEY),
    ("Trigger Charge", WEAPON_TRIGGER_CHARGE_COMPONENT_KEY),
];

struct Entity {
    payload: Vec<u8>,
    types: BTreeSet<String>,
    sample: String,
}

/// An event row without its component indices, which follow each entity's own component order.
#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Link {
    source: Object,
    destination: Object,
    variable: u64,
    names: (u64, u64),
}

fn link(row: &Row) -> Link {
    Link {
        source: object(row, SOURCE).unwrap(),
        destination: object(row, DESTINATION).unwrap(),
        variable: read_u64(row, RECEIVER_VARIABLE).unwrap(),
        names: (read_u64(row, 0).unwrap(), read_u64(row, 0x20).unwrap()),
    }
}

fn rows(entity: &[u8]) -> Vec<Row> {
    Side::new(entity).unwrap().rows
}

/// Stock evidence: which entities carry each owner, and each entity's links.
struct Stock {
    carriers: BTreeMap<u32, BTreeSet<u32>>,
    links: BTreeMap<u32, BTreeSet<Link>>,
}

impl Stock {
    fn new(entities: &BTreeMap<u32, Entity>) -> Self {
        let mut carriers = BTreeMap::<u32, BTreeSet<u32>>::new();
        let mut links = BTreeMap::new();
        for (&tag, entity) in entities {
            for owner in component_owners(&entity.payload).unwrap() {
                carriers.entry(owner).or_default().insert(tag);
            }
            links.insert(tag, rows(&entity.payload).iter().map(link).collect());
        }
        Self { carriers, links }
    }

    /// The stock entities that carry both owners.
    fn witnesses(&self, left: u32, right: u32) -> Vec<u32> {
        let (Some(left), Some(right)) = (self.carriers.get(&left), self.carriers.get(&right))
        else {
            return Vec::new();
        };
        left.intersection(right).copied().collect()
    }
}

#[derive(Default)]
struct Tally {
    results: BTreeMap<(&'static str, String), usize>,
    agree: usize,
    missing: usize,
    failures: Vec<String>,
}

/// Every check on one rewired table. Returns the stock agreements and missing connections.
fn check_rewired(
    stock: &Stock,
    host: &[u8],
    donor: &[u8],
    authored: &[u8],
    bindings: &[u32],
    read: &dyn Fn(u32) -> Result<Vec<u8>, String>,
) -> Result<(usize, usize), String> {
    let replaced = owners_of(host, bindings);
    let moved = owners_of(donor, bindings);
    let authored_rows = rows(authored);
    // Rows that never touched a replaced owner survive, unless an empty one gave its source up.
    let filled = authored_rows
        .iter()
        .map(|row| object(row, SOURCE).unwrap())
        .collect::<BTreeSet<_>>();
    for row in rows(host) {
        let (source, destination) = (object(&row, SOURCE)?, object(&row, DESTINATION)?);
        let untouched = !replaced.contains(&source.owner) && !replaced.contains(&destination.owner);
        let gave_up = destination.owner == NULL_TAG && filled.contains(&source);
        if untouched && !gave_up && !authored_rows.contains(&row) {
            return Err(format!("a target row changed: {:X?}", link(&row)));
        }
    }
    check_events(authored)?;
    let donor_owners = component_owners(donor)?;
    let authored_links = authored_rows.iter().map(link).collect::<BTreeSet<_>>();
    let mut agree = 0;
    for link in &authored_links {
        let (mover, other) = if moved.contains(&link.source.owner) {
            (link.source.owner, link.destination.owner)
        } else if moved.contains(&link.destination.owner) {
            (link.destination.owner, link.source.owner)
        } else {
            continue;
        };
        // An owner the donor itself carries keeps the donor's own row, which proves nothing.
        if other == NULL_TAG || moved.contains(&other) || donor_owners.contains(&other) {
            continue;
        }
        let witnesses = stock.witnesses(mover, other);
        if witnesses.is_empty() {
            continue;
        }
        if !witnesses
            .iter()
            .any(|entity| stock.links[entity].contains(link))
        {
            return Err(format!(
                "no stock weapon with owners 0x{mover:08X} and 0x{other:08X} has {link:X?}"
            ));
        }
        agree += 1;
    }
    let missing = missing_stock_links(stock, &moved, &donor_owners, authored, &authored_links)?;
    round_trip(host, authored, bindings, read)?;
    Ok((agree, missing))
}

fn owners_of(entity: &[u8], bindings: &[u32]) -> BTreeSet<u32> {
    bindings
        .iter()
        .flat_map(|&binding| weapon_component_bindings(entity, binding).unwrap())
        .map(|binding| binding.owner_tag)
        .collect()
}

/// Stock connections between a moved owner and a target owner that the rewire does not make.
fn missing_stock_links(
    stock: &Stock,
    moved: &BTreeSet<u32>,
    donor_owners: &[u32],
    authored: &[u8],
    authored_links: &BTreeSet<Link>,
) -> Result<usize, String> {
    let authored_owners = component_owners(authored)?;
    let mut missing = BTreeSet::new();
    for owner in moved {
        for entity in stock.carriers.get(owner).into_iter().flatten() {
            for link in &stock.links[entity] {
                let other = if link.source.owner == *owner {
                    link.destination.owner
                } else if link.destination.owner == *owner {
                    link.source.owner
                } else {
                    continue;
                };
                let relevant = other != NULL_TAG
                    && !moved.contains(&other)
                    && !donor_owners.contains(&other)
                    && authored_owners.contains(&other);
                if relevant && !authored_links.contains(link) {
                    missing.insert(link.clone());
                }
            }
        }
    }
    Ok(missing.len())
}

/// The target's own owners rewired back onto the result must restore the target's table exactly.
fn round_trip(
    host: &[u8],
    authored: &[u8],
    bindings: &[u32],
    read: &dyn Fn(u32) -> Result<Vec<u8>, String>,
) -> Result<(), String> {
    let mut back = authored.to_vec();
    let grafts = bindings
        .iter()
        .map(|&binding| (binding, host))
        .collect::<Vec<_>>();
    graft_weapon_component_bindings_or_rewire(&mut back, &grafts, read)
        .map_err(|error| format!("the rewire could not be reversed: {error}"))?;
    if rows(&back) != rows(host) {
        return Err("rewiring the target's own owner back did not restore its table".into());
    }
    Ok(())
}

fn normalize(error: &str) -> String {
    let mut out = String::new();
    let mut chars = error.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '0' && chars.peek() == Some(&'x') {
            chars.next();
            while chars.next_if(char::is_ascii_hexdigit).is_some() {}
            out.push_str("0x?");
        } else if c.is_ascii_digit() {
            while chars.next_if(char::is_ascii_digit).is_some() {}
            out.push('N');
        } else {
            out.push(c);
        }
    }
    out
}

fn stock_entities(install: &std::path::Path, manager: &PackageManager) -> BTreeMap<u32, Entity> {
    let catalog = crate::test_support::catalog(install).unwrap();
    let mut entities = BTreeMap::<u32, Entity>::new();
    for donor in catalog.weapon_donors() {
        let Some(index) = donor.weapon_pattern_index else {
            continue;
        };
        let Ok(source) = load_weapon_runtime_entity_at_pattern_index_with_manager(manager, index)
        else {
            continue;
        };
        entities
            .entry(source.entity_tag)
            .or_insert_with(|| Entity {
                payload: source.payload.clone(),
                types: BTreeSet::new(),
                sample: donor.name.clone(),
            })
            .types
            .insert(donor.type_name.trim().to_owned());
    }
    entities
}

fn label(types: &BTreeSet<String>) -> String {
    types
        .iter()
        .filter(|name| !name.is_empty())
        .cloned()
        .collect::<Vec<_>>()
        .join("/")
}

/// The graft, host and donor columns of one artifact row.
fn columns(
    graft: &str,
    (host_tag, host): (u32, &Entity),
    (donor_tag, donor): (u32, &Entity),
) -> String {
    format!(
        "{graft}\t0x{host_tag:08X}\t{}\t{}\t0x{donor_tag:08X}\t{}\t{}",
        label(&host.types),
        host.sample,
        label(&donor.types),
        donor.sample
    )
}

#[test]
#[ignore = "requires SUNDIAL_STOCK_PACKAGES pointing to clean Shadowkeep packages"]
fn every_stock_component_donor_rewires_consistently_with_stock_wiring() {
    let packages = crate::test_support::stock_packages();
    let manager = open_shadowkeep_package_manager(&packages).unwrap();
    let entities = stock_entities(packages.parent().unwrap(), &manager);
    let stock = Stock::new(&entities);
    let cache = RefCell::new(HashMap::<u32, Vec<u8>>::new());
    let read = |tag: u32| -> Result<Vec<u8>, String> {
        if let Some(payload) = cache.borrow().get(&tag) {
            return Ok(payload.clone());
        }
        let payload = manager.read_tag(tag)?;
        cache.borrow_mut().insert(tag, payload.clone());
        Ok(payload)
    };
    let mut table = String::from(
        "graft\thost\thost types\thost sample\tdonor\tdonor types\tdonor sample\tresult\tstock agree\tstock missing\n",
    );
    let mut tally = Tally::default();
    // Each component alone, as a recipe may name it, and once per distinct group the picker
    // changes together, which is what Advanced Gameplay applies.
    let mut groups = BTreeSet::new();
    for (name, binding) in BINDINGS {
        for (&host_tag, host) in &entities {
            let Ok(group) = coupled_weapon_component_bindings(&host.payload, binding) else {
                continue;
            };
            for (&donor_tag, donor) in &entities {
                if host_tag == donor_tag
                    || weapon_component_bindings(&donor.payload, binding).is_err()
                {
                    continue;
                }
                let mut grafts = vec![(name, vec![binding])];
                if groups.insert((host_tag, donor_tag, group.clone())) {
                    grafts.push(("Picker Group", group.clone()));
                }
                for (graft, bindings) in grafts {
                    let pair = format!("{graft} 0x{donor_tag:08X} onto 0x{host_tag:08X}");
                    let (kind, agree, missing) =
                        attempt(&stock, host, donor, &bindings, &read, &mut tally, &pair);
                    *tally.results.entry((graft, kind.clone())).or_default() += 1;
                    let columns = columns(graft, (host_tag, host), (donor_tag, donor));
                    let _ = writeln!(table, "{columns}\t{kind}\t{agree}\t{missing}");
                }
            }
        }
    }
    let prototype = entities.iter().any(|(&host_tag, host)| {
        let rewired = |(&donor_tag, donor): (&u32, &Entity)| {
            let columns = columns("Picker Group", (host_tag, host), (donor_tag, donor));
            donor.types.contains("Hand Cannon") && table.contains(&format!("{columns}\trewired\t"))
        };
        host.types.contains("Sidearm") && entities.iter().any(rewired)
    });
    let mut summary =
        String::from("# Component Rewire E2E\n\n| graft | result | pairs |\n|---|---|---|\n");
    for ((name, kind), count) in &tally.results {
        let _ = writeln!(summary, "| {name} | {kind} | {count} |");
    }
    let _ = writeln!(
        summary,
        "\n{} rewired rows agree with a stock weapon carrying both owners. {} stock connections of a moved owner are absent from a rewire. {} checks failed.",
        tally.agree,
        tally.missing,
        tally.failures.len()
    );
    let out = crate::test_support::artifacts("component-rewire")
        .unwrap_or_else(|| std::env::temp_dir().join("sundial-component-rewire"));
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("rewire.tsv"), &table).unwrap();
    std::fs::write(out.join("summary.md"), &summary).unwrap();
    std::fs::write(out.join("failures.txt"), tally.failures.join("\n")).unwrap();
    eprintln!("{summary}\nArtifacts: {}", out.display());
    assert!(
        tally.failures.is_empty(),
        "see {}",
        out.join("failures.txt").display()
    );
    assert!(
        prototype,
        "a hand cannon's picker group rewires onto a sidearm"
    );
}

/// Grafts `bindings` from the donor and checks a rewired result, recording a failed check.
fn attempt(
    stock: &Stock,
    host: &Entity,
    donor: &Entity,
    bindings: &[u32],
    read: &dyn Fn(u32) -> Result<Vec<u8>, String>,
    tally: &mut Tally,
    pair: &str,
) -> (String, usize, usize) {
    let mut authored = host.payload.clone();
    let grafts = bindings
        .iter()
        .map(|&binding| (binding, donor.payload.as_slice()))
        .collect::<Vec<_>>();
    match graft_weapon_component_bindings_or_rewire(&mut authored, &grafts, read) {
        Ok(ComponentWiring::Paired) => ("paired".to_owned(), 0, 0),
        Ok(ComponentWiring::Rewired) => {
            match check_rewired(
                stock,
                &host.payload,
                &donor.payload,
                &authored,
                bindings,
                read,
            ) {
                Ok((agree, missing)) => {
                    tally.agree += agree;
                    tally.missing += missing;
                    ("rewired".to_owned(), agree, missing)
                }
                Err(error) => {
                    tally.failures.push(format!("{pair}: {error}"));
                    ("rewired, check failed".to_owned(), 0, 0)
                }
            }
        }
        Err(error) => (normalize(&error), 0, 0),
    }
}
