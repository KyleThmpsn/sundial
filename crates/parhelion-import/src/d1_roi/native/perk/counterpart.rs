//! The Shadowkeep version of a source ability, which every source condition naming one ability
//! selects through.
//!
//! Native ability conditions compare an entity graph tag (class `80809C0F`): the selected ability
//! predicate (kinds 20 and 35, `+C8`) through `F04840`, and the roster events (kinds 10 and 11,
//! `+10`) through `107C4B0`. Stock data names Super and grenade entities there. The importer finds
//! that entity structurally, by these rules in order:
//!
//! 1. The same identity. A native definitions row whose `+4` is the source definitions row's
//!    `+0`, mapped through the runtime assignment table to an entity. Blink, Shadowshot, Golden Gun
//!    and the other abilities both games keep resolve this way.
//! 2. A bank-key witness, only when no native row holds the identity. Every source bank row that
//!    one native root bank carries unchanged (`grant::witness`) must name the same native bank, and
//!    one native entity's script must read that bank. Defender's Ward of Dawn (`5E4C7A19`) has no
//!    native identity. Its row under key `7F4BA6E0` is carried by Sentinel Shield bank `80BC9C02`,
//!    so it resolves to Sentinel Shield entity `80BAAD9C`.
//! 3. A script parameter witness, when neither holds. A parameter the source bank sets that the
//!    script owners of exactly one native entity declare names that entity ([`parameter`]). Arc
//!    Blade resolves to Arc Staff this way, the successor Unstable Current's grant also takes.
//!
//! Otherwise the source ability has no Shadowkeep version. The slot the native bank serves is the
//! entity's slot, which a selector's own slot must match.
//!
//! An imported copy of a source ability gets a namespaced entity the build allocates. Each program
//! lists the Shadowkeep entities its conditions name with the source identity each stands for
//! ([`named`]), and the build adds a condition for every imported copy of that identity beside the
//! Shadowkeep one. Sentinel Shield also stands for all three of its paths, since bank key
//! `7D635D60` that makes it Ward of Dawn sets no parameter a predicate reads.
use super::*;
use std::cell::RefCell;
use sundial::package_authoring::ability_bank::AbilityTarget;
use sundial::package_authoring::ability_modifier::{bank_slot, entity_bank};
use sundial::package_authoring::sandbox_perk::program::AbilityCounterpart;

thread_local! {
    /// Every counterpart found, by Shadowkeep entity, with the source identities it stands for.
    /// Each entry is a fact of the two builds' packages, so it holds for every later program.
    static FOUND: RefCell<BTreeMap<u32, BTreeSet<u32>>> = const { RefCell::new(BTreeMap::new()) };
}

