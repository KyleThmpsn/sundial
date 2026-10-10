//! Named constants of researched contact and query expressions. Existing typed Float4 locators
//! remain the persistence contract. Only X changes, after the program and provider are qualified.
use super::*;
use std::collections::{BTreeMap, BTreeSet};

mod provider;
mod storage;
use provider::Provider;
use storage::{Data, Program};

const LITERAL: &[u8] = &[0x34, 0, 0x3E, 0];
const AFFINE: &[u8] = &[0x3C, 1, 0x34, 0, 0x34, 1, 0x12, 0x3E, 0];
const PRODUCT: &[u8] = &[0x34, 0, 0x3C, 1, 1, 0x34, 1, 0x3C, 2, 1, 3, 0x3E, 0];
const ADD: &[u8] = &[0x34, 0, 0x3C, 1, 1, 0x3E, 0];
const MULTIPLY: &[u8] = &[0x34, 0, 0x3C, 1, 3, 0x3E, 0];
const SUBTRACT: &[u8] = &[0x3C, 1, 0x34, 0, 2, 0x3E, 0];

fn roots(graph: &WeaponRuntimeGraph) -> impl Iterator<Item = (u32, &WeaponRuntimeRoot)> {
    graph
        .resources
        .iter()
        .flat_map(|resource| {
            std::iter::once(&resource.instance)
                .chain(resource.definition.iter())
                .map(move |root| (resource.owner_tag, root))
        })
        .chain(
            graph
                .owners
                .iter()
                .flat_map(|owner| owner.roots.iter().map(move |root| (owner.owner_tag, root))),
        )
}

struct Contact {
    trigger: bool,
    program: Program,
    providers: Vec<Provider>,
}

fn contacts(data: &mut Data<'_>, root: &WeaponRuntimeRoot) -> Result<Vec<(bool, Program)>, String> {
    let bytes = data.bytes;
    let definition = root.owner_offset as usize;
    let source = data.pair(definition, 0x8080_388F, 0x8080_3B73)?;
    let programs = data.array(source + 0x40, 0x8080_89F7)?;
    let mut found = Vec::new();
    for group in data.array(definition + 0x218, 0x8080_37CD)? {
        data.pair(group, 0x8080_37CD, 0x8080_37CC)?;
        for node in data.array(group + 0x48, 0x8080_37D0)? {
            data.pair(node, 0x8080_37D0, 0x8080_37CF)?;
            for action in data.array(node + 0x178, 0x8080_37F7)? {
                if bytes_at::<1>(bytes, action + 8)?[0] != 0 {
                    continue;
                }
                let relative = i64_at(bytes, action)?;
                if relative == 0 {
                    continue;
                }
                let parameter = relative_offset(action, 0, relative)?;
                if parameter < 4 || u32_at(bytes, parameter - 4)? != 0x8080_37E1 {
                    continue;
                }
                data.extent(parameter, 0x8080_37E1)?;
                for (offset, trigger) in [(0x20, false), (0x30, true)] {
                    let index = u32_at(bytes, parameter + offset)? as i32;
                    if index == -1 {
                        continue;
                    }
                    let at = *programs
                        .get(
                            usize::try_from(index)
                                .map_err(|_| "Contact program index is negative")?,
                        )
                        .ok_or("Contact program index leaves the projectile array")?;
                    let target = data.pair(at, 0x8080_89F7, 0x8080_89F8)?;
                    if let Some(program) = data.program(target)? {
                        found.push((trigger, program));
                    }
                }
            }
        }
    }
    Ok(found)
}

fn connect(
    manager: &PackageManager,
    graph: &WeaponRuntimeGraph,
    entity: &[u8],
    data: &Data<'_>,
    program: &Program,
) -> Result<Vec<Provider>, String> {
    program
        .providers
        .iter()
        .map(|&at| {
            provider::join(
                manager,
                graph,
                entity,
                data.owner,
                at,
                u32_at(data.bytes, at + 32)?,
            )
        })
        .collect()
}

fn setting(
    owner: u32,
    root: &WeaponRuntimeRoot,
    users: &BTreeMap<usize, BTreeSet<usize>>,
    program: &Program,
    index: usize,
    property: Property,
) -> Option<Setting> {
    let &offset = program.constants.get(index)?;
    if users.get(&offset)? != &BTreeSet::from([program.at]) {
        return None;
    }
    let field = root.fields.iter().find(|field| {
        field.owner_offset as usize == offset
            && field.source == WeaponRuntimeFieldSource::NativeDeclaration
            && field.locator.type_handle.get() == 0x8080_0090
            && field.locator.byte_size == 16
            && matches!(field.value, WeaponRuntimeValue::Vector4Float32Bits(_))
    })?;
    Some(Setting {
        kind: Kind::Native(property),
        owner_tag: owner,
        field: field.clone(),
        lane: None,
        requires_positive_minimum: false,
    })
}