/// The source abilities a program's own conditions name through a Shadowkeep entity found here.
pub(super) fn named(program: &Program) -> Vec<AbilityCounterpart> {
    let entities = program
        .native_trigger
        .iter()
        .chain(&program.alternative_triggers)
        .chain(&program.native_removal)
        .chain(&program.alternative_removals)
        .chain(&program.native_rearm)
        .chain(&program.alternative_rearms)
        .chain(
            program
                .additional_groups
                .iter()
                .flat_map(|group| group.conditions()),
        )
        .filter_map(NativeNode::ability_operand)
        .collect::<BTreeSet<_>>();
    FOUND.with_borrow(|found| {
        entities
            .iter()
            .filter_map(|entity| Some((*entity, found.get(entity)?)))
            .flat_map(|(native, identities)| {
                identities.iter().map(move |&identity| AbilityCounterpart {
                    source: crate::d1_roi::ABILITY_SOURCE.to_owned(),
                    identity,
                    native: Some(native),
                })
            })
            .collect()
    })
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Counterpart {
    pub(super) identity: u32,
    pub(super) entity: u32,
    /// The slot of the bank the entity's script reads, when Sundial's slot table lists it.
    pub(super) slot: Option<AbilityTarget>,
}

/// The Shadowkeep version of the source ability graph `pattern`, when one exists.
pub(super) fn find(
    source: &Source,
    native: &mut Reader,
    pattern: u32,
) -> Result<Option<Counterpart>> {
    let found = resolve(source, native, pattern)?;
    if let Some(found) = found {
        FOUND.with_borrow_mut(|known| {
            known
                .entry(found.entity)
                .or_default()
                .insert(found.identity);
        });
    }
    Ok(found)
}

fn resolve(source: &Source, native: &mut Reader, pattern: u32) -> Result<Option<Counterpart>> {
    let identity = identity(source, pattern)?;
    let entities = native_entities(native)?;
    if let Some(&entity) = entities.get(&identity) {
        return Ok(Some(Counterpart {
            identity,
            entity,
            slot: slot(native, entity)?,
        }));
    }
    let (_, data, rows, _) = grant::property_bank(source, pattern)?;
    if let Some((entity, slot)) = bank_key(native, &data, &rows, &entities)? {
        return Ok(Some(Counterpart {
            identity,
            entity,
            slot: Some(slot),
        }));
    }
    Ok(
        parameter(native, &data, &rows, &entities)?.map(|(entity, slot)| Counterpart {
            identity,
            entity,
            slot: Some(slot),
        }),
    )
}

/// Rule 2: every source bank row one native root bank carries unchanged names that bank, and one
/// native entity's script reads it.
fn bank_key(
    native: &mut Reader,
    data: &Payload,
    rows: &[usize],
    entities: &BTreeMap<u32, u32>,
) -> Result<Option<(u32, AbilityTarget)>> {
    let mut banks = BTreeSet::new();
    for &row in rows {
        if let Some((bank, slot, _)) = grant::witness::find(native, data, rows, row)? {
            banks.insert((bank, slot.byte()));
        }
    }
    let (Some(&(bank, slot)), 1) = (banks.first(), banks.len()) else {
        return Ok(None);
    };
    let mut holders = Vec::new();
    for &entity in entities.values() {
        let graph = native.tag(entity, Some(0x80809C0F))?;
        if entity_bank(&graph.0).map_err(anyhow::Error::msg)? == Some(bank) {
            holders.push(entity);
        }
    }
    Ok(match holders.as_slice() {
        [entity] => Some((*entity, AbilityTarget::from_byte(slot))),
        _ => None,
    })
}

/// Rule 3: a script parameter witness. Some parameter the source bank's own script parameter rows
/// set is declared by the script owners of exactly one native ability entity, every such
/// parameter names the same entity, and that entity's bank has a known slot. Native setter
/// `00576010` writes a name only into components that declare it, so the entity is the one native
/// ability that still reads the source's variable. Bladedancer's Arc Blade (`95A5C27D`) sets
/// `FF78BA8F`, which only Arc Staff's script owner `80BC3BFF` declares.
fn parameter(
    native: &mut Reader,
    data: &Payload,
    rows: &[usize],
    entities: &BTreeMap<u32, u32>,
) -> Result<Option<(u32, AbilityTarget)>> {
    let mut names = BTreeSet::new();
    for &row in rows {
        if let Some(parameters) = grant::witness::script_parameters(data, rows, row)? {
            names.extend(parameters.iter().map(|parameter| parameter.0));
        }
    }
    if names.is_empty() {
        return Ok(None);
    }
    let mut declared = BTreeMap::<u32, BTreeSet<u32>>::new();
    for &entity in entities.values() {
        for (_, words) in owner_words(native, entity)? {
            for word in words.intersection(&names) {
                declared.entry(*word).or_default().insert(entity);
            }
        }
    }
    let witnesses = declared
        .values()
        .filter(|holders| holders.len() == 1)
        .filter_map(|holders| holders.first().copied())
        .collect::<BTreeSet<_>>();
    let (Some(&entity), 1) = (witnesses.first(), witnesses.len()) else {
        return Ok(None);
    };
    Ok(slot(native, entity)?.map(|slot| (entity, slot)))
}

/// The script parameter witness for a source ability bank's rows, over every native ability.
pub(super) fn by_parameter(
    native: &mut Reader,
    data: &Payload,
    rows: &[usize],
) -> Result<Option<(u32, AbilityTarget)>> {
    let entities = native_entities(native)?;
    parameter(native, data, rows, &entities)
}

/// The component owners of a native ability entity whose payload names `name`.
pub(super) fn declaring_owners(native: &mut Reader, entity: u32, name: u32) -> Result<Vec<u32>> {
    Ok(owner_words(native, entity)?
        .into_iter()
        .filter(|(_, words)| words.contains(&name))
        .map(|(owner, _)| owner)
        .collect())
}

/// Each `80809C36` component owner of a native ability entity with the aligned words it holds.
fn owner_words(native: &mut Reader, entity: u32) -> Result<Vec<(u32, BTreeSet<u32>)>> {
    let graph = native.tag(entity, Some(0x80809C0F))?;
    let mut owners = Vec::new();
    for row in graph.array(0x10, 12, Some(0x80809C04))? {
        let tag = graph.u32(row)?;
        let Ok(owner) = native.tag(tag, Some(0x80809C36)) else {
            continue;
        };
        let words = owner
            .0
            .chunks_exact(4)
            .map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]]))
            .collect();
        owners.push((tag, words));
    }
    Ok(owners)
}

/// The slot of the bank a native ability entity's script reads.
pub(super) fn slot(native: &mut Reader, entity: u32) -> Result<Option<AbilityTarget>> {
    let graph = native.tag(entity, Some(0x80809C0F))?;
    Ok(entity_bank(&graph.0)
        .map_err(anyhow::Error::msg)?
        .and_then(bank_slot))
}

/// Native ability identities and the entities the runtime assignment table gives them.
fn native_entities(native: &mut Reader) -> Result<BTreeMap<u32, u32>> {
    let tags = native.classes(0x80805CA4);
    ensure!(
        tags.len() == 1,
        "Native ability definitions are missing or ambiguous"
    );
    let definitions = native.tag(tags[0], Some(0x80805CA4))?;
    let mut identities = BTreeSet::new();
    for row in definitions.array(8, 28, Some(0x80805CAA))? {
        ensure!(
            identities.insert(definitions.u32(row + 4)?),
            "Duplicate native ability identity"
        );
    }
    let roots = native
        .manager
        .lookup
        .named_tags
        .iter()
        .filter(|tag| tag.name == "investment_assets")
        .map(|tag| tag.hash.0)
        .collect::<BTreeSet<_>>();
    ensure!(
        roots.len() == 1,
        "Native investment assets are missing or ambiguous"
    );
    let root = native.tag(*roots.first().context("Native investment assets")?, None)?;
    let assignments = native.tag(root.u32(16)?, Some(0x80809780))?;
    let mut entities = BTreeMap::new();
    for row in assignments.array(8, 8, None)? {
        let identity = assignments.u32(row)?;
        if identities.contains(&identity) {
            ensure!(
                entities
                    .insert(identity, assignments.u32(row + 4)?)
                    .is_none(),
                "Native ability has multiple runtime assignments"
            );
        }
    }
    Ok(entities)
}

/// The source pattern's unique stable identity in the source definitions and runtime assignments.
pub(super) fn identity(source: &Source, pattern: u32) -> Result<u32> {
    ensure!(
        source.reference(pattern)? == 0x80800734,
        "Source ability condition does not select a pattern"
    );
    source.read(pattern)?;
    let assignments = source.runtime_assignments(&mut BTreeMap::new())?;
    let tags = source.classes(0x80803432);
    ensure!(
        tags.len() == 1,
        "Source ability definitions are missing or ambiguous"
    );
    let definitions = source.read(tags[0])?;
    let mut identities = BTreeSet::new();
    let mut matched = None;
    for row in definitions.array(8, 40, Some(0x8080343C))? {
        let identity = definitions.u32(row)?;
        ensure!(
            identities.insert(identity),
            "Duplicate source ability identity"
        );
        if assignments.get(&identity) == Some(&pattern) {
            ensure!(
                matched.replace(identity).is_none(),
                "Source ability pattern has multiple identities"
            );
        }
    }
    matched.context("Source pattern has no ability identity")
}