fn contact_settings(
    manager: &PackageManager,
    graph: &WeaponRuntimeGraph,
    entity: &[u8],
    data: &mut Data<'_>,
    root: &WeaponRuntimeRoot,
) -> Result<Vec<Setting>, String> {
    let users = data.constant_users();
    let contacts = contacts(data, root)?
        .into_iter()
        .filter_map(|(trigger, program)| {
            let providers = connect(manager, graph, entity, data, &program).ok()?;
            Some(Contact {
                trigger,
                program,
                providers,
            })
        })
        .collect::<Vec<_>>();
    let mut found = Vec::new();
    for contact in &contacts {
        let Contact {
            trigger,
            program,
            providers,
        } = contact;
        let choose = |cleanup, on_trigger| if *trigger { on_trigger } else { cleanup };
        let mut add = |index, property| {
            found.extend(setting(data.owner, root, &users, program, index, property));
        };
        match program.code.as_slice() {
            LITERAL => add(
                0,
                choose(Property::ContactCleanupDelay, Property::ContactTriggerDelay),
            ),
            ADD => add(
                0,
                choose(
                    Property::ContactCleanupOffset,
                    Property::ContactTriggerOffset,
                ),
            ),
            MULTIPLY => add(
                0,
                choose(
                    Property::ContactCleanupScaling,
                    Property::ContactTriggerScaling,
                ),
            ),
            AFFINE => {
                if providers[0].variation && !trigger {
                    add(0, Property::ContactCleanupVariation);
                } else if !providers[0].variation {
                    add(
                        0,
                        choose(
                            Property::ContactCleanupScaling,
                            Property::ContactTriggerScaling,
                        ),
                    );
                }
                add(
                    1,
                    choose(
                        Property::ContactCleanupOffset,
                        Property::ContactTriggerOffset,
                    ),
                );
            }
            PRODUCT => {
                // Independent additive timers consume this exact first provider as seconds.
                let witnesses = contacts
                    .iter()
                    .filter(|other| {
                        other.program.code == ADD
                            && other.program.at != program.at
                            && !other.providers[0].variation
                            && other.providers[0] == providers[0]
                    })
                    .map(|other| other.trigger)
                    .collect::<BTreeSet<_>>();
                if witnesses == BTreeSet::from([false, true]) {
                    add(
                        0,
                        choose(
                            Property::ContactCleanupBaseOffset,
                            Property::ContactTriggerBaseOffset,
                        ),
                    );
                    add(
                        1,
                        choose(
                            Property::ContactCleanupScalingOffset,
                            Property::ContactTriggerScalingOffset,
                        ),
                    );
                }
            }
            _ => {}
        }
    }
    Ok(found)
}

fn emitter_settings(
    manager: &PackageManager,
    graph: &WeaponRuntimeGraph,
    entity: &[u8],
    data: &mut Data<'_>,
    root: &WeaponRuntimeRoot,
) -> Result<Vec<Setting>, String> {
    let at = root.owner_offset as usize;
    data.pair(at, 0x8080_3813, 0x8080_3810)?;
    let users = data.constant_users();
    let timed = graph
        .resources
        .iter()
        .any(|resource| resource.instance.schema == 0x8080_3B73)
        && bytes_at::<1>(data.bytes, at + 0x1A8)?[0] != 0
        && bytes_at::<1>(data.bytes, at + 0x14A)?[0] == 0;
    let mut found = Vec::new();
    for (offset, code, property) in [
        (0x60, SUBTRACT, Property::QueryScaleReduction),
        (0x158, MULTIPLY, Property::TriggeredDurationScaling),
    ] {
        if offset == 0x158 && !timed {
            continue;
        }
        let Some(program) = data.program(at + offset)? else {
            continue;
        };
        if program.code != code || connect(manager, graph, entity, data, &program).is_err() {
            continue;
        }
        found.extend(setting(data.owner, root, &users, &program, 0, property));
    }
    Ok(found)
}

pub(in crate::ability::settings) fn discover(
    manager: &PackageManager,
    graph: &WeaponRuntimeGraph,
) -> Vec<Setting> {
    let Ok(entity) = manager.read_tag(graph.entity_tag) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    let mut seen = BTreeSet::new();
    for (owner, root) in roots(graph) {
        if !matches!(root.schema, 0x8080_3813 | 0x8080_388F)
            || !seen.insert((owner, root.schema, root.owner_offset))
        {
            continue;
        }
        let Ok(bytes) = manager.read_tag(owner) else {
            continue;
        };
        let Ok(mut data) = Data::new(&bytes, owner) else {
            continue;
        };
        let settings = if root.schema == 0x8080_3813 {
            emitter_settings(manager, graph, &entity, &mut data, root)
        } else {
            contact_settings(manager, graph, &entity, &mut data, root)
        };
        if let Ok(settings) = settings {
            found.extend(settings);
        }
    }
    found
}
